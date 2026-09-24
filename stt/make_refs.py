#!/usr/bin/env python3
"""Reference transcripts for the held-out test set, from a third system.

The test clips were transcribed live by Deepgram nova-2 (streaming). Scoring
Deepgram against its own output is meaningless, so each test clip is sent
once to AssemblyAI (best tier, batch) and that text becomes the reference.
It is still a machine label, not human truth: web-app corrections (gold)
override it in eval_wer.py as they arrive.

  ASSEMBLYAI_API_KEY=... python stt/make_refs.py      # writes data/refs.jsonl
Idempotent: clips already in refs.jsonl are skipped.
"""
from __future__ import annotations

import json
import os
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import requests

STT_HOME = Path(os.environ.get("STT_HOME", Path.home() / "Programs/voice-stt"))
DATA = STT_HOME / "data"
KEY = os.environ["ASSEMBLYAI_API_KEY"]
H = {"authorization": KEY}
BASE = "https://api.assemblyai.com/v2"


def transcribe(path: Path) -> str:
    up = requests.post(f"{BASE}/upload", headers=H, data=path.read_bytes(), timeout=120)
    up.raise_for_status()
    body = {"audio_url": up.json()["upload_url"], "speech_model": "best", "language_code": "en"}
    r = requests.post(f"{BASE}/transcript", headers=H, json=body, timeout=60)
    if r.status_code >= 400:  # older/newer API naming of the model tier
        body.pop("speech_model")
        r = requests.post(f"{BASE}/transcript", headers=H, json=body, timeout=60)
    r.raise_for_status()
    tid = r.json()["id"]
    while True:
        time.sleep(2)
        s = requests.get(f"{BASE}/transcript/{tid}", headers=H, timeout=60).json()
        if s["status"] == "completed":
            return s.get("text") or ""
        if s["status"] == "error":
            raise RuntimeError(s.get("error"))


def main() -> None:
    rows = [json.loads(l) for l in (DATA / "manifest.jsonl").read_text().splitlines()]
    test = [r for r in rows if r["split"] == "test"]
    out = DATA / "refs.jsonl"
    done = set()
    if out.exists():
        done = {json.loads(l)["id"] for l in out.read_text().splitlines() if l.strip()}
    todo = [r for r in test if r["id"] not in done]
    print(f"test={len(test)} done={len(done)} todo={len(todo)}")

    def one(r):
        try:
            return {"id": r["id"], "ref": transcribe(DATA / "clips" / f"{r['id']}.wav"), "ref_source": "assemblyai-best"}
        except Exception as e:  # keep going; rerun picks it up
            print("FAIL", r["id"], e)
            return None

    with ThreadPoolExecutor(6) as ex, open(out, "a") as f:
        for res in ex.map(one, todo):
            if res:
                f.write(json.dumps(res, ensure_ascii=False) + "\n")
                f.flush()
    print("refs:", sum(1 for _ in open(out)))


if __name__ == "__main__":
    main()
