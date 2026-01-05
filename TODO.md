# hyprwhspr Enhancement Tasks

## Status

- ✅ **Task 1**: hyprwhspr backend fixed - gateway running on port 8765
- ✅ **Task 2**: Keyboard shortcut added to notification - restart waybar to see changes
- ✅ **Task 3**: Initial git commit created
- ✅ **Task 4**: Test module created at `tests/test_shortcut_simulation.py`

**All tasks completed!**

## Context

hyprwhspr is a voice-to-text tool for Hyprland. The user has a local gateway project
(omarchy-voice-typing) that was being developed but hyprwhspr stopped working because
the config was set to use a REST API server that isn't running.

### Key Files
- **User config:** `~/.config/hyprwhspr/config.json` - Contains shortcut (`SUPER+ALT+D`) and backend settings
- **Tray script:** `/usr/lib/hyprwhspr/config/hyprland/hyprwhspr-tray.sh` - Generates waybar tooltip/notification
- **Project dir:** `/home/prabhanshu/Programs/omarchy-voice-typing/` - Gateway project (has git, no commits)

### Current Problem
Config has `"transcription_backend": "rest-api"` pointing to `http://127.0.0.1:8000` but no server is running.
Service logs show: `ERROR: Failed to connect to REST API: Connection refused`

---

## Tasks

### Task 1: Fix hyprwhspr Backend [BLOCKING]

**Goal:** Switch from broken REST API to working local inference

**Steps:**
1. Edit `~/.config/hyprwhspr/config.json`
2. Change line 13 from `"transcription_backend": "rest-api"` to `"transcription_backend": "pywhispercpp"`
3. Run: `systemctl --user restart hyprwhspr.service`
4. Verify: `systemctl --user status hyprwhspr.service` should show active

**Success criteria:** Service running without connection errors

---

### Task 2: Add Keyboard Shortcut to Notification

**Goal:** Display `SUPER+ALT+D` in the tray popup (currently only shows "Left-click: Toggle service")

**File:** `/usr/lib/hyprwhspr/config/hyprland/hyprwhspr-tray.sh` (requires sudo to edit)

**⚠️  Note:** Backup first with: `sudo cp /usr/lib/hyprwhspr/config/hyprland/hyprwhspr-tray.sh{,.backup}`

**Step 2a - Add function after line ~100:**
```bash
get_primary_shortcut() {
    local cfg="$HOME/.config/hyprwhspr/config.json"
    python3 - <<'PY' "$cfg" 2>/dev/null || echo "SUPER+ALT+D"
import json, sys
from pathlib import Path
path = Path(sys.argv[1])
try:
    data = json.loads(path.read_text())
    print(data.get("primary_shortcut", "SUPER+ALT+D"))
except Exception:
    print("SUPER+ALT+D")
PY
}
```

**Step 2b - Add near start of script (after functions defined):**
```bash
PRIMARY_SHORTCUT=$(get_primary_shortcut)
```

**Step 2c - Update ALL tooltip strings to include shortcut:**

Lines to modify: 420, 427, 430, 433, 441, 446, 451

For each tooltip, add `Shortcut: $PRIMARY_SHORTCUT\n` after the first line.

Example (line 441, ready state):
```bash
# Before:
tooltip="hyprwhspr: Ready to record\n\nLeft-click: Toggle service\nRight-click: Restart service"

# After:
tooltip="hyprwhspr: Ready to record\n\nShortcut: $PRIMARY_SHORTCUT\nLeft-click: Toggle service\nRight-click: Restart service"
```

**Success criteria:** Tray popup shows "Shortcut: SUPER+ALT+D" in notification

---

### Task 3: Git Initial Commit

**Goal:** Commit the omarchy-voice-typing project

**Steps:**
```bash
cd /home/prabhanshu/Programs/omarchy-voice-typing
git add -A
git commit -m "Initial commit: AssemblyAI gateway for hyprwhspr"
```

---

### Task 4: Create Test Module

**Goal:** Test that simulates pressing SUPER+ALT+D

**File:** `/home/prabhanshu/Programs/omarchy-voice-typing/tests/test_shortcut_simulation.py`

**Content:**
```python
#!/usr/bin/env python3
"""
Test module to simulate Win+Alt+D shortcut press for hyprwhspr.
Uses evdev UInput to create virtual keyboard events.
"""

import time
import subprocess
from evdev import UInput, ecodes

KEYS = {
    'SUPER': ecodes.KEY_LEFTMETA,
    'ALT': ecodes.KEY_LEFTALT,
    'D': ecodes.KEY_D,
}

def simulate_shortcut():
    """Simulate pressing SUPER+ALT+D"""
    ui = UInput(name="test-hyprwhspr-keyboard")
    try:
        for key in [KEYS['SUPER'], KEYS['ALT'], KEYS['D']]:
            ui.write(ecodes.EV_KEY, key, 1)
            ui.syn()
            time.sleep(0.01)
        time.sleep(0.05)
        for key in [KEYS['D'], KEYS['ALT'], KEYS['SUPER']]:
            ui.write(ecodes.EV_KEY, key, 0)
            ui.syn()
            time.sleep(0.01)
        print("Simulated SUPER+ALT+D press")
    finally:
        ui.close()

def check_hyprwhspr_status():
    result = subprocess.run(
        ['systemctl', '--user', 'status', 'hyprwhspr.service'],
        capture_output=True, text=True
    )
    return result.stdout

if __name__ == "__main__":
    print("Simulating SUPER+ALT+D in 2 seconds...")
    time.sleep(2)
    simulate_shortcut()
    time.sleep(1)
    print("\nService status:")
    print(check_hyprwhspr_status())
```

**Run with:** `sudo python3 tests/test_shortcut_simulation.py` (needs root for UInput)

---

## Verification

After all tasks complete:
1. Press SUPER+ALT+D - should start/stop recording
2. Hover over tray icon - should show "Shortcut: SUPER+ALT+D"
3. `git log` in omarchy-voice-typing - should show initial commit
4. Run test script - should simulate key press
