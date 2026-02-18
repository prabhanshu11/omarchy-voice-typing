# Quick Start: OSD Profiler Testing

## 1. Create New Test Session

```bash
cd ~/Programs/omarchy-voice-typing/test-sessions
./new-session.sh "description-of-what-youre-testing"
```

Example descriptions:
- `baseline-normal-behavior`
- `long-recording-timeout-bug`
- `rapid-fire-recordings`
- `osd-disappears-after-sync`

## 2. Start Profiler

```bash
# Terminal 1: Start profiler with output capture
cd ~/Programs/omarchy-voice-typing/tools
python osd_profiler.py | tee ../test-sessions/YYYYMMDD-HHMM-description/profiler-output.txt
```

Replace `YYYYMMDD-HHMM-description` with your actual session ID.

## 3. Run Voice Typing Tests

In another terminal/window, use voice typing normally:
- Press `Super+\`` to start/stop recording
- Speak your test content
- Observe OSD behavior
- Watch profiler output in Terminal 1

## 4. Document Findings

While testing or immediately after:

```bash
cd ~/Programs/omarchy-voice-typing/test-sessions/YYYYMMDD-HHMM-description
nano notes.md  # or use your preferred editor
```

Fill in the template:
- Test objective
- Pre-test system state
- Each recording attempt (what you said, what happened)
- Key profiler events
- Observations and analysis

## 5. Update Index

After completing the session:

```bash
cd ~/Programs/omarchy-voice-typing/test-sessions
nano INDEX.md
```

Add a row to the session log table with:
- Date
- Session ID
- Type tag ([BASELINE], [BUG], etc.)
- Status (✓ completed, ⚠ partial, ✗ inconclusive)
- Brief key findings
- Related issue numbers (if any)

## Example Workflow

```bash
# 1. Create session
cd ~/Programs/omarchy-voice-typing/test-sessions
./new-session.sh "testing-30-second-timeout"

# Output shows: 20260112-1630-testing-30-second-timeout

# 2. Start profiler in Terminal 1
cd ~/Programs/omarchy-voice-typing/tools
python osd_profiler.py | tee ../test-sessions/20260112-1630-testing-30-second-timeout/profiler-output.txt

# 3. In another window, test voice typing
# Press Super+`, speak for >30 seconds, release

# 4. Watch profiler output for alerts or unusual events

# 5. Stop profiler (Ctrl+C)

# 6. Document findings
cd ~/Programs/omarchy-voice-typing/test-sessions/20260112-1630-testing-30-second-timeout
nano notes.md

# 7. Update index
cd ~/Programs/omarchy-voice-typing/test-sessions
nano INDEX.md
```

## Tips

### Capturing Good Data

1. **Before each test**, check system state:
   ```bash
   systemctl --user status hyprwhspr
   ps aux | grep gateway
   cat ~/.config/hyprwhspr/mic_osd.pid
   ```

2. **During recording**, note timestamps of key events

3. **After recording**, immediately document what you observed

### Common Test Scenarios

**Baseline Test**:
- Fresh hyprwhspr restart
- 3-5 recordings of 5-10 seconds each
- Document normal behavior for comparison

**Long Recording Test**:
- Record for >30 seconds to test timeout hypothesis
- Include 10+ seconds of silence
- Watch for auto-stop or OSD disappearance

**Rapid-Fire Test**:
- Multiple quick recordings (<5s) back-to-back
- Watch for daemon restarts or PID changes

**Post-Sync Test**:
- Wait for "sync" message (file sync, theme sync, etc.)
- Immediately try recording
- Check if OSD appears

### Reading Profiler Output

Look for these patterns:

**Normal sequence**:
```
HH:MM:SS │ REC │ Recording started
HH:MM:SS │ OSD │ Daemon started (PID XXXXX)
HH:MM:SS │ REC │ Recording stopped (duration: Xs)
HH:MM:SS │ API │ Transcription completed (X.Xs)
```

**Problem sequence**:
```
HH:MM:SS │ REC │ Recording started
HH:MM:SS │ OSD │ Daemon dead (PID XXXXX)
HH:MM:SS │ REC │ Recording stopped (duration: 0.1s)  ← Abrupt stop!
```

## Need Help?

- See `README.md` for detailed documentation
- See `session-template.md` for documentation structure
- See `INDEX.md` for past sessions and findings
- See `~/Programs/misc_work/osd_profiler_plan.md` for hypotheses to test
