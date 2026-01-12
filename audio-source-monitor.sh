#!/bin/bash
# audio-source-monitor.sh - Smart audio source fallback for voice typing
#
# Part of omarchy-voice-typing repo
#
# Monitors Bluetooth headset and automatically falls back to laptop mic
# when BT goes silent/suspended (e.g., phone captures it).
#
# Features:
# - Detects silence/suspension on BT device
# - Falls back to laptop mic after 2nd silence
# - Noise threshold filtering to avoid glitchy sources
# - Automatic recovery when BT returns
#
# Usage: ./audio-source-monitor.sh [start|stop|status]

set -euo pipefail

# Configuration
BT_SILENCE_THRESHOLD=2      # Fall back after 2nd silence event
NOISE_LEVEL_THRESHOLD=0.05  # Filter sources with noise > 5%
CHECK_INTERVAL=2            # Check every 2 seconds

# State tracking
STATE_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/omarchy-voice-typing"
STATE_FILE="$STATE_DIR/audio-monitor.state"
PID_FILE="$STATE_DIR/audio-monitor.pid"

mkdir -p "$STATE_DIR"

# Log function
log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" >&2
    logger -t omarchy-voice-typing "$*" || true
}

# Get all available input sources
get_sources() {
    pactl list sources short | awk '{print $2}'
}

# Check if source is suspended
is_suspended() {
    local source="$1"
    pactl list sources | grep -A 10 "Name: $source" | grep -q "State: SUSPENDED"
}

# Get source description (human-readable name)
get_source_description() {
    local source="$1"
    pactl list sources | grep -A 5 "Name: $source" | grep "Description:" | cut -d: -f2- | xargs
}

# Check if source is Bluetooth
is_bluetooth() {
    local source="$1"
    [[ "$source" == bluez_* ]]
}

# Check if source is internal laptop mic
is_internal_mic() {
    local source="$1"
    [[ "$source" == *"platform-skl_hda_dsp_generic"* ]] || \
    [[ "$source" == *"Mic1"* ]] || \
    [[ "$source" == *"Mic2"* ]] || \
    [[ "$source" == *"analog-stereo"* ]]
}

# Measure source noise level (simplified - checks if audio is flowing)
check_noise_level() {
    local source="$1"

    # Check if source is not in ERROR state
    local state
    state=$(pactl list sources | grep -A 10 "Name: $source" | grep "State:" | awk '{print $2}')

    if [[ "$state" == "ERROR" ]]; then
        return 1  # Too problematic
    fi

    # TODO: Implement actual noise measurement using parec + audio analysis
    # For now, accept all non-ERROR sources
    return 0
}

# Get preferred fallback source
get_fallback_source() {
    local sources
    sources=$(get_sources)

    # Priority: Internal Mic1 > Internal Mic2 > Any other non-BT source
    for source in $sources; do
        if is_internal_mic "$source" && ! is_suspended "$source"; then
            if check_noise_level "$source"; then
                echo "$source"
                return 0
            fi
        fi
    done

    # Fallback: any non-BT, non-suspended source
    for source in $sources; do
        if ! is_bluetooth "$source" && ! is_suspended "$source"; then
            if check_noise_level "$source"; then
                echo "$source"
                return 0
            fi
        fi
    done

    return 1
}

# Switch default source
switch_source() {
    local new_source="$1"
    local desc
    desc=$(get_source_description "$new_source")

    pactl set-default-source "$new_source"
    log "Switched to: $desc ($new_source)"

    # Send notification
    notify-send -u normal "Voice Typing Source" "Switched to: $desc" --icon=audio-input-microphone 2>/dev/null || true
}

# Load state
load_state() {
    if [[ -f "$STATE_FILE" ]]; then
        # shellcheck disable=SC1090
        source "$STATE_FILE"
    else
        SILENCE_COUNT=0
        CURRENT_FALLBACK=""
        BT_WAS_ACTIVE=false
    fi
}

# Save state
save_state() {
    cat > "$STATE_FILE" <<EOF
SILENCE_COUNT=$SILENCE_COUNT
CURRENT_FALLBACK="$CURRENT_FALLBACK"
BT_WAS_ACTIVE=$BT_WAS_ACTIVE
EOF
}

# Main monitoring loop
monitor() {
    log "Audio source monitor started (PID: $$)"
    log "BT silence threshold: $BT_SILENCE_THRESHOLD"

    # Save PID
    echo $$ > "$PID_FILE"

    load_state

    while true; do
        # Get current default source
        default_source=$(pactl get-default-source)

        # Check if default source is Bluetooth
        if is_bluetooth "$default_source"; then
            if is_suspended "$default_source"; then
                # BT is suspended
                ((SILENCE_COUNT++)) || true
                log "BT suspended (silence count: $SILENCE_COUNT/$BT_SILENCE_THRESHOLD)"

                if [[ $SILENCE_COUNT -ge $BT_SILENCE_THRESHOLD ]]; then
                    # Fall back to laptop mic
                    fallback_source=$(get_fallback_source)
                    if [[ -n "$fallback_source" ]]; then
                        switch_source "$fallback_source"
                        CURRENT_FALLBACK="$fallback_source"
                        save_state
                    else
                        log "WARNING: No suitable fallback source found"
                    fi
                fi

                BT_WAS_ACTIVE=false
            else
                # BT is active
                if [[ "$BT_WAS_ACTIVE" == "false" ]]; then
                    log "BT resumed - resetting silence count"
                    SILENCE_COUNT=0
                    BT_WAS_ACTIVE=true
                    save_state
                fi
            fi
        else
            # Currently on fallback source
            # Check if BT has returned
            local bt_source
            bt_source=$(get_sources | grep -m1 "^bluez_" || true)

            if [[ -n "$bt_source" ]] && ! is_suspended "$bt_source"; then
                log "BT available again - switching back"
                switch_source "$bt_source"
                SILENCE_COUNT=0
                CURRENT_FALLBACK=""
                BT_WAS_ACTIVE=true
                save_state
            fi
        fi

        sleep "$CHECK_INTERVAL"
    done
}

# Handle cleanup
cleanup() {
    log "Audio source monitor stopped"
    save_state
    rm -f "$PID_FILE"
    exit 0
}

# Command handlers
cmd_start() {
    if [[ -f "$PID_FILE" ]]; then
        local pid
        pid=$(cat "$PID_FILE")
        if kill -0 "$pid" 2>/dev/null; then
            echo "Monitor already running (PID: $pid)"
            exit 1
        else
            rm -f "$PID_FILE"
        fi
    fi

    trap cleanup SIGTERM SIGINT
    monitor "$@"
}

cmd_stop() {
    if [[ ! -f "$PID_FILE" ]]; then
        echo "Monitor not running"
        exit 1
    fi

    local pid
    pid=$(cat "$PID_FILE")
    if kill -0 "$pid" 2>/dev/null; then
        echo "Stopping monitor (PID: $pid)"
        kill "$pid"
        rm -f "$PID_FILE"
        echo "Stopped"
    else
        echo "Monitor not running (stale PID file)"
        rm -f "$PID_FILE"
    fi
}

cmd_status() {
    if [[ ! -f "$PID_FILE" ]]; then
        echo "Monitor: Not running"
        exit 1
    fi

    local pid
    pid=$(cat "$PID_FILE")
    if kill -0 "$pid" 2>/dev/null; then
        echo "Monitor: Running (PID: $pid)"

        # Show current state
        if [[ -f "$STATE_FILE" ]]; then
            echo ""
            echo "State:"
            cat "$STATE_FILE" | sed 's/^/  /'
        fi

        # Show current audio source
        echo ""
        echo "Current source:"
        local source
        source=$(pactl get-default-source)
        echo "  $(get_source_description "$source")"
        echo "  ($source)"
    else
        echo "Monitor: Not running (stale PID file)"
        rm -f "$PID_FILE"
        exit 1
    fi
}

# Main
main() {
    local cmd="${1:-start}"

    case "$cmd" in
        start)
            cmd_start
            ;;
        stop)
            cmd_stop
            ;;
        status)
            cmd_status
            ;;
        *)
            echo "Usage: $0 [start|stop|status]"
            exit 1
            ;;
    esac
}

main "$@"
