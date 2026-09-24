#!/usr/bin/env python3
"""Cut training clips into <=28 s items Whisper can train on.

Whisper trains on 30 s windows, but 421 labelled clips are longer and their
pseudo-labels have no timestamps. For each long clip we run a baseline
Whisper with word timestamps, align the label's words to the recognised
words (difflib on normalised tokens), and cut only at boundaries where both
neighbouring label words are anchored to recognised words. The label text
(not the Whisper text) is kept for every piece. Clips whose label aligns
poorly (<60 % of label words anchored) are dropped as unreliable.

  python stt/segment.py --model ~/Programs/voice-stt/models/turbo-ct2
Writes data/train_items.jsonl: {"id","clip","start","end","text","gold"}.
Short clips pass through whole.
"""
from __future__ import annotations

import argparse
import difflib
import json
import os
import re
from pathlib import Path

STT_HOME = Path(os.environ.get("STT_HOME", Path.home() / "Programs/voice-stt"))
DATA = STT_HOME / "data"
MAX_SEG = 28.0


def norm(w: str) -> str:
    return re.sub(r"[^a-z0-9]", "", w.lower())


def cut(label: str, words: list[tuple[float, float, str]], dur: float):
    lab = label.split()
    ln = [norm(w) for w in lab]
    hn = [norm(w[2]) for w in words]
    sm = difflib.SequenceMatcher(a=ln, b=hn, autojunk=False)
    anchor: dict[int, tuple[float, float]] = {}
    for blk in sm.get_matching_blocks():
        for k in range(blk.size):
            s, e, _ = words[blk.b + k]
            anchor[blk.a + k] = (s, e)
    if len(anchor) < 0.6 * max(len(lab), 1):
        return None, len(anchor) / max(len(lab), 1)
    # candidate boundaries after label word i: both i and i+1 anchored
    bounds = []
    for i in range(len(lab) - 1):
        if i in anchor and i + 1 in anchor:
            t = (anchor[i][1] + anchor[i + 1][0]) / 2
            gap = anchor[i + 1][0] - anchor[i][1]
            punct = lab[i][-1] in ".?!,;:"
            bounds.append((i, t, gap, punct))
    pieces = []
    start_word, start_t = 0, 0.0
    while True:
        if dur - start_t <= MAX_SEG:
            pieces.append((start_t, dur, " ".join(lab[start_word:])))
            break
        cands = [b for b in bounds if b[0] >= start_word and start_t + 5 < b[1] <= start_t + MAX_SEG]
        if not cands:
            break  # cannot cut safely; drop the rest of this clip
        late = [b for b in cands if b[1] > start_t + MAX_SEG - 10] or cands
        i, t, gap, punct = max(late, key=lambda b: (b[3], b[2]))
        pieces.append((start_t, t, " ".join(lab[start_word:i + 1])))
        start_word, start_t = i + 1, t
    return pieces, len(anchor) / max(len(lab), 1)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True)
    ap.add_argument("--compute", default="int8_float16")
    a = ap.parse_args()
    rows = [json.loads(l) for l in (DATA / "manifest.jsonl").read_text().splitlines() if l.strip()]
    train = [r for r in rows if r["split"] == "train"]
    long_ = [r for r in train if r["dur"] > 30]
    items, dropped, kept_s = [], 0, 0.0
    for r in train:
        if r["dur"] <= 30:
            items.append({"id": r["id"], "clip": r["id"], "start": 0.0, "end": r["dur"],
                          "text": r["text"], "gold": r["gold"]})
    from faster_whisper import WhisperModel
    model = WhisperModel(str(Path(a.model).expanduser()), device="cuda", compute_type=a.compute)
    for n, r in enumerate(long_):
        segs, _ = model.transcribe(str(DATA / "clips" / f"{r['id']}.wav"), language="en",
                                   word_timestamps=True, beam_size=1,
                                   condition_on_previous_text=False)
        words = [(w.start, w.end, w.word) for s in segs for w in (s.words or [])]
        pieces, ratio = cut(r["text"], words, r["dur"])
        if not pieces:
            dropped += 1
            continue
        for k, (s, e, t) in enumerate(pieces):
            items.append({"id": f"{r['id']}_{k}", "clip": r["id"], "start": round(s, 2),
                          "end": round(e, 2), "text": t, "gold": r["gold"]})
            kept_s += e - s
        if n % 25 == 0:
            print(f"{n}/{len(long_)} anchored={ratio:.2f} pieces={len(pieces)}", flush=True)
    (DATA / "train_items.jsonl").write_text("".join(json.dumps(i, ensure_ascii=False) + "\n" for i in items))
    print(f"items={len(items)} hours={sum(i['end']-i['start'] for i in items)/3600:.2f} "
          f"long_clips={len(long_)} dropped={dropped} long_hours_kept={kept_s/3600:.2f}")


if __name__ == "__main__":
    main()
