#!/usr/bin/env python3
"""Desktop etiquette guard for local-STT GPU / heavy-CPU jobs.

The desktop GPU (RTX 2060 SUPER, 8 GB) is shared with the LIVE
star-trek-camera tracker (main.py --port 8100). Rules, verbatim:

From the main session (2026-09-25 ~04:40 IST):
> Start no GPU or heavy-CPU job on the desktop before 10:00 IST.
> One more desktop rule from the camera session: /home must keep at least 50 GB free (the
> 1080p recorder pauses under 30 GB).

From programs-b6, the camera session (2026-09-25 ~04:50 IST), approved relative bar,
effective after 10:00 IST, for training AND GPU serving tests:
> (1) Baseline = the 10 min before each run, taken only while load < 8 and no other heavy
> job is running (the datalake refresh is running now and distorts it). A run may start,
> and continue, only while the 5-min rows/s is >= 95% of that baseline AND the p90 of
> `cycle.cycle_ms` in cycles.jsonl is <= 1.2x the baseline p90. Check every 30 s during
> the run; on a breach, pause within 30 s.
> (2) At least 3 GB of VRAM must stay free at all times. The live video estimator OOMed on
> CUDA at 03:21 while another training run shared the GPU. nvidia-smi is broken right now
> (driver/library mismatch), so find a working way to read free VRAM (e.g.
> torch.cuda.mem_get_info inside your process); if you can't read it, don't start.
> (3) /home must stay >= 50 GB free.

How this module implements them:
  * rows/s and p90(cycle_ms) come from star-trek-camera/data/logs/cycles.jsonl (rotation
    aware: the window may start in cycles.jsonl.1). Rates are over the last 5 min.
  * baseline: sampled every 30 s; a sample is dirty if the 1-min load average is >= 8, a
    heavy job is running (any process outside star-trek-camera using >= 1 CPU core over
    the 30 s except IGNORE_CPU, or a command line matching HEAVY_PATTERNS such as the
    datalake refresh), or
    the tracker is not running (< 0.5 rows/s). A dirty sample restarts the 10-min clock:
    the baseline is 10 CONTIGUOUS clean minutes, measured immediately before the run.
  * free VRAM: CUDA driver API cuMemGetInfo via ctypes (works while NVML is broken), or
    torch.cuda.mem_get_info() inside a torch process. Unreadable -> not OK.
  * the 10:00 IST rule is enforced too (NOT_BEFORE_HHMM) for 2026-09-25.

The first training round (04:00) watched star-trek's pose latency in /situation instead;
that did not see the harm (latency 7-35 ms while the cycle rate fell 1.75 -> 0.4).

CLI:
  python3 guard.py status                      # numbers now (no baseline)
  python3 guard.py baseline                    # measure a 10-min baseline, save it, print it
  python3 guard.py run [--need-gb G] -- CMD..  # baseline, then run CMD under the guard:
        breach -> SIGSTOP CMD within 30 s, SIGCONT when clear; VRAM breach or paused
        longer than --max-pause-min -> kill CMD, exit 3. Exit code = CMD's otherwise.
"""
from __future__ import annotations

import argparse
import ctypes
import datetime as dt
import json
import os
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

CYCLES = Path(os.environ.get("STREK_CYCLES",
                             Path.home() / "Programs/star-trek-camera/data/logs/cycles.jsonl"))
STT_HOME = Path(os.environ.get("STT_HOME", Path.home() / "Programs/voice-stt"))
BASELINE_FILE = STT_HOME / "runs/guard-baseline.json"

RATE_FRAC = 0.95        # 5-min rows/s >= 95 % of baseline
P90_MULT = 1.2          # 5-min p90 cycle_ms <= 1.2 x baseline p90
MIN_VRAM_GB = 3.0       # device-wide free VRAM, always
MIN_HOME_GB = 50.0      # /home free
BASELINE_S = 600        # 10 contiguous clean minutes
WINDOW_S = 300          # 5-min running window
POLL_S = 30
MAX_LOAD = 8.0
MIN_TRACKER_RATE = 0.5  # below this the tracker is not running: no baseline
NOT_BEFORE_HHMM = (dt.date(2026, 9, 25), 1000)
# transient heavy jobs, by command line (the always-on datalake watcher is not one of them)
HEAVY_PATTERNS = ("refresh-claude.sh", "datalake_sync_snapshot", "train_lora.py", "segment.py",
                  "eval_wer.py", "merge_convert.py", "archive_training_dates", "makepkg",
                  "cargo build", "pacman -S")
# always-on background load that is part of every baseline (xdg-desktop-portal has spun at
# ~1 core for days on the desktop); excluded from the >= 1 core test
IGNORE_CPU = ("xdg-desktop-portal",)
TAIL_BYTES = 8_000_000


# ---------------------------------------------------------------- cycles.jsonl
def _read_tail(path: Path, since: float) -> tuple[list[tuple[float, float]], bool]:
    try:
        size = path.stat().st_size
    except FileNotFoundError:
        return [], False
    with open(path, "rb") as f:
        f.seek(max(0, size - TAIL_BYTES))
        raw = f.read()
    rows, oldest = [], None
    for line in raw.splitlines():
        try:
            d = json.loads(line)
        except ValueError:
            continue
        if d.get("kind") != "cycle" or "ts" not in d:
            continue
        t = float(d["ts"])
        oldest = t if oldest is None else min(oldest, t)
        if t > since:
            ms = (d.get("cycle") or {}).get("cycle_ms")
            rows.append((t, float(ms) if ms is not None else float("nan")))
    covered = size <= TAIL_BYTES or (oldest is not None and oldest <= since)
    return rows, covered


def cycle_rows(t0: float, t1: float) -> list[tuple[float, float]]:
    rows, covered = _read_tail(CYCLES, t0)
    if not covered or not rows or rows[0][0] > t0 + 30:
        older, _ = _read_tail(CYCLES.with_name(CYCLES.name + ".1"), t0)
        rows = older + rows
    return [r for r in rows if t0 < r[0] <= t1]


def _p90(xs: list[float]) -> float:
    xs = sorted(x for x in xs if x == x)
    return xs[min(len(xs) - 1, int(0.9 * len(xs)))] if xs else float("nan")


def cycle_stats(t0: float, t1: float) -> dict:
    rows = cycle_rows(t0, t1)
    return {"rate": len(rows) / (t1 - t0), "p90_ms": _p90([m for _, m in rows]), "n": len(rows)}


def cycle_rate(window_s: float = WINDOW_S, now: float | None = None) -> float:
    now = time.time() if now is None else now
    return cycle_stats(now - window_s, now)["rate"]


# ---------------------------------------------------------------- host
def home_free_gb() -> float:
    return shutil.disk_usage(Path.home()).free / 1e9


def vram_free_gb() -> float | None:
    """Device-wide free VRAM via the CUDA driver API (NVML is broken on the desktop)."""
    try:
        cu = ctypes.CDLL("libcuda.so.1")
        if cu.cuInit(0) != 0:
            return None
        dev, ctx = ctypes.c_int(), ctypes.c_void_p()
        if cu.cuDeviceGet(ctypes.byref(dev), 0) != 0:
            return None
        if cu.cuDevicePrimaryCtxRetain(ctypes.byref(ctx), dev) != 0:
            return None
        try:
            cu.cuCtxSetCurrent(ctx)
            free, total = ctypes.c_size_t(), ctypes.c_size_t()
            if cu.cuMemGetInfo_v2(ctypes.byref(free), ctypes.byref(total)) != 0:
                return None
            return free.value / 1e9
        finally:
            cu.cuDevicePrimaryCtxRelease(dev)
    except OSError:
        return None


def _cpu_ticks() -> dict[int, tuple[int, str, str]]:
    out = {}
    for p in Path("/proc").iterdir():
        if not p.name.isdigit():
            continue
        try:
            stat = (p / "stat").read_text()
            fields = stat[stat.rindex(")") + 2:].split()
            ticks = int(fields[11]) + int(fields[12])
            cmd = (p / "cmdline").read_bytes().replace(b"\0", b" ").decode(errors="replace").strip()
            try:
                cwd = os.readlink(p / "cwd")
            except OSError:
                cwd = ""
            out[int(p.name)] = (ticks, cmd, cwd)
        except (OSError, ValueError, IndexError):
            continue
    return out


def heavy_jobs(prev: dict, cur: dict, dt_s: float, own: set[int] = frozenset()) -> list[str]:
    hz = os.sysconf("SC_CLK_TCK")
    found = []
    for pid, (ticks, cmd, cwd) in cur.items():
        if pid in own or not cmd:
            continue
        if "star-trek-camera" in cmd or "star-trek-camera" in cwd:  # the tracker + its recorders
            continue
        cores = (ticks - prev[pid][0]) / hz / dt_s if pid in prev else 0.0
        if (cores >= 1.0 and not any(k in cmd for k in IGNORE_CPU)) or any(k in cmd for k in HEAVY_PATTERNS):
            found.append(f"{pid} {cores:.1f} cores {cmd[:80]}")
    return found


def too_early() -> bool:
    day, hhmm = NOT_BEFORE_HHMM
    now = dt.datetime.now()
    return now.date() == day and now.hour * 100 + now.minute < hhmm


# ---------------------------------------------------------------- guard
class Guard:
    def __init__(self, say=print, rate_frac=RATE_FRAC, p90_mult=P90_MULT,
                 min_vram_gb=MIN_VRAM_GB, min_home_gb=MIN_HOME_GB):
        self.say, self.rate_frac, self.p90_mult = say, rate_frac, p90_mult
        self.min_vram_gb, self.min_home_gb = min_vram_gb, min_home_gb
        self.base: dict | None = None

    def measure_baseline(self, own: set[int] = frozenset(), max_wait_min: float = 0) -> dict:
        """Block until 10 contiguous clean minutes have passed; that window is the baseline."""
        t_begin = time.time()
        clean_since = time.time()
        prev, t_prev = _cpu_ticks(), time.time()
        while True:
            time.sleep(POLL_S)
            cur, now = _cpu_ticks(), time.time()
            why = []
            if too_early():
                why.append("before 10:00 IST")
            load = os.getloadavg()[0]
            if load >= MAX_LOAD:
                why.append(f"load {load:.1f} >= {MAX_LOAD}")
            hj = heavy_jobs(prev, cur, now - t_prev, own | {os.getpid()})
            if hj:
                why.append("heavy job: " + "; ".join(hj[:3]))
            r1 = cycle_rate(60, now)
            if r1 < MIN_TRACKER_RATE:
                why.append(f"tracker not running ({r1:.2f} rows/s over 1 min)")
            prev, t_prev = cur, now
            if why:
                if now - clean_since > POLL_S * 1.5:
                    self.say(f"baseline: dirty sample, restarting the 10-min clock ({', '.join(why)})")
                clean_since = now
            elif now - clean_since >= BASELINE_S:
                s = cycle_stats(clean_since, now)
                self.base = {**s, "t0": clean_since, "t1": now, "load1": load,
                             "t0_iso": dt.datetime.fromtimestamp(clean_since).isoformat(timespec="seconds"),
                             "t1_iso": dt.datetime.fromtimestamp(now).isoformat(timespec="seconds")}
                self.say(f"baseline {self.base['t0_iso']}..{self.base['t1_iso']}: "
                         f"{s['rate']:.3f} rows/s, p90 cycle_ms {s['p90_ms']:.1f}, n={s['n']}")
                try:
                    BASELINE_FILE.parent.mkdir(parents=True, exist_ok=True)
                    with open(BASELINE_FILE.with_suffix(".jsonl"), "a") as f:
                        f.write(json.dumps(self.base) + "\n")
                except OSError:
                    pass
                return self.base
            if max_wait_min and now - t_begin > max_wait_min * 60:
                raise TimeoutError("no clean 10-min baseline within the wait limit")

    def check(self, vram_free: float | None = None) -> tuple[bool, bool, str]:
        """(ok, vram_ok, message). vram_free: pass torch.cuda.mem_get_info()[0]/1e9 from a
        torch process; else it is read through the driver API."""
        assert self.base is not None, "measure_baseline() first"
        now = time.time()
        s = cycle_stats(now - WINDOW_S, now)
        vf = vram_free_gb() if vram_free is None else vram_free
        hf = home_free_gb()
        rate_ok = s["rate"] >= self.rate_frac * self.base["rate"]
        p90_ok = s["p90_ms"] <= self.p90_mult * self.base["p90_ms"]
        vram_ok = vf is not None and vf >= self.min_vram_gb
        home_ok = hf >= self.min_home_gb
        ok = rate_ok and p90_ok and vram_ok and home_ok and not too_early()
        msg = (f"rate {s['rate']:.3f}/s (need >= {self.rate_frac * self.base['rate']:.3f}{'' if rate_ok else ' FAIL'}), "
               f"p90 {s['p90_ms']:.1f} ms (need <= {self.p90_mult * self.base['p90_ms']:.1f}{'' if p90_ok else ' FAIL'}), "
               f"VRAM free {'unreadable' if vf is None else f'{vf:.2f}'} GB (need >= {self.min_vram_gb}"
               f"{'' if vram_ok else ' FAIL'}), /home {hf:.0f} GB{'' if home_ok else ' FAIL'}")
        return ok, vram_ok, msg


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["status", "baseline", "run"])
    ap.add_argument("--need-gb", type=float, default=0.0,
                    help="run: VRAM the command will take; start needs free >= 3 + this")
    ap.add_argument("--max-pause-min", type=float, default=30)
    args = sys.argv[1:]
    argv = args[args.index("--") + 1:] if "--" in args else []
    a = ap.parse_args(args[:args.index("--")] if "--" in args else args)

    def say(m):
        print(f"[guard {time.strftime('%H:%M:%S')}] {m}", file=sys.stderr, flush=True)

    if a.cmd == "status":
        now = time.time()
        s5, s1 = cycle_stats(now - WINDOW_S, now), cycle_stats(now - 60, now)
        vf = vram_free_gb()
        say(f"5 min: {s5['rate']:.3f} rows/s p90 {s5['p90_ms']:.1f} ms n={s5['n']}; 1 min: {s1['rate']:.2f}/s; "
            f"VRAM free {vf} GB; /home {home_free_gb():.0f} GB; load {os.getloadavg()[0]:.1f}; "
            f"too_early={too_early()}")
        return
    g = Guard(say)
    base = g.measure_baseline()
    if a.cmd == "baseline":
        print(json.dumps(base))
        return
    if not argv:
        ap.error("run needs -- CMD...")
    vf = vram_free_gb()
    ok, _, msg = g.check()
    if not ok or vf is None or vf < MIN_VRAM_GB + a.need_gb:
        say(f"not starting: {msg}; need {MIN_VRAM_GB + a.need_gb:.1f} GB free VRAM at start")
        sys.exit(3)
    say(f"starting {' '.join(argv)} ({msg})")
    p = subprocess.Popen(argv, start_new_session=True)
    paused_at = None
    while True:
        try:
            rc = p.wait(timeout=POLL_S)
            say(f"command exited {rc}")
            sys.exit(rc)
        except subprocess.TimeoutExpired:
            pass
        ok, vram_ok, msg = g.check()
        if not vram_ok:
            say(f"VRAM breach, killing the command: {msg}")
            os.killpg(p.pid, signal.SIGCONT if paused_at else 0)
            os.killpg(p.pid, signal.SIGTERM)
            p.wait()
            sys.exit(3)
        if not ok and paused_at is None:
            os.killpg(p.pid, signal.SIGSTOP)
            paused_at = time.time()
            say(f"PAUSE (SIGSTOP): {msg}")
        elif ok and paused_at is not None:
            os.killpg(p.pid, signal.SIGCONT)
            say(f"RESUME after {time.time() - paused_at:.0f} s: {msg}")
            paused_at = None
        elif paused_at and time.time() - paused_at > a.max_pause_min * 60:
            say("paused too long, killing the command")
            os.killpg(p.pid, signal.SIGCONT)
            os.killpg(p.pid, signal.SIGTERM)
            p.wait()
            sys.exit(3)


if __name__ == "__main__":
    main()
