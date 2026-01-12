#!/bin/bash
# setup-audio-fallback.sh - Install smart audio fallback for voice typing
#
# Installs:
# 1. Bluetooth codec preferences (SbcXQ) - via local-bootstrapping
# 2. Audio source monitor service - for automatic mic fallback
#
# Usage: ./setup-audio-fallback.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOCAL_BOOTSTRAP_DIR="$HOME/Programs/local-bootstrapping"

echo "==> Setting up voice typing audio fallback system"

# Check dependencies
if ! command -v pactl &>/dev/null; then
    echo "ERROR: pactl not found. Install pipewire or pulseaudio."
    exit 1
fi

# Step 1: Install Bluetooth codec preferences
echo ""
echo "==> Step 1: Bluetooth codec preferences (SbcXQ)"
if [[ -f "$LOCAL_BOOTSTRAP_DIR/scripts/setup-audio-bluetooth.sh" ]]; then
    bash "$LOCAL_BOOTSTRAP_DIR/scripts/setup-audio-bluetooth.sh"
else
    echo "⚠️  WARNING: local-bootstrapping not found at $LOCAL_BOOTSTRAP_DIR"
    echo "Skipping Bluetooth codec setup"
fi

# Step 2: Install audio source monitor service
echo ""
echo "==> Step 2: Audio source monitor service"

# Create systemd user directory
mkdir -p ~/.config/systemd/user

# Copy service file
cp "$SCRIPT_DIR/systemd/audio-source-monitor.service" ~/.config/systemd/user/

# Reload systemd
systemctl --user daemon-reload

# Enable and start service
systemctl --user enable audio-source-monitor.service
systemctl --user restart audio-source-monitor.service

echo "✓ Audio source monitor service installed and started"

# Step 3: Verify setup
echo ""
echo "==> Verifying setup"

# Check if service is running
if systemctl --user is-active --quiet audio-source-monitor.service; then
    echo "✓ Audio source monitor: Running"
else
    echo "✗ Audio source monitor: Not running"
    systemctl --user status audio-source-monitor.service --no-pager -l || true
    exit 1
fi

# Show current audio source
echo ""
echo "Current audio source:"
pactl get-default-source | xargs -I {} pactl list sources | grep -A 5 "Name: {}" | grep "Description:" | cut -d: -f2- | xargs

echo ""
echo "✓ Voice typing audio fallback setup complete!"
echo ""
echo "The system will now automatically:"
echo "  1. Prefer SBC-XQ codec for Bluetooth headsets"
echo "  2. Fall back to laptop mic when BT goes silent (after 2nd silence)"
echo "  3. Restore BT when it becomes available again"
echo ""
echo "Monitor status:"
echo "  sudo journalctl --user -u audio-source-monitor -f"
echo ""
echo "Manual control:"
echo "  systemctl --user start|stop|status audio-source-monitor"
echo "  $SCRIPT_DIR/audio-source-monitor.sh status"
