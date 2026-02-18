"""
Comprehensive logging for streaming transcription experiments.

Logs all events for parameter tuning and analysis.
"""

import json
import logging
import os
from datetime import datetime
from pathlib import Path
from typing import Optional, Dict, Any, List
from dataclasses import dataclass, asdict
import uuid

logger = logging.getLogger(__name__)


@dataclass
class ChunkEvent:
    """Event when a chunk is sent for transcription."""
    event_type: str = "chunk_sent"
    session_id: str = ""
    chunk_id: str = ""
    sequence_number: int = 0
    timestamp: str = ""
    audio_duration_ms: int = 0
    silence_duration_ms: int = 0
    running_time_ms: int = 0
    threshold_used_ms: int = 0
    overlap_ms: int = 0
    params: Dict[str, float] = None

    def __post_init__(self):
        if not self.timestamp:
            self.timestamp = datetime.utcnow().isoformat()
        if self.params is None:
            self.params = {}


@dataclass
class ResponseEvent:
    """Event when a transcription response is received."""
    event_type: str = "response_received"
    session_id: str = ""
    chunk_id: str = ""
    sequence_number: int = 0
    timestamp: str = ""
    upload_time_ms: int = 0
    transcription_time_ms: int = 0
    total_latency_ms: int = 0
    transcript_length: int = 0
    transcript_preview: str = ""
    error: Optional[str] = None

    def __post_init__(self):
        if not self.timestamp:
            self.timestamp = datetime.utcnow().isoformat()


@dataclass
class MergeEvent:
    """Event when transcripts are merged."""
    event_type: str = "transcript_merged"
    session_id: str = ""
    timestamp: str = ""
    chunks_merged: int = 0
    overlap_words: int = 0
    duplicates_removed: int = 0
    final_length: int = 0
    final_preview: str = ""

    def __post_init__(self):
        if not self.timestamp:
            self.timestamp = datetime.utcnow().isoformat()


@dataclass
class SessionSummary:
    """Summary of a complete transcription session."""
    session_id: str = ""
    start_time: str = ""
    end_time: str = ""
    total_duration_ms: int = 0
    total_chunks: int = 0
    total_latency_ms: int = 0
    time_to_first_word_ms: int = 0
    final_transcript_length: int = 0
    params: Dict[str, float] = None
    events: List[Dict] = None


class ExperimentLogger:
    """
    Logger for streaming transcription experiments.

    Writes structured JSON logs for analysis and parameter tuning.
    """

    def __init__(
        self,
        log_dir: str = None,
        session_id: str = None
    ):
        """
        Initialize experiment logger.

        Args:
            log_dir: Directory for log files
            session_id: Session identifier (generated if not provided)
        """
        self.log_dir = Path(log_dir or os.path.expanduser(
            "~/Programs/omarchy-voice-typing/experiments/logs"
        ))
        self.log_dir.mkdir(parents=True, exist_ok=True)

        self.session_id = session_id or str(uuid.uuid4())
        self.session_start = datetime.utcnow()
        self.events: List[Dict] = []

        # Track metrics
        self.first_response_time: Optional[datetime] = None
        self.chunk_count = 0
        self.total_latency_ms = 0

        # Current log file
        date_str = self.session_start.strftime("%Y%m%d")
        self.log_file = self.log_dir / f"session_{date_str}_{self.session_id[:8]}.jsonl"

    def _write_event(self, event: Dict):
        """Write event to log file."""
        self.events.append(event)

        with open(self.log_file, 'a') as f:
            f.write(json.dumps(event) + '\n')

    def log_chunk_sent(
        self,
        chunk_id: str,
        sequence_number: int,
        audio_duration_ms: int,
        silence_duration_ms: int,
        running_time_ms: int,
        threshold_used_ms: int,
        overlap_ms: int = 500,
        params: Dict[str, float] = None
    ):
        """Log a chunk being sent for transcription."""
        event = ChunkEvent(
            session_id=self.session_id,
            chunk_id=chunk_id,
            sequence_number=sequence_number,
            audio_duration_ms=audio_duration_ms,
            silence_duration_ms=silence_duration_ms,
            running_time_ms=running_time_ms,
            threshold_used_ms=threshold_used_ms,
            overlap_ms=overlap_ms,
            params=params or {}
        )

        self.chunk_count += 1
        self._write_event(asdict(event))

        logger.debug(
            f"Chunk {sequence_number} sent: {audio_duration_ms}ms audio, "
            f"silence={silence_duration_ms}ms, threshold={threshold_used_ms}ms"
        )

    def log_response_received(
        self,
        chunk_id: str,
        sequence_number: int,
        upload_time_ms: int,
        transcription_time_ms: int,
        transcript: str,
        error: str = None
    ):
        """Log a transcription response."""
        total_latency = upload_time_ms + transcription_time_ms

        event = ResponseEvent(
            session_id=self.session_id,
            chunk_id=chunk_id,
            sequence_number=sequence_number,
            upload_time_ms=upload_time_ms,
            transcription_time_ms=transcription_time_ms,
            total_latency_ms=total_latency,
            transcript_length=len(transcript) if transcript else 0,
            transcript_preview=transcript[:100] if transcript else "",
            error=error
        )

        # Track first response
        if self.first_response_time is None and not error:
            self.first_response_time = datetime.utcnow()

        self.total_latency_ms += total_latency
        self._write_event(asdict(event))

        logger.debug(
            f"Response {sequence_number}: latency={total_latency}ms, "
            f"chars={len(transcript) if transcript else 0}"
        )

    def log_merge(
        self,
        chunks_merged: int,
        overlap_words: int,
        duplicates_removed: int,
        final_transcript: str
    ):
        """Log transcript merge operation."""
        event = MergeEvent(
            session_id=self.session_id,
            chunks_merged=chunks_merged,
            overlap_words=overlap_words,
            duplicates_removed=duplicates_removed,
            final_length=len(final_transcript),
            final_preview=final_transcript[:200]
        )

        self._write_event(asdict(event))

        logger.debug(
            f"Merged {chunks_merged} chunks: "
            f"removed {duplicates_removed} duplicates, "
            f"final length={len(final_transcript)}"
        )

    def get_session_summary(self) -> SessionSummary:
        """Get summary of current session."""
        end_time = datetime.utcnow()
        total_duration = int((end_time - self.session_start).total_seconds() * 1000)

        time_to_first_word = 0
        if self.first_response_time:
            time_to_first_word = int(
                (self.first_response_time - self.session_start).total_seconds() * 1000
            )

        # Find final transcript from last merge event
        final_length = 0
        for event in reversed(self.events):
            if event.get('event_type') == 'transcript_merged':
                final_length = event.get('final_length', 0)
                break

        return SessionSummary(
            session_id=self.session_id,
            start_time=self.session_start.isoformat(),
            end_time=end_time.isoformat(),
            total_duration_ms=total_duration,
            total_chunks=self.chunk_count,
            total_latency_ms=self.total_latency_ms,
            time_to_first_word_ms=time_to_first_word,
            final_transcript_length=final_length,
            events=self.events
        )

    def save_summary(self):
        """Save session summary to separate file."""
        summary = self.get_session_summary()
        summary_file = self.log_dir / f"summary_{self.session_id[:8]}.json"

        with open(summary_file, 'w') as f:
            json.dump(asdict(summary), f, indent=2)

        logger.info(f"Session summary saved to {summary_file}")


def load_experiment_logs(log_dir: str = None) -> List[Dict]:
    """Load all experiment logs from directory."""
    log_dir = Path(log_dir or os.path.expanduser(
        "~/Programs/omarchy-voice-typing/experiments/logs"
    ))

    events = []
    for log_file in log_dir.glob("session_*.jsonl"):
        with open(log_file) as f:
            for line in f:
                events.append(json.loads(line))

    return events


def analyze_params(events: List[Dict]) -> Dict[str, Any]:
    """
    Analyze logged events to suggest parameter improvements.

    Returns dict with analysis and suggested params.
    """
    # Group by session
    sessions = {}
    for event in events:
        sid = event.get('session_id', 'unknown')
        if sid not in sessions:
            sessions[sid] = []
        sessions[sid].append(event)

    # Calculate metrics per session
    metrics = []
    for sid, session_events in sessions.items():
        latencies = [
            e['total_latency_ms']
            for e in session_events
            if e.get('event_type') == 'response_received'
        ]
        chunks = len([
            e for e in session_events
            if e.get('event_type') == 'chunk_sent'
        ])

        if latencies:
            metrics.append({
                'session_id': sid,
                'avg_latency': sum(latencies) / len(latencies),
                'max_latency': max(latencies),
                'chunk_count': chunks,
                'params': session_events[0].get('params', {})
            })

    # Find best params
    if metrics:
        best = min(metrics, key=lambda m: m['avg_latency'])
        return {
            'best_params': best['params'],
            'best_avg_latency_ms': best['avg_latency'],
            'total_sessions': len(sessions),
            'all_metrics': metrics
        }

    return {'message': 'No data available'}
