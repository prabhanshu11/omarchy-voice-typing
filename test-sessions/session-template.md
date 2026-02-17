# Test Session: [DESCRIPTION]

**Date**: YYYY-MM-DD HH:MM
**Session ID**: YYYYMMDD-HHMM-description
**Type**: [TAG] (BASELINE/BUG/HYPOTHESIS/etc.)
**Status**: ⚠ In Progress

---

## Test Objective

<!-- What are you trying to learn or reproduce? -->

---

## Pre-Test Conditions

### System State
- **OSD Daemon**: ☐ Fresh start ☐ Already running (uptime: ___)
- **hyprwhspr Service**: `systemctl --user status hyprwhspr`
  - Uptime: ___
  - Recent restarts: ___
- **Gateway Status**: ☐ Running ☐ Stopped
  - Process ID: ___
- **Last Recording**: ___ minutes ago

### Configuration
- **hyprwhspr config**: (any recent changes?)
- **Gateway config**: (any recent changes?)
- **Environment**: (any system updates, theme changes, etc.?)

---

## Test Actions

### Recording 1: [Brief Description]
**Time**: HH:MM:SS
**Duration**: Expected ___ seconds, Actual ___ seconds
**Content**: (what you said)
**Pauses**: (any significant silence periods?)

**Observations**:
- ☐ OSD appeared immediately
- ☐ OSD appeared with delay (___s)
- ☐ OSD never appeared
- ☐ OSD disappeared during recording
- ☐ Recording stopped abruptly
- ☐ Recording completed successfully
- ☐ Transcription received
- ☐ Transcription failed

**Profiler Events** (key events from timeline):
```
HH:MM:SS │ CAT │ Event description
```

---

### Recording 2: [Brief Description]
<!-- Copy template above for additional recordings -->

---

## Profiler Analysis

### Key Event Sequences
<!-- Document important event sequences from profiler output -->

Example:
```
15:30:40 │ REC │ Recording started
15:30:40 │ OSD │ SIGUSR1 sent
15:30:45 │ REC │ Recording stopped (5.2s)
15:30:46 │ API │ Transcription completed (1.2s latency)
```

### Alerts Triggered
<!-- List any alerts from profiler -->
- [WARN] Example alert
- [ERROR] Example error

### Daemon Behavior
- **PID Changes**: Did daemon restart? When?
- **Signals**: SIGUSR1/SIGUSR2 timing
- **Crashes**: Any evidence of daemon crashes?

---

## Observations

### Expected Behavior
<!-- What did you expect to happen? -->

### Actual Behavior
<!-- What actually happened? -->

### Differences
<!-- What was unexpected or different? -->

---

## Analysis

### Patterns Identified
<!-- Any patterns you noticed -->

### Possible Causes
<!-- Based on profiler data, what might be causing the issue? -->

### Correlation with Known Issues
<!-- Does this match issues from hyprwhspr_osd_issues.txt or osd_profiler_plan.md? -->

---

## Hypotheses

### Confirmed
<!-- What did this test confirm? -->

### Rejected
<!-- What hypotheses were proven false? -->

### New Questions
<!-- What new questions arose from this test? -->

---

## Follow-up Actions

- [ ] Action item 1
- [ ] Action item 2
- [ ] Need to test scenario X
- [ ] Update INDEX.md with findings
- [ ] Update progress_actual.md if significant

---

## Artifacts

- `profiler-output.txt` - Full profiler output
- `screenshots/` - Screenshots of OSD behavior (if captured)
- `recordings/` - Test recordings (if saved)

---

## Related Sessions

- Previous: [session-id]
- Next: [session-id]
- Similar: [session-id]

---

## Notes

<!-- Additional observations, thoughts, or context -->
