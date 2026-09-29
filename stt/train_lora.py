#!/usr/bin/env python3
"""LoRA fine-tune of Whisper large-v3-turbo on the user's own voice.

Runs on the desktop GPU (RTX 2060 SUPER, 8 GB) which is SHARED with the live
star-trek-camera tracker. Etiquette built in (rules: docs/HANDOVER-local-stt-2026-09-25.md):
  * star-trek guard (stt/guard.py; its docstring holds the camera session's rules
    verbatim): before loading anything it measures a baseline over 10 contiguous
    clean minutes (load < 8, no other heavy job, tracker running). It runs only
    while the 5-min tracker rows/s >= 95 % of that baseline, the 5-min p90 of
    cycle_ms <= 1.2x the baseline p90, device free VRAM >= 3 GB (read with
    torch.cuda.mem_get_info) and /home >= 50 GB free. Checked every <= 30 s,
    also between micro-batches and during the duty-cycle sleep, so a breach
    pauses within 30 s. A VRAM breach that emptying our cache does not fix, or
    pausing longer than --max-pause-min, checkpoints and exits 3 (frees the
    VRAM); rerun with --resume (a fresh baseline is taken first).
  * checkpoint (adapter + optimizer + data position) every --save-every
    steps, written atomically; --resume continues from it.
  * nice 19 + ionice idle, --threads CPU threads, and a duty cycle: sleeps
    --idle-frac of each step's time so their kernels get the GPU.
  * VRAM cap for this process (--vram-gb): checked after every micro-batch;
    above it we free the cache and, if still above, stop (we fail, they never
    do). (torch's set_per_process_memory_fraction needs NVML, which is broken
    on the desktop until it reboots into the new NVIDIA driver.)
  * refuses to start if free VRAM < cap + 1.5 GB headroom.

Data: data/train_items.jsonl (stt/segment.py) with gold corrections applied.
Word-list training: with probability --prompt-p an item is trained with a
<|startofprev|> prompt made of words from the concentrate-on list that occur
in its text (plus distractors), so the model learns to use hotwords/prompts.

  env/bin/python code/train_lora.py --out runs/r1 [--resume]
Then merge_convert.py -> CTranslate2 model for faster-whisper.
"""
from __future__ import annotations

import argparse
import json
import math
import os
import random
import shutil
import subprocess
import sys
import time
from pathlib import Path

import numpy as np
import soundfile as sf
import torch
from peft import LoraConfig, get_peft_model
from transformers import WhisperForConditionalGeneration, WhisperProcessor

sys.path.insert(0, str(Path(__file__).resolve().parent))
import guard  # noqa: E402  (stt/guard.py: star-trek rules)

STT_HOME = Path(os.environ.get("STT_HOME", Path.home() / "Programs/voice-stt"))
DATA = STT_HOME / "data"
BASE = "openai/whisper-large-v3-turbo"


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
    ap.add_argument("--bs", type=int, default=1)
    ap.add_argument("--accum", type=int, default=16)
    ap.add_argument("--lr", type=float, default=5e-4)
    ap.add_argument("--rank", type=int, default=16)
    ap.add_argument("--quant", choices=["4bit", "8bit", "none"], default="4bit",
                    help="base weights: 4bit NF4 (QLoRA, ~0.5 GB) so >= 3 GB stays free for star-trek")
    ap.add_argument("--vram-gb", type=float, default=1.9,
                    help="cap on our reserved VRAM; start needs device free >= cap + 3 GB")
    ap.add_argument("--idle-frac", type=float, default=1.0,
                    help="sleep this fraction of each step's GPU time (1.0 = 50%% duty)")
    ap.add_argument("--threads", type=int, default=2, help="torch CPU threads")
    ap.add_argument("--save-every", type=int, default=25)
    ap.add_argument("--resume", action="store_true", help="continue from <out>/ckpt")
    ap.add_argument("--max-pause-min", type=float, default=30,
                    help="checkpoint and exit (code 3) after pausing this long")
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

    os.nice(19)
    subprocess.run(["ionice", "-c", "3", "-p", str(os.getpid())], check=False)
    torch.set_num_threads(a.threads)
    if guard.too_early():
        say("before 10:00 IST on 2026-09-25 -- not starting (exit 3)")
        sys.exit(3)
    g = guard.Guard(say)
    say("measuring the star-trek baseline (10 clean minutes) before touching the GPU")
    g.measure_baseline(own={os.getpid()})
    free, total = torch.cuda.mem_get_info()
    ok, _, msg = g.check(vram_free=free / 1e9)
    say(f"guard at start: {msg}; GPU free {free/1e9:.2f} / {total/1e9:.2f} GB; cap {a.vram_gb} GB")
    if not ok or free / 1e9 < a.vram_gb + guard.MIN_VRAM_GB:
        say("guard says no (or free VRAM < cap + 3 GB) -- not starting (exit 3)")
        sys.exit(3)
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
    if a.quant == "none":
        model = WhisperForConditionalGeneration.from_pretrained(BASE, torch_dtype=torch.float16)
        model.gradient_checkpointing_enable(gradient_checkpointing_kwargs={"use_reentrant": False})
        model.enable_input_require_grads()
    else:
        from peft import prepare_model_for_kbit_training
        from transformers import BitsAndBytesConfig
        q = (BitsAndBytesConfig(load_in_4bit=True, bnb_4bit_quant_type="nf4", bnb_4bit_use_double_quant=True,
                                bnb_4bit_compute_dtype=torch.float16)
             if a.quant == "4bit" else BitsAndBytesConfig(load_in_8bit=True))
        model = WhisperForConditionalGeneration.from_pretrained(BASE, torch_dtype=torch.float16,
                                                                quantization_config=q, device_map={"": 0})
        model = prepare_model_for_kbit_training(
            model, use_gradient_checkpointing=True, gradient_checkpointing_kwargs={"use_reentrant": False})
    model.config.forced_decoder_ids = None
    cfg = LoraConfig(r=a.rank, lora_alpha=2 * a.rank, lora_dropout=0.05,
                     target_modules=["q_proj", "k_proj", "v_proj", "out_proj", "fc1", "fc2"])
    model = get_peft_model(model, cfg)
    for n, p in model.named_parameters():
        if p.requires_grad:
            p.data = p.data.float()
    if a.quant == "none":
        model.cuda()
    model.print_trainable_parameters()
    say(f"base weights {a.quant}; after load: reserved {torch.cuda.memory_reserved()/1e9:.2f} GB, "
        f"device free {torch.cuda.mem_get_info()[0]/1e9:.2f} GB")

    batcher = Batcher(items, proc, vocab, a.prompt_p)
    params = [p for p in model.parameters() if p.requires_grad]
    opt = torch.optim.AdamW(params, lr=a.lr, weight_decay=0.01)
    steps_per_epoch = math.ceil(len(items) / (a.bs * a.accum))
    total_steps = a.max_steps or int(steps_per_epoch * a.epochs)
    warm = max(10, total_steps // 20)
    sched = torch.optim.lr_scheduler.LambdaLR(
        opt, lambda s: min(1.0, (s + 1) / warm) * max(0.05, 0.5 * (1 + math.cos(math.pi * s / total_steps))))
    scaler = torch.amp.GradScaler("cuda")

    ckpt = out / "ckpt"
    trainable = [n for n, p in model.named_parameters() if p.requires_grad]

    def save_ckpt(step: int, i: int) -> None:
        if guard.home_free_gb() < guard.MIN_HOME_GB:
            say(f"/home free {guard.home_free_gb():.0f} GB < {guard.MIN_HOME_GB:.0f}: NOT writing a checkpoint")
            return
        tmp = out / "ckpt.tmp"
        if tmp.exists():
            shutil.rmtree(tmp)
        tmp.mkdir()
        sd = dict(model.named_parameters())
        torch.save({"step": step, "i": i, "order": [it["id"] for it in items],
                    "trainable": {n: sd[n].detach().cpu() for n in trainable},
                    "opt": opt.state_dict(), "sched": sched.state_dict(), "scaler": scaler.state_dict(),
                    "py_rng": random.getstate(), "total_steps": total_steps}, tmp / "state.pt")
        model.save_pretrained(tmp / "adapter")
        old = out / "ckpt.old"
        if old.exists():
            shutil.rmtree(old)
        if ckpt.exists():
            ckpt.rename(old)
        tmp.rename(ckpt)
        if old.exists():
            shutil.rmtree(old)
        adapter = out / "adapter"  # merge_convert.py reads <run>/adapter
        if adapter.exists():
            shutil.rmtree(adapter)
        shutil.copytree(ckpt / "adapter", adapter)
        say(f"checkpoint at step {step}")

    step, i = 0, 0
    if a.resume and (ckpt / "state.pt").exists():
        st = torch.load(ckpt / "state.pt", map_location="cpu", weights_only=False)
        by_id = {it["id"]: it for it in items}
        if set(st["order"]) != set(by_id):
            raise SystemExit("--resume: the dataset changed since the checkpoint; start a new --out")
        items[:] = [by_id[k] for k in st["order"]]
        sd = dict(model.named_parameters())
        with torch.no_grad():
            for n, t in st["trainable"].items():
                sd[n].copy_(t.to(sd[n].device))
        opt.load_state_dict(st["opt"])
        sched.load_state_dict(st["sched"])
        scaler.load_state_dict(st["scaler"])
        random.setstate(st["py_rng"])
        step, i = st["step"], st["i"]
        if st["total_steps"] != total_steps:
            say(f"note: total_steps {total_steps} differs from checkpoint's {st['total_steps']}")
        say(f"resumed from checkpoint at step {step}")
    elif a.resume:
        say("--resume given but no checkpoint yet: starting from step 0")
    idle = a.idle_frac
    t_last = time.time()
    t_guard = [0.0]

    def guard_point(step: int, i: int) -> None:
        """Check the star-trek rules at most every 30 s; pause here while they fail."""
        if time.time() - t_guard[0] < guard.POLL_S:
            return
        t_guard[0] = time.time()
        ok, vram_ok, msg = g.check(vram_free=torch.cuda.mem_get_info()[0] / 1e9)
        if ok:
            return
        say(f"PAUSE at step {step}: {msg}")
        p0 = time.time()
        while not ok:
            if not vram_ok:
                torch.cuda.empty_cache()
                ok, vram_ok, msg = g.check(vram_free=torch.cuda.mem_get_info()[0] / 1e9)
                if not vram_ok:
                    save_ckpt(step, i)
                    say(f"VRAM breach ({msg}): checkpointed, exiting to free VRAM (rerun with --resume)")
                    sys.exit(3)
            if time.time() - p0 > a.max_pause_min * 60:
                save_ckpt(step, i)
                say(f"paused > {a.max_pause_min} min: checkpointed, exiting to free VRAM (rerun with --resume)")
                sys.exit(3)
            time.sleep(guard.POLL_S)
            ok, vram_ok, msg = g.check(vram_free=torch.cuda.mem_get_info()[0] / 1e9)
        say(f"RESUME after {time.time()-p0:.0f} s: {msg}")
        t_guard[0] = time.time()

    model.train()
    losses = []
    while step < total_steps:
        guard_point(step, i)
        t0 = time.time()
        i_step = i  # data position at the step boundary (what a checkpoint must record)
        for _ in range(a.accum):
            guard_point(step, i_step)
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
                    raise SystemExit(f"VRAM {torch.cuda.memory_reserved()/1e9:.2f} GB > cap {a.vram_gb} GB; "
                                     "stopped (last checkpoint kept) -- lower --bs")
        scaler.unscale_(opt)
        torch.nn.utils.clip_grad_norm_(params, 1.0)
        scaler.step(opt)
        scaler.update()
        opt.zero_grad(set_to_none=True)
        sched.step()
        step += 1
        busy = time.time() - t0
        if step % 10 == 0 or step <= 3:
            say(f"step {step}/{total_steps} loss {np.mean(losses[-10*a.accum:]):.4f} "
                f"lr {sched.get_last_lr()[0]:.2e} step_s {busy:.1f} idle {idle:.2f} "
                f"strek {guard.cycle_rate():.3f}/s (base {g.base['rate']:.3f}) "
                f"peak_alloc {torch.cuda.max_memory_allocated()/1e9:.2f}GB "
                f"reserved {torch.cuda.memory_reserved()/1e9:.2f}GB")
        t_end = time.time() + busy * idle
        while time.time() < t_end:  # duty-cycle sleep, guard still watching
            time.sleep(min(5.0, max(0.0, t_end - time.time())))
            guard_point(step, i)
        if step % a.save_every == 0 or step == total_steps:
            save_ckpt(step, i)
    (out / "done").write_text(json.dumps({"steps": step, "items": len(items), "vocab": len(vocab),
                                          "elapsed_s": time.time() - t_last}))
    say("done")


if __name__ == "__main__":
    main()
