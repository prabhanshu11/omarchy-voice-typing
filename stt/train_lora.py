#!/usr/bin/env python3
"""LoRA fine-tune of Whisper large-v3-turbo on the user's own voice.

Runs on the desktop GPU (RTX 2060 SUPER, 8 GB) which is SHARED with the live
star-trek-camera service. Etiquette built in:
  * VRAM cap for this process (--vram-gb): checked after every micro-batch;
    above it we free the cache and, if still above, stop (we fail, they never
    do). (torch's set_per_process_memory_fraction needs NVML, which is broken
    on the desktop until it reboots into the new NVIDIA driver.)
  * refuses to start if free VRAM < cap + 1.5 GB headroom;
  * duty cycle: sleeps --idle-frac of each step's time so their kernels
    get the GPU; and it watches star-trek's pose latency (localhost:8100
    /situation) and backs off harder when it rises above 2x its baseline.

Data: data/train_items.jsonl (stt/segment.py) with gold corrections applied.
Word-list training: with probability --prompt-p an item is trained with a
<|startofprev|> prompt made of words from the concentrate-on list that occur
in its text (plus distractors), so the model learns to use hotwords/prompts.

  .venv/bin/python train_lora.py --out ~/Programs/voice-stt/runs/r1
Then merge_convert.py -> CTranslate2 model for faster-whisper.
"""
from __future__ import annotations

import argparse
import json
import math
import os
import random
import time
import urllib.request
from pathlib import Path

import numpy as np
import soundfile as sf
import torch
from peft import LoraConfig, get_peft_model
from transformers import WhisperForConditionalGeneration, WhisperProcessor

STT_HOME = Path(os.environ.get("STT_HOME", Path.home() / "Programs/voice-stt"))
DATA = STT_HOME / "data"
BASE = "openai/whisper-large-v3-turbo"


def star_trek_latency() -> float | None:
    try:
        with urllib.request.urlopen("http://127.0.0.1:8100/situation", timeout=2) as r:
            d = json.load(r)
        return float(d["sees"]["measured_pose"]["latency_s"])
    except Exception:
        return None


def load_items(vocab: list[str]):
    items = [json.loads(l) for l in (DATA / "train_items.jsonl").read_text().splitlines() if l.strip()]
    gold = {}
    cf = STT_HOME / "labels/corrections.jsonl"
    if cf.exists():
        for l in cf.read_text().splitlines():
            if l.strip():
                r = json.loads(l)
                gold[r["clip_id"]] = r["text"]
    for it in items:  # a correction of a whole short clip replaces its text
        if it["clip"] in gold and it["start"] == 0.0 and "_" not in it["id"]:
            it["text"], it["gold"] = gold[it["clip"]], True
    return items


class Batcher:
    def __init__(self, items, proc: WhisperProcessor, vocab: list[str], prompt_p: float):
        self.items, self.proc, self.vocab, self.prompt_p = items, proc, vocab, prompt_p
        self.tok = proc.tokenizer
        self.sot = self.tok.convert_tokens_to_ids("<|startoftranscript|>")
        self.prev = self.tok.convert_tokens_to_ids("<|startofprev|>")
        self.head = [self.sot] + [self.tok.convert_tokens_to_ids(t)
                                  for t in ("<|en|>", "<|transcribe|>", "<|notimestamps|>")]
        self.eot = self.tok.eos_token_id
        self.cache: dict[str, np.ndarray] = {}

    def audio(self, it) -> np.ndarray:
        clip = it["clip"]
        if clip not in self.cache:
            if len(self.cache) > 64:
                self.cache.pop(next(iter(self.cache)))
            a, sr = sf.read(DATA / "clips" / f"{clip}.wav", dtype="float32")
            self.cache[clip] = a
        a = self.cache[clip]
        return a[int(it["start"] * 16000): int(it["end"] * 16000)]

    def prompt_ids(self, text: str) -> list[int]:
        if not self.vocab or random.random() > self.prompt_p:
            return []
        low = text.lower()
        hits = [w for w in self.vocab if w.lower() in low]
        others = random.sample(self.vocab, min(len(self.vocab), random.randint(2, 8)))
        words = list(dict.fromkeys(hits + others))
        random.shuffle(words)
        return [self.prev] + self.tok.encode(" " + ", ".join(words), add_special_tokens=False)[:100]

    def __call__(self, batch):
        feats = self.proc.feature_extractor([self.audio(it) for it in batch], sampling_rate=16000,
                                            return_tensors="pt").input_features
        seqs, masks = [], []
        for it in batch:
            p = self.prompt_ids(it["text"])
            body = self.tok.encode(" " + it["text"].strip(), add_special_tokens=False)[:400]
            s = p + self.head + body + [self.eot]
            seqs.append(s)
            masks.append(len(p) + len(self.head) - 1)  # targets before this index are not scored
        L = max(len(s) for s in seqs) - 1
        dec = torch.full((len(seqs), L), self.eot, dtype=torch.long)
        lab = torch.full((len(seqs), L), -100, dtype=torch.long)
        for i, (s, m) in enumerate(zip(seqs, masks)):
            n = len(s) - 1
            dec[i, :n] = torch.tensor(s[:-1])
            tgt = torch.tensor(s[1:])
            tgt[:m] = -100
            lab[i, :n] = tgt
        return feats, dec, lab


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--epochs", type=float, default=2.0)
    ap.add_argument("--bs", type=int, default=4)
    ap.add_argument("--accum", type=int, default=4)
    ap.add_argument("--lr", type=float, default=5e-4)
    ap.add_argument("--rank", type=int, default=32)
    ap.add_argument("--vram-gb", type=float, default=3.5)
    ap.add_argument("--idle-frac", type=float, default=0.25)
    ap.add_argument("--prompt-p", type=float, default=0.3)
    ap.add_argument("--vocab", default=str(STT_HOME / "labels/vocab.txt"))
    ap.add_argument("--max-steps", type=int, default=0)
    a = ap.parse_args()
    out = Path(a.out).expanduser()
    out.mkdir(parents=True, exist_ok=True)
    log = open(out / "train.log", "a")

    def say(msg):
        line = f"[{time.strftime('%H:%M:%S')}] {msg}"
        print(line, flush=True)
        log.write(line + "\n")
        log.flush()

    free, total = torch.cuda.mem_get_info()
    say(f"GPU free {free/1e9:.2f} / {total/1e9:.2f} GB; cap {a.vram_gb} GB")
    if free / 1e9 < a.vram_gb + 1.5:
        raise SystemExit("not enough free VRAM for cap + 1.5 GB headroom; GPU busy -- not starting")
    cap = a.vram_gb * 1e9

    vocab = []
    vp = Path(a.vocab)
    if vp.exists():
        vocab = [w.strip() for w in vp.read_text().splitlines() if w.strip() and not w.startswith("#")]
    items = load_items(vocab)
    random.seed(0)
    random.shuffle(items)
    say(f"items={len(items)} hours={sum(i['end']-i['start'] for i in items)/3600:.2f} vocab={len(vocab)} "
        f"gold={sum(i['gold'] for i in items)}")

    proc = WhisperProcessor.from_pretrained(BASE)
    model = WhisperForConditionalGeneration.from_pretrained(BASE, torch_dtype=torch.float16)
    model.config.forced_decoder_ids = None
    model.gradient_checkpointing_enable(gradient_checkpointing_kwargs={"use_reentrant": False})
    model.enable_input_require_grads()
    cfg = LoraConfig(r=a.rank, lora_alpha=2 * a.rank, lora_dropout=0.05,
                     target_modules=["q_proj", "k_proj", "v_proj", "out_proj", "fc1", "fc2"])
    model = get_peft_model(model, cfg)
    for n, p in model.named_parameters():
        if p.requires_grad:
            p.data = p.data.float()
    model.cuda()
    model.print_trainable_parameters()

    batcher = Batcher(items, proc, vocab, a.prompt_p)
    params = [p for p in model.parameters() if p.requires_grad]
    opt = torch.optim.AdamW(params, lr=a.lr, weight_decay=0.01)
    steps_per_epoch = math.ceil(len(items) / (a.bs * a.accum))
    total_steps = a.max_steps or int(steps_per_epoch * a.epochs)
    warm = max(10, total_steps // 20)
    sched = torch.optim.lr_scheduler.LambdaLR(
        opt, lambda s: min(1.0, (s + 1) / warm) * max(0.05, 0.5 * (1 + math.cos(math.pi * s / total_steps))))
    scaler = torch.amp.GradScaler("cuda")

    base_lat = [x for x in (star_trek_latency() for _ in range(5)) if x is not None]
    base_lat = float(np.median(base_lat)) if base_lat else None
    say(f"star-trek pose latency baseline: {base_lat}")
    idle = a.idle_frac

    step, i, t_last = 0, 0, time.time()
    model.train()
    losses = []
    while step < total_steps:
        t0 = time.time()
        for _ in range(a.accum):
            if i + a.bs > len(items):
                random.shuffle(items)
                i = 0
            feats, dec, lab = batcher(items[i:i + a.bs])
            i += a.bs
            with torch.autocast("cuda", dtype=torch.float16):
                loss = model(input_features=feats.cuda().half(), decoder_input_ids=dec.cuda(),
                             labels=lab.cuda()).loss / a.accum
            scaler.scale(loss).backward()
            losses.append(loss.item() * a.accum)
            del loss
            if torch.cuda.memory_reserved() > cap:
                torch.cuda.empty_cache()
                if torch.cuda.memory_reserved() > cap:
                    model.save_pretrained(out / "adapter")
                    raise SystemExit(f"VRAM {torch.cuda.memory_reserved()/1e9:.2f} GB > cap {a.vram_gb} GB; "
                                     "stopped (adapter saved) -- lower --bs")
        scaler.unscale_(opt)
        torch.nn.utils.clip_grad_norm_(params, 1.0)
        scaler.step(opt)
        scaler.update()
        opt.zero_grad(set_to_none=True)
        sched.step()
        step += 1
        busy = time.time() - t0
        if step % 10 == 0 or step <= 3:
            lat = star_trek_latency()
            if base_lat and lat and lat > 2 * max(base_lat, 0.02):
                idle = min(idle * 1.5, 3.0)
            elif idle > a.idle_frac:
                idle = max(a.idle_frac, idle / 1.2)
            say(f"step {step}/{total_steps} loss {np.mean(losses[-10*a.accum:]):.4f} "
                f"lr {sched.get_last_lr()[0]:.2e} step_s {busy:.1f} idle {idle:.2f} "
                f"st_lat {lat} peak_alloc {torch.cuda.max_memory_allocated()/1e9:.2f}GB "
                f"reserved {torch.cuda.memory_reserved()/1e9:.2f}GB")
        time.sleep(busy * idle)
        if step % 200 == 0 or step == total_steps:
            model.save_pretrained(out / "adapter")
            say(f"saved adapter at step {step}")
    (out / "done").write_text(json.dumps({"steps": step, "items": len(items), "vocab": len(vocab),
                                          "elapsed_s": time.time() - t_last}))
    say("done")


if __name__ == "__main__":
    main()
