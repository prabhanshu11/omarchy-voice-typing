#!/usr/bin/env python3
"""Inventory voice-typing audio + transcript pairs on THIS machine.

Stdlib only, so it runs anywhere (laptop, desktop) with plain python3.

Scans:
  ~/Programs/recordings/                         (Rust gateway era, Feb 2026 ->)
  ~/Programs/transcripts/
  ~/Programs/omarchy-voice-typing/recordings/    (Go gateway era, Jan 2026)
  ~/Programs/omarchy-voice-typing/transcripts/

Pairing rules
  * Rust era: transcript  <TS>_<backend>.txt  <->  <TS>_audio.wav  (same TS)
  * Go era (AssemblyAI):   <TS2>_<uuid>.txt   <->  nearest <TS>_audio.wav with
    TS <= TS2 <= TS + 180 s that is not already paired.
  * Files named <TS>_<TS>_..._audio.wav are orphan-recovery re-archives of an
    older clip (a cascade bug, see docs/local-stt.md); counted, never used.

Writes one JSON line per audio file (paired or not) to stdout, e.g.
  python3 stt/inventory.py --machine laptop > /tmp/inv-laptop.jsonl
and a summary to stderr.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import wave
from datetime import datetime
from pathlib import Path

HOME = Path.home()
AUDIO_DIRS = [HOME / "Programs/recordings", HOME / "Programs/omarchy-voice-typing/recordings"]
TEXT_DIRS = [HOME / "Programs/transcripts", HOME / "Programs/omarchy-voice-typing/transcripts"]

RE_AUDIO = re.compile(r"^(\d{8}_\d{6})_audio\.wav$")
RE_CASCADE = re.compile(r"^(\d{8}_\d{6}_){2,}audio\.wav$")
RE_TEXT = re.compile(r"^(\d{8}_\d{6})_(.+)\.txt$")
UUID = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")


def ts(s: str) -> datetime:
    return datetime.strptime(s, "%Y%m%d_%H%M%S")


def wav_info(p: Path) -> tuple[float, int, int]:
    try:
        with wave.open(str(p), "rb") as w:
            return w.getnframes() / w.getframerate(), w.getframerate(), w.getnchannels()
    except Exception:
        return -1.0, 0, 0


def sha1(p: Path) -> str:
    h = hashlib.sha1()
    with open(p, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--machine", required=True)
    ap.add_argument("--no-hash", action="store_true")
    a = ap.parse_args()

    audio: dict[str, list[Path]] = {}
    cascade = 0
    for d in AUDIO_DIRS:
        if not d.is_dir():
            continue
        for p in d.iterdir():
            if RE_CASCADE.match(p.name):
                cascade += 1
                continue
            m = RE_AUDIO.match(p.name)
            if m:
                audio.setdefault(m.group(1), []).append(p)

    texts: list[tuple[str, str, Path]] = []
    for d in TEXT_DIRS:
        if not d.is_dir():
            continue
        for p in d.iterdir():
            m = RE_TEXT.match(p.name)
            if m:
                texts.append((m.group(1), m.group(2), p))

    pairs: dict[Path, tuple[Path, str]] = {}
    # exact timestamp matches first
    for t, backend, p in texts:
        if t in audio and not UUID.match(backend):
            for ap_ in audio[t]:
                pairs.setdefault(ap_, (p, backend))
    # Go-era AssemblyAI: transcript written a few seconds after the audio
    audio_sorted = sorted(((ts(k), ap_) for k, v in audio.items() for ap_ in v), key=lambda x: x[0])
    for t, backend, p in sorted(texts):
        if not UUID.match(backend):
            continue
        tt = ts(t)
        best = None
        for at, ap_ in audio_sorted:
            if at > tt:
                break
            if (tt - at).total_seconds() <= 180 and ap_ not in pairs:
                best = ap_
        if best is not None:
            pairs[best] = (p, "assemblyai")

    n = paired = 0
    hours = paired_hours = 0.0
    for k, paths in sorted(audio.items()):
        for p in paths:
            dur, sr, ch = wav_info(p)
            rec = {"machine": a.machine, "audio": str(p), "ts": k, "dur": round(dur, 2),
                   "sr": sr, "ch": ch, "bytes": p.stat().st_size}
            if not a.no_hash:
                rec["sha1"] = sha1(p)
            if p in pairs:
                tp, backend = pairs[p]
                rec["transcript"] = str(tp)
                rec["backend"] = backend
                rec["text"] = tp.read_text(errors="replace").strip()
                paired += 1
                paired_hours += max(dur, 0) / 3600
            n += 1
            hours += max(dur, 0) / 3600
            print(json.dumps(rec, ensure_ascii=False))
    print(f"[{a.machine}] audio={n} ({hours:.2f} h)  paired={paired} ({paired_hours:.2f} h)  "
          f"transcripts={len(texts)}  orphan-cascade-files={cascade}", file=sys.stderr)


if __name__ == "__main__":
    main()
