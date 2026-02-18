# Streaming Chunked Transcription Architecture

## Problem
Current flow: Record → Stop → Upload (slow) → Transcribe → Return
For a 6-minute speech: ~17s upload + ~10s transcription = 27s latency after stopping

## Solution: Adaptive Chunked Streaming

### Core Idea
Instead of waiting for recording to stop, detect silences and send overlapping chunks during recording.

```
Audio Stream: ─────────────────────────────────────────►
                   │         │              │
                   ▼         ▼              ▼
              [silence]  [silence]     [silence]
                   │         │              │
              ┌────┴────┐ ┌──┴──┐      ┌────┴────┐
              │ Chunk 1 │ │ Ch2 │      │ Chunk 3 │
              └────┬────┘ └──┬──┘      └────┬────┘
                   │    ╲    │    ╲         │
                   ▼     ╲   ▼     ╲        ▼
              [Req 1]  [Req 2]   [Req 3]
                   │         │         │
                   └────┬────┴────┬────┘
                        ▼         ▼
                   [Merge & Dedupe]
                        │
                        ▼
                   [Final Transcript]
```

## Adaptive Chunk Trigger Formula

```
log_β(total_running_time) = α × current_silence_time + C
```

Rearranged to get trigger threshold:
```
silence_threshold = (log_β(T) - C) / α

Where:
  T = total recording time so far (seconds)
  α = silence sensitivity (higher = need longer silence)
  β = time scaling factor (logarithmic base)
  C = constant offset
```

### Behavior
- **Early in recording (T small)**: threshold is small → trigger on short silences
- **Later (T large)**: threshold grows logarithmically → wait for longer pauses

### Default Starting Values (tune via experiments)
```
α = 2.0      # sensitivity
β = 2.0      # log base
C = -0.5    # offset
```

## Components

### 1. Silence Detection (VAD)
**Use: Silero VAD** (SOTA, <1ms per frame, MIT license)

```python
import torch
torch.set_num_threads(1)

model, utils = torch.hub.load('snakers4/silero-vad', 'silero_vad')
(get_speech_timestamps, _, _, _, _) = utils

# Returns list of {start, end} speech segments
speech_timestamps = get_speech_timestamps(audio, model, sampling_rate=16000)
```

### 2. Chunker
- Maintains rolling buffer of audio
- On silence detection exceeding threshold:
  - Extract chunk with 500ms overlap on both ends
  - Send to transcription
  - Keep overlap in buffer for next chunk

### 3. Parallel Request Manager
- Fire Request N+1 before Request N completes
- Track sequence numbers
- Handle out-of-order completions

### 4. Transcript Merger
- Fuzzy match overlapping regions
- Use edit distance or word-level alignment
- Deduplicate repeated phrases

### 5. Comprehensive Logger
Log everything for parameter tuning:
```json
{
  "session_id": "uuid",
  "timestamp": "ISO8601",
  "event": "chunk_sent|response_received|merged",
  "chunk_number": 1,
  "audio_duration_ms": 3500,
  "silence_duration_ms": 450,
  "running_time_ms": 12000,
  "threshold_used_ms": 380,
  "upload_time_ms": 1200,
  "transcription_time_ms": 800,
  "total_latency_ms": 2000,
  "transcript_length": 45,
  "overlap_words": 3,
  "params": {"alpha": 2.0, "beta": 2.0, "c": -0.5}
}
```

## Implementation Plan

### Phase 1: VAD Integration
1. Add Silero VAD to gateway (Python sidecar or Go port)
2. Stream audio through VAD
3. Detect speech/silence boundaries

### Phase 2: Chunking Logic
1. Implement adaptive threshold calculator
2. Build audio buffer with overlap management
3. Trigger chunk extraction on silence

### Phase 3: Parallel Requests
1. Async HTTP client for overlapping requests
2. Sequence tracking
3. Timeout and retry handling

### Phase 4: Merging
1. Word-level alignment for overlaps
2. Confidence-based selection
3. Final transcript assembly

### Phase 5: Logging & Experiments
1. Structured logging to JSON/SQLite
2. Web UI for viewing experiment data
3. DSPy integration for parameter optimization

## DSPy Integration Points

1. **Transcript Merging**: Optimize prompts for overlap resolution
2. **Parameter Tuning**: Learn α, β, C from logged data
3. **Quality Assessment**: Score transcripts vs corrections

## File Structure
```
omarchy-voice-typing/
├── streaming/
│   ├── vad.py              # Silero VAD wrapper
│   ├── chunker.py          # Adaptive chunking logic
│   ├── parallel_sender.py  # Concurrent request manager
│   ├── merger.py           # Transcript combination
│   └── logger.py           # Experiment logging
├── experiments/
│   ├── logs/               # JSON log files
│   └── analyze.py          # Parameter analysis
└── web/                    # Review UI
```

## Metrics to Track
- Time to first word (T1W)
- Total latency (stop → complete transcript)
- Word error rate (WER) vs non-chunked
- Chunk count per session
- Overlap duplication rate
