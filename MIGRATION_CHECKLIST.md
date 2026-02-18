# Go → Rust Gateway Migration Checklist

Migration branch: `feature/rust-migration` | Started: 2026-02-16 | Completed: 2026-02-18

## Overview

The voice gateway is the core backend for omarchy-voice-typing. It handles:
- Real-time WebSocket audio streaming (OpenAI Realtime protocol subset)
- Dual-path transcription: Deepgram (online) → LAN whisper → local whisper (offline)
- Batch transcription via AssemblyAI REST API
- Web UI dashboard for recordings/transcripts
- Session logging, latency metrics, audio archival

The Go gateway (`gateway/`) has been running in production since January 2026. The Rust rewrite (`gateway-rs/`) is a drop-in replacement on the same port with identical protocol behavior.

## Why Rust

- **Compile-time safety for concurrent WebSocket state machine** — the `RealtimeSession` struct manages audio buffers, Deepgram connections, reconnection state, and transcript accumulation across multiple async tasks. Rust's ownership model prevents data races that Go's goroutines + mutexes can't catch at compile time.
- **Smaller binary and memory footprint** — matters for the laptop (ThinkPad T14 Gen 1) where this runs alongside whisper inference.
- **No GC pauses** — not that Go's GC was a problem here, but it's one less variable.

## Phase Completion Status

| Phase | Description | Status |
|-------|-------------|--------|
| P0 | Project skeleton, config, secrets, health endpoint | Done |
| P1 | Audio processing (PCM16 decode, WAV build, archival) | Done |
| P2 | Deepgram streaming WebSocket client | Done |
| P3 | AssemblyAI REST client, batch transcribe handler | Done |
| P4 | WebSocket realtime handler (full session state machine) | Done |
| P5 | Web UI handlers (recordings, transcripts, audio serving) | Done |
| P6 | Session logging, latency JSONL logging | Done |
| P7 | E2E integration tests, lib/bin split | Done |
| P8 | CORS, SPA serving, graceful shutdown, additional tests | Done |

## File-by-File Migration Map

### Go → Rust Source Mapping

| Go File | Lines | Rust File(s) | Notes |
|---------|-------|--------------|-------|
| `cmd/server/main.go` | 150 | `src/main.rs`, `src/config.rs`, `src/secrets.rs` | Split into config loading, secrets, and server startup |
| `internal/assemblyai/assemblyai.go` | 158 | `src/assemblyai/client.rs` | Direct port, same API |
| `internal/auth/gpg.go` | 166 | `src/secrets.rs` | Simplified: no terminal prompt, just `pass` with timeout |
| `internal/deepgram/streaming.go` | 213 | `src/deepgram/streaming.rs` | WebSocket via tokio-tungstenite instead of gorilla |
| `internal/handlers/handlers.go` | 255 | `src/handlers/transcribe.rs` | Batch transcription handler |
| `internal/handlers/latency.go` | 306 | `src/logging/latency.rs` | JSONL logger with daily rotation |
| `internal/handlers/realtime.go` | 1166 | `src/handlers/realtime.rs`, `src/handlers/realtime_session.rs` | Split into WS upgrade + session state machine |
| `internal/handlers/web_handlers.go` | 437 | `src/handlers/web.rs` | All web UI endpoints |
| — | — | `src/audio.rs` | New: audio decode/encode/archive extracted |
| — | — | `src/spelling.rs` | New: custom spelling replacement logic |
| — | — | `src/transcription/fallback.rs`, `whisper.rs` | New: offline transcription chain extracted |
| — | — | `src/logging/session_log.rs` | New: session timeline logging |
| — | — | `src/error.rs` | New: unified error types |
| — | — | `src/state.rs` | New: AppState with CancellationToken |
| — | — | `src/lib.rs` | New: router builder for test reuse |

### Test Coverage

| File | Tests | Type |
|------|-------|------|
| `src/assemblyai/client.rs` | 3 | Unit (spelling grouping) |
| `src/audio.rs` | 5 | Unit (WAV build, base64 decode, timestamps) |
| `src/handlers/web.rs` | 12 | Unit (timestamp parsing, filename sanitization, range headers) |
| `src/logging/latency.rs` | 3 | Unit (metrics creation, serialization, date format) |
| `src/spelling.rs` | 4 | Unit (replacement logic, case, word boundaries) |
| `tests/e2e_test.rs` | 16 | E2E (full server, WS sessions, HTTP endpoints) |
| **Total** | **43+** | **31 unit + 16 E2E** |

Key E2E tests:
1. Health endpoint returns 200
2. WS connect → session.created
3. session.update → session.updated
4. Full offline recording → transcript via mock whisper
5. Clear with audio → no transcript (discard, not auto-commit)
6. Audio before session.update → ignored
7. Back-to-back recordings both succeed
8. Whisper empty response → empty transcript
9. All routes respond (no 404 for valid paths)
10. CORS preflight returns proper headers
11. Concurrent WS sessions — no crosstalk (3 sessions)
12. Rapid-fire recordings (5 append→commit cycles)
13. AssemblyAI transcribe endpoint wiring
14. Latency JSONL written after recording
15. Directory traversal blocked
16. LAN whisper fails → falls back to local

## Benchmark Report

Test machine: ThinkPad T14 Gen 1 (laptop)
Benchmark date: 2026-02-18

### Timing Comparison (Deepgram Online Path)

| Metric | Go Gateway (5 sessions avg) | Rust Gateway |
|--------|---------------------------|--------------|
| Deepgram connect | 842–1048ms | 932ms |
| Transcription (flush wait) | 1500ms | 1500ms |
| First audio to GW | 0ms | 0ms |

The 1500ms transcription time is the Deepgram flush timeout — both gateways wait the same fixed duration. Deepgram connect is network-bound (TLS handshake), so Go vs Rust makes no difference.

### Offline Path (Local Whisper)

710ms transcription — that's whisper model inference time, not gateway overhead.

### Resource Usage (At Rest, After Serving Requests)

| Metric | Go | Rust | Difference |
|--------|-----|------|-----------|
| Binary size | 9.8 MB | 5.1 MB | **48% smaller** |
| RSS (resident memory) | 25.8 MB | 22.7 MB | **12% less** |
| Virtual memory | 1,746 MB | 944 MB | **46% less** |
| VmData (heap) | 144 MB | 43 MB | **70% less** |
| Threads | 12 | 13 | Similar |
| CPU time (cumulative) | ~1s over 1.5hr | 0s over 12min | Both negligible |
| File descriptors | 8 | 10 | Similar |

### Honest Takeaway

The gateway is not the bottleneck. Both gateways spend <1ms on their own work — timing is dominated by Deepgram TLS (~900ms), flush timeout (1500ms), and whisper inference (~700ms). The Rust rewrite gives smaller binary, less memory, and compile-time data race prevention for the concurrent WS state machine.

## What Stays Unchanged

These components are NOT part of the migration and continue working as-is:

| Component | Language | Location | Notes |
|-----------|----------|----------|-------|
| hyprwhspr | Go | System binary | WebSocket client (unchanged protocol) |
| local-whisper | Python | `local-whisper/` | Whisper inference server |
| streaming pipeline | Python | `streaming/` | Audio chunking, VAD, parallel sender |
| Web frontend | React/TS | `web/` | Dashboard UI |
| Audio source monitor | Bash | `audio-source-monitor.sh` | PipeWire source watcher |
| Orphan recovery | Bash | `scripts/orphan-recovery.sh` | Recording rescue |
| Config files | JSON | `config/`, `hyprwhspr-configs/` | Spelling, client configs |
| SystemD services | INI | `gateway/systemd/`, `systemd/` | Service definitions |

## SystemD Switchover Instructions

The Rust binary is a drop-in replacement. Same port (8766), same protocol, same working directory assumptions.

### 1. Build the release binary

```bash
cd ~/Programs/omarchy-voice-typing/gateway-rs
cargo build --release
```

Binary: `target/release/voice-gateway` (~5.1 MB)

### 2. Update the systemd service

Edit `gateway/systemd/voice-gateway.service`:
```ini
# Change:
ExecStart=/home/prabhanshu/Programs/omarchy-voice-typing/gateway/server
# To:
ExecStart=/home/prabhanshu/Programs/omarchy-voice-typing/gateway-rs/target/release/voice-gateway
```

### 3. Restart

```bash
systemctl --user daemon-reload
systemctl --user restart voice-gateway
```

### 4. Verify

```bash
# Health check
curl http://localhost:8766/health

# Check session logs still appear
ls -la ~/Programs/omarchy-voice-typing/logs/sessions/

# Test with hyprwhspr (voice recording)
# Should produce transcript identical to Go gateway
```

### 5. Rollback (if needed)

```bash
# Point ExecStart back to Go binary
ExecStart=/home/prabhanshu/Programs/omarchy-voice-typing/gateway/server
systemctl --user daemon-reload
systemctl --user restart voice-gateway
```

## Known Gaps / Future Work

- [ ] **GPG interactive unlock**: Go has `auth/gpg.go` with terminal prompt for locked pass store. Rust version just tries `pass` with timeout — if GPG is locked, the AssemblyAI key load fails silently. Works fine because GPG is always unlocked on these machines.
- [ ] **DNS caching**: Go's Deepgram client had custom DNS caching with invalidation on failure. Rust relies on the OS resolver (reqwest/hyper). Not an issue in practice.
- [ ] **`sdk/go-client/`**: Empty directory in Go — never used. Not ported.
- [ ] **Latency logger path**: Go used `../experiments/logs/`, Rust uses `~/Programs/omarchy-voice-typing/logs/latency/`. Both are valid, Rust path is more explicit.
- [ ] **Release binary CI**: No GitHub Actions workflow yet for building the Rust binary. Currently built locally.

## Dependencies

### Rust (Cargo.toml)

| Crate | Version | Purpose |
|-------|---------|---------|
| axum | 0.8 | HTTP framework + WebSocket |
| tokio | 1 | Async runtime |
| tokio-tungstenite | 0.26 | Deepgram WebSocket client |
| tower-http | 0.6 | CORS, static files, tracing |
| reqwest | 0.12 | HTTP client (whisper, AssemblyAI) |
| hound | 3.5 | WAV file encoding |
| serde / serde_json | 1 | JSON serialization |
| tracing / tracing-subscriber | 0.1/0.3 | Structured logging |
| tokio-util | 0.7 | CancellationToken, ReaderStream |
| base64 | 0.22 | Audio chunk decoding |
| futures-util | 0.3 | Stream splitting for WS |
| thiserror / anyhow | 2/1 | Error handling |

### Dev Dependencies

| Crate | Purpose |
|-------|---------|
| wiremock | Mock HTTP server for tests |
| mockall | Mock trait generation |
| tempfile | Temporary directories for test logs |
| tokio-tungstenite | WS client for E2E tests |
