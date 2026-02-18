# OSD Profiler Tool

Real-time monitoring and debugging tool for hyprwhspr OSD (On-Screen Display) issues.

## Features

- **OSD Daemon Monitor** - Tracks PID status, detects crashes and restarts
- **Recording State Monitor** - Watches `recording_status` file changes with inotify
- **Journal Monitor** - Tails `journalctl --user -u hyprwhspr` for key events
- **Gateway Log Monitor** - Tracks transcription API requests and responses
- **Real-time Dashboard** - Live terminal UI with event timeline and alerts

## Installation

```bash
cd ~/Programs/omarchy-voice-typing/tools
pip install -r requirements.txt
# or with uv:
uv pip install -r requirements.txt
```

## Usage

```bash
# Run the profiler
python osd_profiler.py

# Or make it executable and run directly
chmod +x osd_profiler.py
./osd_profiler.py
```

The profiler displays a live dashboard with:
- **Status Bar**: OSD daemon status (PID), recording status (with duration)
- **Timeline**: Last 15 events from all monitored sources
- **Alerts**: Critical issues like daemon crashes, errors, frequent restarts

## Dashboard Example

```
┌─ OSD Profiler ────────────────────────────────────────────────┐
│ OSD Daemon: ● ALIVE (PID 544268)    Recording: ○ INACTIVE     │
├─ Timeline ────────────────────────────────────────────────────┤
│ 15:30:40 │ REC │ Recording started                            │
│ 15:30:40 │ OSD │ Daemon started (PID 544268)                  │
│ 15:30:45 │ REC │ Recording stopped (duration: 5.2s)           │
│ 15:30:46 │ API │ Transcription completed (1.2s)               │
├─ Alerts ──────────────────────────────────────────────────────┤
│ [INFO] Profiler started - monitoring OSD activity             │
└───────────────────────────────────────────────────────────────┘
```

## Event Categories

- **OSD** - OSD daemon events (startup, PID changes, signals)
- **REC** - Recording events (start/stop, duration, silence detection)
- **API** - Gateway transcription events (requests, responses, errors)
- **SYS** - System events (profiler status)

## Alert Levels

- **[ERROR]** - Critical issues (daemon crashes, API failures)
- **[WARN]** - Warnings (frequent restarts, long recordings)
- **[INFO]** - Informational (normal events)

## Monitored Files

- `~/.config/hyprwhspr/mic_osd.pid` - OSD daemon PID
- `~/.config/hyprwhspr/recording_status` - "true" during recording
- `~/Programs/omarchy-voice-typing/logs/gateway.log` - Gateway logs
- `journalctl --user -u hyprwhspr` - hyprwhspr service journal

## Troubleshooting

**No events showing up:**
- Check that hyprwhspr service is running: `systemctl --user status hyprwhspr`
- Verify gateway is running: `ps aux | grep gateway`
- Try recording with `Super+\`` to trigger events

**OSD shows as DEAD:**
- Restart hyprwhspr: Right-click tray icon → Restart
- Check PID file exists: `cat ~/.config/hyprwhspr/mic_osd.pid`

**Journal events missing:**
- Ensure you have permission to read user journal
- Test manually: `journalctl --user -u hyprwhspr -n 10`

## Exit

Press `Ctrl+C` to stop the profiler.

## Related Documentation

- `~/Programs/misc_work/osd_profiler_plan.md` - Original design plan
- `~/Programs/omarchy-voice-typing/progress_actual.md` - Current issues and decisions
- `/usr/lib/hyprwhspr/lib/mic_osd/main.py` - OSD daemon source (30s timeout at line 148)
