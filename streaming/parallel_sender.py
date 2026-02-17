"""
Parallel request manager for overlapping transcription requests.

Fires multiple requests concurrently and handles out-of-order responses.
"""

import asyncio
import aiohttp
import time
from dataclasses import dataclass
from typing import Optional, Dict, List, Callable, Any
from datetime import datetime
import logging

from .chunker import AudioChunk
from .logger import ExperimentLogger

logger = logging.getLogger(__name__)


@dataclass
class TranscriptionResult:
    """Result of a transcription request."""
    chunk_id: str
    sequence_number: int
    transcript: str
    upload_time_ms: int
    transcription_time_ms: int
    total_time_ms: int
    error: Optional[str] = None
    raw_response: Optional[Dict] = None


class ParallelSender:
    """
    Manages parallel transcription requests.

    - Fires requests without waiting for previous to complete
    - Handles out-of-order responses
    - Tracks timing for logging
    """

    def __init__(
        self,
        endpoint_url: str = "http://127.0.0.1:8765/v1/transcribe",
        max_concurrent: int = 3,
        timeout_seconds: int = 120,
        experiment_logger: Optional[ExperimentLogger] = None
    ):
        """
        Initialize parallel sender.

        Args:
            endpoint_url: Transcription API endpoint
            max_concurrent: Max concurrent requests
            timeout_seconds: Request timeout
            experiment_logger: Logger for experiments
        """
        self.endpoint_url = endpoint_url
        self.max_concurrent = max_concurrent
        self.timeout_seconds = timeout_seconds
        self.logger = experiment_logger

        # State
        self.pending_requests: Dict[str, asyncio.Task] = {}
        self.results: Dict[int, TranscriptionResult] = {}  # keyed by sequence
        self.semaphore: Optional[asyncio.Semaphore] = None

        # Callbacks
        self.on_result: Optional[Callable[[TranscriptionResult], None]] = None

    async def _ensure_semaphore(self):
        """Create semaphore if not exists (must be in async context)."""
        if self.semaphore is None:
            self.semaphore = asyncio.Semaphore(self.max_concurrent)

    async def send_chunk(self, chunk: AudioChunk) -> TranscriptionResult:
        """
        Send a single chunk for transcription.

        Args:
            chunk: Audio chunk to transcribe

        Returns:
            TranscriptionResult
        """
        await self._ensure_semaphore()

        async with self.semaphore:
            start_time = time.time()
            upload_end_time = None

            try:
                # Prepare form data
                audio_bytes = chunk.to_bytes()

                timeout = aiohttp.ClientTimeout(total=self.timeout_seconds)
                async with aiohttp.ClientSession(timeout=timeout) as session:
                    form = aiohttp.FormData()
                    form.add_field(
                        'file',
                        audio_bytes,
                        filename=f'{chunk.chunk_id}.wav',
                        content_type='audio/wav'
                    )

                    # Upload
                    async with session.post(self.endpoint_url, data=form) as response:
                        upload_end_time = time.time()

                        if response.status != 200:
                            error_text = await response.text()
                            raise Exception(f"HTTP {response.status}: {error_text}")

                        result_json = await response.json()

                # Parse response
                end_time = time.time()
                upload_ms = int((upload_end_time - start_time) * 1000)
                transcription_ms = int((end_time - upload_end_time) * 1000)
                total_ms = int((end_time - start_time) * 1000)

                transcript = result_json.get('text', '')

                result = TranscriptionResult(
                    chunk_id=chunk.chunk_id,
                    sequence_number=chunk.sequence_number,
                    transcript=transcript,
                    upload_time_ms=upload_ms,
                    transcription_time_ms=transcription_ms,
                    total_time_ms=total_ms,
                    raw_response=result_json
                )

                # Log
                if self.logger:
                    self.logger.log_response_received(
                        chunk_id=chunk.chunk_id,
                        sequence_number=chunk.sequence_number,
                        upload_time_ms=upload_ms,
                        transcription_time_ms=transcription_ms,
                        transcript=transcript
                    )

                return result

            except Exception as e:
                error_msg = str(e)
                logger.error(f"Transcription failed for chunk {chunk.chunk_id}: {error_msg}")

                upload_ms = int((upload_end_time - start_time) * 1000) if upload_end_time else 0
                total_ms = int((time.time() - start_time) * 1000)

                result = TranscriptionResult(
                    chunk_id=chunk.chunk_id,
                    sequence_number=chunk.sequence_number,
                    transcript="",
                    upload_time_ms=upload_ms,
                    transcription_time_ms=0,
                    total_time_ms=total_ms,
                    error=error_msg
                )

                if self.logger:
                    self.logger.log_response_received(
                        chunk_id=chunk.chunk_id,
                        sequence_number=chunk.sequence_number,
                        upload_time_ms=upload_ms,
                        transcription_time_ms=0,
                        transcript="",
                        error=error_msg
                    )

                return result

    async def send_chunk_async(self, chunk: AudioChunk):
        """
        Fire and forget chunk transcription.

        Result will be stored in self.results and callback fired.
        """
        task = asyncio.create_task(self._send_and_store(chunk))
        self.pending_requests[chunk.chunk_id] = task

    async def _send_and_store(self, chunk: AudioChunk):
        """Send chunk and store result."""
        try:
            result = await self.send_chunk(chunk)
            self.results[chunk.sequence_number] = result

            # Fire callback
            if self.on_result:
                self.on_result(result)

            return result
        finally:
            self.pending_requests.pop(chunk.chunk_id, None)

    async def wait_for_all(self) -> List[TranscriptionResult]:
        """Wait for all pending requests to complete."""
        if self.pending_requests:
            await asyncio.gather(*self.pending_requests.values())

        # Return results in sequence order
        return [
            self.results[seq]
            for seq in sorted(self.results.keys())
        ]

    def get_completed_results(self) -> List[TranscriptionResult]:
        """Get completed results in sequence order."""
        return [
            self.results[seq]
            for seq in sorted(self.results.keys())
        ]

    def get_pending_count(self) -> int:
        """Get number of pending requests."""
        return len(self.pending_requests)


class SyncParallelSender:
    """
    Synchronous wrapper for ParallelSender.

    Useful when not in async context.
    """

    def __init__(self, **kwargs):
        self.async_sender = ParallelSender(**kwargs)
        self._loop: Optional[asyncio.AbstractEventLoop] = None

    def _get_loop(self) -> asyncio.AbstractEventLoop:
        """Get or create event loop."""
        try:
            return asyncio.get_running_loop()
        except RuntimeError:
            if self._loop is None or self._loop.is_closed():
                self._loop = asyncio.new_event_loop()
            return self._loop

    def send_chunk(self, chunk: AudioChunk) -> TranscriptionResult:
        """Send chunk synchronously."""
        loop = self._get_loop()
        return loop.run_until_complete(self.async_sender.send_chunk(chunk))

    def send_chunk_background(self, chunk: AudioChunk):
        """Fire chunk in background."""
        loop = self._get_loop()
        loop.run_until_complete(self.async_sender.send_chunk_async(chunk))

    def wait_for_all(self) -> List[TranscriptionResult]:
        """Wait for all pending requests."""
        loop = self._get_loop()
        return loop.run_until_complete(self.async_sender.wait_for_all())

    @property
    def on_result(self):
        return self.async_sender.on_result

    @on_result.setter
    def on_result(self, callback):
        self.async_sender.on_result = callback
