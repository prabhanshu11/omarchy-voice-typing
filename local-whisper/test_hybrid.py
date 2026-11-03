#!/usr/bin/env python3
"""Hybrid routing test for server.py (2026-11-03), on spare ports, without touching the live
service: base on the CPU, and as the "long" model base again on the CPU (so it needs no GPU
memory; the routing, loading, unloading and fallback are what is tested).

    .venv/bin/python test_hybrid.py SHORT.wav LONG.wav      # LONG.wav must be >= 25 s
    TEST_LONG_MODEL=distil-large-v3 TEST_LONG_DEVICE=cuda TEST_LONG_COMPUTE=int8_float32 \
        .venv/bin/python test_hybrid.py SHORT.wav LONG.wav  # the real laptop setup (GPU free?)

Checks: the long model loads only while the provider file says offline; a long clip goes to
it, a short one stays on base; switching the provider back unloads it (child process gone).
"""
import json, os, subprocess, sys, tempfile, time, urllib.request, wave
from pathlib import Path

PORT, LONG_PORT = 8794, 8795
short_wav, long_wav = sys.argv[1], sys.argv[2]
for f in (short_wav, long_wav):
    with wave.open(f) as w:
        print(f"{Path(f).name}: {w.getnframes() / w.getframerate():.1f} s")

prov = Path(tempfile.mkdtemp()) / "stt-provider"
prov.write_text("deepgram\n")
env = dict(os.environ, WHISPER_MODEL="base", WHISPER_DEVICE="cpu", WHISPER_COMPUTE="int8",
           WHISPER_CPU_THREADS="4", WHISPER_PORT=str(PORT), WHISPER_BIND="127.0.0.1",
           WHISPER_LONG_MODEL=os.environ.get("TEST_LONG_MODEL", "base"),
           WHISPER_LONG_DEVICE=os.environ.get("TEST_LONG_DEVICE", "cpu"),
           WHISPER_LONG_COMPUTE=os.environ.get("TEST_LONG_COMPUTE", "int8"),
           WHISPER_LONG_PORT=str(LONG_PORT), WHISPER_LONG_AFTER_S="25",
           VOICE_STT_PROVIDER_FILE=str(prov))
srv = subprocess.Popen([sys.executable, str(Path(__file__).with_name("server.py"))], env=env,
                       stdout=open(os.environ.get("HYBRID_TEST_LOG", os.devnull), "w"), stderr=subprocess.STDOUT)
fails = 0


def health():
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{PORT}/health", timeout=2) as r:
            return json.loads(r.read())
    except OSError:
        return {}


def wait(cond, what, limit=90):
    t = time.monotonic()
    while time.monotonic() - t < limit:
        h = health()
        if h and cond(h):
            print(f"  ok  {what} ({time.monotonic() - t:.1f} s)")
            return h
        time.sleep(0.5)
    global fails
    fails += 1
    print(f"  FAIL {what}: {health()}")
    return health()


def check(ok, what):
    global fails
    print(f"  {'ok ' if ok else 'FAIL'} {what}")
    fails += 0 if ok else 1


def post(f):
    data = Path(f).read_bytes()
    req = urllib.request.Request(f"http://127.0.0.1:{PORT}/transcribe", data=data, method="POST")
    t = time.monotonic()
    with urllib.request.urlopen(req, timeout=300) as r:
        out = json.loads(r.read())
    out["wall"] = round(time.monotonic() - t, 2)
    return out


def _is_child(pid):
    try:
        return open(f"/proc/{pid}/stat").read().split()[3] == str(srv.pid)
    except OSError:
        return False


try:
    print("online source:")
    wait(lambda h: h.get("warm"), "base loaded and warm")
    time.sleep(2)
    h = health()
    check(h.get("long", {}).get("state") == "unloaded", f"long model not loaded ({h.get('long')})")
    r = post(long_wav)
    check(r.get("hybrid", "").startswith("short"), f"long clip on base while online: {r.get('hybrid')} {r['wall']} s")

    print("offline source (Local):")
    prov.write_text("offline\n")
    wait(lambda h: h.get("long", {}).get("state") == "loading", "long model loading")
    wait(lambda h: h.get("long", {}).get("state") == "ready", "long model ready")
    r = post(short_wav)
    check(r.get("hybrid") == "short", f"short clip -> base: {r['wall']} s, {r['text'][:50]!r}")
    r = post(long_wav)
    check(r.get("hybrid") == "long", f"long clip -> long model: {r['wall']} s, {r['text'][:50]!r}")

    print("back online:")
    prov.write_text("deepgram\n")
    wait(lambda h: h.get("long", {}).get("state") == "unloaded", "long model unloaded")
    time.sleep(1)
    kids = [p for p in os.listdir("/proc") if p.isdigit() and _is_child(p)
            and "server.py" in open(f"/proc/{p}/cmdline").read()]   # not multiprocessing's helper
    check(not kids, "child process gone" + "".join(
        f" [{k}: {open(f'/proc/{k}/cmdline').read().replace(chr(0), ' ')[:80]}]" for k in kids))
finally:
    srv.terminate()
    srv.wait(timeout=20)
print("PASS" if fails == 0 else f"{fails} FAILED")
sys.exit(1 if fails else 0)
