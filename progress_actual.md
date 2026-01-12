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

## Smart Audio Fallback (2026-01-13)
- **New Feature**: Automatic audio source fallback for voice typing
  - Bluetooth headphones (WF-1000XM5) can be interrupted by phone
  - System now automatically falls back to laptop internal mic after 2nd silence
  - Restores BT when it becomes available again
- **Bluetooth Codec Preference**: SBC-XQ over AAC for stability
  - Config: `~/.config/pipewire/pipewire.conf.d/50-bluetooth-codec.conf`
  - Applied via `local-bootstrapping/scripts/setup-audio-bluetooth.sh`
- **Audio Source Monitor**: Runs as systemd user service
  - Script: `audio-source-monitor.sh`
  - Service: `systemd/audio-source-monitor.service`
  - Setup: `./setup-audio-fallback.sh`
  - Monitors PulseAudio/PipeWire sources every 2 seconds
  - Tracks silence count, switches to fallback after threshold
  - Logs to journal: `journalctl --user -u audio-source-monitor -f`

## Todo for Next Session
- Test audio fallback with actual BT headphone connection/disconnection
- Implement noise level measurement (currently simplified)
- Implement streaming (WebSocket) support.
- Deploy systemd service to `/etc/systemd/system/`.

## Session Cleanup
- Background processes terminated.
- Logs preserved in `logs/` for reference.
