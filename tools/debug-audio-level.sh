#!/bin/bash
# Debug script for audio_level file investigation
# Run this WHILE recording to see if the file is being created/updated

AUDIO_LEVEL_FILE="$HOME/.config/hyprwhspr/audio_level"
RECORDING_STATUS_FILE="$HOME/.config/hyprwhspr/recording_status"

echo "=== Waveform Debug Tool ==="
echo "This script monitors the audio_level file during recording."
echo "Start a voice recording (Super+\`) and watch for changes."
echo ""
echo "Press Ctrl+C to stop."
echo ""

while true; do
    clear
    echo "=== $(date '+%Y-%m-%d %H:%M:%S') ==="
    echo ""

    # Check recording status
    echo "Recording status file:"
    if [[ -f "$RECORDING_STATUS_FILE" ]]; then
        echo "  Exists: YES"
        echo "  Content: $(cat "$RECORDING_STATUS_FILE" 2>/dev/null)"
        echo "  Age: $(($(date +%s) - $(stat -c %Y "$RECORDING_STATUS_FILE" 2>/dev/null || echo 0)))s"
    else
        echo "  Exists: NO (not recording or file cleaned up)"
    fi
    echo ""

    # Check audio level file
    echo "Audio level file:"
    if [[ -f "$AUDIO_LEVEL_FILE" ]]; then
        echo "  Exists: YES"
        echo "  Content: $(cat "$AUDIO_LEVEL_FILE" 2>/dev/null)"
        echo "  Age: $(($(date +%s) - $(stat -c %Y "$AUDIO_LEVEL_FILE" 2>/dev/null || echo 0)))s"

        # Show visualization
        level=$(cat "$AUDIO_LEVEL_FILE" 2>/dev/null || echo "0")
        echo ""
        echo "  Visualization: $(
            num_segments=20
            active=$(awk -v l="$level" -v n="$num_segments" 'BEGIN {
                scaled = sqrt(l) * n
                segs = int(scaled + 0.5)
                if (segs > n) segs = n
                if (segs < 0) segs = 0
                print segs
            }')
            for ((i=0; i<num_segments; i++)); do
                if [ $i -lt $active ]; then
                    printf '▪'
                else
                    printf '·'
                fi
            done
        )"
    else
        echo "  Exists: NO"
        echo ""
        echo "  ⚠ The audio_level file should exist during active recording!"
        echo "  This indicates the audio level monitoring thread may not be running."
    fi
    echo ""

    # Check hyprwhspr process
    echo "hyprwhspr process:"
    if pgrep -f "hyprwhspr" > /dev/null 2>&1; then
        echo "  Running: YES (PID: $(pgrep -f "hyprwhspr" | head -1))"
    else
        echo "  Running: NO"
    fi
    echo ""

    # Check service status
    echo "Service status:"
    if systemctl --user is-active --quiet hyprwhspr.service; then
        echo "  hyprwhspr.service: active"
    else
        echo "  hyprwhspr.service: $(systemctl --user is-active hyprwhspr.service)"
    fi

    sleep 0.5
done
