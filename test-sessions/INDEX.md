# Test Sessions Index

Track all OSD profiler test sessions here. Update after each session with key findings.

## Session Log

| Date | Session ID | Type | Status | Key Findings | Issues |
|------|------------|------|--------|--------------|--------|
| YYYY-MM-DD | YYYYMMDD-HHMM-description | [TAG] | ✓/✗/⚠ | Brief summary | Issue# |

### Status Legend
- ✓ - Completed, conclusive results
- ⚠ - Partial data, needs follow-up
- ✗ - Inconclusive or failed test

### Type Tags
- **[BASELINE]** - Normal behavior documentation
- **[BUG]** - Bug reproduction
- **[REGRESSION]** - Verify no regression after changes
- **[HYPOTHESIS]** - Testing specific theories
- **[LONG-REC]** - Long duration recording tests
- **[RAPID-FIRE]** - Quick repeated recordings
- **[COLD-START]** - After hyprwhspr restart
- **[EXPLORATORY]** - General investigation

---

## Example Entry

| Date | Session ID | Type | Status | Key Findings | Issues |
|------|------------|------|--------|--------------|--------|
| 2026-01-12 | 20260112-1530-baseline | [BASELINE] | ✓ | OSD works consistently for recordings < 10s, transcription avg 1.2s latency | - |

---

## Findings Summary

### Confirmed Behaviors
- (Document confirmed OSD behaviors here after testing)

### Confirmed Bugs
- (List reproducible bugs with session references)

### Open Questions
- Does "sync" message correlate with daemon restarts?
- Is 30-second timeout triggering for long recordings?
- Why does OSD fallback to old system after successful transcription?

### Hypotheses to Test
- [ ] OSD daemon crashes on theme sync events
- [ ] 30s timeout in mic_osd/main.py:148 triggers for long recordings
- [ ] Successful transcription causes `show()` to return False on next invocation
- [ ] Multiple rapid recordings cause PID file corruption

---

## Related Documents
- `~/Programs/misc_work/osd_profiler_plan.md` - Original plan and root cause analysis
- `~/Programs/misc_work/hyprwhspr_osd_issues.txt` - User's original bug report
- `/usr/lib/hyprwhspr/lib/mic_osd/main.py` - OSD daemon source (line 148: 30s timeout)
- `~/Programs/omarchy-voice-typing/progress_actual.md` - Current work notes
