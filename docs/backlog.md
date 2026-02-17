# Backlog

## Mandatory: Verify waybar mic indicator after ANY voice typing change

After ANY change to hyprwhspr-toggle, main.py, gateway realtime.go, systemd service files:

1. `~/.local/bin/hyprwhspr-tray.sh status` → valid JSON, class is `ready` (not `error:model_missing`)
2. Trigger recording (Super+backtick) → tray class becomes `recording`
3. Stop recording → tray returns to `ready`
4. Confirm `~/.local/bin/hyprwhspr-tray.sh` symlink exists
5. Verify services: `systemctl --user is-active hyprwhspr voice-gateway`
6. If you restarted `voice-gateway`, confirm hyprwhspr also restarted (PartOf= dependency)

**Rationale:** The tray reads `recording_status` and `audio_level` from `~/.config/hyprwhspr/`.
Pipeline changes can break these files' update timing without obvious errors. The `error:model_missing`
bug (realtime-ws not recognized) has been fixed once — watch for regressions.

**Service dependency (Feb 2026):** `hyprwhspr` has `PartOf=voice-gateway.service` in its override.
This means restarting the gateway automatically cascades to hyprwhspr. Without this, the Python
WebSocket client's connection breaks, reconnect attempts exhaust (5 retries), and all subsequent
recordings silently produce no transcription. If you ever remove or modify the override, preserve
the `PartOf=` directive.

## Recording start latency with BT headphones (user-reported Feb 18 2026)

**Problem:** With Bluetooth headphones (A2DP → HFP profile switch), there's a long delay
between pressing Super+` and the recording actually starting. The user has to wait significantly,
and words get clipped at the beginning. If they wait too long, the system times out.

**Root cause chain (each adds latency):**
1. **BT profile switch: ~3s** — `hyprwhspr-toggle-v2` calls `switch_to_hfp` which does
   `pactl set-card-profile` then `sleep 3` (PipeWire needs time to create the HFP source node)
2. **Verification timeout: 500ms** — System `_start_recording()` calls `verify_and_play_sound()`
   which waits up to 500ms for the first audio callback. With BT, PipeWire may need longer.
3. **Stability check: 200ms** — `verify_stream_stable()` waits another 200ms to confirm
   audio keeps flowing
4. **Deepgram connect: ~900ms** — Gateway connects to Deepgram synchronously on first audio chunk.
   Blocks the WebSocket read loop, queuing audio in gorilla/websocket's buffer.

**Total worst case: ~4.6s** from keypress to first audio reaching Deepgram.

**Silent failure mode:** If `verify_and_play_sound()` fails (BT too slow), the system code at
`/usr/lib/hyprwhspr/lib/main.py:557-576` sets `is_recording = False` and calls
`_notify_zero_volume()` — but prints **nothing** to stdout. Journalctl shows "Recording started"
then silence. The recording appears to start then auto-stop with no visible error.

**Potential fixes (not yet implemented):**
- Increase verify timeout from 500ms → 2000ms for BT sources (requires full `_start_recording` override in monkey-patch)
- Start BT profile switch earlier (pre-switch on first keypress, confirm on second)
- Pre-warm Deepgram connection: connect on BT switch, not on first audio chunk
- Show OSD immediately on keypress with a "connecting..." state, transition to waveform when audio flows
- Make OSD show even the tiny digital noise from BT mic activation as visual confirmation

**Design consideration:** The user wants the OSD to appear instantly on keypress and show
audio waveform immediately — even the faint digital noise from HFP activation should be visible,
so they can correlate "headphone switched to talk mode" with "OSD shows audio signal" before
they start speaking.

## Mic OSD color based on transcription backend

Change the mic overlay color to indicate which backend is being used (Deepgram vs local-whisper vs offline/no connection). Gives immediate visual feedback on connectivity state while recording.

**Complexity notes:**
- Involves gtk4-layer-shell OSD (LD_PRELOAD quirks, see `local-bootstrapping/docs/mic-osd-lessons.md`)
- Must work identically on both desktop and laptop
- Notification/OSD changes historically don't work on first try — budget time for cross-device testing
