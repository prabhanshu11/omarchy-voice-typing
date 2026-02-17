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

## Python 3.14 Compatibility Fix (2026-02-17)

**Problem**: System upgraded Python 3.13 → 3.14 on Feb 16. Voice typing completely broken:
- `hyprwhspr.service` crash-looping (25,505+ restarts) with `ImportError: No module named 'sounddevice'`
- Package `python-sounddevice` 0.5.3-1 installed to `/usr/lib/python3.13/site-packages/` but Python 3.14 looks in `/usr/lib/python3.14/site-packages/`
- Also affected: `python-pulsectl` (AUR), `python-websocket-client` (missing)

**Fix applied**:
1. Updated `python-sounddevice` 0.5.3-1 → 0.5.5-1 via `yay -S python-sounddevice` (AUR rebuild for Python 3.14)
2. Rebuilt `python-pulsectl` via `yay -S python-pulsectl` (same version, new Python target)
3. Installed `python-websocket-client` 1.9.0-3 via `pacman -S` (was missing, required for `realtime-ws` backend)

**Deepgram API key setup**:
- Added `DEEPGRAM_API_KEY` to `.env` for streaming (realtime-ws) transcription
- Stored in `pass insert api/deepgram` on desktop
- Gateway now shows: `Deepgram API key loaded`, `Connected to streaming API (sample_rate=24000)`

**Bluetooth headphone mic fix (Sony WH-1000XM6)**:
- A2DP profile = no mic (source produces silence). HFP profile = mic works.
- WirePlumber autoswitch too slow — race condition with hyprwhspr's 500ms verification timeout
- Fix: Modified `hyprwhspr-toggle-v2` to manually switch BT profile:
  - Before recording: `pactl set-card-profile <card> headset-head-unit` + 1.5s wait
  - After recording: background `pactl set-card-profile <card> a2dp-sink` after 3s delay
- Also fixed stale Bluetooth transport (error 24) by restarting WirePlumber

**Verification**:
- Transcription test: "That. Yes. I can see the OSD now." — 33 chars, Deepgram backend, 1.5s transcribe time
- Audio: 22.1s recorded, archived to `recordings/20260217_225919_audio.wav`

## Todo for Next Session
- Test full Win+` toggle cycle (start → speak → stop → text pasted) with the updated toggle script
- Consider making BT profile switch configurable (not all users have BT headphones)
- Implement noise level measurement (currently simplified)
- Deploy systemd service to `/etc/systemd/system/`

## Session Cleanup
- Background processes terminated.
- Logs preserved in `logs/` for reference.
