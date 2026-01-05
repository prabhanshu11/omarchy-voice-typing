
# voice-gateway-voice-typing — Project Context (Agent-ready)

## Summary (one-liner)
Provide a minimal, correct, production-minded **AssemblyAI gateway** (Go) that lets `hyprwhspr` use AssemblyAI's REST transcription without forking upstream. Initial scope: REST (upload → create → poll), validate with `Harvard_speech100.wav`. Deployable via Docker and systemd. Streaming (WebSocket) is planned later.

---

## Required environment (must exist before running tests)
- `ASSEMBLYAI_API_KEY` exported in the shell or provided to service.
  - Example (persist across reboots): add `export ASSEMBLYAI_API_KEY="sk-..."` to `~/.bashrc` or use systemd EnvironmentFile.
- `Harvard_speech100.wav` present at project root for tests.
- `go` (>=1.19), `docker` (for container builds), `python3`, `pip` (for tests).

---

## Repo layout (approximate paths agent should create/use)
```
voice-gateway-voice-typing/
├── gateway/
│   ├── cmd/server/main.go
│   ├── internal/assemblyai/assemblyai.go
│   ├── internal/handlers/handlers.go
│   ├── sdk/go-client/main.go
│   ├── Dockerfile
│   └── systemd/voice-gateway-gateway.service
├── tests/
│   ├── test_assemblyai_direct.py       # optional helper: raw AssemblyAI upload+transcript
│   └── test_gateway_rest_integration.py
├── hyprwhspr-configs/
│   └── rest-gateway.json
├── Harvard_speech100.wav
└── project_context.md
```

---

## Agent (gemini-cli) hard constraints (do not violate)
1. Language: **Go** for the gateway. Only Python for tests (pytest + requests).
2. Work only inside these folders: `/gateway`, `/tests`, `/hyprwhspr-configs`. Agent must **not** modify other upstream files.
3. **Do not commit secrets.** Read `ASSEMBLYAI_API_KEY` from env at runtime; never write it to disk.
4. Tests-first: every feature must be accompanied by tests. Streaming tests are out of scope.
5. Branch name for commits: `feat/assemblyai-gateway`.
6. Stop on first failing test. Report logs. Do not proceed after failures.
7. Minimal invasive approach: Implement gateway that matches hyprwhspr `rest-api` contract; do not fork hyprwhspr initially.

---

## Gateway API contract (exact)
- `POST /v1/transcribe`
  - Accepts:
    - `multipart/form-data` with file field name `file`
    - OR JSON body `{ "audio_url": "https://..." }`
  - Behavior:
    1. If file supplied: upload bytes to AssemblyAI `POST https://api.assemblyai.com/v2/upload` using header `authorization: $ASSEMBLYAI_API_KEY`.
    2. Create transcript job `POST https://api.assemblyai.com/v2/transcript` with body:
       ```json
       {
         "audio_url": "<upload_url>",
         "punctuate": true,
         "format_text": true,
         "language_detection": true
       }
       ```
    3. Poll GET `/v2/transcript/{id}` until `status` is `completed` or `error`.
    4. Return JSON with at least:
       ```json
       {
         "text": "<final transcript text>",
         "raw": { /* full AssemblyAI final response */ },
         "duration_s": <float>
       }
       ```
    5. On error return HTTP 4xx/5xx with `{"error": "...", "details": ...}`.

- `GET /v1/transcribe/{job_id}` (optional) — return current job status and partials.

---

## Tests (exact files to produce)
1. `tests/test_assemblyai_direct.py` (optional helper)
   - Purpose: raw AssemblyAI sanity test (upload → create → poll). Uses `ASSEMBLYAI_API_KEY`. Fails loudly if any step errors.
2. `tests/test_gateway_rest_integration.py` (required)
   - Purpose: integration test that POSTs `Harvard_speech100.wav` to `http://127.0.0.1:8765/v1/transcribe` and asserts the returned JSON has non-empty `text`.
   - Use `requests` + `pytest`.
   - The test must exit non-zero on assertion failure.

Test environment variables:
- `ASSEMBLYAI_API_KEY` must be set for gateway to work.
- Optionally `GATEWAY_URL` for tests (default `http://127.0.0.1:8765`).

---

## hyprwhspr config example (exact file content)
`hyprwhspr-configs/rest-gateway.json`
```json
{
  "transcription_backend": "rest-api",
  "rest_endpoint_url": "http://127.0.0.1:8765/v1/transcribe",
  "rest_timeout": 120
}
```
Instruction: copy this into `~/.config/hyprwhspr/config.json` or configure using hyprwhspr setup to point to the gateway.

---

## Dockerfile (agent should produce this)
- Multistage Go build (builder + minimal runtime).
- Expose port `8765`.
- Entrypoint runs the compiled binary.

---

## systemd unit (agent should produce this)
`gateway/systemd/voice-gateway-gateway.service` example:
```ini
[Unit]
Description=Voice Gateway AssemblyAI Gateway
After=network.target

[Service]
Type=simple
EnvironmentFile=/etc/voice-gateway/voice-gateway.env
ExecStart=/usr/local/bin/voice-gateway-gateway
Restart=on-failure
User=youruser
WorkingDirectory=/opt/voice-gateway-voice-typing/gateway

[Install]
WantedBy=multi-user.target
```
- Agent should add note to create `/etc/voice-gateway/voice-gateway.env` and set `chmod 600`.

---

## Run & test commands (exact sequence for agent or dev)
1. Build & run gateway (local dev)
```bash
# in gateway/
go build -o voice-gateway-gateway ./cmd/server
ASSEMBLYAI_API_KEY="$ASSEMBLYAI_API_KEY" ./voice-gateway-gateway
```
2. Run gateway in Docker (example)
```bash
docker build -t voice-gateway-gateway gateway/
docker run -e ASSEMBLYAI_API_KEY="$ASSEMBLYAI_API_KEY" -p 8765:8765 voice-gateway-gateway
```
3. Run Python tests (assuming gateway is running)
```bash
pip install -r requirements-test.txt   # requests, pytest
pytest tests/test_gateway_rest_integration.py -q
```

---

## Developer guidance & constraints for implementation
- Use Go standard lib + one HTTP lib if needed (e.g., net/http, gorilla/mux or chi). Keep dependencies minimal.
- For AssemblyAI HTTP calls, use context with timeouts and exponential backoff on 429/5xx.
- Log request/response shapes (stdout JSON), but **strip or never print** `ASSEMBLYAI_API_KEY`.
- Poll interval: default 2s (configurable via flags/env). Max wait: default 120s.
- Accept WAV and MP3. If conversion is required, note `ffmpeg` command (not mandatory).
- Branch name: `feat/assemblyai-gateway`. Use clear, atomic commits.

---

## Assumptions (explicit)
1. You will persist `ASSEMBLYAI_API_KEY` in your shell or systemd environment before testing (you already exported it).
2. `Harvard_speech100.wav` is present at project root.
3. Initial integration is REST only; streaming will be handled later.
4. Agent has network access to `api.assemblyai.com` and GitHub.

---

## Minimal checklist for gemini-cli (copy-paste into agent)
- Clone `https://github.com/goodroot/hyprwhspr` locally (agent may use but must not modify upstream).
- Create project tree above in working directory.
- Implement Go gateway following the API contract.
- Implement `tests/test_gateway_rest_integration.py`.
- Build binary and run tests locally; stop on any failing test.
- Commit on branch `feat/assemblyai-gateway` with small, logical commits.
- Provide patch/PR and test output logs.

---

## Contact points (when agent stops)
If tests fail, agent must output:
- failing test name
- stdout/stderr logs from gateway (last 200 lines)
- exact HTTP request/response bodies (masked for secrets), and suggestions for next steps.

---

*End of project_context.md*
