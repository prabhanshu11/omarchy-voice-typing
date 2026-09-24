#!/usr/bin/env python3
"""Build the local-STT dataset on the desktop from the per-machine inventories.

  inputs : $STT_HOME/raw/inv-laptop.jsonl, $STT_HOME/raw/inv-desktop.jsonl
           (from stt/inventory.py; laptop-only files copied under raw/laptop/)
           $STT_HOME/labels/corrections.jsonl  (optional, from the web app)
  outputs: $STT_HOME/data/clips/<id>.wav  16 kHz mono PCM16
           $STT_HOME/data/manifest.jsonl  one row per unique clip

Split (deterministic, never changes for a given clip):
  test  : Deepgram-era clips, 3-120 s, whose sha1 sorts into the first
          TEST_FRACTION of that pool. References for scoring come from
          AssemblyAI (stt/make_refs.py), so Deepgram can be scored too.
  train : every other clip with an AssemblyAI/Deepgram/recovered label.
  unlabeled : whisper-labelled or unpaired clips (not used for training).
Web-app corrections replace the pseudo-label text and set gold=true.
"""
from __future__ import annotations

import json
import os
import subprocess
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

STT_HOME = Path(os.environ.get("STT_HOME", Path.home() / "Programs/voice-stt"))
RAW = STT_HOME / "raw"
DATA = STT_HOME / "data"
CLIPS = DATA / "clips"
TEST_FRACTION = 0.22  # of the Deepgram pool -> ~120 clips
GOOD = {"assemblyai": 3, "deepgram": 3, "recovered": 2, "local-whisper": 1}


def local_path(row: dict) -> Path:
    p = Path(row["audio"])
    if row["machine"] == "laptop":
        return RAW / "laptop" / str(p).lstrip("/")
    return p


def load_corrections() -> dict[str, dict]:
    """Latest correction per clip id (the web app appends; last one wins)."""
    out: dict[str, dict] = {}
    f = STT_HOME / "labels/corrections.jsonl"
    if f.exists():
        for line in f.read_text().splitlines():
            if line.strip():
                r = json.loads(line)
                out[r["clip_id"]] = r
    return out


def convert(src: Path, dst: Path) -> bool:
    if dst.exists():
        return True
    tmp = dst.with_suffix(".tmp.wav")
    r = subprocess.run(["ffmpeg", "-nostdin", "-loglevel", "error", "-y", "-i", str(src),
                        "-ac", "1", "-ar", "16000", "-sample_fmt", "s16", str(tmp)])
    if r.returncode == 0:
        tmp.rename(dst)
        return True
    return False


def main() -> None:
    CLIPS.mkdir(parents=True, exist_ok=True)
    rows = [json.loads(l) for f in ("inv-desktop.jsonl", "inv-laptop.jsonl")
            for l in (RAW / f).read_text().splitlines() if l.strip()]
    best: dict[str, dict] = {}
    for r in rows:
        if r["dur"] <= 0:
            continue
        k = r["sha1"]
        score = GOOD.get(r.get("backend", ""), 0)
        if k not in best or score > GOOD.get(best[k].get("backend", ""), 0):
            best[k] = r

    # The test set is chosen once and then frozen in data/test_sha1.txt, so
    # clips added later can never move a trained-on clip into the test set.
    frozen = DATA / "test_sha1.txt"
    if frozen.exists():
        test_ids = set(frozen.read_text().split())
    else:
        dg_pool = sorted(k for k, r in best.items()
                         if r.get("backend") == "deepgram" and 3 <= r["dur"] <= 120)
        test_ids = set(dg_pool[: round(len(dg_pool) * TEST_FRACTION)])
        DATA.mkdir(parents=True, exist_ok=True)
        frozen.write_text("\n".join(sorted(test_ids)) + "\n")
    corrections = load_corrections()

    manifest = []
    for k, r in best.items():
        cid = k[:16]
        backend = r.get("backend", "")
        if k in test_ids:
            split = "test"
        elif GOOD.get(backend, 0) >= 2 and r.get("text", "").strip():
            split = "train"
        else:
            split = "unlabeled"
        row = {"id": cid, "sha1": k, "ts": r["ts"], "dur": r["dur"], "sr_orig": r["sr"],
               "machine": r["machine"], "src": str(local_path(r)), "backend": backend,
               "text": r.get("text", ""), "split": split, "gold": False}
        if cid in corrections:
            row["text"] = corrections[cid]["text"]
            row["gold"] = True
            if split == "unlabeled":
                row["split"] = "train"
        manifest.append(row)

    with ThreadPoolExecutor(4) as ex:
        ok = list(ex.map(lambda m: convert(Path(m["src"]), CLIPS / f"{m['id']}.wav"), manifest))
    manifest = [m for m, good in zip(manifest, ok) if good]
    manifest.sort(key=lambda m: m["ts"])
    (DATA / "manifest.jsonl").write_text("".join(json.dumps(m, ensure_ascii=False) + "\n" for m in manifest))

    for s in ("train", "test", "unlabeled"):
        sel = [m for m in manifest if m["split"] == s]
        print(f"{s:10s} n={len(sel):5d}  hours={sum(m['dur'] for m in sel)/3600:.2f}  "
              f"gold={sum(m['gold'] for m in sel)}")
    print(f"failed conversions: {ok.count(False)}")


if __name__ == "__main__":
    main()
