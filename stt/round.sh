#!/bin/bash
# round.sh — one local-STT training round, run ON THE DESKTOP.
#
#   ~/Programs/voice-stt/code/round.sh [--name rN] [--dry-run]
#
# 1. pulls the laptop's inventory, new laptop audio, and the user's labels
#    (corrections.jsonl + vocab.txt from the voice-labels web app)
# 2. rebuilds the dataset (corrections become gold labels; test set frozen)
# 3. segments long clips, trains a LoRA round, merges + converts it
# 4. evaluates it against the model currently served, on the same test set
# 5. promotes it (models/current -> new) ONLY if its WER is not worse
# Every GPU step waits for the star-trek guard (stt/guard.py: tracker >= 1.5
# cycles/s over 5 min and /home >= 50 GB free); training pauses itself below
# that, checkpoints every 25 steps, and after 30 min of pausing exits (code 3)
# to free the VRAM -- this script then waits and resumes it.
# The desktop's local-whisper is stopped while training (frees ~1 GB VRAM for
# the shared GPU; the laptop gateway falls back to its own slow Whisper) and
# restarted at the end whatever happens.
# Docs: omarchy-voice-typing/docs/local-stt.md
set -euo pipefail
STT="$HOME/Programs/voice-stt"
CODE="$STT/code"
V="$HOME/Programs/omarchy-voice-typing/local-whisper/.venv"
export LD_LIBRARY_PATH="$V/lib/python3.13/site-packages/nvidia/cublas/lib:$V/lib/python3.13/site-packages/nvidia/cudnn/lib"
NAME="r$(date +%Y%m%d)"
DRY=false
while [[ $# -gt 0 ]]; do
  case "$1" in --name) NAME="$2"; shift 2 ;; --dry-run) DRY=true; shift ;; *) echo "bad arg $1"; exit 2 ;; esac
done
cd "$STT"
LOG="$STT/runs/round-$NAME.log"
mkdir -p runs
exec > >(tee -a "$LOG") 2>&1
echo "== round $NAME $(date -Is)"

echo "-- 1. pull from the laptop"
ssh -o BatchMode=yes laptop 'python3 - --machine laptop' < "$CODE/inventory.py" > raw/inv-laptop.jsonl.new \
  && mv raw/inv-laptop.jsonl.new raw/inv-laptop.jsonl
python3 "$CODE/inventory.py" --machine desktop > raw/inv-desktop.jsonl
python3 - <<'EOF' > raw/laptop-fetch.txt
import json, os
from pathlib import Path
raw = Path.home() / "Programs/voice-stt/raw"
have = {json.loads(l)["sha1"] for l in open(raw / "inv-desktop.jsonl")}
for l in open(raw / "inv-laptop.jsonl"):
    r = json.loads(l)
    for p in (r["audio"], r.get("transcript")):
        if p and r["sha1"] not in have and not (raw / "laptop" / p.lstrip("/")).exists():
            print(p)
EOF
if [[ -s raw/laptop-fetch.txt ]]; then
  echo "fetching $(wc -l < raw/laptop-fetch.txt) files from the laptop"
  ssh -o BatchMode=yes laptop 'cd / && tar cf - -T -' < <(sed 's|^/||' raw/laptop-fetch.txt) | tar xf - -C raw/laptop
fi
mkdir -p labels
scp -q -o BatchMode=yes 'laptop:Programs/voice-stt/labels/*' labels/ || echo "(no labels on the laptop yet)"

echo "-- 2. dataset"
python3 "$CODE/build_dataset.py"
set -a; [[ -f .aai.env ]] && . ./.aai.env; set +a
uv run -q --no-project --with requests python "$CODE/make_refs.py"
$DRY && { echo "dry run: stopping before GPU work"; exit 0; }

restart_server() { systemctl --user start local-whisper || true; }
trap restart_server EXIT
systemctl --user stop local-whisper || true
sleep 2

gate() { python3 "$CODE/guard.py" wait; }

echo "-- 3. segment + train + convert"
gate
nice "$V/bin/python" "$CODE/segment.py" --model "$STT/models/turbo-base-ct2"
until gate && nice env/bin/python "$CODE/train_lora.py" --out "runs/$NAME" --epochs 2 --bs 2 --accum 8 \
        --vram-gb 3.5 --resume; do
  rc=$?
  [[ $rc -eq 3 ]] || { echo "training failed (exit $rc)"; exit $rc; }
  echo "training paused out (exit 3): waiting for the tracker, then resuming"
done
gate
nice env/bin/python "$CODE/merge_convert.py" --run "runs/$NAME" --name "ft-$NAME"

echo "-- 4. evaluate new vs current (same test set, gold refs where corrected)"
CUR="$(readlink -f models/current || echo "$STT/models/turbo-base-ct2")"
HW=(--hotwords-file labels/vocab.txt)
gate
nice "$V/bin/python" "$CODE/eval_wer.py" --model "$CUR" --name "current-before-$NAME" "${HW[@]}" | tail -1 > "runs/$NAME/eval-current.json"
gate
nice "$V/bin/python" "$CODE/eval_wer.py" --model "models/ft-$NAME" --name "ft-$NAME+hotwords" "${HW[@]}" | tail -1 > "runs/$NAME/eval-new.json"
old=$(python3 -c "import json;print(json.load(open('runs/$NAME/eval-current.json'))['wer'])")
new=$(python3 -c "import json;print(json.load(open('runs/$NAME/eval-new.json'))['wer'])")
echo "WER current=$old new=$new"

echo "-- 5. promote?"
if python3 -c "import sys; sys.exit(0 if $new <= $old else 1)"; then
  ln -sfn "ft-$NAME" models/current
  echo "PROMOTED models/current -> ft-$NAME"
else
  echo "kept current (new model is worse)"
fi
