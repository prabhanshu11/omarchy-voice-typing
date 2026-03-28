# Feature Proposals

This document tracks proposed features for future development.

---

## 1. API Failsafes for Recording Protection

### Problem
The voice typing service can potentially run indefinitely if:
- The user forgets to stop recording
- hyprwhspr or the toggle mechanism malfunctions
- The stop command fails to register

This could lead to:
- Excessive API costs (large audio files sent to AssemblyAI)
- Resource exhaustion
- Poor user experience

### Proposed Solution

#### Failsafe A: Maximum Recording Duration (10 minutes)
- If the microphone is active for more than **10 minutes**, automatically:
  1. Stop/pause the recording
  2. Trigger transcription of the recorded audio
- This protects against runaway recordings while still providing the transcription

#### Failsafe B: Three-Strike Service Stop
- Track occurrences of 10-minute auto-stops
- If **3 or more** 10-minute recordings occur (within a session or time window):
  1. Stop the hyprwhspr service entirely
  2. Require manual restart
- **Rationale**: Repeated 10-minute recordings indicate something is wrong - either the service is misbehaving, or the user cannot turn it off. Stopping the service prevents continued API abuse.

### Implementation Considerations
- Where to implement: `hyprwhspr-toggle` script, separate monitoring daemon, or gateway-side detection
- Strike counter persistence: session-only vs. persisted to file
- User notification: visual/audio alert when auto-stop occurs

---

## 2. Right-Click Widget Stop Behavior

### Problem
The tray/widget right-click behavior should provide a way to **stop the service**, but this functionality may be getting lost or changed in newer versions of the application.

### Proposed Solution
- Ensure right-click on the system tray widget/icon **stops the hyprwhspr service**
- This provides an emergency stop mechanism accessible via mouse
- Should be preserved across updates

### Implementation Considerations
- Check current tray widget implementation
- Ensure right-click menu includes "Stop Service" option
- Consider adding confirmation dialog to prevent accidental stops

---

## Status

| Feature | Status | Priority |
|---------|--------|----------|
| 10-minute recording limit | Proposed | High |
| 3-strike service stop | Proposed | High |
| Right-click stop behavior | Proposed | Medium |

---

*Last updated: 2026-01-08*
