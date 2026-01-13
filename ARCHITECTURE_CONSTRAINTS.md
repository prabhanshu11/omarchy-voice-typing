# Architecture Constraints & Guidance for Voice Typing

**From:** main1 (System Architect)
**To:** subagent1
**Date:** 2026-01-14

## Your Mission

Fix voice typing reliability issues on **both** desktop and laptop. You own this service end-to-end.

## System Architecture You Must Respect

### 1. Local-Bootstrapping Philosophy

**Core principle:** All system modifications must be **reproducible** via `~/Programs/local-bootstrapping`

**What this means for you:**
- ✅ DO: Modify service files in `local-bootstrapping/dotfiles/systemd-user/`
- ✅ DO: Update scripts in `local-bootstrapping/dotfiles/local-bin/`
- ✅ DO: Document changes in `local-bootstrapping/docs/`
- ❌ DON'T: Make one-off changes to `~/.config/systemd/user/` without syncing to repo
- ❌ DON'T: Create scripts in random locations that won't survive disaster recovery

**Why:** When we reinstall a machine or set up a new one, `local-bootstrapping` must recreate the entire voice typing setup automatically.

### 2. Multi-Device Sync Strategy

**Devices in play:**
- Desktop (omarchy): `100.92.71.80` - Primary workstation, subject to power cuts
- Laptop (omarchy-1): `100.103.8.87` - Battery backup, always available

**Repos involved:**
1. `omarchy-voice-typing` - Service-specific code (gateway, configs)
2. `local-bootstrapping` - System-level integration (systemd, scripts)

**Sync workflow you MUST follow:**
```bash
# After fixing something in omarchy-voice-typing
cd ~/Programs/omarchy-voice-typing
git add -A
git commit -m "[voice-typing] Fix X"
git push origin main  # or your branch

# If you changed systemd services or scripts
cd ~/Programs/local-bootstrapping
cp ~/.config/systemd/user/hyprwhspr.service.d/override.conf \
   dotfiles/systemd-user/hyprwhspr.service.d/
git add dotfiles/
git commit -m "[voice-typing] Update service config"
git push origin main  # or your branch
```

**Desktop pulls changes:**
```bash
# User will run this, or it happens via git pull timer
cd ~/Programs/omarchy-voice-typing && git pull
cd ~/Programs/local-bootstrapping && git pull
./scripts/install.sh  # Applies systemd changes
systemctl --user daemon-reload
systemctl --user restart hyprwhspr voice-gateway
```

### 3. Systemd Service Design Patterns

**Services you're working with:**
- `voice-gateway.service` - AssemblyAI gateway on port 8765
- `hyprwhspr.service` - Voice typing widget

**Critical patterns to follow:**

**Service dependencies (already fixed, don't break this):**
```ini
[Unit]
After=voice-gateway.service
Wants=voice-gateway.service
```
This ensures hyprwhspr waits for voice-gateway before starting.

**Port conflict prevention (already fixed, don't break this):**
```ini
[Service]
ExecStartPre=-/usr/bin/fuser -k 8765/tcp
ExecStopPost=-/usr/bin/fuser -k 8765/tcp
```
This kills any orphan processes on port 8765.

**Restart policy:**
```ini
[Service]
Restart=always
RestartSec=5
```
If you add this, services auto-recover from crashes. Good for reliability.

### 4. What You CAN Change

✅ **Allowed modifications:**
- Add logging to hyprwhspr scripts (use `debug-with-logging` skill pattern)
- Fix bugs in voice-gateway code
- Improve error handling in transcription pipeline
- Add timeout configurations
- Enhance clipboard integration
- Fix initialization race conditions

✅ **Encouraged additions:**
- Health check endpoints
- Status monitoring
- Automatic recovery logic
- Better error messages in logs

### 5. What You CANNOT Change (Without Asking main1)

❌ **Forbidden changes:**
- Moving service files to different locations (breaks local-bootstrapping)
- Changing port numbers (8765 is hardcoded in many places)
- Removing service dependencies we just added
- Installing system-wide packages without updating install scripts
- Creating cron jobs instead of systemd timers
- Hard-coding paths that differ between desktop/laptop

❌ **Needs approval:**
- Major architectural changes (e.g., switching from REST API to WebSocket)
- Adding new system dependencies (new packages, Python libs)
- Changing the hyprwhspr keybinding (Super+Caps)
- Modifying audio device selection logic

### 6. Debugging Strategy

**Priority order for Issue #2 (no text output):**

1. **Add logging first** (use `debug-with-logging` skill)
   ```bash
   mkdir -p ~/Programs/omarchy-voice-typing/logs
   echo "*.log" >> ~/Programs/omarchy-voice-typing/logs/.gitignore
   ```

2. **Log at each stage:**
   - Keybinding trigger (Super+Caps pressed)
   - Audio recording start/stop
   - File saved to recordings/
   - Gateway API call (request + response)
   - Clipboard copy attempt
   - Any errors

3. **Check each component:**
   - Is hyprwhspr receiving the keybind?
   - Is audio being recorded to WAV file?
   - Is the WAV file being sent to gateway?
   - Is gateway responding with transcript?
   - Is wl-copy working?

4. **Only then fix the actual bug**

**For Issue #5 (first recording fails after restart):**
This smells like an **initialization race condition**. Possible causes:
- hyprwhspr starts before audio device is ready
- Gateway needs warm-up time after boot
- Clipboard (wl-copy) not initialized

Add delays or health checks during startup.

### 7. Testing Requirements

**Before marking anything as "fixed":**

✅ Must test on BOTH machines:
- Desktop: Test after reboot, after service restart, after hyprwhspr restart
- Laptop: Same tests

✅ Must test edge cases:
- Long recordings (>1 minute)
- Back-to-back recordings (spam Super+Caps)
- Recording while other apps use microphone
- Recording during high CPU load

✅ Must verify persistence:
- Reboot machine, voice typing still works
- Service crashes and auto-restarts, voice typing still works

### 8. Integration with Datalake (Future)

**FYI:** Your recordings and transcripts are being ingested into datalake for analysis:
- `~/Programs/recordings/*.wav` → Tracked
- `~/Programs/transcripts/*.txt` → Parsed and stored

**This means:**
- Don't change file naming schemes without coordinating
- Don't delete old recordings (they're data)
- Add metadata if you can (timestamps, duration, etc.)

### 9. Git Commit Standards

**Good commit message:**
```
[voice-typing] Fix clipboard failure on Wayland

- Added full path to wl-copy: /usr/bin/wl-copy
- Set WAYLAND_DISPLAY env var in hyprwhspr service
- Tested on desktop: clipboard now works reliably

Fixes: Issue #2 (no text output)
Status: Needs laptop testing
```

**Bad commit message:**
```
fix stuff
```

### 10. When to Ask main1 for Help

**Ask if:**
- You need to change system-level configs (udev rules, kernel params)
- You want to add new package dependencies
- You're stuck debugging for >2 hours
- You found a bug in local-bootstrapping itself
- You need to coordinate with datalake agent

**Don't ask if:**
- You can solve it by reading code/logs
- It's a normal debugging task
- You just need to add logging

## Success Criteria

Voice typing is "fixed" when:
1. ✅ Works reliably on desktop (95%+ success rate)
2. ✅ Works reliably on laptop (95%+ success rate)
3. ✅ Survives reboots on both machines
4. ✅ All changes synced to local-bootstrapping
5. ✅ No regressions (old working features still work)

## Your Authority

You have **full authority** to:
- Modify voice-gateway code
- Fix hyprwhspr integration
- Add logging and debugging
- Write tests
- Update documentation

You do **NOT** need permission for normal bug fixes. Just commit and push.

---

**Remember:** You're the expert on voice typing. main1 trusts you to fix it properly. Just follow these constraints so your fixes are **reproducible** and **don't break the system architecture**.

Good hunting!
