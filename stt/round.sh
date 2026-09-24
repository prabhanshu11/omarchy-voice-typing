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
# Every GPU / heavy-CPU step runs under the star-trek guard (stt/guard.py, rules
# verbatim in its docstring): a fresh 10-min clean baseline before each step, then
# the step runs only while the tracker keeps >= 95 % of the baseline rows/s and
# <= 1.2x its p90 cycle time, >= 3 GB VRAM stays free and /home >= 50 GB. Guarded
# steps that exit 3 (paused out / VRAM breach) are retried after a new baseline;
# training checkpoints every 25 steps and resumes.
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

G="$CODE/guard.py"
# guarded CMD...: run under the guard; retry while it exits 3 (paused out)
guarded() {
  local need="$1"; shift
  local rc
  while true; do
    rc=0; python3 "$G" run --need-gb "$need" -- "$@" || rc=$?
    [[ $rc -eq 3 ]] || return $rc
    echo "guarded step paused out (exit 3): new baseline, then retry" >&2
  done
}

echo "-- 3. segment + train + convert"
guarded 1.5 nice "$V/bin/python" "$CODE/segment.py" --model "$STT/models/turbo-base-ct2"
while true; do  # train_lora.py guards itself (baseline, pause, checkpoint, exit 3)
  rc=0; nice env/bin/python "$CODE/train_lora.py" --out "runs/$NAME" --epochs 2 --resume || rc=$?
  [[ $rc -eq 3 ]] || break
  echo "training paused out (exit 3): resuming after a new baseline"
done
[[ $rc -eq 0 ]] || { echo "training failed (exit $rc)"; exit $rc; }
guarded 0 nice env/bin/python "$CODE/merge_convert.py" --run "runs/$NAME" --name "ft-$NAME"

echo "-- 4. evaluate new vs current (same test set, gold refs where corrected)"
CUR="$(readlink -f models/current || echo "$STT/models/turbo-base-ct2")"
HW=(--hotwords-file labels/vocab.txt)
guarded 1.5 nice "$V/bin/python" "$CODE/eval_wer.py" --model "$CUR" --name "current-before-$NAME" "${HW[@]}" | tail -1 > "runs/$NAME/eval-current.json"
guarded 1.5 nice "$V/bin/python" "$CODE/eval_wer.py" --model "models/ft-$NAME" --name "ft-$NAME+hotwords" "${HW[@]}" | tail -1 > "runs/$NAME/eval-new.json"
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
