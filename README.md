# Voice Gateway Voice Typing Gateway

A minimal, production-minded AssemblyAI gateway written in Go. This service allows `hyprwhspr` to use AssemblyAI's REST transcription API without forking the upstream project.

## Summary
The goal is to provide a deployable gateway (via Docker or systemd) that accepts audio via REST, uploads it to AssemblyAI, polls for the transcript, and returns the result.

## Structure
- `gateway/`: Go source code for the server.
- `tests/`: Python integration tests (`pytest`).
- `hyprwhspr-configs/`: Configuration snippets for `hyprwhspr`.

## Prerequisites
- **Go** (>=1.19)
- **Python 3** (for tests)
- **AssemblyAI API Key**: Must be exported as `ASSEMBLYAI_API_KEY`.

## Usage

### Local Development
1.  **Build and Run Gateway:**
    ```bash
    cd gateway
    go build -o voice-gateway ./cmd/server
    ASSEMBLYAI_API_KEY="your_key" ./voice-gateway
    ```

2.  **Run Tests:**
    ```bash
    # From project root
    uv run pytest tests/test_gateway_rest_integration.py
    ```

### Hyprwhspr Integration
1.  **The Hotkey**
    Press **`Super + Alt + D`** to start dictating.
    *   **Mode:** Toggle. Press once to start, speak, and press again to stop.
    *   **Audio Feedback:** You will hear a `ping-up.ogg` when it starts and `ping-down.ogg` when it stops.

2.  **Live Monitoring**
    To watch the gateway process audio in real-time, run:
    ```bash
    journalctl --user -f -u voice-gateway.service
    ```

### Docker
```bash
docker build -t voice-gateway gateway/
docker run -e ASSEMBLYAI_API_KEY="$ASSEMBLYAI_API_KEY" -p 8765:8765 voice-gateway
```

## API
- `POST /v1/transcribe`: Upload audio file or provide URL for transcription.

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
