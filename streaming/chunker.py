"""
Adaptive audio chunker with overlap management.

Chunks audio at silence boundaries with configurable overlap
for seamless transcript merging.
"""

import numpy as np
from dataclasses import dataclass, field
from typing import Optional, List, Callable
from datetime import datetime
import uuid
import logging

from .vad import get_vad, should_trigger_chunk, VADResult

logger = logging.getLogger(__name__)


@dataclass
class AudioChunk:
    """A chunk of audio ready for transcription."""
    chunk_id: str
    sequence_number: int
    audio: np.ndarray
    start_ms: int  # Position in full recording
    end_ms: int
    overlap_start_ms: int  # Overlap with previous chunk
    overlap_end_ms: int  # Overlap for next chunk
    timestamp: datetime = field(default_factory=datetime.utcnow)

    @property
    def duration_ms(self) -> int:
        return self.end_ms - self.start_ms

    @property
    def sample_rate(self) -> int:
        return 16000  # Standard

    def to_bytes(self) -> bytes:
        """Convert to WAV bytes for transmission."""
        import io
        import wave

        buffer = io.BytesIO()
        with wave.open(buffer, 'wb') as wf:
            wf.setnchannels(1)
            wf.setsampwidth(2)  # 16-bit
            wf.setframerate(self.sample_rate)
            # Convert float32 to int16
            audio_int16 = (self.audio * 32767).astype(np.int16)
            wf.writeframes(audio_int16.tobytes())

        buffer.seek(0)
        return buffer.read()


class AdaptiveChunker:
    """
    Chunks audio stream at adaptive silence boundaries.

    Uses VAD to detect speech/silence and triggers chunks
    based on adaptive threshold formula.
    """

    def __init__(
        self,
        sample_rate: int = 16000,
        overlap_ms: int = 500,
        min_chunk_ms: int = 1000,
        max_chunk_ms: int = 30000,
        params: Optional[dict] = None
    ):
        """
        Initialize chunker.

        Args:
            sample_rate: Audio sample rate
            overlap_ms: Overlap duration between chunks
            min_chunk_ms: Minimum chunk duration
            max_chunk_ms: Maximum chunk duration (force split)
            params: VAD threshold params (alpha, beta, c)
        """
        self.sample_rate = sample_rate
        self.overlap_ms = overlap_ms
        self.min_chunk_ms = min_chunk_ms
        self.max_chunk_ms = max_chunk_ms
        self.params = params or {'alpha': 2.0, 'beta': 2.0, 'c': -0.5}

        self.vad = get_vad()

        # State
        self.buffer: np.ndarray = np.array([], dtype=np.float32)
        self.chunk_start_ms = 0
        self.sequence_number = 0
        self.session_id = str(uuid.uuid4())
        self.last_chunk_end_samples = 0

        # Callbacks
        self.on_chunk: Optional[Callable[[AudioChunk], None]] = None

    def reset(self):
        """Reset chunker state for new recording session."""
        self.buffer = np.array([], dtype=np.float32)
        self.chunk_start_ms = 0
        self.sequence_number = 0
        self.session_id = str(uuid.uuid4())
        self.last_chunk_end_samples = 0

    def _samples_to_ms(self, samples: int) -> int:
        return int(samples * 1000 / self.sample_rate)

    def _ms_to_samples(self, ms: int) -> int:
        return int(ms * self.sample_rate / 1000)

    def add_audio(self, audio: np.ndarray) -> Optional[AudioChunk]:
        """
        Add audio samples to buffer and check for chunk trigger.

        Args:
            audio: Audio samples (float32, mono)

        Returns:
            AudioChunk if triggered, None otherwise
        """
        # Append to buffer
        self.buffer = np.concatenate([self.buffer, audio])

        buffer_duration_ms = self._samples_to_ms(len(self.buffer))
        total_time_ms = self.chunk_start_ms + buffer_duration_ms

        # Check minimum duration
        if buffer_duration_ms < self.min_chunk_ms:
            return None

        # Force chunk at max duration
        if buffer_duration_ms >= self.max_chunk_ms:
            logger.info(f"Force chunking at max duration: {buffer_duration_ms}ms")
            return self._create_chunk(force=True)

        # Analyze for silence
        vad_result = self.vad.analyze_for_chunking(self.buffer, buffer_duration_ms)

        # Check if we should trigger
        should_trigger, threshold = should_trigger_chunk(
            vad_result, total_time_ms, self.params
        )

        if should_trigger:
            logger.info(
                f"Triggering chunk: silence={vad_result.silence_duration_ms}ms, "
                f"threshold={threshold}ms, total_time={total_time_ms}ms"
            )
            return self._create_chunk(vad_result=vad_result, threshold=threshold)

        return None

    def _create_chunk(
        self,
        force: bool = False,
        vad_result: Optional[VADResult] = None,
        threshold: int = 0
    ) -> AudioChunk:
        """Create a chunk from current buffer."""
        overlap_samples = self._ms_to_samples(self.overlap_ms)

        # Determine chunk boundaries
        if vad_result and vad_result.speech_segments:
            # End chunk at last speech end + some padding
            last_speech = vad_result.speech_segments[-1]
            end_samples = self._ms_to_samples(last_speech.end_ms + 100)
            end_samples = min(end_samples, len(self.buffer))
        else:
            # Use full buffer minus overlap reserve
            end_samples = len(self.buffer)

        # Include overlap from previous chunk
        chunk_audio = self.buffer[:end_samples].copy()

        # Calculate overlap regions
        overlap_start_ms = 0
        if self.sequence_number > 0:
            overlap_start_ms = self.overlap_ms

        chunk = AudioChunk(
            chunk_id=f"{self.session_id}_{self.sequence_number}",
            sequence_number=self.sequence_number,
            audio=chunk_audio,
            start_ms=self.chunk_start_ms,
            end_ms=self.chunk_start_ms + self._samples_to_ms(end_samples),
            overlap_start_ms=overlap_start_ms,
            overlap_end_ms=self.overlap_ms
        )

        # Update state - keep overlap for next chunk
        keep_samples = max(0, len(self.buffer) - end_samples + overlap_samples)
        if keep_samples > 0:
            self.buffer = self.buffer[-keep_samples:].copy()
        else:
            self.buffer = np.array([], dtype=np.float32)

        self.chunk_start_ms = chunk.end_ms - self.overlap_ms
        self.sequence_number += 1
        self.last_chunk_end_samples = end_samples

        # Fire callback
        if self.on_chunk:
            self.on_chunk(chunk)

        return chunk

    def flush(self) -> Optional[AudioChunk]:
        """
        Flush remaining audio as final chunk.

        Call this when recording stops.
        """
        if len(self.buffer) < self._ms_to_samples(100):  # Skip tiny remnants
            return None

        return self._create_chunk(force=True)

    def get_stats(self) -> dict:
        """Get chunker statistics."""
        return {
            'session_id': self.session_id,
            'chunks_created': self.sequence_number,
            'buffer_duration_ms': self._samples_to_ms(len(self.buffer)),
            'total_time_ms': self.chunk_start_ms + self._samples_to_ms(len(self.buffer)),
            'params': self.params.copy()
        }
