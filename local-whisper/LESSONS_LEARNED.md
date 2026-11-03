# Local Whisper — Lessons Learned

Hard-won knowledge from debugging offline voice typing on ThinkPad T14 (MX330).

## CTranslate2 JIT Warmup Problem

**Symptom**: First transcription after service start takes 41s for 4s of audio (distil-large-v3). Subsequent calls are fast.

**Root cause**: CTranslate2 JIT-compiles CUDA kernels on the first inference call. Model loading (`WhisperModel(...)`) only loads weights into VRAM — the actual compute kernels aren't compiled until `model.transcribe()` runs.

**Solution**: Warmup function that transcribes 1s of silence after model loads:
```python
def warmup_model():
    silence = np.zeros(16000, dtype=np.float32)  # 1s at 16kHz
    segments, _ = _model.transcribe(silence, beam_size=5)
    _ = list(segments)  # force lazy iterator evaluation
```

**Measured warmup times** (int8_float32 on MX330):
- `base`: 4.0s
- `distil-large-v3`: expected longer (untested)

**Key detail**: The warmup thread waits for model loading to complete before running. Both load and warmup happen in background threads so the HTTP server starts accepting connections immediately.

## Model Selection: base vs distil-large-v3

For offline voice typing (short clips, 3-10s):

| Model | RTF | First-call penalty | Warmup | VRAM |
|-------|-----|--------------------|--------|------|
| `base` | 0.29x | ~10s (estimated) | 4.0s | 225 MB |
| `distil-large-v3` | 1.24x | 41s | untested | 1089 MB |

**Default is `base`** — warmup completes in 4s, transcription is sub-second for short clips. Accuracy is good enough for voice typing.

**Switch at runtime** if you need better accuracy:
```bash
curl http://localhost:8767/switch?model=distil-large-v3
```

## 2026-11-03: the laptop ran distil-large-v3 for 8 months

The 02-13 switch to `base` was made in this repo's unit file, but the unit that is actually deployed
is local-bootstrapping `dotfiles/systemd/user/local-whisper.service`, tracked on 02-14 with
`distil-large-v3`. Result: every local recording took a flat ~21-23 s on the MX330 (5 s or 25 s of
audio alike), noticed when the popup's Local source was used. Fixed in the deployed unit: `base`
measures 2.5-2.8 s for 8-10 s clips (warm), 3.0-3.3 s end to end through the gateway. Greedy
decoding (beam 1) saved only 10-20 % on the same clips, so beam 5 stays.
**Lesson:** the deployed unit lives in local-bootstrapping; change it there, not only here.

## Stale Deepgram WebSocket Race Condition

**Symptom**: First offline transcription works. Second recording produces empty transcript.

**Root cause**: When Deepgram connection fails and we fall back to local whisper, the gateway's `handleAudioCommit()` checked `offline || dgNil` to decide the transcription path. But after a successful Deepgram connection that later went stale, `deepgramClient` was non-nil (stale reference) and `offlineMode` was false (set during initial connect success). So the second recording took the online path with a dead WebSocket.

**Fix**: In the offline path of `handleAudioCommit()`, explicitly call `closeDeepgram()` before `transcribeLocal()` to clean up any stale connection:
```go
if offline || dgNil {
    s.closeDeepgram()  // clean up stale connection
    fullTranscript, backend = s.transcribeLocal(audioData)
}
```

**The race sequence was**:
1. Gateway starts → connects Deepgram → `offlineMode=false`, `deepgramClient=<live>`
2. Network drops → Deepgram WebSocket dies silently
3. User records → audio sent to dead WebSocket → empty finals
4. Commit → online path (stale client) → empty transcript
5. Next session → still has stale reference → same problem

## Python stdout Buffering in systemd

**Symptom**: `journalctl --user -u local-whisper` shows no output even though server is running.

**Root cause**: Python buffers stdout by default. When running under systemd (no TTY), output stays in the buffer and never reaches journald.

**Fix**: Set `PYTHONUNBUFFERED=1` in the systemd service file:
```ini
Environment=PYTHONUNBUFFERED=1
```

Alternative: use `python -u` flag or `sys.stdout.reconfigure(line_buffering=True)`.

## Diagnostic Logging

The gateway (`realtime.go`) has detailed logging at every decision point:

- `[Realtime]` — session lifecycle, path decisions (online vs offline)
- `[Deepgram]` — interim/final transcripts, connection state
- `[Whisper]` — WAV building, POST timing, response parsing
- `[Archive]` — recording/transcript file saves

**Where to find logs**:
```bash
# Gateway (Go)
journalctl --user -u voice-gateway -f

# Local whisper (Python)
journalctl --user -u local-whisper -f

# Both together
journalctl --user -u voice-gateway -u local-whisper -f
```

## 2026-11-03: the hybrid (base on the CPU, distil on the GPU from 25 s)

- **Free a GPU model by ending its process.** Deleting a CTranslate2 CUDA model leaves the
  CUDA context (~300 MB, GPU kept awake) until the process exits. So the long model runs as a
  child process of server.py (same script, 127.0.0.1:8777), started while the source is Local
  and terminated when it is not.
- **Warm-ups on silence decode to the token limit.** Whisper "hears" words in 1 s of silence
  and decodes up to 448 tokens with beam 5: distil's warm-up took 41 s. Cap it
  (`max_new_tokens=4`). And on the MX330 even a capped pass costs a full 30 s window
  (~20.5 s) with no first-run penalty to save, so the long model skips warm-up altogether.
- **ThreadingHTTPServer.** A plain HTTPServer answers /health only after a running 20-70 s
  distil transcription finishes; the popup's Source button polls /health while loading.
- **Bound the caller by audio length.** The gateway's fixed 120 s local timeout would have cut
  distil off past ~2.5 minutes of speech; it is now max(120 s, 1.2 s per audio second + 30 s).

## Quick Troubleshooting

| Symptom | Check |
|---------|-------|
| No whisper logs at all | `PYTHONUNBUFFERED=1` missing? |
| First transcription very slow | Warmup didn't run — check startup logs |
| Empty transcript (offline) | Stale Deepgram? Check `closeDeepgram()` before offline path |
| Empty transcript (online) | Deepgram WebSocket dead? Check `[Deepgram] ReadLoop ended` |
| Model switch hangs | Check VRAM — previous model may not have freed memory |
| "model not ready" 503 | Model still loading or warmup in progress |
| Long dictation came back from base | `curl localhost:8767/health` → `long.state`: distil loads only while the source is Local; `error` = it failed (retries with back-off) |
