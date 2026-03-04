# voice-type

A production speech-to-text platform with real-time transcription, offline fallback, and deep session profiling. Built with Rust (gateway) and React/TypeScript (dashboard).

**Live demo:** [prabhanshu.space/voice-type](https://prabhanshu.space/voice-type)

## What it does

voice-type captures audio, transcribes it in real-time via streaming speech providers, and provides a profiling dashboard to analyze every session's performance — connection latency, transcription timing, backend selection, and transcript accuracy.

```
Audio Input ──→ Rust Gateway ──→ Deepgram (streaming) ──→ Transcript
                    │                                         │
                    ├──→ Local Whisper (offline fallback)      │
                    │                                         ▼
                    └──→ Session Profiling ──→ Dashboard UI
```

### Key capabilities

- **Real-time streaming transcription** via Deepgram Nova-2 WebSocket (transcript appears ~1-2s after speaking)
- **Automatic offline fallback** — if the cloud provider is unreachable, seamlessly switches to local Whisper (GPU or CPU)
- **Session profiling dashboard** with UML sequence diagrams, timing waterfall, speed profiles, and editable transcripts for training feedback
- **361 recordings, 303 session logs, 180 latency records** of production data powering the analytics

## Architecture

### Gateway (Rust)

The gateway is a protocol translator and session orchestrator:

- Accepts audio via WebSocket (OpenAI Realtime protocol) or REST upload
- Routes to the best available transcription backend
- Logs every session with sub-millisecond timing data
- Serves the web dashboard and all API endpoints
- 47 tests (31 unit + 16 e2e)

### Dashboard (React/TypeScript)

Two main views:

**Recordings** — Browse and play back all captured audio with transcripts. Edit transcripts inline.

**Profiling** — Session explorer with:
- KPI summary bar (total sessions, success rate, P50/P95 latency, backend split)
- Scrollable session list with status/backend filtering
- Per-session detail: audio player, editable transcript, UML sequence diagram, timing waterfall, speed profile bars

The sequence diagram renders the actual event flow between components (hyprwhspr, Gateway, Deepgram/Whisper) with latency annotations — parsed directly from production session logs.

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Gateway | Rust, axum, tokio, tower-http |
| Streaming STT | Deepgram Nova-2 (WebSocket) |
| Batch STT | AssemblyAI (REST) |
| Offline STT | Whisper (local Python server) |
| Frontend | React 19, TypeScript, Vite, Recharts |
| Testing | cargo test (Rust), Vitest + RTL (frontend) |

## Quick Start

### Build & Run

```bash
# Gateway
cd gateway-rs
cargo build --release
./target/release/voice-type
# → Serving on 0.0.0.0:8766

# Dashboard
cd web
npm install && npm run build
# → Built to web/dist/, served by gateway at /
```

### Test

```bash
cargo test                    # 47 Rust tests
cd web && npm test            # Frontend tests
```

### API

| Endpoint | Description |
|----------|-------------|
| `WS /v1/realtime` | Streaming transcription (OpenAI Realtime protocol) |
| `POST /v1/transcribe` | Batch file upload transcription |
| `GET /api/linked` | Recordings matched with transcripts |
| `GET /api/profiling/latency` | Latency time-series data (JSONL) |
| `GET /api/profiling/sessions` | Session summaries with timeline events |
| `GET /api/profiling/summary` | Aggregate KPIs (percentiles, backend stats) |
| `PUT /api/transcript/:filename` | Save transcript correction (training data) |

## Project Structure

```
voice-type/
├── gateway-rs/          # Rust gateway (axum + tokio)
│   ├── src/handlers/    # WebSocket, REST, web, profiling endpoints
│   ├── src/deepgram/    # Deepgram streaming client
│   ├── src/transcription/ # Offline fallback chain
│   ├── src/logging/     # Session logs + latency metrics
│   └── tests/           # e2e integration tests
├── web/                 # React/TypeScript dashboard
│   ├── src/pages/recordings/   # Audio browser + transcript editor
│   ├── src/pages/profiling/    # Session explorer + sequence diagrams
│   └── src/lib/                # API client, types, hooks
├── local-whisper/       # Python Whisper inference server
├── logs/                # Production session + latency data
├── recordings/          # Archived audio files
└── transcripts/         # Archived transcripts
```

## Performance

| Metric | Value |
|--------|-------|
| Gateway binary | 5.1 MB (LTO + stripped) |
| Memory (RSS) | 22.7 MB |
| Heap | 43 MB |
| Streaming latency | ~2.4s (Deepgram TLS + flush) |
| Offline latency | 3-14s (depends on audio length) |

## License

MIT
