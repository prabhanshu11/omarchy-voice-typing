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
1. **BT profile switch: ~1.5s** — `hyprwhspr-toggle-v2` calls `switch_to_hfp` which does
   `pactl set-card-profile` then `sleep 1.5` (recording starts BEFORE the switch for instant
   OSD feedback — audio capture records silence until HFP source activates)
2. **Verification timeout: 500ms** — System `_start_recording()` calls `verify_and_play_sound()`
   which waits up to 500ms for the first audio callback. With BT, PipeWire may need longer.
3. **Stability check: 200ms** — `verify_stream_stable()` waits another 200ms to confirm
   audio keeps flowing
4. **Deepgram connect: ~900ms** — Gateway connects to Deepgram synchronously on first audio chunk.
   Blocks the WebSocket read loop, queuing audio in gorilla/websocket's buffer.

**Total worst case: ~3.1s** from keypress to first audio reaching Deepgram.

## SOLVED: Mute detection kills BT recordings (diagnosed Feb 18 2026)

**Problem:** Recordings with BT headphones would silently fail ~80% of the time. Python logged
"Recording started" but no audio ever reached the gateway. No error messages. User had to press
Super+` 3-7 times before a recording would succeed.

**Root cause:** System mute detection at `/usr/lib/hyprwhspr/lib/main.py:937-982`.
The audio level monitor samples every 100ms, and after 10 consecutive samples below
threshold `5e-7` (1 second of silence), calls `_cancel_recording_muted()`. BT headphones
in HFP mode produce digital silence for 1-3 seconds while the codec initializes. The mute
detector fires before real audio flows, silently canceling the recording.

**Why it was invisible:** `_cancel_recording_muted()` (line 654-675) sets `is_recording=False`,
stops audio capture, and plays an error sound — but prints **zero log output** on the success
path. Only `except Exception` at line 668 logs anything. So journalctl shows "Recording started"
then nothing — the recording appears to vanish.

**Diagnostic evidence (from live session):**
```
# Python: 6 starts, 0 stops — each "started" then silently killed by mute detector
03:59:13  [CONTROL] Recording start requested → Recording started
03:59:17  [CONTROL] Recording start requested → Recording started   # is_recording was False again!
03:59:22  [CONTROL] Recording start requested → Recording started
03:59:28  [CONTROL] Recording start requested → Recording started
03:59:33  [CONTROL] Recording start requested → Recording started
03:59:54  [CONTROL] Recording start requested → Recording started   # this one finally worked

# Gateway: connected to Deepgram each time, but got only 1 chunk then silence
03:59:13  [rec-006] Recording started → Deepgram connected
03:59:47  [Deepgram] ReadLoop ended: "did not receive audio data within timeout"
```

**Fix applied:** Set `"mute_detection": false` in `~/.config/hyprwhspr/config.json`.
This disables the system's zero-volume cancellation entirely.

**Better long-term fix (not yet implemented):**
- Delay mute detection for the first 3-5s of recording when BT source is active
- Or increase `samples_to_cancel` from 10 (1s) to 30-50 (3-5s) for BT sources
- This preserves mute protection for actual muted-mic scenarios while tolerating BT startup

**Config location:** `~/.config/hyprwhspr/config.json` → `"mute_detection": false`

## OSD not showing during recording (observed Feb 18 2026)

**Problem:** After restarting hyprwhspr, the MicOSD overlay does not appear during recording.
The Python logs show `[MIC-OSD] Found orphaned daemon (PID XXXX), reusing it` on every
recording start. The daemon process is running but the OSD window doesn't become visible.

**Likely cause:** The MicOSD daemon (GTK4 + layer-shell) loses its Wayland connection
or layer surface when the parent hyprwhspr process restarts. The "orphaned daemon" detection
reuses the stale process instead of spawning a fresh one.

**Investigation needed:**
- Check if killing the orphaned daemon PID and letting a new one spawn fixes it
- May need to add a `kill_orphaned_daemon()` step in `_patched_init` or `_patched_start_recording`
- See `local-bootstrapping/docs/mic-osd-lessons.md` for GTK4 layer-shell quirks

## Potential fixes for BT latency (not yet implemented)

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
