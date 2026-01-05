# Internal Progress Notes

## Current State
- Gateway is fully functional and verified.
- Tests passed for both WAV and MP3 (Harvard speech sample).
- Docker image `voice-gateway-gateway` built successfully using Go 1.25.
- `ASSEMBLY_API_KEY` validated (variable name in .env is `ASSEMBLY_API_KEY`, but code expects `ASSEMBLYAI_API_KEY` or `ASSEMBLY_API_KEY`).

## Critical Configuration (Speed & Stability)
- **Timeouts**: Gateway requires explicit 600s timeouts on `http.Server` and 300s on `http.Client` to handle large WAV uploads (25MB+).
- **Context**: Go handlers use `request.Context()` to ensure client cancellations/timeouts are respected.
- **Docker**: Requires `golang:1.25-alpine` (or newer) due to `go.mod` settings.

## Todo for Next Session
- Implement streaming (WebSocket) support.
- Deploy systemd service to `/etc/systemd/system/`.

## Session Cleanup
- Background processes terminated.
- Logs preserved in `logs/` for reference.
