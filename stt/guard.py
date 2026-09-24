#!/usr/bin/env python3
"""Desktop etiquette guard for local-STT GPU / heavy-CPU jobs.

The desktop GPU is shared with the LIVE star-trek-camera tracker. Rules from
the camera session (2026-09-25, verbatim in docs/HANDOVER-local-stt-2026-09-25.md):
  * GPU/heavy work may run only while the tracker stays above 1.5 cycles/s,
    measured as rows/s in ~/Programs/star-trek-camera/data/logs/cycles.jsonl
    over 5 min, before and during;
  * /home keeps >= 50 GB free (the 1080p recorder pauses under 30 GB).

The star-trek pose latency in /situation does NOT show the harm (it stayed at
7-35 ms while the cycle rate collapsed from 1.75 to 0.4), so this guard reads
the cycle rate itself.

CLI (exit 0 = OK to run GPU work, 1 = not OK; prints the numbers):
  python3 guard.py check              # rate + disk now
  python3 guard.py wait [--max-min N] # block until OK (exit 1 on timeout)
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import time
from pathlib import Path

CYCLES = Path(os.environ.get("STREK_CYCLES",
                             Path.home() / "Programs/star-trek-camera/data/logs/cycles.jsonl"))
MIN_RATE = 1.5          # cycles/s
WINDOW_S = 300          # 5 min
MIN_FREE_GB = 50.0      # /home
TAIL_BYTES = 4_000_000  # ~8k rows; 5 min at 2/s is ~600 rows


def _tail_ts(path: Path, since: float) -> tuple[list[float], bool]:
    """Timestamps of cycle rows newer than `since` from the tail of `path`.
    Second value: True if the tail read reaches back past `since` (or the file start)."""
    try:
        size = path.stat().st_size
    except FileNotFoundError:
        return [], False
    with open(path, "rb") as f:
        f.seek(max(0, size - TAIL_BYTES))
        raw = f.read()
    ts, oldest = [], None
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
            ts.append(t)
    covered = size <= TAIL_BYTES or (oldest is not None and oldest <= since)
    return ts, covered


def cycle_rate(window_s: float = WINDOW_S, now: float | None = None) -> float:
    """Tracker cycle rows per second over the last `window_s` (log rotation aware).
    0.0 if the log is missing or silent (tracker down counts as 'not OK')."""
    now = time.time() if now is None else now
    since = now - window_s
    ts, covered = _tail_ts(CYCLES, since)
    if not covered or not ts or min(ts) > since + 30:
        # the window may start in the rotated file
        older, _ = _tail_ts(CYCLES.with_name(CYCLES.name + ".1"), since)
        ts = older + ts
    return sum(1 for t in ts if since < t <= now) / window_s


def free_gb(path: str | Path = Path.home()) -> float:
    return shutil.disk_usage(path).free / 1e9


def status(min_rate: float = MIN_RATE, min_free_gb: float = MIN_FREE_GB) -> tuple[bool, str]:
    r, r1 = cycle_rate(), cycle_rate(60)
    g = free_gb()
    ok = r >= min_rate and g >= min_free_gb
    return ok, (f"star-trek {r:.2f} cycles/s over 5 min (1 min: {r1:.2f}; need >= {min_rate}); "
                f"/home free {g:.0f} GB (need >= {min_free_gb:.0f})")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["check", "wait"])
    ap.add_argument("--min-rate", type=float, default=MIN_RATE)
    ap.add_argument("--min-free-gb", type=float, default=MIN_FREE_GB)
    ap.add_argument("--max-min", type=float, default=0, help="wait: give up after N minutes (0 = never)")
    ap.add_argument("--poll-s", type=float, default=30)
    a = ap.parse_args()
    t0 = time.time()
    while True:
        ok, msg = status(a.min_rate, a.min_free_gb)
        print(f"[{time.strftime('%H:%M:%S')}] {'OK' if ok else 'WAIT'} {msg}", flush=True)
        if ok or a.cmd == "check":
            sys.exit(0 if ok else 1)
        if a.max_min and time.time() - t0 > a.max_min * 60:
            sys.exit(1)
        time.sleep(a.poll_s)


if __name__ == "__main__":
    main()
