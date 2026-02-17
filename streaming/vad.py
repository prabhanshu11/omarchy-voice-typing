"""
Voice Activity Detection using Silero VAD.

Silero VAD is state-of-the-art for speech/silence detection:
- <1ms per frame inference
- Works offline
- MIT licensed
"""

import torch
import numpy as np
from typing import List, Dict, Optional, Tuple
from dataclasses import dataclass
import logging

logger = logging.getLogger(__name__)

# Optimize for CPU inference
torch.set_num_threads(1)


@dataclass
class SpeechSegment:
    """A segment of detected speech."""
    start_ms: int
    end_ms: int

    @property
    def duration_ms(self) -> int:
        return self.end_ms - self.start_ms


@dataclass
class VADResult:
    """Result of VAD analysis."""
    speech_segments: List[SpeechSegment]
    is_currently_speech: bool
    silence_duration_ms: int  # Duration since last speech ended


class SileroVAD:
    """Wrapper for Silero VAD model."""

    def __init__(self, threshold: float = 0.5, sampling_rate: int = 16000):
        """
        Initialize Silero VAD.

        Args:
            threshold: Speech probability threshold (0-1)
            sampling_rate: Audio sample rate (8000 or 16000)
        """
        self.threshold = threshold
        self.sampling_rate = sampling_rate
        self.model = None
        self.utils = None
        self._loaded = False

    def load(self):
        """Lazy load the model."""
        if self._loaded:
            return

        logger.info("Loading Silero VAD model...")
        self.model, self.utils = torch.hub.load(
            repo_or_dir='snakers4/silero-vad',
            model='silero_vad',
            force_reload=False,
            trust_repo=True
        )
        self._loaded = True
        logger.info("Silero VAD model loaded")

    def get_speech_timestamps(
        self,
        audio: np.ndarray,
        min_speech_duration_ms: int = 250,
        min_silence_duration_ms: int = 100,
        speech_pad_ms: int = 30
    ) -> List[SpeechSegment]:
        """
        Get timestamps of speech segments in audio.

        Args:
            audio: Audio samples as numpy array (float32, -1 to 1)
            min_speech_duration_ms: Minimum speech segment duration
            min_silence_duration_ms: Minimum silence to split segments
            speech_pad_ms: Padding around speech segments

        Returns:
            List of SpeechSegment with start/end in milliseconds
        """
        self.load()

        # Convert to torch tensor
        if isinstance(audio, np.ndarray):
            audio_tensor = torch.from_numpy(audio).float()
        else:
            audio_tensor = audio

        # Ensure 1D
        if audio_tensor.dim() > 1:
            audio_tensor = audio_tensor.squeeze()

        # Get speech timestamps using Silero's utility
        get_speech_timestamps = self.utils[0]

        timestamps = get_speech_timestamps(
            audio_tensor,
            self.model,
            sampling_rate=self.sampling_rate,
            threshold=self.threshold,
            min_speech_duration_ms=min_speech_duration_ms,
            min_silence_duration_ms=min_silence_duration_ms,
            speech_pad_ms=speech_pad_ms
        )

        # Convert sample indices to milliseconds
        segments = []
        for ts in timestamps:
            start_ms = int(ts['start'] * 1000 / self.sampling_rate)
            end_ms = int(ts['end'] * 1000 / self.sampling_rate)
            segments.append(SpeechSegment(start_ms=start_ms, end_ms=end_ms))

        return segments

    def analyze_for_chunking(
        self,
        audio: np.ndarray,
        current_time_ms: int
    ) -> VADResult:
        """
        Analyze audio for chunking decision.

        Args:
            audio: Audio samples
            current_time_ms: Current position in recording

        Returns:
            VADResult with speech info and silence duration
        """
        segments = self.get_speech_timestamps(audio)

        if not segments:
            # All silence
            return VADResult(
                speech_segments=segments,
                is_currently_speech=False,
                silence_duration_ms=current_time_ms
            )

        last_segment = segments[-1]
        audio_duration_ms = int(len(audio) * 1000 / self.sampling_rate)

        # Is the end of audio within speech?
        is_speech = last_segment.end_ms >= audio_duration_ms - 50  # 50ms tolerance

        # How long since speech ended?
        if is_speech:
            silence_duration = 0
        else:
            silence_duration = audio_duration_ms - last_segment.end_ms

        return VADResult(
            speech_segments=segments,
            is_currently_speech=is_speech,
            silence_duration_ms=silence_duration
        )


def calculate_chunk_threshold(
    total_time_ms: int,
    alpha: float = 2.0,
    beta: float = 2.0,
    c: float = -0.5,
    min_threshold_ms: int = 300,
    max_threshold_ms: int = 2000
) -> int:
    """
    Calculate adaptive silence threshold for triggering a chunk.

    Formula: threshold = (log_β(T) - C) / α

    Args:
        total_time_ms: Total recording time so far
        alpha: Silence sensitivity (higher = need longer silence)
        beta: Log base for time scaling
        c: Constant offset
        min_threshold_ms: Minimum silence threshold
        max_threshold_ms: Maximum silence threshold

    Returns:
        Silence threshold in milliseconds
    """
    import math

    # Avoid log(0)
    total_time_s = max(total_time_ms / 1000.0, 0.1)

    # Calculate threshold in seconds
    log_time = math.log(total_time_s, beta)
    threshold_s = (log_time - c) / alpha

    # Convert to ms and clamp
    threshold_ms = int(threshold_s * 1000)
    threshold_ms = max(min_threshold_ms, min(threshold_ms, max_threshold_ms))

    return threshold_ms


def should_trigger_chunk(
    vad_result: VADResult,
    total_time_ms: int,
    params: Optional[Dict] = None
) -> Tuple[bool, int]:
    """
    Decide whether to trigger a chunk based on VAD result.

    Args:
        vad_result: Result from VAD analysis
        total_time_ms: Total recording time
        params: Optional dict with alpha, beta, c overrides

    Returns:
        (should_trigger, threshold_used_ms)
    """
    params = params or {}

    threshold = calculate_chunk_threshold(
        total_time_ms,
        alpha=params.get('alpha', 2.0),
        beta=params.get('beta', 2.0),
        c=params.get('c', -0.5)
    )

    should_trigger = (
        not vad_result.is_currently_speech and
        vad_result.silence_duration_ms >= threshold
    )

    return should_trigger, threshold


# Module-level singleton for reuse
_vad_instance: Optional[SileroVAD] = None

def get_vad() -> SileroVAD:
    """Get or create singleton VAD instance."""
    global _vad_instance
    if _vad_instance is None:
        _vad_instance = SileroVAD()
    return _vad_instance
