# Handover — local-stt lane (2026-09-25 ~04:40 IST)

Read `docs/local-stt.md` first (design, data inventory, WER table, runbook), then this.

## HARD RULES from the main session (verbatim)

> STOP GPU training on the desktop now; this is from the main session. Your train_lora.py
> run (pid 3357019, runs/r1) was starving the live star-trek-camera tracker: its cycle rate
> fell from about 1.75 to 0.4–1.35 cycles/s, and it was finding people on frames up to 19 s
> old. I have paused the process with SIGSTOP. Do NOT resume it (no SIGCONT). Start no GPU
> or heavy-CPU job on the desktop before 10:00 IST. After that, training may run only while
> the tracker stays above 1.5 cycles/s: measure rows/s in
> ~/Programs/star-trek-camera/data/logs/cycles.jsonl over 5 min, before and during.

> One more desktop rule from the camera session: /home must keep at least 50 GB free (the
> 1080p recorder pauses under 30 GB). Check with `df -h /home` before writing datasets,
> checkpoints or caches there, and write this rule verbatim into the handover next to the
> GPU rule.

Lesson: my own guard (star-trek pose `latency_s` from `/situation`) did NOT see the harm —
pose latency stayed 7–35 ms while the **cycle rate** collapsed. `train_lora.py` must watch
cycles/s from `cycles.jsonl` instead (see "What is left" 1).

## State right now

- Desktop pid **3357019** (`train_lora.py --out runs/r1`) is **SIGSTOPped** (state `T`) by the
  main session, at step ~80/246. It still holds ~3.2 GB VRAM. **No checkpoint exists**
  (adapter saves every 200 steps; `runs/r1/` has only `train.log`). There is nothing to
  resume: when allowed, ask the main session whether to `kill -9 3357019` (frees the VRAM)
  and start a fresh round. Never SIGCONT it.
- Desktop `local-whisper.service` is **stopped** (I stopped it to free VRAM for training).
  Harmless now: the provider is still `deepgram`. Starting it again loads a model on the GPU
  — that counts as GPU work, so not before 10:00 and only under the cycles/s rule.
- Desktop disk at 04:40: `/home` 79 GB free; `~/Programs/voice-stt` = 4.2 GB.
- Laptop: gateway rebuilt + restarted with the new code (master 49534de+), provider
  **deepgram** (unchanged behaviour), `LAN_WHISPER_URL=http://100.92.71.80:8767` via drop-in.
  hyprwhspr tray = ready. voice-labels web app live at http://127.0.0.1:8771/ (unit
  `voice-labels.service`, enabled). Bar card has the link (screenshot
  `docs/img/voice-card-link-2026-09-25.png`).
- Desktop omarchy-voice-typing fast-forwarded to laptop master (its own uncommitted
  `gateway-rs/src/audio.rs` SILENCE threshold change was kept, not touched).

## Done (commits)

omarchy-voice-typing (master, not pushed): f6aa387 orphan-recovery cascade fix · 230b408
provider switch + hotwords + stt tools + web app · aefebd6 round.sh/frozen test/doc · a93f1f1
CLAUDE.md · 49534de merge · d2851ba backlog.
local-bootstrapping (not pushed): c789279 card `links` + voice-labels unit + voice-stt-provider
· 18fcb02 drop-ins + setup-voice-typing.sh.

## Verified numbers (n = 118 held-out clips, 4,045 words, refs = AssemblyAI best)

| System | WER | vocab recall (25 occurrences) | latency |
|---|---|---|---|
| Deepgram nova-2 stream (current) | 7.61 % | 40 % | streaming |
| turbo stock | 5.29 % | 52 % | 0.52 s/clip desktop GPU (idle) |
| turbo stock + hotwords (37 starter words) | 5.32 % | 76 % | (GPU shared) |
| fine-tuned | NOT YET — training stopped | | |

Laptop CPU int8 turbo: 10–13 s per clip (RTF 0.90, i7-10510U) — too slow as primary.
Data: 1,545 unique clips / 15.0 h; 1,378 labelled / 11.3 h; train 1,962 items / 9.06 h
(`~/Programs/voice-stt/data/train_items.jsonl`); test frozen in `data/test_sha1.txt`.
Gateway WS end-to-end with provider=local verified via `stt/ws_feed.py` on a test instance.

## What is left

1. **Make training star-trek-safe before any rerun**: in `stt/train_lora.py` replace the
   `/situation` pose-latency guard with a cycles/s guard: read
   `~/Programs/star-trek-camera/data/logs/cycles.jsonl`, rows/s over the last 5 min; measure a
   5-min baseline before starting (refuse if < 1.5), pause (sleep, freeing nothing) while
   < 1.5, and consider `--idle-frac` ≥ 1.0 and `nice -n 19` + `ionice`. Save the adapter
   every 25 steps so a stop never loses the run. Check `df -h /home` ≥ 50 GB free first.
2. After 10:00 IST and under that rule: train round 1 (`runs/r1` again or `stt/round.sh`),
   `merge_convert.py --run runs/r1 --name ft-r1`, eval with and without hotwords
   (`eval_wer.py`, `vocab_recall.py`), fill the WER table in `docs/local-stt.md`.
3. Serving: `ln -sfn <best model> ~/Programs/voice-stt/models/current` (the stock
   `turbo-base-ct2` is already a valid choice: it beats Deepgram), start desktop
   `local-whisper` (drop-in `desktop-voice-stt.conf` is installed there; serving is ~1 GB VRAM
   and short bursts — confirm with the main session it is acceptable under the GPU rule),
   check `curl http://100.92.71.80:8767/health`, feed clips through the LIVE gateway with
   `stt/ws_feed.py` (never start hyprwhspr recordings), then `voice-stt-provider local`.
   One-command fallback: `voice-stt-provider deepgram`.
4. Report decisions for the user (in backlog): orphan copies (27k), shadow cloud labels once
   local is live, nightly `round.sh` timer, desktop reboot for the NVIDIA driver mismatch.

## Paths

Desktop `~/Programs/voice-stt/`: `code/` (copy of repo `stt/`; re-copy with
`tar cf - -C stt . | ssh desktop 'tar xf - -C ~/Programs/voice-stt/code'`), `env/` (overlay
venv: shared torch + peft/ctranslate2), `models/turbo-base-ct2`, `data/`, `raw/`, `runs/`,
`labels/`, `.aai.env` (AssemblyAI key, 0600). Eval/segment use the local-whisper venv with
`LD_LIBRARY_PATH` set as in `stt/round.sh`. Laptop labels: `~/Programs/voice-stt/labels/`.
