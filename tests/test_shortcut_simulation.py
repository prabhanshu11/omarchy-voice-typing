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
        # Press keys
        for key in [KEYS['SUPER'], KEYS['ALT'], KEYS['D']]:
            ui.write(ecodes.EV_KEY, key, 1)  # key down
            ui.syn()
            time.sleep(0.01)

        time.sleep(0.05)  # Hold briefly

        # Release keys (reverse order)
        for key in [KEYS['D'], KEYS['ALT'], KEYS['SUPER']]:
            ui.write(ecodes.EV_KEY, key, 0)  # key up
            ui.syn()
            time.sleep(0.01)

        print("Simulated SUPER+ALT+D press")
    finally:
        ui.close()

def check_hyprwhspr_status():
    """Check if hyprwhspr service responded"""
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
