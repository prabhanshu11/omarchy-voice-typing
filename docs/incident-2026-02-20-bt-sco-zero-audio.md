# Incident: BT SCO Zero Audio After Process Crash

**Date:** 2026-02-20 (00:00–04:00 IST)
**Machine:** Desktop (omarchy, PRIME A520M-K)
**BT adapter:** TP-Link UB500 (Realtek RTL8761B) on USB bus 1, PCI 0000:01:00.0
**Anchor:** enthixercdghrieugprfprfyhduidbotxhkutehvw

---

## What Happened (User's Perspective)

Voice typing was working fine (00:02–00:39 IST, producing 56–1332 char transcripts).
After a hyprwhspr crash and restart at ~00:45, pressing the voice typing key did nothing.
First, the toggle was stuck (always sending "stop"). After fixing that, recordings
completed the full cycle but produced empty transcripts. The user lost 2 recordings
worth of work before realizing the issue — no error was shown anywhere.

The user tried different headphones (WF-1000XM5 earbuds instead of WH-1000XM6).
Same result: complete silence.

## Two Separate Bugs, Same Session

### Bug 1: Stale recording_status deadlock

**Symptom:** Every toggle press sends "stop", hyprwhspr says "Not currently recording,
ignoring stop request". Permanent deadlock — voice typing completely non-functional.

**Root cause:** hyprwhspr was killed with SIGKILL (signal 9) at 00:47:03. The process
never ran its cleanup code, so `~/.config/hyprwhspr/recording_status` was left containing
`true`. The toggle script checks this file to decide start vs stop — with it stuck on
"true", every press takes the "stop" branch.

**How it was hiding:** The toggle script printed nothing on the "stop" path, and hyprwhspr's
"Not currently recording" message was only visible in journalctl. The user just sees:
press key → nothing happens.

**Fix (committed to local-bootstrapping as 6b8aeb2):**
1. `main.py _patched_init`: Unconditionally delete recording_status on startup
   (hyprwhspr can never be recording at startup, so the file is always stale)
2. `hyprwhspr-toggle-v2 is_recording()`: Compare recording_status file mtime against
   process start time. If process is newer than file, the file is stale — clear it.

**Lesson for omarchy devs:** Any persistent state file that gates control flow (like
recording_status) needs crash recovery. If the process that writes it can be killed
ungracefully, the state will be wrong on next start. Defense: always reconcile state
against reality (is the process actually recording?) rather than trusting a file.

---

### Bug 2: BT SCO transport sending zero-filled frames

**Symptom:** Voice typing completes the full cycle — toggle works, audio is captured for
the correct duration, sent to gateway, forwarded to Deepgram — but the transcript is
empty. Saved WAV files contain near-silence (RMS 0.1–27.3 on 16-bit scale where speech
is ~2000+). PipeWire shows the BT input source as RUNNING.

**Root cause:** The BT adapter's SCO (Synchronous Connection-Oriented) transport, which
carries bidirectional audio for HFP, got stuck sending zero-filled frames. The SCO link
was active (PipeWire saw audio "flowing" at 16kHz), the node was RUNNING, pw-top showed
non-zero BUSY time — but the actual PCM samples were all zeros.

This happened after the hyprwhspr crash/restart. The exact trigger is unclear — possibly
the abrupt audio stream disconnection left the BT controller firmware in a bad state where
the SCO socket was "open" but not actually carrying microphone data.

**How it was hiding:** This is the nastiest kind of failure. PipeWire reports RUNNING.
wpctl shows volume at 1.00. The audio buffer has non-zero byte count. The duration is
correct. Deepgram connects and processes the audio — it just returns an empty transcript
because the audio is silence. There is zero indication of failure anywhere in the
pipeline. The user speaks, waits, stops recording, and gets... nothing.

---

## Diagnostic Journey: What Was Tried (In Order)

Everything below was tried for Bug 2. All produced RMS ~0 until #14.

| # | Action | Result | Notes |
|---|--------|--------|-------|
| 1 | Direct pw-record from BT mic | RMS=1.1 | Confirmed issue is below hyprwhspr |
| 2 | Check BT profile | Was a2dp-sink | A2DP has no mic! But switching to HFP didn't help |
| 3 | Switch to HFP (mSBC codec) | RMS=0.0 | HFP active but zero audio |
| 4 | BT disconnect + reconnect | RMS=0.0 | Headphones reconnected, still dead |
| 5 | CVSD codec (alternative HFP codec) | RMS=6.8 | Marginally better, still silence |
| 6 | Full PipeWire restart (pipewire + pipewire-pulse + wireplumber) | RMS=0.0 | Didn't help |
| 7 | Bluetooth systemd service restart | RMS=0.3 | Didn't help |
| 8 | Different headphones (WF-1000XM5) | RMS=0.0 | **Proved it's not headphone-specific** |
| 9 | Test ALL audio inputs via sounddevice | All RMS=0.0 | Even motherboard ALC897 (no mic plugged in) |
| 10 | Native 16kHz mono recording | RMS=0.0 | Rules out resampling artifact |
| 11 | Record from raw hardware node (bypass WirePlumber smart filter) | RMS=0.0 | Issue below PipeWire entirely |
| 12 | btusb kernel module unload + reload | RMS=4.0 | Marginal improvement, controller partially reset |
| 13 | USB device deauthorize/reauthorize (sysfs authorized=0→1) | RMS=1.4 | USB reset not deep enough |
| **14** | **`bluetoothctl power off && sleep 2 && bluetoothctl power on`** | **RMS=6.1→40.1** | **FIXED** |

**Key insight from the escalation ladder:**
- Tests 1–7: Software-level resets (PipeWire, BlueZ service, device reconnect) — none worked
- Test 8: Different hardware (different headphones) — ruled out headphone fault
- Tests 10–11: Different capture points (raw node, native rate) — ruled out PipeWire routing/resampling
- Test 12: Kernel module reload — marginal improvement, hinting the issue is at controller level
- Test 13: USB device reset — not deep enough
- Test 14: Full adapter power cycle — works because it resets the controller firmware

**The hierarchy of BT resets (from least to most effective):**
```
Device disconnect/reconnect     → resets BT link, not controller
Profile switch (A2DP↔HFP)       → renegotiates audio codec, not controller
PipeWire restart                → resets audio pipeline, not BT stack
Bluetooth service restart       → restarts BlueZ daemon, controller keeps state
btusb module reload             → reloads driver, partial controller reset
USB authorized toggle           → USB device reset, not controller firmware
bluetoothctl power off/on       → FULL controller firmware reset ← THIS WORKS
PCI FLR reset                   → nuclear option, resets entire USB controller
System reboot                   → resets everything
```

---

## Discoveries Useful for Omarchy Developers

### 1. PipeWire "RUNNING" does not mean audio is flowing

A BT input node can show state=RUNNING, non-zero BUSY time in pw-top, and deliver
frames at the expected rate — while every sample is zero. PipeWire has no concept of
"audio quality" — it moves samples from A to B. If the hardware sends zeros, PipeWire
faithfully delivers zeros.

**Implication:** Any audio app on omarchy that uses BT mic needs its own silence
detection. You cannot rely on PipeWire state to know if audio is actually present.

### 2. The WirePlumber smart filter architecture

BT audio on PipeWire/WirePlumber uses a 3-layer node chain:
```
Node 85: bluez_input_internal (raw hardware, SCO/mSBC codec)
    ↓ smart filter (filter.smart=True, node.link-group=loopback-*)
Node 108: bluez_capture_internal (internal stream)
    ↓ published to applications
Node 86: bluez_input (public source, what apps see)
```

When debugging BT audio, you need to check the **internal** node to see if the issue
is in PipeWire routing or in the hardware itself. Recording from node 85 proved the
zeros were coming from the BT controller, not from PipeWire routing.

### 3. Two BT headphones, same problem = adapter/controller issue

When testing WF-1000XM5 earbuds after WH-1000XM6 headphones both produced zero audio,
this immediately pointed to the BT adapter (TP-Link UB500, Realtek RTL8761B), not the
headphones. The adapter firmware was stuck.

### 4. btusb module reload is a partial reset

Unloading and reloading btusb caused a marginal improvement (RMS 0→4). This suggests
the module reload partially resets the controller but doesn't fully clear the firmware
state. The `bluetoothctl power off/on` sends an HCI reset command to the controller,
which is more thorough.

### 5. Voice typing silent failure is the worst UX failure mode

The user spoke for 6 seconds, waited, pressed stop, and got nothing. Twice. There was
no beep, no notification, no visual indicator that the recording was empty. This is
worse than an error — it's a lie by omission. The system said "recording started" and
"recording stopped" and produced nothing.

**Fix implemented:** Gateway now computes RMS of the audio buffer before sending to
Deepgram. If RMS < 100 (silence threshold), it:
- Logs WARN: "SILENCE DETECTED — mic may be broken or muted"
- Skips the Deepgram API call (saves cost and 2s latency)
- Returns empty transcript immediately
- Archives the silent WAV for debugging
- Logs status as "SILENCE" (distinct from "OK_EMPTY")

**Not yet implemented (TODO):** Play an error sound and/or show a notification when
silence is detected. Currently the detection only lives in the gateway logs.

### 6. Crash recovery needs to be defense-in-depth

The SIGKILL at 00:47 caused two cascading failures:
1. Stale recording_status file → toggle deadlock (Bug 1)
2. Abrupt audio stream disconnect → BT controller firmware stuck (Bug 2)

Bug 1 was fixable with a simple file check. Bug 2 required a BT adapter power cycle.
The lesson: when a voice typing process crashes, the recovery procedure should be:
1. Clear stale state files (recording_status, audio_level, etc.)
2. Power-cycle the BT adapter if BT mic was in use
3. Restart the voice typing stack

This could be automated in a crash handler or a "recover" command.

---

## Files Modified This Session

### In `local-bootstrapping` repo (committed + pushed as 6b8aeb2):
- `dotfiles/local-lib/hyprwhspr-patch/main.py` — Fix 7: stale recording_status cleanup on init
- `dotfiles/local-bin/hyprwhspr-toggle-v2` — Staleness guard: compare file mtime vs process start

### In `omarchy-voice-typing` repo (branch `fix/silent-recording-failure`):
- `gateway-rs/src/audio.rs` — `compute_rms_i16()` + `SILENCE_RMS_THRESHOLD` (commit 7a05334)
- `gateway-rs/src/handlers/realtime_session.rs` — Silence detection at commit time (commit 7a05334)
- `docs/backlog.md` — Bug report + root cause documentation (commits e84b127, 2269d15)
- `docs/incident-2026-02-20-bt-sco-zero-audio.md` — This document

---

## Future Work

1. **Automated BT recovery:** When gateway detects silence on consecutive recordings,
   automatically run `bluetoothctl power off/on` and reconnect.

2. **User-facing silence notification:** Play error sound + show notification when
   silence is detected, so the user knows immediately instead of finding out after stopping.

3. **Crash handler for hyprwhspr:** On restart after abnormal exit, run full recovery
   (clear state files + BT power cycle if BT was in use).

4. **Pre-recording mic check:** Before starting a recording, capture 100ms of audio and
   verify RMS > threshold. If not, alert the user that the mic seems dead.
