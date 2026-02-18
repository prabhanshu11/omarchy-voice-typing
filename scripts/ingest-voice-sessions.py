#!/usr/bin/env python3
"""
ingest-voice-sessions.py — Ingest voice typing session logs into the datalake.

Reads:
  - ~/Programs/omarchy-voice-typing/logs/sessions/*.log  (session timeline)
  - ~/Programs/omarchy-voice-typing/logs/latency/*.jsonl (timing + request IDs)
  - ~/Programs/recordings/*.wav                          (audio files)

Writes to:
  - ~/Programs/datalake/datalake.db  (audio, transcripts, voice_sessions tables)

Usage:
  python3 scripts/ingest-voice-sessions.py [--date YYYYMMDD] [--corrections corrections.json]
  python3 scripts/ingest-voice-sessions.py --date 20260218

Corrections JSON format (optional, maps raw → corrected per session):
  {"rec-001": "This is the test of the Rust based recorder now."}
"""

import argparse
import json
import os
import re
import sqlite3
import sys
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).parent.parent
RECORDINGS_DIR = Path.home() / "Programs" / "recordings"
DATALAKE_DB = Path.home() / "Programs" / "datalake" / "datalake.db"
SESSIONS_DIR = REPO_ROOT / "logs" / "sessions"
LATENCY_DIR = REPO_ROOT / "logs" / "latency"
SOURCE_DEVICE = "laptop"


def parse_session_log(log_path: Path) -> dict | None:
    """Parse a session .log file into a structured dict."""
    content = log_path.read_text()
    lines = content.splitlines()

    session = {"log_path": str(log_path), "log_filename": log_path.name}

    # Session ID from filename: 20260218_144349_rec-001.log → rec-001
    m = re.match(r"\d{8}_\d{6}_(rec-\d+)\.log", log_path.name)
    if not m:
        return None
    session["rec_id"] = m.group(1)

    for line in lines:
        if line.startswith("Started:"):
            session["started"] = line.split("Started:")[1].strip()
        elif line.startswith("Ended:"):
            session["ended"] = line.split("Ended:")[1].strip()
        elif line.startswith("Duration:"):
            session["duration_s"] = float(line.split("Duration:")[1].strip().rstrip("s"))
        elif "Gateway bytes:" in line:
            session["gateway_bytes"] = int(line.split("Gateway bytes:")[1].strip())
        elif line.strip().startswith("Transcript:"):
            # Transcript:   "This is the test..." (47 chars)
            m2 = re.search(r'Transcript:\s+"(.+?)"\s+\(\d+ chars\)', line)
            if m2:
                session["transcript"] = m2.group(1)
        elif "backend=" in line and "commit complete" in line:
            m3 = re.search(r"backend=(\w+)", line)
            if m3:
                session["backend"] = m3.group(1)
        elif "Status:" in line:
            session["status"] = line.split("Status:")[1].strip()

    return session if "started" in session else None


def load_latency_by_rec_number(date_str: str) -> dict[int, dict]:
    """Load latency JSONL entries keyed by recording_number."""
    latency_file = LATENCY_DIR / f"latency_{date_str[:4]}-{date_str[4:6]}-{date_str[6:8]}.jsonl"
    by_num: dict[int, dict] = {}
    if not latency_file.exists():
        print(f"  [warn] No latency file: {latency_file}", file=sys.stderr)
        return by_num
    for line in latency_file.read_text().splitlines():
        if not line.strip():
            continue
        entry = json.loads(line)
        n = entry.get("recording_number")
        if n is not None:
            by_num[n] = entry
    return by_num


def find_wav_by_bytes(target_bytes: int) -> Path | None:
    """Find WAV file whose size is target_bytes + 44 (WAV header overhead)."""
    for wav in RECORDINGS_DIR.glob("*.wav"):
        # WAV header is 44 bytes; gateway reports PCM bytes only
        if wav.stat().st_size == target_bytes + 44:
            return wav
    # Fallback: allow ±100 bytes tolerance
    for wav in RECORDINGS_DIR.glob("*.wav"):
        if abs(wav.stat().st_size - target_bytes - 44) <= 100:
            return wav
    return None


def iso_from_log_ts(ts_str: str) -> str:
    """Convert '2026-02-18 14:43:49.858' to ISO 8601 UTC string."""
    # The log uses local time; treat as-is (no TZ conversion needed for storage)
    return ts_str.replace(" ", "T")


def ingest_session(
    cursor: sqlite3.Cursor,
    session: dict,
    latency: dict | None,
    wav_path: Path | None,
    corrected_transcript: str | None,
) -> None:
    rec_id = session["rec_id"]
    started_iso = iso_from_log_ts(session["started"])
    ended_iso = iso_from_log_ts(session["ended"])
    transcript_text = session.get("transcript", "")
    backend = session.get("backend", latency.get("backend", "deepgram") if latency else "deepgram")
    success = 1 if session.get("status", "OK") == "OK" else 0

    audio_id = None
    if wav_path:
        stat = wav_path.stat()
        # WAV sample rate from hyprwhspr is 16000 Hz, mono (matching gateway chunks)
        audio_duration = latency["audio_duration_s"] if latency else session.get("duration_s", 0)
        cursor.execute(
            """
            INSERT OR IGNORE INTO audio
              (file_path, filename, original_filename, duration_seconds, format,
               sample_rate, channels, size_bytes, tags, source_device, source_project,
               created_at, metadata)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                str(wav_path),
                wav_path.name,
                wav_path.name,
                audio_duration,
                "wav",
                16000,
                1,
                stat.st_size,
                "voice-typing,deepgram",
                SOURCE_DEVICE,
                "omarchy-voice-typing",
                started_iso,
                json.dumps({"rec_id": rec_id, "gateway_bytes": session.get("gateway_bytes")}),
            ),
        )
        cursor.execute("SELECT id FROM audio WHERE file_path = ?", (str(wav_path),))
        row = cursor.fetchone()
        if row:
            audio_id = row[0]

    transcript_id = None
    if transcript_text:
        word_count = len(transcript_text.split())
        # Use the session log path as the canonical file_path for the transcript entry
        t_file_path = f"session-log:{session['log_filename']}"
        cursor.execute(
            """
            INSERT OR IGNORE INTO transcripts
              (file_path, filename, audio_id, content, word_count, language,
               provider, size_bytes, tags, source_device, created_at, metadata)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                t_file_path,
                session["log_filename"],
                audio_id,
                transcript_text,
                word_count,
                "en",
                backend,
                len(transcript_text.encode()),
                "voice-typing,deepgram",
                SOURCE_DEVICE,
                started_iso,
                json.dumps(
                    {
                        "rec_id": rec_id,
                        "deepgram_connect_ms": latency.get("deepgram_connect_ms") if latency else None,
                        "transcription_ms": latency.get("transcription_ms") if latency else None,
                        "total_commit_ms": latency.get("total_commit_ms") if latency else None,
                    }
                ),
            ),
        )
        cursor.execute("SELECT id FROM transcripts WHERE file_path = ?", (t_file_path,))
        row = cursor.fetchone()
        if row:
            transcript_id = row[0]

    session_uuid = latency.get("request_id") if latency else None
    cursor.execute(
        """
        INSERT OR IGNORE INTO voice_sessions
          (audio_id, transcript_id, session_uuid, trigger_type,
           trigger_timestamp, recording_start, recording_end,
           transcription_end, output_timestamp, success,
           source_device, corrected_transcript, metadata)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (
            audio_id,
            transcript_id,
            session_uuid,
            "keybinding",
            started_iso,
            started_iso,
            ended_iso,
            ended_iso,
            ended_iso,
            success,
            SOURCE_DEVICE,
            corrected_transcript,
            json.dumps(
                {
                    "rec_id": rec_id,
                    "backend": backend,
                    "duration_s": session.get("duration_s"),
                    "gateway_bytes": session.get("gateway_bytes"),
                }
            ),
        ),
    )
    print(
        f"  [{rec_id}] ingested: audio_id={audio_id}, transcript_id={transcript_id}"
        + (f", corrected" if corrected_transcript else "")
    )


def main():
    parser = argparse.ArgumentParser(description="Ingest voice sessions into datalake")
    parser.add_argument("--date", help="Date to ingest (YYYYMMDD), default: today")
    parser.add_argument(
        "--corrections",
        help="JSON file mapping rec-id to corrected transcript (e.g. corrections.json)",
    )
    args = parser.parse_args()

    date_str = args.date or datetime.now().strftime("%Y%m%d")
    print(f"Ingesting voice sessions for {date_str}...")

    # Load corrections if provided
    corrections: dict[str, str] = {}
    if args.corrections:
        corrections = json.loads(Path(args.corrections).read_text())
        print(f"  Corrections loaded: {list(corrections.keys())}")

    # Load session logs for this date
    session_logs = sorted(SESSIONS_DIR.glob(f"{date_str}_*.log"))
    if not session_logs:
        print(f"  No session logs found for {date_str} in {SESSIONS_DIR}")
        sys.exit(0)
    print(f"  Found {len(session_logs)} session log(s)")

    # Load latency data
    latency_by_num = load_latency_by_rec_number(date_str)

    # Parse sessions
    sessions = []
    for log_path in session_logs:
        s = parse_session_log(log_path)
        if s:
            sessions.append(s)
        else:
            print(f"  [warn] Could not parse {log_path.name}", file=sys.stderr)

    # Connect to datalake
    conn = sqlite3.connect(DATALAKE_DB)
    conn.execute("PRAGMA foreign_keys = ON")
    cursor = conn.cursor()

    ingested = 0
    for i, session in enumerate(sessions, start=1):
        rec_id = session["rec_id"]
        latency = latency_by_num.get(i)

        # Find WAV file by gateway_bytes
        wav_path = None
        gw_bytes = session.get("gateway_bytes") or (latency.get("audio_bytes") if latency else None)
        if gw_bytes:
            wav_path = find_wav_by_bytes(gw_bytes)
            if not wav_path:
                print(f"  [warn] No WAV found for {rec_id} (gateway_bytes={gw_bytes})")

        corrected = corrections.get(rec_id)
        ingest_session(cursor, session, latency, wav_path, corrected)
        ingested += 1

    conn.commit()
    conn.close()
    print(f"Done. Ingested {ingested} session(s) into {DATALAKE_DB}")


if __name__ == "__main__":
    main()
