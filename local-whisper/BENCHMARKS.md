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

## 2026-11-03: short voice clips, GPU vs CPU (power profile "balanced")

Three real voice-typing recordings (8.2 s, 10.5 s, 24.4 s), beam 5, `condition_on_previous_text=False`,
warm model, seconds to transcribe. CPU = i7-10510U (4 cores / 8 threads), int8. GPU = MX330, int8_float32.

| Model | Device | 8.2 s clip | 10.5 s clip | 24.4 s clip |
|-------|--------|-----------:|------------:|------------:|
| `distil-large-v3` | GPU | 21.4 | 21.1 | 23.5 |
| `distil-large-v3` | CPU, 4 threads | 31.2 | 34.0 | 32.6 |
| `distil-large-v3` | CPU, 8 threads | 37.4 | 33.9 | 34.8 |
| `base` | GPU | 2.7 | 2.4 | 4.3 |
| `base` | CPU, 4 threads | 2.3 | 2.2 | 2.7 |

- distil on the CPU is ~50 % slower than on the MX330; 8 threads do not help (hyper-threads).
- The time is nearly flat in clip length: Whisper encodes a full 30 s window per chunk.
- `base` on the CPU matches or beats `base` on the MX330 (clearly on the 24 s clip).

## 2026-11-03 late: the hybrid as deployed (base on the CPU, distil on the MX330 from 25 s)

His choice (2026-11-03): short dictations on `base`, from 25 s on `distil-large-v3`, the whole
recording after Stop. Measured through the gateway (`ws` → offline provider → local-whisper),
seconds from Stop to text, hotwords on:

| Clip | Model | Seconds |
|------|-------|--------:|
| 10.5 s | `base`, CPU int8, 4 threads | 1.1 |
| 74.7 s | `distil-large-v3`, MX330 int8_float32 | 72.4 |
| 74.7 s | `base`, CPU (distil not loaded) | 4.3 |

- distil runs at ~1× real time on long clips (72 s for 75 s; three 30 s windows at ~20.5 s
  each plus decoding), so a long dictation waits about as long as it lasted. Estimate before
  measuring was 0.8×; the measurement is the number.
- distil's encoder costs ~20.5 s per 30 s window on the MX330, the first pass as much as later
  ones (20.7 / 20.5 / 20.5 s on 1 s of silence), so a warm-up pass buys nothing.
- Ready after switching to Local: 5.0 s (model load ~4 s; the GPU wakes from suspend).
  With the old warm-up it was 46 s (silence decoded to the 448-token limit), then 26 s with
  the decode capped (one window). The long model now skips the warm-up.
- GPU memory with distil loaded: 994 MiB. With the source online nothing is on the GPU.
