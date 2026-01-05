#!/bin/bash
set -e

# Script to add keyboard shortcut display to hyprwhspr tray notification

TRAY_SCRIPT="/usr/lib/hyprwhspr/config/hyprland/hyprwhspr-tray.sh"
BACKUP="${TRAY_SCRIPT}.backup.$(date +%Y%m%d_%H%M%S)"

echo "=== Adding Keyboard Shortcut to hyprwhspr Tray ==="
echo

# Check if script exists
if [ ! -f "$TRAY_SCRIPT" ]; then
    echo "ERROR: $TRAY_SCRIPT not found"
    exit 1
fi

# Create backup
echo "Creating backup: $BACKUP"
sudo cp "$TRAY_SCRIPT" "$BACKUP"

# Add the get_primary_shortcut function
echo "Adding get_primary_shortcut function..."
sudo sed -i '/^# Microphone detection functions/i\
# Function to get primary shortcut from config\
get_primary_shortcut() {\
    local cfg="$HOME/.config/hyprwhspr/config.json"\
    python3 - <<'"'"'PY'"'"' "$cfg" 2>/dev/null\
import json, sys\
from pathlib import Path\
path = Path(sys.argv[1])\
try:\
    data = json.loads(path.read_text())\
    print(data.get("primary_shortcut", "SUPER+ALT+D"))\
except Exception:\
    print("SUPER+ALT+D")\
PY\
}\
\
' "$TRAY_SCRIPT"

# Update tooltip for recording state (line ~420)
echo "Updating recording state tooltip..."
sudo sed -i 's|tooltip="hyprwhspr: Currently recording\\n\\nLeft-click:|tooltip="hyprwhspr: Currently recording\\n\\nShortcut: $(get_primary_shortcut)\\nLeft-click:|' "$TRAY_SCRIPT"

# Update tooltip for mic_unavailable error (line ~427)
echo "Updating mic_unavailable tooltip..."
sudo sed -i 's|tooltip="hyprwhspr: Microphone not available\\n\\nMicrophone hardware is present|tooltip="hyprwhspr: Microphone not available\\n\\nShortcut: $(get_primary_shortcut)\\nMicrophone hardware is present|' "$TRAY_SCRIPT"

# Update tooltip for mic_no_audio error (line ~430)
echo "Updating mic_no_audio tooltip..."
sudo sed -i 's|tooltip="hyprwhspr: Recording but no audio input\\n\\nRecording is active|tooltip="hyprwhspr: Recording but no audio input\\n\\nShortcut: $(get_primary_shortcut)\\nRecording is active|' "$TRAY_SCRIPT"

# Update tooltip for generic error (line ~433)
echo "Updating generic error tooltip..."
sudo sed -i 's|tooltip="hyprwhspr: Issue detected\${reason:+ (\$reason)}\\n\\nLeft-click:|tooltip="hyprwhspr: Issue detected\${reason:+ (\$reason)}\\n\\nShortcut: $(get_primary_shortcut)\\nLeft-click:|' "$TRAY_SCRIPT"

# Update tooltip for ready state (line ~441)
echo "Updating ready state tooltip..."
sudo sed -i 's|tooltip="hyprwhspr: Ready to record\\n\\nLeft-click:|tooltip="hyprwhspr: Ready to record\\n\\nShortcut: $(get_primary_shortcut)\\nLeft-click:|' "$TRAY_SCRIPT"

# Update tooltip for stopped state (line ~446)
echo "Updating stopped state tooltip..."
sudo sed -i 's|tooltip="hyprwhspr: Stopped\\n\\nLeft-click:|tooltip="hyprwhspr: Stopped\\n\\nShortcut: $(get_primary_shortcut)\\nLeft-click:|' "$TRAY_SCRIPT"

# Update tooltip for unknown state (line ~451)
echo "Updating unknown state tooltip..."
sudo sed -i 's|tooltip="hyprwhspr: Unknown state\\n\\nLeft-click:|tooltip="hyprwhspr: Unknown state\\n\\nShortcut: $(get_primary_shortcut)\\nLeft-click:|' "$TRAY_SCRIPT"

echo
echo "=== Done! ==="
echo
echo "Backup saved to: $BACKUP"
echo
echo "To revert changes, run:"
echo "  sudo cp $BACKUP $TRAY_SCRIPT"
echo
echo "To see the changes, restart waybar or hover over the hyprwhspr tray icon."
