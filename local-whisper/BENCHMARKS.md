# Local Whisper Benchmarks — NVIDIA GeForce MX330

Benchmarked 2026-02-13 on ThinkPad T14 Gen 1 (laptop).

## GPU Specs

| Property | Value |
|----------|-------|
| GPU | NVIDIA GeForce MX330 |
| VRAM | 2048 MB |
| Architecture | Pascal (GP108) |
| Compute Capability | 6.1 |
| float16 support | No (too slow / unsupported on Pascal consumer) |
| Display | Not connected (dedicated compute GPU) |
| Idle VRAM | ~49 MB |

## Audio Test File

- **File**: `Harvard_speech100.wav` — Harvard sentences, clear English speech
- **Duration**: 263.3s (~4.4 min)
- **Size**: 24.1 MB (PCM16, 24kHz, mono)

## Model Comparison (CUDA, int8_float32)

All tests run with `--device cuda --compute int8_float32` on the MX330.

| Model | Load (s) | Transcribe (s) | RTF | VRAM (MB) | Notes |
|-------|----------|-----------------|-----|-----------|-------|
| `tiny` | ~1 | ~15 | 0.06x | ~200 | Low accuracy |
| `base` | ~1 | ~76 | 0.29x | 225 | Good accuracy, fast |
| `small` | ~2 | ~180 | 0.68x | ~609 | Good accuracy |
| `distil-large-v3` | ~5 | ~327 | 1.24x | 1089 | Best accuracy, slower than real-time |
| `distil-medium.en` | ~3 | ~200 | ~0.76x | ~700 | English-only distilled |
| `medium` | 344 | OOM | — | >2048 | **OOM during transcription** |

## Compute Type Comparison (model: `small`)

| Config | Load (s) | Transcribe (s) | RTF | VRAM (MB) |
|--------|----------|-----------------|-----|-----------|
| cuda/float32 | 1.76 | 344.93 | 1.31x | 1365 |
| cuda/int8 | ~2 | ~180 | ~0.68x | ~550 |
| cuda/int8_float32 | ~2 | ~180 | 0.68x | 609 |
| cpu/int8 | ~2 | ~300 | ~1.14x | N/A |
| cpu/float32 | ~2 | ~400 | ~1.52x | N/A |
| cuda/float16 | — | FAIL | — | — | Not supported on Pascal |

## Key Findings

1. **float16 is not supported** on MX330 (Pascal consumer GPU, compute 6.1). All float16 attempts fail.

2. **int8_float32 is the sweet spot** for this GPU — good balance of speed and VRAM usage.

3. **distil-large-v3 fits in 2GB VRAM** (1089 MB) but runs at 1.24x RTF (slower than real-time). For a 5-second voice clip, this means ~6 seconds of transcription — acceptable for voice typing.

4. **medium model OOMs** — loads (344s!) but crashes during transcription. Too large for 2GB.

5. **base model is the fast option** — 0.29x RTF with 225MB VRAM. Good for quick transcription when accuracy is less critical.

## Recommendations

### Default: `distil-large-v3` / `int8_float32`
- Best accuracy available on this GPU
- 1089 MB VRAM — fits comfortably
- ~1.2x RTF — for typical voice typing clips (3-10s), latency is 4-12s
- Worth the wait for accuracy

### Fast mode: `base` / `int8_float32`
- Switchable at runtime via `GET /switch?model=base`
- 225 MB VRAM — minimal GPU usage
- 0.29x RTF — near-instant for short clips
- Good enough for quick notes, commands

### Not recommended
- `medium` — OOMs on MX330
- `small/float32` — 1.31x RTF and 1365 MB VRAM, worse than distil-large-v3 at similar speed
- Any `float16` — not supported on Pascal

## CTranslate2 JIT Warmup

CTranslate2 JIT-compiles CUDA kernels on the first inference call. Model loading only puts weights into VRAM — compute kernels are compiled lazily.

**Impact without warmup**: First transcription takes dramatically longer than subsequent ones.

| Model | First-call time (4s audio) | After warmup |
|-------|---------------------------|-------------|
| `distil-large-v3` | ~41s | ~5s |
| `base` | ~10s (estimated) | <1s |

**Warmup solution**: Transcribe 1s of silence at startup to force kernel compilation.

| Model | Warmup time | Notes |
|-------|------------|-------|
| `base` | 4.0s | Measured. Default model. |
| `distil-large-v3` | untested | Expected longer due to larger model. |

The warmup runs in a background thread after model loading completes. The HTTP server accepts connections immediately — requests during warmup get a 503 "model not ready" response.

**Default model changed to `base`** (from `distil-large-v3`) because:
1. Warmup completes in 4s vs unknown longer time for distil-large-v3
2. Transcription is sub-second for typical voice clips (3-10s)
3. VRAM usage is minimal (225 MB vs 1089 MB)
4. Accuracy is sufficient for voice typing use case
5. Can switch to distil-large-v3 at runtime via `GET /switch?model=distil-large-v3`
