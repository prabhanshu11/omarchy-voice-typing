# Voice Typing Gateway

A Rust gateway providing speech-to-text for `hyprwhspr`. Supports two backends:

1. **Streaming (Deepgram Nova-2)** — Real-time WebSocket transcription. Audio streams during recording, transcript appears ~1-2s after you stop speaking. **Currently active.**
2. **Batch (AssemblyAI)** — REST upload + polling. Full audio uploaded after recording stops, 10-20s processing delay. Available as fallback.

> **Migrated from Go to Rust** (Feb 2026). See [MIGRATION_CHECKLIST.md](MIGRATION_CHECKLIST.md) for details. The Go source remains in `gateway/` for reference but is no longer used.

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│ Streaming mode (realtime-ws) — ACTIVE                        │
│                                                              │
│ hyprwhspr ←──WebSocket──→ Rust Gateway ←──WebSocket──→ Deepgram│
│ (records audio,           (protocol       (Nova-2 streaming  │
│  sends PCM16 chunks       translator)      STT, $0.35/hr)   │
│  via OpenAI Realtime                                         │
│  protocol)                                                   │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│ Batch mode (rest-api) — FALLBACK                             │
│                                                              │
│ hyprwhspr ──POST WAV──→ Rust Gateway ──upload+poll──→ AssemblyAI│
│ (records full audio,     (REST proxy)    (batch STT,         │
│  sends WAV file)                          10-20s delay)      │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│ Offline fallback (automatic)                                 │
│                                                              │
│ Deepgram fails → try LAN whisper (desktop GPU via Tailscale) │
│                → try local whisper (laptop CPU)              │
└──────────────────────────────────────────────────────────────┘
```

### Streaming flow (per utterance)

1. hyprwhspr starts → connects WebSocket to gateway (`ws://127.0.0.1:8766/v1/realtime`)
2. Gateway sends `session.created`, hyprwhspr sends `session.update`
3. User presses keybind → hyprwhspr sends `input_audio_buffer.clear` (new recording)
4. During recording: hyprwhspr sends `input_audio_buffer.append` (base64 PCM16 at 24kHz)
5. Gateway lazily opens Deepgram WebSocket on first audio chunk
6. Gateway decodes base64 → forwards raw PCM16 to Deepgram as binary WebSocket frames
7. Deepgram returns interim/final transcript segments in real-time
8. User presses keybind again → hyprwhspr sends `input_audio_buffer.commit`
9. Gateway sends Deepgram `Finalize`, waits ~1.5s, collects all final segments
10. Gateway applies spelling replacements, sends `conversation.item.input_audio_transcription.completed`
11. hyprwhspr pastes transcript via clipboard
12. Gateway archives audio (WAV) and transcript (TXT) in background

### Key details

- **Deepgram connections are per-utterance**, not persistent. Each recording cycle gets a fresh WebSocket. The hyprwhspr↔gateway WebSocket stays open across utterances.
- **Offline fallback is automatic**: if Deepgram connect fails, the gateway accumulates audio and transcribes via local whisper at commit time. Background reconnection probes run every 5s.
- **Graceful shutdown**: SIGTERM/SIGINT cancels in-flight WebSocket sessions with a 3s drain period.

## Structure

- `gateway-rs/` — **Rust source code (active)**
  - `src/main.rs` — Entry point, config loading, server startup
  - `src/lib.rs` — Router builder (CORS, SPA serving, all routes)
  - `src/handlers/realtime.rs` — WebSocket upgrade + message dispatch
  - `src/handlers/realtime_session.rs` — Full session state machine (Deepgram, offline fallback, logging)
  - `src/handlers/transcribe.rs` — REST `/v1/transcribe` handler (AssemblyAI batch)
  - `src/handlers/web.rs` — Web UI endpoints (recordings, transcripts, audio, stats)
  - `src/deepgram/streaming.rs` — Deepgram WebSocket client
  - `src/assemblyai/client.rs` — AssemblyAI REST client
  - `src/transcription/` — Offline fallback chain (LAN whisper → local whisper)
  - `src/logging/` — Session timeline logs + latency JSONL metrics
  - `tests/e2e_test.rs` — 16 end-to-end integration tests
- `gateway/` — Go source code (legacy, kept for reference)
- `local-whisper/` — Python whisper inference server
- `streaming/` — Python audio pipeline (chunker, VAD, parallel sender)
- `web/` — React/TypeScript dashboard UI
- `config/replacements.json` — Custom spelling corrections
- `hyprwhspr-configs/` — Config presets for switching backends
- `scripts/` — Orphan recovery, log tools
- `tools/` — OSD profiler, audio debugger
- `test-sessions/` — Structured debugging sessions

## Prerequisites

- **Rust** (stable, edition 2024)
- **Deepgram API Key** (for streaming): `DEEPGRAM_API_KEY` env var or via `pass api/deepgram`
- **AssemblyAI API Key** (for batch fallback): `ASSEMBLYAI_API_KEY` env var or via `pass api/assemblyai`

## Usage

### Build

```bash
cd gateway-rs
cargo build --release
# Binary: target/release/voice-gateway (~5.1 MB)
```

### Run

```bash
# Keys loaded from environment or pass (password-store)
./target/release/voice-gateway
# → Starting gateway server on 0.0.0.0:8766
```

### Test

```bash
cargo test
# 31 unit tests + 16 e2e tests = 47 total
```

### Monitor

```bash
journalctl --user -f -u voice-gateway
```

### Switch backends

**To streaming (Deepgram):**
```bash
cp hyprwhspr-configs/realtime-gateway.json ~/.config/hyprwhspr/config.json
systemctl --user restart hyprwhspr
```

**To batch (AssemblyAI):**
```bash
cp hyprwhspr-configs/rest-gateway.json ~/.config/hyprwhspr/config.json
systemctl --user restart hyprwhspr
```

## API Endpoints

- `GET /health` — Health check (JSON)
- `WS /v1/realtime` — OpenAI Realtime protocol (streaming, used by hyprwhspr)
- `POST /v1/transcribe` — REST file upload (batch, AssemblyAI)
- `GET /api/recordings` — List audio files
- `GET /api/transcripts` — List transcript files
- `GET /api/transcript/{filename}` — Read/update transcript
- `GET /api/audio/{filename}` — Serve audio with range request support
- `GET /api/stats` — Aggregate statistics
- `GET /api/linked` — Recordings matched with transcripts
- `/*` — SPA fallback (serves `web/dist/` if built)

## Logging & Archival

The gateway automatically archives all processed data:
- **Audio Recordings:** `recordings/` (e.g., `20260218_034231_audio.wav`)
- **Transcripts:** `transcripts/` (e.g., `20260218_034231_deepgram.txt`)
- **Session Logs:** `logs/sessions/` (timeline with speed profile per recording)
- **Latency Metrics:** `logs/latency/latency_YYYY-MM-DD.jsonl` (structured per-recording metrics)

## Custom Word Replacements

Define custom spelling corrections in `config/replacements.json`:

```json
[
  {
    "from": ["Dovac", "Dovak"],
    "to": "Dvorak"
  }
]
```

Changes require a service restart: `systemctl --user restart voice-gateway`

## Performance (vs Go gateway)

| Metric | Go | Rust |
|--------|-----|------|
| Binary size | 9.8 MB | 5.1 MB |
| RSS memory | 25.8 MB | 22.7 MB |
| Heap (VmData) | 144 MB | 43 MB |
| Transcription latency | ~2.4s | ~2.4s |

Latency is identical because it's dominated by Deepgram (~900ms TLS + 1500ms flush). See [MIGRATION_CHECKLIST.md](MIGRATION_CHECKLIST.md) for full benchmark.

## Debugging & Profiling

### OSD Profiler Tool

For hyprwhspr OSD issues, use `tools/osd_profiler.py`:

```bash
cd tools && python osd_profiler.py
```

### Test Sessions

For systematic debugging:

```bash
cd test-sessions && ./new-session.sh "description-of-issue"
```

See `test-sessions/QUICK-START.md` for details.

## Multi-Machine Development

This project runs on multiple machines (desktop + laptop). Git is the only sync method.

- Push to a branch, pull on the other machine, verify, merge to master
- See `status.md` for cross-machine debugging notes
- Related repos: `~/Programs/local-bootstrapping` (system config, systemd services)
