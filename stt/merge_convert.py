#!/usr/bin/env python3
"""Merge a LoRA adapter into Whisper turbo and convert it for faster-whisper.

  python merge_convert.py --run ~/Programs/voice-stt/runs/r1 --name ft-r1
-> ~/Programs/voice-stt/models/<name>/  (CTranslate2, float16 weights; serve with
   compute_type int8_float16 on the desktop GPU or int8 on a CPU)
Runs on the CPU so it never touches the shared GPU.
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
from datetime import datetime
from pathlib import Path

import torch
from peft import PeftModel
from transformers import WhisperForConditionalGeneration, WhisperProcessor

STT_HOME = Path(os.environ.get("STT_HOME", Path.home() / "Programs/voice-stt"))
BASE = "openai/whisper-large-v3-turbo"

ap = argparse.ArgumentParser()
ap.add_argument("--run", required=True)
ap.add_argument("--name", required=True)
a = ap.parse_args()
run = Path(a.run).expanduser()
merged = run / "merged"
out = STT_HOME / "models" / a.name

model = WhisperForConditionalGeneration.from_pretrained(BASE, torch_dtype=torch.float32)
model = PeftModel.from_pretrained(model, run / "adapter").merge_and_unload()
model.save_pretrained(merged, safe_serialization=True)
proc = WhisperProcessor.from_pretrained(BASE)
proc.save_pretrained(merged)
if out.exists():
    shutil.rmtree(out)
conv = Path(sys.executable).with_name("ct2-transformers-converter")
subprocess.run([str(conv), "--model", str(merged), "--output_dir", str(out), "--quantization", "float16",
                "--copy_files", "tokenizer.json", "preprocessor_config.json"], check=True)
(out / "provenance.json").write_text(json.dumps({
    "base": BASE, "adapter": str(run / "adapter"), "train_log": str(run / "train.log"),
    "created": datetime.now().isoformat(timespec="seconds"),
    "pipeline": "omarchy-voice-typing/stt/merge_convert.py"}, indent=2))
shutil.rmtree(merged)
print("wrote", out)
