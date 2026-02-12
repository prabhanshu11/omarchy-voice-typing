# Voice Typing Gateway

A Go gateway providing speech-to-text for `hyprwhspr`. Supports two backends:

1. **Streaming (Deepgram Nova-2)** — Real-time WebSocket transcription. Audio streams during recording, transcript appears ~1-2s after you stop speaking. **Currently active.**
2. **Batch (AssemblyAI)** — REST upload + polling. Full audio uploaded after recording stops, 10-20s processing delay. Available as fallback.

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│ Streaming mode (realtime-ws) — ACTIVE                        │
│                                                              │
│ hyprwhspr ←──WebSocket──→ Go Gateway ←──WebSocket──→ Deepgram│
│ (records audio,           (protocol     (Nova-2 streaming    │
│  sends PCM16 chunks       translator)    STT, $0.35/hr)     │
│  via OpenAI Realtime                                         │
│  protocol)                                                   │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│ Batch mode (rest-api) — FALLBACK                             │
│                                                              │
│ hyprwhspr ──POST WAV──→ Go Gateway ──upload+poll──→ AssemblyAI│
│ (records full audio,     (REST proxy)  (batch STT,           │
│  sends WAV file)                        10-20s delay)        │
└──────────────────────────────────────────────────────────────┘
```

### Streaming flow (per utterance)

1. hyprwhspr starts → connects WebSocket to gateway (`ws://127.0.0.1:8765/v1/realtime`)
2. Gateway sends `session.created`, hyprwhspr sends `session.update`
3. Gateway opens Deepgram WebSocket (`wss://api.deepgram.com/v1/listen`)
4. User presses keybind → hyprwhspr sends `input_audio_buffer.clear` (new recording)
5. During recording: hyprwhspr sends `input_audio_buffer.append` (base64 PCM16 at 24kHz)
6. Gateway decodes base64 → forwards raw PCM16 to Deepgram as binary WebSocket frames
7. Deepgram returns interim/final transcript segments in real-time
8. User presses keybind again → hyprwhspr sends `input_audio_buffer.commit`
9. Gateway sends Deepgram `Finalize`, waits ~1.5s, collects all final segments
10. Gateway applies spelling replacements, sends `conversation.item.input_audio_transcription.completed`
11. hyprwhspr pastes transcript via clipboard
12. Gateway archives audio (WAV) and transcript (TXT) in background
13. Deepgram connection closed; reconnects lazily on next utterance

### Key detail: Deepgram lifecycle

Deepgram streaming connections are **per-utterance**, not persistent. Each recording cycle (clear → append × N → commit) gets a fresh Deepgram WebSocket. The hyprwhspr↔gateway WebSocket stays open across utterances.

## Structure
- `gateway/` — Go source code
  - `cmd/server/main.go` — Entry point, API key loading, route registration
  - `internal/handlers/realtime.go` — OpenAI Realtime ↔ Deepgram protocol translator
  - `internal/handlers/handlers.go` — REST `/v1/transcribe` handler (AssemblyAI batch)
  - `internal/deepgram/streaming.go` — Deepgram WebSocket client
  - `internal/assemblyai/assemblyai.go` — AssemblyAI REST client
  - `internal/auth/gpg.go` — GPG/pass-based key loading
- `config/replacements.json` — Custom spelling corrections
- `hyprwhspr-configs/` — Config presets for switching backends
- `tests/` — Python integration tests

## Prerequisites
- **Go** (>=1.19)
- **Deepgram API Key** (for streaming): `DEEPGRAM_API_KEY` env var or in `.env`
- **AssemblyAI API Key** (for batch fallback): `ASSEMBLYAI_API_KEY` env var or in `.env`
- **Python `websocket-client`** package (for hyprwhspr realtime-ws backend): `pip install websocket-client`

## Usage

### Build
```bash
cd gateway
go build -o voice-gateway ./cmd/server
```

### Run
```bash
# Keys loaded from .env (EnvironmentFile in systemd) or environment
./voice-gateway
# → Starting gateway server on :8765
# → Deepgram API key loaded
# → AssemblyAI API key loaded
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
- `WS /v1/realtime` — OpenAI Realtime protocol (streaming, used by hyprwhspr in `realtime-ws` mode)
- `POST /v1/transcribe` — REST file upload (batch, used by hyprwhspr in `rest-api` mode)

## Advanced Features

### Logging & Archival
The gateway automatically archives all processed data relative to its working directory:
- **Audio Recordings:** Saved in `recordings/` (e.g., `20260101_120000_filename.wav`).
- **Transcripts:** Saved in `transcripts/` (e.g., `20260101_120000_uuid.txt`).

### Custom Word Replacements
You can define custom spelling corrections (e.g., mapping "Dovac" to "Dvorak") by adding entries to the configuration file.

**File Location:** `config/replacements.json` (relative to project root)

**How to add entries:**
Open the file and add a new object to the JSON array. Each object requires:
- `from`: An array of words/phrases the AI typically mishears.
- `to`: The single word you want it to be replaced with.

**Format Example:**
```json
[
  {
    "from": ["Dovac", "Dovak"],
    "to": "Dvorak"
  },
  {
    "from": ["Omarchy"],
    "to": "Omarchy"
  }
]
```
Changes to this file require a service restart:
```bash
sudo systemctl restart voice-gateway
```

See `project_context.md` for detailed specifications.

## Multi-Machine Development Workflow

This project runs on multiple machines (desktop + laptop). **Git is the ONLY sync method.**

### Sync Protocol

**When working on LAPTOP and need to sync to DESKTOP (or vice versa):**

1. **Push to a branch** from current machine:
   ```bash
   git checkout -b fix/descriptive-name
   git add .
   git commit -m "[voice-typing] Description of changes"
   git push -u origin fix/descriptive-name
   ```

2. **On the other machine**, pull the branch:
   ```bash
   cd ~/Programs/omarchy-voice-typing
   git fetch origin
   git checkout fix/descriptive-name
   ```

3. **User verifies** the changes work on that machine

4. **Continue working** on the branch until issue is fully resolved

5. **Merge to master** once user is happy:
   ```bash
   git checkout master
   git merge fix/descriptive-name
   git push origin master
   ```

### Branch Naming Convention
- `fix/` - Bug fixes (e.g., `fix/gateway-service-not-running`)
- `feature/` - New features
- `test/` - Testing changes

### Related Repos
Changes may also need to be synced in:
- `~/Programs/local-bootstrapping` - System config, systemd services, dotfiles

**IMPORTANT:** Same git workflow applies to local-bootstrapping. You CANNOT directly edit files on the other machine. Push to a branch, pull on the other machine, verify with user, then merge.

### Investigation Tracking
For debugging sessions spanning multiple machines:
- Use `status.md` in project root to track progress
- Commit frequently with `[voice-typing]` prefix
- Document timeline of what was tried and what worked
