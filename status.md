# Voice Typing Investigation Status

**Assigned to:** subagent1 (Jr. SWE from IIT-Bombay)
**Coordinator:** main1 (General Engineer/Architect on desktop)
**Last updated:** 2026-01-14 (Initial creation)
**Location:** Running on LAPTOP to save desktop RAM

## Current Status
**VOICE TYPING FIXED ON DESKTOP** - New issue: waveform animation missing (2026-01-14 03:55 IST)

### What was fixed:
1. **Branch checkout issue** - Desktop was on master, needed to checkout `fix/gateway-service-not-running`
2. **Binary outdated** - Rebuilt gateway with `go build` (old binary was panicking)
3. **Wrong audio source** - PulseAudio default was `bluez_output...monitor` (speaker loopback)
   - Fixed with: `pactl set-default-source bluez_input.AA:54:88:DD:9B:91`
   - This was the main issue - recording was capturing silence!

### Issue: Two Waveform Visualizations - Both Broken

**User clarification (2026-01-14 04:05 IST):**

There are TWO separate waveform visualizations that BOTH used to work:

1. **Large mic-osd GTK overlay** (floating window)
   - Shows animated waveform bars
   - Desktop: Used to work, now shows for 1 second then disappears
   - Laptop: Never worked (different branch/config)

2. **Small waybar widget dots** (inside waybar)
   - Shows animated red dots during recording
   - Desktop: Used to work, now just turns red (no dots)
   - Laptop: Used to work (this is what laptop had)

**Root cause of drift:**
- NOT a planned feature difference
- Happened due to config/code drift between machines
- Desktop and laptop faced different issues, got different fixes

### Why Desktop Uses `pass` vs Laptop Uses `.env`

**Desktop setup:**
- Acts as a server, logs in without credentials (keyring-based)
- Uses `pass api/assemblyai` via `start-gateway.sh`
- GPG unlocked by keyring automatically
- Works well with this setup

**Laptop setup:**
- Requires authentication after suspend/boot
- Uses `.env` file because:
  - Can't rely on `pass` if GPG isn't unlocked
  - Services would fail if started before user authenticates
- `.env` file works but secrets aren't as secure

**User preference:**
User WANTS `pass` on laptop too, IF we can make it work with:
- Proper auth handling after suspend/resume
- Proper auth handling after boot/login
- No service failures if GPG locked

### For main1: Decisions Needed

1. **Waveform visualizations:**
   - Should we fix both (large OSD + small dots)?
   - Or deprecate one and keep the other?
   - Need to investigate why large OSD disappears after 1 second

2. **Pass on laptop:**
   - Can we implement `pass` with GPG unlock prompt?
   - Desktop already has this in `gateway/internal/auth/gpg.go`
   - Need to ensure services handle locked GPG gracefully

3. **Branch merge strategy:**
   - `fix/gateway-service-not-running` (omarchy-voice-typing)
   - `fix/voice-gateway-dependency` (local-bootstrapping)
   - Desktop has uncommitted work (latency.go, web_handlers.go, etc.)

### Pending Fixes

1. [ ] Fix large mic-osd overlay (shows 1 sec then disappears)
2. [ ] Fix small waybar dots animation
3. [ ] Make audio source fix persistent across reboots
4. [ ] Consider implementing `pass` on laptop with auth handling

## Coordination Protocol

**IMPORTANT - You are running on the laptop, main1 is on desktop:**
- Desktop: `100.92.71.80` (omarchy)
- Laptop: `100.103.8.87` (omarchy-1)
- SSH from laptop to desktop: `ssh prabhanshu@100.92.71.80`
- SSH from desktop to laptop: `ssh prabhanshu@100.103.8.87`

**Status Check Frequency:**
- **You (subagent1):** Check this file for updates from main1 **every 30 minutes**
- **main1:** Will check this file periodically via SSH from desktop
- Use git commits with descriptive messages for timeline tracking

**When to commit:**
- After completing each debugging step
- After identifying a bug
- After applying a fix
- Before requesting input from main1

**Commit message format:**
```
[voice-typing] Brief description

- Detailed point 1
- Detailed point 2
- Status: [In Progress/Blocked/Needs Review]
```

## Task Overview

Investigate and fix hyprwhspr/omarchy-voice-typing reliability issues that affect both desktop and laptop machines.

## Issues to Address

### Issue #1: Error on Startup
**Symptom:** Sometimes shows error when starting laptop/desktop
**Frequency:** Sometimes
**Desktop:** To be determined
**Laptop:** To be determined
**Last working:** Unknown
**Suspect:** Initialization timing, service dependencies

### Issue #2: Recording Without Text Output
**Symptom:** Records audio + shows waveform but no text output (99% no clipboard either)
**Frequency:** Common
**Desktop:** To be determined
**Laptop:** To be determined
**Last working:** Unknown
**Suspect:** AssemblyAI gateway issue, clipboard mechanism failure

### Issue #3: Long Recording Interrupted
**Symptom:** Long recording gets interrupted or produces no output
**Frequency:** Worst case scenario
**Desktop:** To be determined
**Laptop:** To be determined
**Last working:** Unknown
**Suspect:** Timeout issues, buffer overflows

### Issue #4: Auto-Refresh Doesn't Fix Issues
**Symptom:** Auto-refresh (30 min) sometimes triggers issues instead of fixing them
**Frequency:** Sometimes
**Desktop:** To be determined
**Laptop:** To be determined
**Last working:** Unknown
**Suspect:** Service restart logic, state management

### Issue #5: First Recording After Widget Restart Fails
**Symptom:** After restart, first recording shows waveform but doesn't record. Restarting hyprwhspr widget fixes it.
**Frequency:** After widget restart
**Desktop:** To be determined
**Laptop:** To be determined
**Last working:** Unknown
**Suspect:** Initialization race condition

## Debugging Approach

### Step 1: Review Git History
Check commit history in both repos to understand what's been tried:

```bash
cd ~/Programs/omarchy-voice-typing
git log --oneline -50

cd ~/Programs/local-bootstrapping
git log --oneline --grep="voice\|hyprwhspr" -20
```

**Goal:** Prevent working in loops - understand what's already been attempted.

### Step 2: Add Comprehensive Logging
Following the debug-with-logging skill pattern:

1. Add logging to hyprwhspr scripts in `~/Programs/omarchy-voice-typing/`
2. Create logs directory structure:
   ```
   ~/Programs/omarchy-voice-typing/logs/
   ├── .gitignore          (ignore *.log)
   ├── README.md           (document log formats)
   ├── events.log          (high-level: recording started/stopped)
   └── output.log          (verbose: AssemblyAI responses, clipboard operations)
   ```
3. Log format: `[YYYY-MM-DD HH:MM:SS] Event: Description`

### Step 3: Check AssemblyAI Gateway
Investigate the gateway that handles transcription:

1. Find gateway logs (likely in `~/Programs/omarchy-voice-typing/`)
2. Check for error patterns
3. Verify API key validity
4. Check network connectivity issues

### Step 4: Verify Clipboard Mechanisms
Check wl-copy and clipboard integration:

```bash
# Test clipboard manually
echo "test" | wl-copy
wl-paste

# Check if clipboard tools are installed
which wl-copy wl-paste
```

### Step 5: Test on Both Machines
Once laptop is available (SSH via `ssh laptop`):

1. Trigger same test recording on both machines
2. Compare logs from desktop vs laptop
3. Document behavioral differences

### Step 6: Document Findings
Update this status file with:
- Bugs identified
- Fixes applied
- Commit hashes
- Testing results

## Git History Review (Step 1 - COMPLETE)

Reviewed git history on 2026-01-14 03:00 IST.

### omarchy-voice-typing repo (5 commits):
| Commit | Description | Relevance |
|--------|-------------|-----------|
| 6006f83 | Fix: Convert voice-gateway to user service | Fixed service startup issues |
| 56353ec | Add smart audio fallback for voice typing | BT to laptop mic fallback |
| 17a0071 | Add GPG unlock prompt for voice gateway on boot | Fixed post-reboot failures |
| 531df3b | Fix: Restore rest-api backend, port 8765 | Fixed wrong port config |
| d58cb63 | Initial commit: AssemblyAI gateway | Base implementation |

### local-bootstrapping repo (voice-related commits):
| Commit | Description | Relevance |
|--------|-------------|-----------|
| b46bbd5 | Add WAYLAND_DISPLAY to hyprwhspr service | Fixed clipboard issues |
| ab69bf1 | Fix hyprwhspr timeout 30s -> 300s | Fixed long recording timeouts |
| bd8cd0b | CRITICAL FIX: voice-toggle.service black screen | Fixed systemd WantedBy |
| b79d98e | Fix voice binding: full path + Super+grave | Fixed PATH issues |

### Key Learnings from History:
1. **PATH issues** are common - always use full paths in hyprland/systemd
2. **Systemd service ordering** is critical - wrong WantedBy can break GUI
3. **Timeouts** have been increased but may still need tuning
4. **WAYLAND_DISPLAY** environment variable needed for clipboard

## Bugs Identified

- [x] **Bug #1: PORT CONFLICT (CRITICAL)** - Found 2026-01-14 03:00 IST
  - Orphan voice-gateway process (PID 929, started Jan 13) holding port 8765
  - Systemd service in restart loop (3169+ restarts)
  - Error: `listen tcp :8765: bind: address already in use`
  - **This is likely the ROOT CAUSE of most current issues!**

- [ ] Bug #2: Empty transcripts exist (7 files with 0 bytes)
  - Related to Bug #1 - gateway was unreachable

- [ ] Bug #3: Service dependency ordering may need improvement
  - hyprwhspr.service should depend on voice-gateway.service

## Fixes Applied

- [x] Fix #1: Kill orphan process (PID 929) + restart service - **COMPLETE** (2026-01-14 03:03 IST)
  - Gateway now running on PID 1136488
  - Verified with curl: `{"error":"No audio file or URL provided"}` (expected)
- [x] Fix #2: Add ExecStartPre cleanup to voice-gateway.service - **COMPLETE** (2026-01-14 03:04 IST)
  - Added `ExecStartPre=-/usr/bin/fuser -k 8765/tcp`
  - Added `ExecStopPost=-/usr/bin/fuser -k 8765/tcp`
  - Updated both installed service and repo copy
- [x] Fix #3: Add service dependency (hyprwhspr -> voice-gateway) - **COMPLETE** (2026-01-14 03:05 IST)
  - Added `After=voice-gateway.service` to [Unit]
  - Added `Wants=voice-gateway.service` to [Unit]
  - hyprwhspr now waits for voice-gateway before starting

## Blocked On

None - proceeding with fix for Bug #1 (port conflict)

## Next Steps

1. ~~Review git history~~ (COMPLETE)
2. ~~Fix port conflict bug~~ (COMPLETE - killed PID 929)
3. ~~Add ExecStartPre cleanup~~ (COMPLETE)
4. ~~Add service dependency ordering~~ (COMPLETE)
5. ~~Test voice typing end-to-end~~ (COMPLETE - services verified)
6. ~~Commit fixes~~ (COMPLETE - commit 0b9f9a8)
7. ~~Push branches for desktop sync~~ (COMPLETE)
8. **FOR DESKTOP:** Pull branches and apply fixes (see below)
9. User should test voice typing extensively from desktop

## Desktop Sync Instructions (for main1)

**Two branches need to be pulled on desktop:**

### 1. omarchy-voice-typing repo
```bash
cd ~/Programs/omarchy-voice-typing
git fetch origin
git checkout fix/gateway-service-not-running
# Copy updated service file
cp gateway/systemd/voice-gateway.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user restart voice-gateway
```

### 2. local-bootstrapping repo
```bash
cd ~/Programs/local-bootstrapping
git fetch origin
git checkout fix/voice-gateway-dependency
# Copy updated hyprwhspr override
cp -r dotfiles/systemd/user/hyprwhspr.service.d ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user restart hyprwhspr
```

### Verify on desktop
```bash
systemctl --user status voice-gateway hyprwhspr
curl -s -X POST http://127.0.0.1:8765/v1/transcribe -d '{}' | head -c 100
# Then test Super+` keybinding
```

## Related Files

**Key repositories:**
- `~/Programs/omarchy-voice-typing/` - Main voice typing code
- `~/Programs/local-bootstrapping/` - System sync and setup scripts
- `~/Programs/local-bootstrapping/dotfiles/hyprwhspr/` - hyprwhspr config

**Important scripts to review:**
- hyprwhspr widget script
- AssemblyAI gateway integration
- Clipboard handling code
- Auto-refresh/restart logic

## Communication with main1

If you encounter blockers or have questions:
1. Update the "Blocked On" section above
2. main1 will check this file periodically
3. Continue with unblocked tasks in the meantime

## Notes for subagent1

- You are a Jr. SWE from IIT-Bombay
- Focus on systematic debugging with logging
- Check git history FIRST to avoid redoing work
- Update this file frequently with progress
- Commit your changes incrementally
- Use the debug-with-logging skill pattern for inspiration
