"""
Transcript merger for combining overlapping chunks.

Uses word-level alignment to deduplicate overlap regions.
"""

import re
from dataclasses import dataclass
from typing import List, Optional, Tuple
import logging

logger = logging.getLogger(__name__)


@dataclass
class MergedTranscript:
    """Result of merging multiple transcript chunks."""
    text: str
    chunks_merged: int
    overlap_words_removed: int
    confidence: float  # 0-1 based on alignment quality


def tokenize(text: str) -> List[str]:
    """Split text into words for alignment."""
    # Keep punctuation attached to words
    return re.findall(r'\S+', text.lower())


def find_overlap(
    words1: List[str],
    words2: List[str],
    min_overlap: int = 2,
    max_overlap: int = 20
) -> Tuple[int, int]:
    """
    Find overlap between end of words1 and start of words2.

    Returns:
        (overlap_start_in_words1, overlap_length)
    """
    if not words1 or not words2:
        return (len(words1), 0)

    best_match_len = 0
    best_match_start = len(words1)

    # Try different overlap lengths
    for overlap_len in range(min_overlap, min(max_overlap, len(words1), len(words2)) + 1):
        # Check if end of words1 matches start of words2
        end_words1 = words1[-overlap_len:]
        start_words2 = words2[:overlap_len]

        # Calculate match score
        matches = sum(1 for w1, w2 in zip(end_words1, start_words2) if w1 == w2)
        match_ratio = matches / overlap_len

        # Accept if good enough match
        if match_ratio >= 0.7 and overlap_len > best_match_len:
            best_match_len = overlap_len
            best_match_start = len(words1) - overlap_len

    return (best_match_start, best_match_len)


def merge_two_transcripts(
    transcript1: str,
    transcript2: str,
    overlap_hint_words: int = 10
) -> Tuple[str, int]:
    """
    Merge two transcripts, removing duplicate overlap.

    Args:
        transcript1: First transcript
        transcript2: Second transcript (may overlap with end of first)
        overlap_hint_words: Expected overlap size hint

    Returns:
        (merged_text, words_removed)
    """
    if not transcript1:
        return transcript2, 0
    if not transcript2:
        return transcript1, 0

    words1 = tokenize(transcript1)
    words2 = tokenize(transcript2)

    # Find overlap
    overlap_start, overlap_len = find_overlap(
        words1, words2,
        min_overlap=2,
        max_overlap=overlap_hint_words * 2
    )

    if overlap_len == 0:
        # No overlap found, just concatenate
        return f"{transcript1} {transcript2}", 0

    # Reconstruct keeping original capitalization/punctuation
    # Split original texts preserving format
    original_words1 = re.findall(r'\S+', transcript1)
    original_words2 = re.findall(r'\S+', transcript2)

    # Keep words1 up to overlap start
    kept_words1 = original_words1[:overlap_start]

    # Merge
    merged = ' '.join(kept_words1 + original_words2)

    logger.debug(
        f"Merged transcripts: removed {overlap_len} overlapping words "
        f"at position {overlap_start}"
    )

    return merged, overlap_len


def merge_transcripts(
    transcripts: List[str],
    overlap_hint_words: int = 10
) -> MergedTranscript:
    """
    Merge multiple sequential transcripts.

    Args:
        transcripts: List of transcripts in order
        overlap_hint_words: Expected overlap between chunks

    Returns:
        MergedTranscript with combined text and stats
    """
    if not transcripts:
        return MergedTranscript(
            text="",
            chunks_merged=0,
            overlap_words_removed=0,
            confidence=1.0
        )

    if len(transcripts) == 1:
        return MergedTranscript(
            text=transcripts[0],
            chunks_merged=1,
            overlap_words_removed=0,
            confidence=1.0
        )

    # Merge sequentially
    merged = transcripts[0]
    total_removed = 0

    for i in range(1, len(transcripts)):
        merged, removed = merge_two_transcripts(
            merged,
            transcripts[i],
            overlap_hint_words
        )
        total_removed += removed

    # Calculate confidence based on how much overlap was found
    expected_overlap = (len(transcripts) - 1) * overlap_hint_words
    if expected_overlap > 0:
        confidence = min(1.0, total_removed / expected_overlap)
    else:
        confidence = 1.0

    return MergedTranscript(
        text=merged.strip(),
        chunks_merged=len(transcripts),
        overlap_words_removed=total_removed,
        confidence=confidence
    )


class TranscriptBuffer:
    """
    Buffer for assembling transcripts as they arrive.

    Handles out-of-order results and incremental merging.
    """

    def __init__(self, overlap_hint_words: int = 10):
        self.overlap_hint_words = overlap_hint_words
        self.transcripts: dict[int, str] = {}  # seq -> transcript
        self.last_merged_seq: int = -1
        self.merged_text: str = ""

    def add_transcript(self, sequence: int, transcript: str):
        """Add a transcript for a sequence number."""
        self.transcripts[sequence] = transcript
        self._try_merge()

    def _try_merge(self):
        """Try to merge any consecutive transcripts."""
        while self.last_merged_seq + 1 in self.transcripts:
            next_seq = self.last_merged_seq + 1
            next_transcript = self.transcripts[next_seq]

            self.merged_text, _ = merge_two_transcripts(
                self.merged_text,
                next_transcript,
                self.overlap_hint_words
            )

            self.last_merged_seq = next_seq

    def get_current_text(self) -> str:
        """Get current merged text."""
        return self.merged_text

    def get_pending_sequences(self) -> List[int]:
        """Get sequences we're waiting for."""
        if not self.transcripts:
            return [0] if self.last_merged_seq < 0 else []

        max_seq = max(self.transcripts.keys())
        return [
            i for i in range(self.last_merged_seq + 1, max_seq + 1)
            if i not in self.transcripts
        ]

    def finalize(self) -> MergedTranscript:
        """Finalize and return complete merged transcript."""
        # Force merge any remaining
        all_transcripts = [
            self.transcripts[i]
            for i in sorted(self.transcripts.keys())
        ]

        return merge_transcripts(all_transcripts, self.overlap_hint_words)
