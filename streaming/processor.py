"""
Main streaming transcription processor.

Orchestrates VAD, chunking, parallel sending, and merging
into a single high-level interface.
"""

import asyncio
import numpy as np
from typing import Optional, Callable, Dict, List
from dataclasses import dataclass
import logging

from .vad import get_vad, should_trigger_chunk
from .chunker import AdaptiveChunker, AudioChunk
from .parallel_sender import ParallelSender, TranscriptionResult
from .merger import TranscriptBuffer, MergedTranscript
from .logger import ExperimentLogger

logger = logging.getLogger(__name__)


@dataclass
class StreamingConfig:
    """Configuration for streaming processor."""
    # VAD/Chunking params
    alpha: float = 2.0
    beta: float = 2.0
    c: float = -0.5

    # Chunk settings
    overlap_ms: int = 500
    min_chunk_ms: int = 1000
    max_chunk_ms: int = 30000

    # Sender settings
    endpoint_url: str = "http://127.0.0.1:8765/v1/transcribe"
    max_concurrent: int = 3
    timeout_seconds: int = 120

    # Logging
    enable_logging: bool = True
    log_dir: Optional[str] = None


class StreamingProcessor:
    """
    High-level streaming transcription processor.

    Usage:
        processor = StreamingProcessor(config)

        # Option 1: Process full audio file
        result = await processor.process_file("audio.wav")

        # Option 2: Stream audio chunks
        processor.start_session()
        for audio_chunk in audio_stream:
            processor.add_audio(audio_chunk)
        result = await processor.finish_session()
    """

    def __init__(self, config: Optional[StreamingConfig] = None):
        self.config = config or StreamingConfig()

        # Components
        self.chunker = AdaptiveChunker(
            overlap_ms=self.config.overlap_ms,
            min_chunk_ms=self.config.min_chunk_ms,
            max_chunk_ms=self.config.max_chunk_ms,
            params={
                'alpha': self.config.alpha,
                'beta': self.config.beta,
                'c': self.config.c
            }
        )

        self.sender = ParallelSender(
            endpoint_url=self.config.endpoint_url,
            max_concurrent=self.config.max_concurrent,
            timeout_seconds=self.config.timeout_seconds
        )

        self.transcript_buffer: Optional[TranscriptBuffer] = None
        self.logger: Optional[ExperimentLogger] = None

        # Callbacks
        self.on_partial_transcript: Optional[Callable[[str], None]] = None
        self.on_chunk_sent: Optional[Callable[[AudioChunk], None]] = None
        self.on_result: Optional[Callable[[TranscriptionResult], None]] = None

        # State
        self._session_active = False

    def start_session(self):
        """Start a new transcription session."""
        self.chunker.reset()
        self.transcript_buffer = TranscriptBuffer(
            overlap_hint_words=int(self.config.overlap_ms / 100)  # ~100ms per word
        )

        if self.config.enable_logging:
            self.logger = ExperimentLogger(log_dir=self.config.log_dir)
            self.sender.logger = self.logger

        # Set up result callback
        self.sender.on_result = self._on_transcription_result

        self._session_active = True
        logger.info(f"Started streaming session: {self.chunker.session_id}")

    def _on_transcription_result(self, result: TranscriptionResult):
        """Handle incoming transcription result."""
        if result.error:
            logger.warning(f"Chunk {result.sequence_number} failed: {result.error}")
            return

        # Add to buffer
        self.transcript_buffer.add_transcript(
            result.sequence_number,
            result.transcript
        )

        # Fire callbacks
        if self.on_result:
            self.on_result(result)

        if self.on_partial_transcript:
            self.on_partial_transcript(self.transcript_buffer.get_current_text())

    async def add_audio(self, audio: np.ndarray):
        """
        Add audio samples and process.

        May trigger chunk sending if silence detected.
        """
        if not self._session_active:
            raise RuntimeError("No active session. Call start_session() first.")

        chunk = self.chunker.add_audio(audio)

        if chunk:
            await self._send_chunk(chunk)

    async def _send_chunk(self, chunk: AudioChunk):
        """Send chunk for transcription."""
        # Log chunk
        if self.logger:
            vad_result = get_vad().analyze_for_chunking(
                chunk.audio,
                chunk.duration_ms
            )
            _, threshold = should_trigger_chunk(
                vad_result,
                chunk.end_ms,
                self.chunker.params
            )

            self.logger.log_chunk_sent(
                chunk_id=chunk.chunk_id,
                sequence_number=chunk.sequence_number,
                audio_duration_ms=chunk.duration_ms,
                silence_duration_ms=vad_result.silence_duration_ms,
                running_time_ms=chunk.end_ms,
                threshold_used_ms=threshold,
                overlap_ms=chunk.overlap_start_ms,
                params=self.chunker.params
            )

        # Fire callback
        if self.on_chunk_sent:
            self.on_chunk_sent(chunk)

        # Send asynchronously
        await self.sender.send_chunk_async(chunk)

    async def finish_session(self) -> MergedTranscript:
        """
        Finish session and return final transcript.

        Flushes remaining audio and waits for all responses.
        """
        if not self._session_active:
            raise RuntimeError("No active session.")

        # Flush remaining audio
        final_chunk = self.chunker.flush()
        if final_chunk:
            await self._send_chunk(final_chunk)

        # Wait for all pending requests
        await self.sender.wait_for_all()

        # Finalize transcript
        result = self.transcript_buffer.finalize()

        # Log merge
        if self.logger:
            self.logger.log_merge(
                chunks_merged=result.chunks_merged,
                overlap_words=result.overlap_words_removed,
                duplicates_removed=result.overlap_words_removed,
                final_transcript=result.text
            )
            self.logger.save_summary()

        self._session_active = False
        logger.info(
            f"Session complete: {result.chunks_merged} chunks, "
            f"{len(result.text)} chars"
        )

        return result

    async def process_file(self, audio_path: str) -> MergedTranscript:
        """
        Process an entire audio file.

        Args:
            audio_path: Path to audio file (wav)

        Returns:
            MergedTranscript
        """
        import wave

        self.start_session()

        # Read audio file
        with wave.open(audio_path, 'rb') as wf:
            sample_rate = wf.getframerate()
            n_channels = wf.getnchannels()
            sample_width = wf.getsampwidth()
            n_frames = wf.getnframes()

            # Read in chunks of 1 second
            chunk_frames = sample_rate
            position = 0

            while position < n_frames:
                frames_to_read = min(chunk_frames, n_frames - position)
                audio_bytes = wf.readframes(frames_to_read)
                position += frames_to_read

                # Convert to numpy float32
                if sample_width == 2:
                    audio = np.frombuffer(audio_bytes, dtype=np.int16)
                    audio = audio.astype(np.float32) / 32768.0
                elif sample_width == 4:
                    audio = np.frombuffer(audio_bytes, dtype=np.int32)
                    audio = audio.astype(np.float32) / 2147483648.0
                else:
                    raise ValueError(f"Unsupported sample width: {sample_width}")

                # Convert stereo to mono if needed
                if n_channels == 2:
                    audio = audio.reshape(-1, 2).mean(axis=1)

                await self.add_audio(audio)

        return await self.finish_session()


# Convenience function for simple use
async def transcribe_streaming(
    audio_path: str,
    config: Optional[StreamingConfig] = None,
    on_partial: Optional[Callable[[str], None]] = None
) -> str:
    """
    Transcribe audio file using streaming chunked approach.

    Args:
        audio_path: Path to audio file
        config: Optional configuration
        on_partial: Callback for partial transcripts

    Returns:
        Final transcript text
    """
    processor = StreamingProcessor(config)
    processor.on_partial_transcript = on_partial

    result = await processor.process_file(audio_path)
    return result.text
