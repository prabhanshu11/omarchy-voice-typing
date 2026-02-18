"""
Streaming chunked transcription system.

Components:
- VAD: Silero-based voice activity detection
- Chunker: Adaptive silence-based audio chunking
- Parallel Sender: Concurrent transcription requests
- Merger: Transcript deduplication and combination
- Logger: Experiment logging for parameter tuning
"""

from .vad import (
    SileroVAD,
    get_vad,
    SpeechSegment,
    VADResult,
    calculate_chunk_threshold,
    should_trigger_chunk
)

from .chunker import (
    AudioChunk,
    AdaptiveChunker
)

from .parallel_sender import (
    TranscriptionResult,
    ParallelSender,
    SyncParallelSender
)

from .merger import (
    MergedTranscript,
    merge_transcripts,
    TranscriptBuffer
)

from .logger import (
    ExperimentLogger,
    ChunkEvent,
    ResponseEvent,
    MergeEvent,
    load_experiment_logs,
    analyze_params
)

__all__ = [
    # VAD
    'SileroVAD',
    'get_vad',
    'SpeechSegment',
    'VADResult',
    'calculate_chunk_threshold',
    'should_trigger_chunk',
    # Chunker
    'AudioChunk',
    'AdaptiveChunker',
    # Sender
    'TranscriptionResult',
    'ParallelSender',
    'SyncParallelSender',
    # Merger
    'MergedTranscript',
    'merge_transcripts',
    'TranscriptBuffer',
    # Logger
    'ExperimentLogger',
    'ChunkEvent',
    'ResponseEvent',
    'MergeEvent',
    'load_experiment_logs',
    'analyze_params',
]
