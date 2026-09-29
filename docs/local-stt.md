# Local speech-to-text (fine-tuned Whisper) — design and runbook

Started 2026-09-25. Goal (the user): voice typing uses a **local** model trained on
all of his own voice data, plus a web page where he fixes wrongly transcribed
words and keeps a list of words the model should concentrate on. Both feed the
next training rounds.

## Where things are

| What | Where |
|------|-------|
| Code (this repo) | `stt/` (dataset, training, eval, feeder), `stt/webapp/` (voice-labels), `local-whisper/server.py` (serving), `gateway-rs/src/transcription/provider.rs` |
| Data, models, runs | `~/Programs/voice-stt/` on the **desktop** (`raw/`, `data/`, `models/`, `runs/`, `env/`) |
| Labels (the user's corrections + word list) | `~/Programs/voice-stt/labels/{corrections.jsonl,vocab.txt}` on the **laptop**, copied to the desktop after every save |
| Web app | `http://127.0.0.1:8771/` (laptop), user unit `voice-labels.service`; link on the voice-typing bar card |
| Provider switch | `voice-stt-provider [local|deepgram]` (local-bootstrapping `dotfiles/local-bin/`) |
| Units / drop-ins | local-bootstrapping: `dotfiles/systemd-user/voice-labels.service`, `dotfiles/systemd/user/voice-gateway.service.d/local-stt.conf` (laptop), `dotfiles/systemd/user/local-whisper.service.d/desktop-voice-stt.conf` (desktop) |

## Architecture

```
hyprwhspr ──ws /v1/realtime──> voice-gateway (laptop :8765)       (protocol unchanged)
                                 │ provider = ~/.config/voice-typing/stt-provider (read per recording)
                                 ├─ deepgram: stream to Deepgram nova-2 (as before)
                                 └─ local: buffer audio; at commit POST WAV + ?hotwords=<vocab.txt>
                                      1. desktop local-whisper :8767 (Tailscale 100.92.71.80)
                                         fine-tuned large-v3-turbo, CTranslate2 int8_float16, RTX 2060 S
                                         timeout 2.5 s + 0.15 s/audio-s (max 20 s), connect 2 s
                                      2. unreachable/slow/empty -> Deepgram nova-2 BATCH (automatic)
                                      3. laptop local-whisper :8767 (MX330, distil-large-v3) — last resort
voice-labels (laptop :8771) ── corrections.jsonl / vocab.txt ──scp──> desktop labels/ ──> next training round
```

Switch back to Deepgram with one command: `voice-stt-provider deepgram` (next
recording; no restart). `voice-stt-provider local` switches back. With `local`, a desktop that is
unreachable or slow falls back to Deepgram batch automatically (tested
2026-09-25 on a test gateway: desktop port closed -> Deepgram in 2.4-3.1 s;
desktop hanging -> Deepgram after the timeout, 5.8-7.4 s total; log line
`LAN whisper failed or slow, falling back to Deepgram`, backend `deepgram-batch`). `curl -s
localhost:8765/health` shows `"backend"` = the provider the next recording uses.

## Data inventory (2026-09-25)

`stt/inventory.py` on each machine, merged and de-duplicated by SHA-1 of the audio.

| | clips | hours |
|---|---|---|
| Audio files (laptop 729 + desktop 883, both repos' `recordings/`) | 1,612 | 15.3 |
| Unique audio | 1,545 | 15.0 |
| Unique audio with a transcript | 1,378 | 11.3 (65 k words) |
| — labelled by AssemblyAI (Jan 2026) / Deepgram nova-2 stream (Feb–Sep) | 720 / 576 | |
| — labelled by Whisper itself (not used as labels) / recovered | 77 / 5 | |
| Present on both machines | 37 | |
| Clips > 30 s (need cutting for training) | 421 | |

Sample rates: 16 kHz (Go era) and 24 kHz (Rust gateway). Median clip 16.5 s,
p90 67 s, max 14 min. Monthly: Jan 702, Feb 499, then 8–36 a month.

Not counted: **27,126** (laptop) and 88 (desktop) files named
`<ts>_<ts>_…_audio.wav` — copies created by an orphan-recovery loop (each
15-minute run resubmitted every unmatched copy and the gateway saved it again
under a longer name, until "File name too long"). Fixed in f6aa387; the copies are
left in place (they are the same audio as a handful of originals; deleting them is
the user's call).

Splits (`stt/build_dataset.py`, deterministic by SHA-1):
- **test**: 118 Deepgram-era clips, 3–120 s, 0.70 h, 4,045 reference words.
- **train**: 1,183 clips, 10.1 h → `stt/segment.py` cuts long clips at word
  boundaries aligned with a baseline Whisper pass → **1,962 items, 9.06 h**
  (16 long clips dropped: label and audio aligned < 60 %).
- unlabeled: 244 clips (Whisper-labelled or no transcript).

## Model choice

**openai/whisper-large-v3-turbo**, LoRA fine-tuned, served with faster-whisper
(CTranslate2). Why:
- Best accuracy that fits: 809 M params, 4-layer decoder (fast); int8_float16 ≈ 1 GB
  VRAM, so it sits beside the live star-trek-camera models on the 8 GB desktop GPU.
- Out of the box it already beats Deepgram on his speech (table below).
- faster-whisper has **`hotwords`**: a word list sent as a decoder prompt at
  inference. **So yes, the word list works at inference time** — as soft
  biasing (it raises the chance of those spellings; it is not a hard
  constraint, and a very long list is truncated to ~220 tokens).
- Trainable on the desktop GPU with LoRA in < 3.5 GB (bs 2, fp16, gradient
  checkpointing), leaving headroom for star-trek.
- Training items sometimes carry a prompt of word-list words, so the model
  learns to use the list rather than ignore it.

**Where inference runs**: the laptop cannot do it fast enough. Measured on 8 real
laptop clips (3–36 s), turbo CPU int8 on the i7-10510U (8 threads) takes **10–13 s
per clip regardless of length (25 s for a 36 s clip)**, RTF 0.90 — the encoder
always processes a 30 s window. The MX330 (2 GB) runs distil-large-v3 at RTF
≈1.2 (BENCHMARKS.md). So inference runs on the **desktop GPU over Tailscale**
(≈0.5 s per clip when the GPU is not training), with the laptop's
distil-large-v3 as the slow fallback and Deepgram as the one-command fallback.

## Evaluation

References: the test clips' live transcripts are Deepgram's own output, so each
test clip was sent once to AssemblyAI (best tier, `stt/make_refs.py`) and that
is the reference. It is a machine label, not human truth; web-app corrections
(gold) replace it clip by clip as they arrive (`eval_wer.py` prefers gold).
Normalisation: lowercase, punctuation stripped. n = 118 clips, 4,045 words.

| System | WER | vocab-word recall | mean latency / clip |
|---|---|---|---|
| Deepgram nova-2 streaming (what he had) | 7.61 % | 10/25 (40 %) | streaming |
| Whisper large-v3-turbo, stock | 5.29 % | 13/25 (52 %) | 0.52 s (desktop GPU idle) |
| stock + hotwords (starter list, 37 words) | 5.32 % | 19/25 (76 %) | — (GPU shared with training) |
| fine-tuned round 1 | _pending_ | | |
| fine-tuned round 1 + hotwords | _pending_ | | |

(vocab-word recall: of the 25 places a word from the list occurs in a test
reference, how many the system spelled right; `stt/vocab_recall.py`.)

## Training round (repeatable)

On the desktop, from `~/Programs/voice-stt` (overlay env `env/` = the shared
`~/Programs/.venv` torch/transformers + peft, ctranslate2):

```bash
# 1. refresh inventories (laptop + desktop) and copy laptop-only audio -> raw/laptop/
python3 stt/inventory.py --machine laptop > inv-laptop.jsonl     # on the laptop
# 2. dataset (applies labels/corrections.jsonl: corrected clips become gold)
python3 code/build_dataset.py
# 3. cut long clips (GPU, ~10 min)
python3 code/guard.py run --need-gb 1.5 -- $V/bin/python code/segment.py --model models/turbo-base-ct2        # V = local-whisper venv
# 4. train (star-trek guard built in, see below; exit 3 = paused out -> rerun with --resume)
env/bin/python code/train_lora.py --out runs/rN --epochs 2 --resume   # rerun while it exits 3
# 5. merge + convert (CPU)
env/bin/python code/merge_convert.py --run runs/rN --name ft-rN
# 6. evaluate, then point `current` at the winner and restart the desktop server
$V/bin/python code/eval_wer.py --model models/ft-rN --name ft-rN [--hotwords-file labels/vocab.txt]
ln -sfn ft-rN models/current && systemctl --user restart local-whisper
```

GPU etiquette (star-trek-camera is live on the same GPU). The rules, verbatim:

From the main session (2026-09-25 ~04:40 IST):
> Start no GPU or heavy-CPU job on the desktop before 10:00 IST.
> One more desktop rule from the camera session: /home must keep at least 50 GB free (the
> 1080p recorder pauses under 30 GB).

From programs-b6, the camera session (2026-09-25 ~04:50 IST), approved relative bar,
effective after 10:00 IST, for training AND GPU serving tests:
> (1) Baseline = the 10 min before each run, taken only while load < 8 and no other heavy
> job is running (the datalake refresh is running now and distorts it). A run may start,
> and continue, only while the 5-min rows/s is >= 95% of that baseline AND the p90 of
> `cycle.cycle_ms` in cycles.jsonl is <= 1.2x the baseline p90. Check every 30 s during
> the run; on a breach, pause within 30 s.
> (2) At least 3 GB of VRAM must stay free at all times. The live video estimator OOMed on
> CUDA at 03:21 while another training run shared the GPU. nvidia-smi is broken right now
> (driver/library mismatch), so find a working way to read free VRAM (e.g.
> torch.cuda.mem_get_info inside your process); if you can't read it, don't start.
> (3) /home must stay >= 50 GB free.

`stt/guard.py` implements them (`python3 code/guard.py status | baseline | run
[--need-gb G] -- CMD`):
- baseline = 10 **contiguous** clean minutes right before each run, sampled every
  30 s; a sample is dirty if load1 >= 8, a heavy job runs (any non-star-trek
  process >= 1 core, except the always-spinning xdg-desktop-portal, or a command
  line like the datalake refresh `refresh-claude.sh`), or the tracker is below
  0.5 rows/s. Each baseline is appended to `runs/guard-baseline.jsonl`.
- during the run, every <= 30 s: 5-min rows/s >= 95 % of baseline, 5-min p90
  `cycle_ms` <= 1.2x baseline p90, device free VRAM >= 3 GB, /home >= 50 GB.
- free VRAM: CUDA driver API `cuMemGetInfo` via ctypes (0.12 s, works while
  NVML is broken), or `torch.cuda.mem_get_info()` inside train_lora.
- `guard.py run` SIGSTOPs its command on a breach and SIGCONTs when clear; VRAM
  breach or > 30 min paused kills it (exit 3). `round.sh` retries exit-3 steps.
- `train_lora.py` guards itself: baseline before loading the model, checks
  between micro-batches and during its duty-cycle sleep (pause <= 30 s after a
  breach), checkpoint (LoRA weights, optimizer, scheduler, data order and
  position) every 25 steps (atomic; skipped if /home < 50 GB), exit 3 after a
  VRAM breach or 30 min paused, `--resume` continues. nice 19, ionice idle,
  2 CPU threads, 50 % duty. Defaults sized for the 3 GB-free rule: the desktop
  GPU had only 5.1-5.9 GB free with star-trek running (05:00), so the base
  weights load in 4-bit NF4 (QLoRA, bitsandbytes 0.50.2 installed --no-deps
  into `env/`), bs 1 x accum 16, LoRA rank 16, VRAM cap 1.9 GB (start needs
  device free >= cap + 3). `--quant none` = the old fp16 base (r1 reserved 3.3 GB).
  merge_convert.py merges the adapter into the fp16 base as before.

The first round (04:00, rank 32, bs 2) reserved 3.3 GB and watched star-trek's
pose latency instead of its cycle rate; pose latency stayed 7-35 ms while the
cycle rate fell from 1.75 to 0.4 rows/s, so that guard was replaced.
The desktop's own local-whisper is stopped during training.

**How the user's input feeds training**: every correction or "correct as is" in
the web app is a gold label (it overrides the pseudo-label, and in the test set
it overrides the AssemblyAI reference). The word list is (a) sent as hotwords on
every recording, immediately, and (b) used as training prompts. Once the local
provider is live, new recordings get **no** cloud transcript, so new training
labels come only from corrections — see decisions below.

## Operations

- Logs: `journalctl --user -u voice-gateway -f` (look for `STT provider for this
  recording`, `Sending hotwords`, `Whisper OK label="lan-whisper"`), on the
  desktop `journalctl --user -u local-whisper -f`.
- Test without a mic and without typing anywhere:
  `uv run stt/ws_feed.py ws://127.0.0.1:8765/v1/realtime ~/Programs/recordings/<ts>_audio.wav`
- Web app headless test: start a copy with a throwaway `STT_HOME`, then
  `uv run stt/webapp/test_ui.py http://127.0.0.1:<port> <outdir>`.
