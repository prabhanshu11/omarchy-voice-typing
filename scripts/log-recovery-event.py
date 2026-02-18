#!/usr/bin/env python3
"""
log-recovery-event.py - Log orphan recovery events to datalake

Logs events from orphan-recovery.sh to the voice_events table in datalake.

Usage:
    log-recovery-event.py <event_type> [recording] [error]

Event types:
    - recovery_scan_started: Timer triggered a scan
    - recovery_orphan_found: Orphan detected
    - recovery_attempt: Submitting to gateway
    - recovery_success: Transcription completed
    - recovery_failed: HTTP error or API failure
    - recovery_skipped: Skipped due to backoff/age

Examples:
    log-recovery-event.py recovery_scan_started
    log-recovery-event.py recovery_orphan_found "20260125_120000_audio.wav"
    log-recovery-event.py recovery_failed "20260125_120000_audio.wav" "HTTP 500"
"""

import json
import os
import socket
import sqlite3
import sys
from datetime import datetime
from pathlib import Path


DATALAKE_PATH = Path.home() / "Programs" / "datalake" / "datalake.db"


def get_source_device() -> str:
    """Get device identifier from hostname."""
    hostname = socket.gethostname()
    # Both machines are 'omarchy', use additional heuristics
    if hostname == "omarchy":
        # Check if this is laptop (has battery) or desktop
        battery_path = Path("/sys/class/power_supply/BAT0")
        if battery_path.exists():
            return "laptop"
        return "desktop"
    return hostname


def log_event(event_type: str, recording: str = "", error: str = "") -> bool:
    """Log an event to the voice_events table."""
    if not DATALAKE_PATH.exists():
        print(f"Warning: Datalake not found at {DATALAKE_PATH}", file=sys.stderr)
        return False

    try:
        conn = sqlite3.connect(str(DATALAKE_PATH))
        cursor = conn.cursor()

        # Build event_data JSON
        event_data = {}
        if recording:
            event_data["recording"] = recording
        if error:
            event_data["error"] = error

        # Insert event
        cursor.execute(
            """
            INSERT INTO voice_events (
                event_type,
                event_data,
                source_device,
                timestamp
            ) VALUES (?, ?, ?, ?)
            """,
            (
                event_type,
                json.dumps(event_data) if event_data else None,
                get_source_device(),
                datetime.now().isoformat(),
            ),
        )

        conn.commit()
        conn.close()
        return True

    except sqlite3.Error as e:
        print(f"SQLite error: {e}", file=sys.stderr)
        return False
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        return False


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)

    event_type = sys.argv[1]
    recording = sys.argv[2] if len(sys.argv) > 2 else ""
    error = sys.argv[3] if len(sys.argv) > 3 else ""

    valid_types = {
        "recovery_scan_started",
        "recovery_orphan_found",
        "recovery_attempt",
        "recovery_success",
        "recovery_failed",
        "recovery_skipped",
    }

    if event_type not in valid_types:
        print(f"Invalid event type: {event_type}", file=sys.stderr)
        print(f"Valid types: {', '.join(sorted(valid_types))}", file=sys.stderr)
        sys.exit(1)

    if log_event(event_type, recording, error):
        sys.exit(0)
    else:
        sys.exit(1)


if __name__ == "__main__":
    main()
