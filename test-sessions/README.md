# OSD Profiler Test Sessions

This directory tracks test sessions for debugging hyprwhspr OSD issues using the profiler tool.

## Directory Structure

```
test-sessions/
├── README.md                    # This file
├── INDEX.md                     # Summary of all test sessions
├── session-template.md          # Template for new session notes
├── new-session.sh               # Script to create new session directory
├── .gitignore                   # Ignore profiler output logs
└── YYYYMMDD-HHMM-description/   # Individual session directories
    ├── notes.md                 # Observations and findings
    ├── profiler-output.txt      # Captured profiler output (gitignored)
    ├── screenshots/             # Screenshots of OSD behavior (optional)
    └── recordings/              # Links or copies of test recordings (optional)
```

## Quick Start

### Creating a New Test Session

```bash
cd ~/Programs/omarchy-voice-typing/test-sessions
./new-session.sh "short description of test"
```

This creates a timestamped directory with the template pre-filled.

### Running a Test Session

1. **Create session directory**:
   ```bash
   ./new-session.sh "testing-long-recordings"
   cd 20260112-1530-testing-long-recordings/
   ```

2. **Start profiler with output capture**:
   ```bash
   cd ~/Programs/omarchy-voice-typing/tools
   python osd_profiler.py | tee ../test-sessions/20260112-1530-testing-long-recordings/profiler-output.txt
   ```

3. **Test voice typing** in another terminal/window

4. **Document findings** in `notes.md` during or after the test

5. **Update INDEX.md** with key findings

### Alternative: Screen Recording

For complex issues, record the terminal:
```bash
# Using asciinema (terminal recording)
asciinema rec profiler-session.cast

# Or use script command
script -f profiler-output.txt
```

## Session Types

### Session Categories

Tag your sessions in INDEX.md:

- **[BASELINE]** - Normal behavior documentation
- **[BUG]** - Bug reproduction attempts
- **[REGRESSION]** - After code changes, verify no regression
- **[HYPOTHESIS]** - Testing specific theories from osd_profiler_plan.md
- **[LONG-REC]** - Long duration recording tests
- **[RAPID-FIRE]** - Quick repeated recordings
- **[COLD-START]** - Testing after hyprwhspr restart

## What to Document

### In notes.md

1. **Test Conditions**
   - OSD daemon state before test (fresh start, running for hours)
   - hyprwhspr service uptime
   - Gateway status

2. **Test Actions**
   - What you said/recorded
   - How long you held the recording key
   - Any pauses or silence periods
   - Any system events during recording (notifications, sync, etc.)

3. **Observed Behavior**
   - Did OSD appear/disappear?
   - Recording duration vs. expected
   - Transcription success/failure
   - Any errors in profiler output

4. **Profiler Events**
   - Key event sequences from timeline
   - Any alerts triggered
   - Daemon PID changes
   - Timing correlations

5. **Hypothesis Testing**
   - What did you expect to happen?
   - What actually happened?
   - Does it match any pattern from osd_profiler_plan.md?

## Analysis Tips

### Finding Patterns

Compare multiple sessions:
```bash
# Search all sessions for specific events
grep -r "Daemon restarted" test-sessions/*/notes.md

# Compare timelines
diff session-A/profiler-output.txt session-B/profiler-output.txt
```

### Key Things to Look For

1. **OSD Daemon Restarts**
   - Do they correlate with "sync" messages?
   - Frequency of restarts

2. **Recording Duration**
   - Compare recorded duration vs. actual speaking time
   - 30-second timeout triggers?

3. **Event Timing**
   - Delay between "Recording stopped" and "Transcription completed"
   - Time between SIGUSR1 and OSD appearance

4. **Error Patterns**
   - Specific errors that precede OSD failures
   - API errors vs. recording errors

## Integration with Issue Tracker

After identifying patterns, update:
- `~/Programs/misc_work/osd_profiler_plan.md` - Add new hypotheses
- `~/Programs/omarchy-voice-typing/progress_actual.md` - Document findings
- GitHub issues (if applicable)

## Cleanup

Profiler output logs can be large. Clean up old sessions:
```bash
# Remove profiler output older than 7 days (keeps notes.md)
find test-sessions/ -name "profiler-output.txt" -mtime +7 -delete
```

## Example Session

See `INDEX.md` for links to example sessions with detailed documentation.
