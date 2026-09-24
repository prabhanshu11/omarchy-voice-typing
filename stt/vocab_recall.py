#!/usr/bin/env python3
"""How often each system gets the words-to-concentrate-on right.

For every eval run in runs/eval/*.jsonl: over test clips whose reference
contains a vocab word/phrase (normalised), the fraction whose hypothesis
contains it too. Prints one line per run.
  python stt/vocab_recall.py [--vocab labels/vocab.txt]
"""
import argparse
import json
import os
import re
from pathlib import Path

STT_HOME = Path(os.environ.get("STT_HOME", Path.home() / "Programs/voice-stt"))


def norm(t: str) -> str:
    t = re.sub(r"[‘’']", "", t.lower())
    return " " + " ".join(re.sub(r"[^a-z0-9]+", " ", t).split()) + " "


ap = argparse.ArgumentParser()
ap.add_argument("--vocab", default=str(STT_HOME / "labels/vocab.txt"))
a = ap.parse_args()
vocab = [w.strip() for w in Path(a.vocab).read_text().splitlines() if w.strip() and not w.startswith("#")]
keys = {w: norm(w) for w in vocab}
for f in sorted((STT_HOME / "runs/eval").glob("*.jsonl")):
    if f.name == "summary.jsonl":
        continue
    hit = tot = 0
    missed = {}
    for line in f.read_text().splitlines():
        r = json.loads(line)
        ref, hyp = norm(r["ref"]), norm(r["hyp"])
        for w, k in keys.items():
            if k in ref:
                tot += 1
                if k in hyp:
                    hit += 1
                else:
                    missed[w] = missed.get(w, 0) + 1
    print(f"{f.stem:32s} vocab occurrences={tot:3d} recalled={hit:3d} "
          f"({hit / max(tot, 1):.0%})  missed={dict(sorted(missed.items(), key=lambda x: -x[1]))}")
