#!/usr/bin/env python3
"""Word error rate on the held-out test set.

Runs with any python that has faster-whisper (e.g. local-whisper/.venv):
  python stt/eval_wer.py --system deepgram
  python stt/eval_wer.py --model large-v3-turbo --name turbo-base
  python stt/eval_wer.py --model ~/Programs/voice-stt/models/ft-r1-ct2 --name ft-r1 --hotwords-file vocab.txt

Reference per clip: web-app gold correction if present, else the AssemblyAI
reference from make_refs.py. Text is normalised identically for every system
(lowercase, punctuation stripped, hyphens split). WER is corpus-level:
sum(substitutions+deletions+insertions) / sum(reference words).
Writes runs/eval/<name>.jsonl (per clip) and appends a line to runs/eval/summary.jsonl.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import time
from datetime import datetime
from pathlib import Path

STT_HOME = Path(os.environ.get("STT_HOME", Path.home() / "Programs/voice-stt"))
DATA = STT_HOME / "data"
OUT = STT_HOME / "runs/eval"


def normalize(t: str) -> list[str]:
    t = t.lower().replace("%", " percent").replace("&", " and ")
    t = re.sub(r"[‘’']", "", t)
    t = re.sub(r"[^a-z0-9 ]+", " ", t)
    return t.split()


def edits(ref: list[str], hyp: list[str]) -> int:
    prev = list(range(len(hyp) + 1))
    for i, r in enumerate(ref, 1):
        cur = [i] + [0] * len(hyp)
        for j, h in enumerate(hyp, 1):
            cur[j] = min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (r != h))
        prev = cur
    return prev[-1]


def load_test() -> list[dict]:
    rows = [json.loads(l) for l in (DATA / "manifest.jsonl").read_text().splitlines() if l.strip()]
    refs = {json.loads(l)["id"]: json.loads(l) for l in (DATA / "refs.jsonl").read_text().splitlines() if l.strip()}
    gold = {}
    cf = STT_HOME / "labels/corrections.jsonl"
    if cf.exists():
        for l in cf.read_text().splitlines():
            if l.strip():
                r = json.loads(l)
                gold[r["clip_id"]] = r["text"]
    test = []
    for r in rows:
        if r["split"] != "test" or r["id"] not in refs:
            continue
        r = dict(r)
        r["ref"] = gold.get(r["id"], refs[r["id"]]["ref"])
        r["ref_source"] = "gold" if r["id"] in gold else refs[r["id"]]["ref_source"]
        test.append(r)
    return test


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--system", default="whisper", choices=["whisper", "deepgram"])
    ap.add_argument("--model", default="large-v3-turbo")
    ap.add_argument("--name")
    ap.add_argument("--device", default="cuda")
    ap.add_argument("--compute", default="int8_float16")
    ap.add_argument("--beam", type=int, default=5)
    ap.add_argument("--hotwords-file")
    ap.add_argument("--limit", type=int)
    a = ap.parse_args()
    name = a.name or ("deepgram-nova2-stream" if a.system == "deepgram" else Path(a.model).name)

    test = load_test()[: a.limit]
    hotwords = None
    if a.hotwords_file:
        words = [w.strip() for w in Path(a.hotwords_file).expanduser().read_text().splitlines()
                 if w.strip() and not w.startswith("#")]
        hotwords = ", ".join(words)

    model = None
    if a.system == "whisper":
        from faster_whisper import WhisperModel
        t0 = time.perf_counter()
        model = WhisperModel(str(Path(a.model).expanduser()) if "/" in a.model else a.model,
                             device=a.device, compute_type=a.compute)
        print(f"loaded {a.model} in {time.perf_counter()-t0:.1f}s")
        import numpy as np
        model.transcribe(np.zeros(16000, dtype=np.float32), language="en")  # warm-up

    OUT.mkdir(parents=True, exist_ok=True)
    tot_e = tot_w = 0
    lat = []
    audio_s = 0.0
    with open(OUT / f"{name}.jsonl", "w") as f:
        for r in test:
            if a.system == "deepgram":
                hyp, dt = r["text"], None
            else:
                t0 = time.perf_counter()
                segs, _ = model.transcribe(str(DATA / "clips" / f"{r['id']}.wav"), language="en",
                                           beam_size=a.beam, hotwords=hotwords,
                                           condition_on_previous_text=False)
                hyp = " ".join(s.text.strip() for s in segs).strip()
                dt = time.perf_counter() - t0
                lat.append(dt)
                audio_s += r["dur"]
            ref_w, hyp_w = normalize(r["ref"]), normalize(hyp)
            e = edits(ref_w, hyp_w)
            tot_e += e
            tot_w += len(ref_w)
            f.write(json.dumps({"id": r["id"], "dur": r["dur"], "ref": r["ref"], "hyp": hyp,
                                "errors": e, "ref_words": len(ref_w), "secs": dt}, ensure_ascii=False) + "\n")
    wer = tot_e / max(tot_w, 1)
    summary = {"name": name, "model": a.model if a.system == "whisper" else "deepgram", "n": len(test),
               "ref_words": tot_w, "wer": round(wer, 4), "hotwords": bool(hotwords),
               "compute": a.compute if model else None, "device": a.device if model else None,
               "mean_latency_s": round(sum(lat) / len(lat), 3) if lat else None,
               "rtf": round(sum(lat) / audio_s, 4) if lat else None,
               "gold_refs": sum(r["ref_source"] == "gold" for r in test),
               "when": datetime.now().isoformat(timespec="seconds")}
    with open(OUT / "summary.jsonl", "a") as f:
        f.write(json.dumps(summary) + "\n")
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
