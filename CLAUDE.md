# omarchy-voice-typing — Claude Code Context

## Build Contract

**local-bootstrapping calls `./build.sh`. Binary always lands at `./bin/voice-gateway`.**

This is the single source of truth for how to build the gateway. Never hardcode Go or Rust
specifics in local-bootstrapping — update `build.sh` here instead.

### Changing the Gateway Implementation

If you migrate to a new language or restructure the gateway directory:
1. Update `build.sh` detection logic (Rust: `gateway-rs/Cargo.toml`, Go: `gateway/go.mod`)
2. `bin/voice-gateway` must always be the output — the service file never changes
3. Update `CLAUDE.md` (this file) with what changed and why
4. The `bin/` directory is gitignored — it is always rebuilt by `setup-voice-typing.sh`

### Gateway Implementations

| Status | Language | Dir | Binary |
|--------|----------|-----|--------|
| Active | Rust | `gateway-rs/` | `target/release/voice-gateway` → `bin/voice-gateway` |
| Legacy | Go | `gateway/` | `gateway/voice-gateway` → `bin/voice-gateway` |

### Canonical Service Path

The systemd service file in local-bootstrapping always uses:
```
ExecStart=%h/Programs/omarchy-voice-typing/bin/voice-gateway
WorkingDirectory=%h/Programs/omarchy-voice-typing
```

## Architecture

- **hyprwhspr**: Audio capture daemon (AUR package), controlled via `hyprwhspr-toggle`
- **voice-gateway**: HTTP gateway (Rust, port 8765) — receives audio, calls AssemblyAI/Deepgram
- **local-whisper**: Python venv, offline fallback via local Whisper model. On the desktop
  it serves the user's fine-tuned large-v3-turbo (`~/Programs/voice-stt/models/current`)
- **Local STT (2026-09-25)**: `voice-stt-provider local|deepgram` picks the backend per
  recording (`~/.config/voice-typing/stt-provider`); local = desktop GPU Whisper over
  Tailscale (LAN_WHISPER_URL) → laptop local-whisper. Training/eval tools in `stt/`,
  corrections web app `stt/webapp` (voice-labels, :8771). **Read `docs/local-stt.md` first.**
- **Source button (2026-11-03)**: the mic popup's "Source:" button names where speech is
  transcribed: `Source: Deepgram (online)` or `Source: Local · base on i7-10510U · distil from 25 s on MX330`
  (provider `offline` = this laptop's own local-whisper only, no internet, no desktop). It is
  highlighted while local; a tap switches in place (`voice-stt-provider toggle-offline`, label from
  `voice-stt-provider label`, bar-touch `state-cmd`). No notification. While the source is not ready
  (local models loading, or Deepgram's `/v1/auth/token` not answering) it pulses yellow, one redraw
  per 0.8 s, until it is. Screenshots: `docs/img/popup-*.png`.
- **Laptop local-whisper is a hybrid** (2026-11-03, his choice): recordings under 25 s go to `base`
  on the CPU (~1 s from Stop to text); 25 s and longer go whole to `distil-large-v3` on the MX330
  (~1× real time: a 75 s dictation took 72 s; more accurate). distil runs in a child process loaded
  only while the Source is Local (ready ~5 s after the switch; the button pulses until then) and
  ended when the Source goes online, so the GPU holds nothing then. Not ready -> base takes the long
  ones too. `curl localhost:8767/health` shows both. Lessons + numbers: `local-whisper/LESSONS_LEARNED.md`,
  `local-whisper/BENCHMARKS.md`; test: `local-whisper/test_hybrid.py`.
- **Retry button (2026-11-03)**: the popup's "Retry the last failed recording" runs
  `voice-retranscribe --last --copy --notify` (newest failed/crashed client spool → clipboard).
  Backend auto: offline mode → laptop Whisper; otherwise Deepgram, then laptop Whisper on failure.
- **MicOSD**: GTK4 overlay showing recording state (managed by hyprwhspr-patch)

## Key Files

| File | Purpose |
|------|---------|
| `build.sh` | Build contract — builds binary to `bin/voice-gateway` |
| `.env` | API keys (ASSEMBLYAI_API_KEY, DEEPGRAM_API_KEY) — gitignored |
| `gateway-rs/` | Rust gateway source |
| `gateway/` | Legacy Go gateway source |
| `local-whisper/` | Python offline fallback; desktop: fine-tuned model server (`?hotwords=`) |
| `stt/` | Local STT: inventory, dataset, LoRA training, eval, `round.sh`, `webapp/` |
| `docs/local-stt.md` | Local STT design, data inventory, WER table, runbook |
| `logs/` | Runtime logs (gitignored) |
| `scripts/voice-retranscribe` | Re-transcribe any saved WAV (`--backend auto/deepgram/local`); `--pending` retries every failed client spool file, `--last` only the newest |
| `~/Programs/recordings/client/` | hyprwhspr-side spool: every recording written to disk while recording (voice_safety.py in the local-bootstrapping hyprwhspr-patch) |

**No audio may exist only in memory** (2026-09-30 incident, `docs/issues/2026-09-30-voice-data-loss.md`):
the client spools while recording, the gateway saves the WAV before transcribing and on disconnect,
and every network wait is bounded. Keep it that way.

## Deployment

This repo is deployed by `local-bootstrapping/scripts/setup-voice-typing.sh`.
That script:
1. Pulls this repo to master
2. Calls `./build.sh` to build the binary
3. Installs the systemd service file with canonical path
4. Enables and restarts the service

Do not manually run `cargo build` and copy binaries — always let `setup-voice-typing.sh` handle it.
