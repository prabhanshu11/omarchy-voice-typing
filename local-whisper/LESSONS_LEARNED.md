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

## Quick Troubleshooting

| Symptom | Check |
|---------|-------|
| No whisper logs at all | `PYTHONUNBUFFERED=1` missing? |
| First transcription very slow | Warmup didn't run — check startup logs |
| Empty transcript (offline) | Stale Deepgram? Check `closeDeepgram()` before offline path |
| Empty transcript (online) | Deepgram WebSocket dead? Check `[Deepgram] ReadLoop ended` |
| Model switch hangs | Check VRAM — previous model may not have freed memory |
| "model not ready" 503 | Model still loading or warmup in progress |
