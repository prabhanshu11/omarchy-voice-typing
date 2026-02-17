#!/usr/bin/env python3
"""
OSD Profiler - Real-time monitoring tool for hyprwhspr OSD debugging

Monitors:
- OSD daemon status (PID, signals)
- Recording status file changes
- hyprwhspr journal events
- Gateway transcription logs
- Audio level activity

Usage: python osd_profiler.py
"""

import os
import sys
import time
import signal
import subprocess
from pathlib import Path
from datetime import datetime
from threading import Thread, Event as ThreadEvent
from collections import deque
from dataclasses import dataclass
from typing import Optional, List

try:
    from rich.console import Console
    from rich.live import Live
    from rich.table import Table
    from rich.panel import Panel
    from rich.layout import Layout
    from rich.text import Text
except ImportError:
    print("Error: 'rich' library not found. Install with: pip install rich")
    sys.exit(1)

try:
    from watchdog.observers import Observer
    from watchdog.events import FileSystemEventHandler
except ImportError:
    print("Error: 'watchdog' library not found. Install with: pip install watchdog")
    sys.exit(1)


# Configuration
CONFIG_DIR = Path.home() / ".config" / "hyprwhspr"
PID_FILE = CONFIG_DIR / "mic_osd.pid"
RECORDING_STATUS_FILE = CONFIG_DIR / "recording_status"
AUDIO_LEVEL_FILE = CONFIG_DIR / "audio_level"
GATEWAY_LOG = Path.home() / "Programs" / "omarchy-voice-typing" / "logs" / "gateway.log"

console = Console()


@dataclass
class Event:
    """Represents a monitored event"""
    timestamp: datetime
    category: str  # OSD, REC, API, AUDIO
    message: str


class EventLog:
    """Thread-safe event log with fixed size"""

    def __init__(self, maxlen: int = 50):
        self.events = deque(maxlen=maxlen)
        self.alerts = deque(maxlen=10)

    def add(self, category: str, message: str):
        event = Event(datetime.now(), category, message)
        self.events.append(event)

    def add_alert(self, level: str, message: str):
        alert = f"[{level}] {message}"
        self.alerts.append((datetime.now(), alert))


class OSDMonitor:
    """Monitors OSD daemon PID and signals"""

    def __init__(self, event_log: EventLog):
        self.event_log = event_log
        self.last_pid: Optional[int] = None
        self.restart_count = 0
        self.last_check = datetime.now()

    def check(self) -> tuple[bool, Optional[int]]:
        """Check if OSD daemon is alive. Returns (is_alive, pid)"""
        try:
            if not PID_FILE.exists():
                if self.last_pid is not None:
                    self.event_log.add("OSD", "PID file deleted")
                    self.event_log.add_alert("WARN", "OSD daemon PID file missing")
                self.last_pid = None
                return False, None

            pid = int(PID_FILE.read_text().strip())

            # Check if process is alive
            try:
                os.kill(pid, 0)  # Signal 0 just checks if process exists
                is_alive = True
            except OSError:
                is_alive = False

            # Track restarts
            if self.last_pid is not None and pid != self.last_pid:
                self.restart_count += 1
                self.event_log.add("OSD", f"Daemon restarted (PID {self.last_pid} → {pid})")

                # Alert on frequent restarts
                time_diff = (datetime.now() - self.last_check).total_seconds()
                if time_diff < 300 and self.restart_count >= 2:  # 2+ restarts in 5 min
                    self.event_log.add_alert("WARN", f"OSD daemon restarted {self.restart_count} times in last 5 minutes")

            if self.last_pid is None and is_alive:
                self.event_log.add("OSD", f"Daemon started (PID {pid})")
            elif not is_alive:
                self.event_log.add("OSD", f"Daemon dead (PID {pid})")
                self.event_log.add_alert("ERROR", f"OSD daemon process {pid} not responding")

            self.last_pid = pid if is_alive else None
            return is_alive, pid if is_alive else None

        except Exception as e:
            self.event_log.add_alert("ERROR", f"OSD check failed: {e}")
            return False, None


class RecordingStatusHandler(FileSystemEventHandler):
    """Watches recording_status file changes"""

    def __init__(self, event_log: EventLog):
        self.event_log = event_log
        self.is_recording = False
        self.recording_start: Optional[datetime] = None

    def check_initial_state(self):
        """Check recording state at startup"""
        if RECORDING_STATUS_FILE.exists():
            content = RECORDING_STATUS_FILE.read_text().strip()
            if content == "true":
                self.is_recording = True
                self.recording_start = datetime.now()
                self.event_log.add("REC", "Recording active (at startup)")

    def on_modified(self, event):
        if event.src_path == str(RECORDING_STATUS_FILE):
            try:
                content = RECORDING_STATUS_FILE.read_text().strip()
                if content == "true" and not self.is_recording:
                    self.is_recording = True
                    self.recording_start = datetime.now()
                    self.event_log.add("REC", "Recording started")
            except FileNotFoundError:
                pass

    def on_deleted(self, event):
        if event.src_path == str(RECORDING_STATUS_FILE):
            if self.is_recording:
                duration = (datetime.now() - self.recording_start).total_seconds() if self.recording_start else 0
                self.event_log.add("REC", f"Recording stopped (duration: {duration:.1f}s)")
                self.is_recording = False
                self.recording_start = None


class JournalMonitor:
    """Monitors hyprwhspr systemd journal"""

    def __init__(self, event_log: EventLog, stop_event: ThreadEvent):
        self.event_log = event_log
        self.stop_event = stop_event
        self.thread: Optional[Thread] = None

    def start(self):
        self.thread = Thread(target=self._monitor_journal, daemon=True)
        self.thread.start()

    def _monitor_journal(self):
        """Tail hyprwhspr journal and extract key events"""
        try:
            # Start following journal from now
            proc = subprocess.Popen(
                ["journalctl", "--user", "-u", "hyprwhspr", "-f", "-n", "0", "-o", "cat"],
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                text=True,
                bufsize=1
            )

            for line in proc.stdout:
                if self.stop_event.is_set():
                    proc.terminate()
                    break

                line = line.strip()
                if not line:
                    continue

                # Extract key events
                if "Recording started" in line or "recording_started" in line.lower():
                    self.event_log.add("REC", "hyprwhspr: Recording started")
                elif "Recording stopped" in line or "recording_stopped" in line.lower():
                    self.event_log.add("REC", "hyprwhspr: Recording stopped")
                elif "silence" in line.lower() or "mute" in line.lower():
                    self.event_log.add("REC", f"Silence/mute detected: {line[:60]}")
                    self.event_log.add_alert("INFO", "Silence detection triggered")
                elif "error" in line.lower() or "failed" in line.lower():
                    self.event_log.add("REC", f"Error: {line[:60]}")
                    self.event_log.add_alert("ERROR", f"hyprwhspr error: {line[:40]}")
                elif "osd" in line.lower():
                    self.event_log.add("OSD", f"OSD event: {line[:60]}")

        except Exception as e:
            self.event_log.add_alert("ERROR", f"Journal monitoring failed: {e}")


class GatewayLogMonitor:
    """Monitors gateway.log for transcription events"""

    def __init__(self, event_log: EventLog, stop_event: ThreadEvent):
        self.event_log = event_log
        self.stop_event = stop_event
        self.thread: Optional[Thread] = None
        self.last_position = 0

    def start(self):
        # Get initial file size to start from end
        if GATEWAY_LOG.exists():
            self.last_position = GATEWAY_LOG.stat().st_size

        self.thread = Thread(target=self._monitor_log, daemon=True)
        self.thread.start()

    def _monitor_log(self):
        """Tail gateway.log and extract transcription events"""
        try:
            while not self.stop_event.is_set():
                if not GATEWAY_LOG.exists():
                    time.sleep(1)
                    continue

                with open(GATEWAY_LOG, 'r') as f:
                    f.seek(self.last_position)
                    new_lines = f.readlines()
                    self.last_position = f.tell()

                for line in new_lines:
                    line = line.strip()
                    if not line:
                        continue

                    # Extract transcription events
                    if "transcription" in line.lower() and "success" in line.lower():
                        # Try to extract latency if present
                        words = line.split()
                        latency_info = ""
                        for i, word in enumerate(words):
                            if "latency" in word.lower() and i + 1 < len(words):
                                latency_info = f" ({words[i+1]})"
                                break
                        self.event_log.add("API", f"Transcription completed{latency_info}")
                    elif "error" in line.lower() and "api" in line.lower():
                        self.event_log.add("API", f"API error: {line[:50]}")
                        self.event_log.add_alert("ERROR", "Gateway API error")
                    elif "request" in line.lower() and "received" in line.lower():
                        self.event_log.add("API", "Transcription request received")

                time.sleep(0.5)  # Poll every 500ms

        except Exception as e:
            self.event_log.add_alert("ERROR", f"Gateway log monitoring failed: {e}")


class OSDProfiler:
    """Main profiler orchestrator"""

    def __init__(self):
        self.event_log = EventLog()
        self.osd_monitor = OSDMonitor(self.event_log)
        self.stop_event = ThreadEvent()

        # File watchers
        self.observer = Observer()
        self.recording_handler = RecordingStatusHandler(self.event_log)

        # Background monitors
        self.journal_monitor = JournalMonitor(self.event_log, self.stop_event)
        self.gateway_monitor = GatewayLogMonitor(self.event_log, self.stop_event)

    def start(self):
        """Start all monitoring threads"""
        # Check initial recording state
        self.recording_handler.check_initial_state()

        # Start file watcher for recording_status
        self.observer.schedule(
            self.recording_handler,
            str(CONFIG_DIR),
            recursive=False
        )
        self.observer.start()

        # Start journal monitor
        self.journal_monitor.start()

        # Start gateway log monitor
        self.gateway_monitor.start()

        self.event_log.add("SYS", "Profiler started - monitoring OSD activity")

    def stop(self):
        """Stop all monitoring"""
        self.stop_event.set()
        self.observer.stop()
        self.observer.join()

    def render_dashboard(self) -> Layout:
        """Render the live dashboard"""
        # Check OSD status
        osd_alive, osd_pid = self.osd_monitor.check()

        # Check recording status
        is_recording = self.recording_handler.is_recording
        recording_duration = ""
        if is_recording and self.recording_handler.recording_start:
            duration = (datetime.now() - self.recording_handler.recording_start).total_seconds()
            recording_duration = f" ({duration:.1f}s)"

        # Status line
        osd_status = "● ALIVE" if osd_alive else "○ DEAD"
        osd_color = "green" if osd_alive else "red"
        pid_info = f"(PID {osd_pid})" if osd_pid else "(no PID)"

        rec_status = "● RECORDING" if is_recording else "○ INACTIVE"
        rec_color = "yellow" if is_recording else "dim"

        status_text = Text()
        status_text.append("OSD Daemon: ", style="bold")
        status_text.append(f"{osd_status} ", style=f"bold {osd_color}")
        status_text.append(f"{pid_info}    ", style="dim")
        status_text.append("Recording: ", style="bold")
        status_text.append(f"{rec_status}", style=f"bold {rec_color}")
        status_text.append(recording_duration, style="dim")

        # Timeline table
        timeline = Table(show_header=False, box=None, padding=(0, 1))
        timeline.add_column("Time", style="dim", width=8)
        timeline.add_column("Cat", style="bold", width=5)
        timeline.add_column("Event", style="", no_wrap=False)

        # Show last 15 events
        for event in list(self.event_log.events)[-15:]:
            time_str = event.timestamp.strftime("%H:%M:%S")
            cat_style = {
                "OSD": "cyan",
                "REC": "yellow",
                "API": "green",
                "SYS": "blue",
                "AUDIO": "magenta"
            }.get(event.category, "white")

            timeline.add_row(
                time_str,
                f"[{cat_style}]{event.category}[/{cat_style}]",
                event.message
            )

        # Alerts table
        alerts = Table(show_header=False, box=None, padding=(0, 1))
        alerts.add_column("Alert", no_wrap=False)

        for timestamp, alert in list(self.event_log.alerts)[-5:]:
            time_str = timestamp.strftime("%H:%M:%S")
            # Color code by severity
            if "[ERROR]" in alert:
                style = "bold red"
            elif "[WARN]" in alert:
                style = "bold yellow"
            else:
                style = "bold blue"
            alerts.add_row(f"[dim]{time_str}[/dim] [{style}]{alert}[/{style}]")

        # Build layout
        layout = Layout()
        layout.split_column(
            Layout(Panel(status_text, title="OSD Profiler", border_style="blue"), size=3),
            Layout(Panel(timeline, title="Timeline", border_style="cyan"), name="timeline"),
            Layout(Panel(alerts, title="Alerts", border_style="yellow"), size=10)
        )

        return layout


def signal_handler(signum, frame):
    """Handle Ctrl+C gracefully"""
    console.print("\n[bold yellow]Stopping profiler...[/bold yellow]")
    sys.exit(0)


def main():
    # Check dependencies
    if not CONFIG_DIR.exists():
        console.print(f"[red]Error: hyprwhspr config directory not found: {CONFIG_DIR}[/red]")
        console.print("[yellow]Make sure hyprwhspr is installed and configured.[/yellow]")
        sys.exit(1)

    # Register signal handler
    signal.signal(signal.SIGINT, signal_handler)

    # Create and start profiler
    profiler = OSDProfiler()
    profiler.start()

    try:
        with Live(profiler.render_dashboard(), refresh_per_second=4, console=console) as live:
            while True:
                time.sleep(0.25)
                live.update(profiler.render_dashboard())
    except KeyboardInterrupt:
        pass
    finally:
        profiler.stop()
        console.print("[green]Profiler stopped.[/green]")


if __name__ == "__main__":
    main()
