# 2026-09-30 — hyprwhspr deadlock: voice message recovered from a core dump

## What happened (IST)
- 02:17:42 recording (32 s) — gateway got it, Deepgram transcribed it, but the final text never reached hyprwhspr;
  hyprwhspr timed out after 30 s ("No transcription generated"). No WAV saved (gateway only saves on success).
  Text recovered from `journalctl --user -u voice-gateway` Deepgram `final text=` lines.
- 02:18:20 onward the gateway stopped reading hyprwhspr's websocket (2.6 MB stuck in the TCP send queue).
- 02:24:28 next recording: `audio_callback` holds `AudioCapture.lock` while calling the streaming callback →
  `ws.send()` blocks forever → stop press, record thread and main thread all wait on the lock. Deadlock.
  Nothing on disk, nothing in the gateway. Audio only existed in process memory.

## Recovery
1. `sudo gcore` of the live process → `hyprwhspr.1442246.core` (gitignored: contains secrets) + `maps.txt`.
2. `uv run extract_audio.py hyprwhspr.1442246.core .` — finds the `list` of float32[1024] numpy chunks
   (PyList_Type from libpython symbols; ndarray type via the `numpy.ndarray` tp_name string in the .so on disk,
   because gcore omits read-only file mappings) and writes `recovered_list0_660chunks.wav` (42.2 s, gitignored).
3. Deepgram nova-3 prerecorded → `transcript.txt`. Stops mid-sentence: the callback froze at 42 s.

Failed attempts: `sys.remote_exec` (runs on the main thread, which was itself blocked on the lock);
`ss -K` on the stuck socket unblocked the send but the lock stayed held.

## Root causes to fix (lane: voice-typing data-loss investigation)
- Network I/O inside `AudioCapture.lock` (upstream hyprwhspr `audio_capture.py` audio_callback).
- Blocking `ws.send` with no timeout; gateway session stalls without closing the socket.
- Audio is persisted by the gateway only after a successful transcription; the client keeps nothing on disk.

## Correction and follow-up (investigation lane, 03:30 IST)
- The 02:17:42 recording *was* saved eventually. The gateway sat 1050 s in an unbounded Deepgram close wait,
  then wrote `recordings/20260929_204742_audio.wav` and `transcripts/20260929_204742_deepgram.txt`
  (338 chars) at 02:35:46. Nothing was on disk at the time of the loss.
- Root causes, history, loss counts and the fix: `docs/issues/2026-09-30-voice-data-loss.md`.
