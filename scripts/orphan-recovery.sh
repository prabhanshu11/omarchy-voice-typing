#!/bin/bash
# orphan-recovery.sh - Find and transcribe orphaned voice recordings
#
# A recording is "orphaned" if it reached the gateway but transcription failed.
# This script:
#   1. Scans ~/Programs/recordings/ for .wav files
#   2. Uses O(1) hash map lookup to check if transcript exists
#   3. Respects backoff for previously failed files
#   4. Re-submits orphaned recordings to the gateway for transcription
#   5. Logs events to datalake via Python helper
#
# Edge cases handled:
#   - Files still being written (skip if modified < 60s ago)
#   - Empty/corrupted files (skip if < 10KB)
#   - Gateway not running (fail gracefully with notification)
#   - Ancient recordings (skip if > 48 hours old)
#   - Previously failed files (exponential backoff)
#
# Usage: orphan-recovery.sh [--dry-run] [--verbose] [--reset-failures]

set -euo pipefail

RECORDINGS_DIR="${HOME}/Programs/recordings"
TRANSCRIPTS_DIR="${HOME}/Programs/transcripts"
GATEWAY_URL="http://localhost:8765/v1/transcribe"
STATE_DIR="${HOME}/.cache/voice-recovery"
STATE_FILE="${STATE_DIR}/state.json"
LOG_DIR="${HOME}/Programs/omarchy-voice-typing/logs"
DATALAKE_HELPER="${HOME}/Programs/omarchy-voice-typing/scripts/log-recovery-event.py"

# Constraints
MIN_AGE_SECONDS=60       # Skip files modified less than 60s ago
MIN_SIZE_BYTES=10000     # 10KB minimum
TIME_WINDOW_SECONDS=300  # 5 minute matching window
MAX_AGE_HOURS=48         # Ignore recordings older than 48 hours
MAX_FAILURES=5           # After 5 failures, skip permanently

DRY_RUN=false
VERBOSE=false
RESET_FAILURES=false

# Backoff intervals in seconds: 1h, 6h, 24h, 48h, permanent
BACKOFF_INTERVALS=(3600 21600 86400 172800)

log() {
    local timestamp
    timestamp=$(date '+%Y-%m-%d %H:%M:%S')
    echo "[$timestamp] $1"
    # Also write to log file
    mkdir -p "$LOG_DIR"
    echo "[$timestamp] $1" >> "$LOG_DIR/orphan-recovery.log"
}

log_verbose() {
    [[ "$VERBOSE" == "true" ]] && log "$1"
}

usage() {
    echo "Usage: $0 [--dry-run] [--verbose] [--reset-failures]"
    echo "  --dry-run         Show what would be done without actually doing it"
    echo "  --verbose         Show detailed output"
    echo "  --reset-failures  Clear all failed file tracking (retry everything)"
    exit 1
}

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        --verbose)
            VERBOSE=true
            shift
            ;;
        --reset-failures)
            RESET_FAILURES=true
            shift
            ;;
        -h|--help)
            usage
            ;;
        *)
            echo "Unknown option: $1"
            usage
            ;;
    esac
done

# Initialize state directory and file
init_state() {
    mkdir -p "$STATE_DIR"
    if [[ ! -f "$STATE_FILE" ]] || [[ "$RESET_FAILURES" == "true" ]]; then
        echo '{"last_scan": null, "failed_files": {}}' > "$STATE_FILE"
        [[ "$RESET_FAILURES" == "true" ]] && log "Reset failure tracking"
    fi
}

# Log event to datalake (if helper exists)
log_event() {
    local event_type="$1"
    local recording="${2:-}"
    local error="${3:-}"

    if [[ -x "$DATALAKE_HELPER" ]]; then
        "$DATALAKE_HELPER" "$event_type" "$recording" "$error" 2>/dev/null || true
    fi
}

# Get failed file info from state
# Returns: attempts last_attempt skip_until (or empty if not found)
get_failure_info() {
    local filename="$1"
    if [[ -f "$STATE_FILE" ]]; then
        jq -r --arg f "$filename" '.failed_files[$f] // empty | "\(.attempts) \(.last_attempt) \(.skip_until)"' "$STATE_FILE" 2>/dev/null || true
    fi
}

# Update failed file in state
update_failure() {
    local filename="$1"
    local error="$2"
    local now
    now=$(date -Iseconds)

    local current_attempts
    current_attempts=$(jq -r --arg f "$filename" '.failed_files[$f].attempts // 0' "$STATE_FILE" 2>/dev/null || echo "0")
    local new_attempts=$((current_attempts + 1))

    # Calculate backoff interval
    local backoff_idx=$((new_attempts - 1))
    if [[ $backoff_idx -ge ${#BACKOFF_INTERVALS[@]} ]]; then
        backoff_idx=$((${#BACKOFF_INTERVALS[@]} - 1))
    fi
    local backoff_seconds=${BACKOFF_INTERVALS[$backoff_idx]}
    local skip_until
    skip_until=$(date -Iseconds -d "+${backoff_seconds} seconds")

    # Update state file
    local tmp_file
    tmp_file=$(mktemp)
    jq --arg f "$filename" \
       --arg now "$now" \
       --arg skip "$skip_until" \
       --arg err "$error" \
       --argjson attempts "$new_attempts" \
       '.failed_files[$f] = {
           "attempts": $attempts,
           "last_attempt": $now,
           "skip_until": $skip,
           "last_error": $err
       }' "$STATE_FILE" > "$tmp_file" && mv "$tmp_file" "$STATE_FILE"

    log "Recorded failure #$new_attempts for $(basename "$filename"), retry after $(date -d "$skip_until" '+%Y-%m-%d %H:%M')"
}

# Check if file should be skipped due to backoff
should_skip_backoff() {
    local filename="$1"
    local info
    info=$(get_failure_info "$filename")

    [[ -z "$info" ]] && return 1  # Not in failed list

    local attempts skip_until
    read -r attempts _ skip_until <<< "$info"

    # Check if max failures exceeded
    if [[ $attempts -ge $MAX_FAILURES ]]; then
        log_verbose "SKIP (permanent failure after $attempts attempts): $(basename "$filename")"
        return 0
    fi

    # Check if still in backoff period
    local now_epoch skip_epoch
    now_epoch=$(date +%s)
    skip_epoch=$(date -d "$skip_until" +%s 2>/dev/null || echo "0")

    if [[ $now_epoch -lt $skip_epoch ]]; then
        log_verbose "SKIP (backoff until $(date -d "$skip_until" '+%H:%M')): $(basename "$filename")"
        return 0
    fi

    return 1  # OK to retry
}

# Remove file from failed list (on success)
clear_failure() {
    local filename="$1"
    local tmp_file
    tmp_file=$(mktemp)
    jq --arg f "$filename" 'del(.failed_files[$f])' "$STATE_FILE" > "$tmp_file" && mv "$tmp_file" "$STATE_FILE"
}

# Update last scan time
update_scan_time() {
    local tmp_file
    tmp_file=$(mktemp)
    jq --arg now "$(date -Iseconds)" '.last_scan = $now' "$STATE_FILE" > "$tmp_file" && mv "$tmp_file" "$STATE_FILE"
}

# Check gateway health
check_gateway() {
    if ! curl -sf "${GATEWAY_URL%/v1/transcribe}/health" > /dev/null 2>&1; then
        log "ERROR: Gateway is not running at $GATEWAY_URL"
        log_event "recovery_scan_started" "" "gateway_not_running"
        # No notification for gateway down - it's expected when not in use
        exit 0  # Exit cleanly, don't spam errors
    fi
}

# Extract timestamp from filename (YYYYMMDD_HHMMSS)
# Returns epoch seconds
get_file_epoch() {
    local filename="$1"
    local ts
    ts=$(basename "$filename" | grep -oP '^\d{8}_\d{6}' || echo "")
    if [[ -z "$ts" ]]; then
        echo "0"
        return
    fi
    # Convert YYYYMMDD_HHMMSS to epoch
    date -d "${ts:0:4}-${ts:4:2}-${ts:6:2} ${ts:9:2}:${ts:11:2}:${ts:13:2}" +%s 2>/dev/null || echo "0"
}

# Build hash map of transcript timestamps for O(1) lookup
# Key: epoch second, Value: 1 (exists)
declare -A TRANSCRIPT_INDEX

build_transcript_index() {
    log_verbose "Building transcript index..."
    local count=0

    for transcript in "$TRANSCRIPTS_DIR"/*.txt; do
        [[ -f "$transcript" ]] || continue
        local trans_epoch
        trans_epoch=$(get_file_epoch "$transcript")
        [[ "$trans_epoch" == "0" ]] && continue

        # Index the timestamp and a window around it
        # This allows matching within TIME_WINDOW_SECONDS
        local start_epoch=$((trans_epoch - TIME_WINDOW_SECONDS))
        local end_epoch=$trans_epoch

        for ((e = start_epoch; e <= end_epoch; e++)); do
            TRANSCRIPT_INDEX[$e]=1
        done

        count=$((count + 1))
    done

    log_verbose "Indexed $count transcripts"
}

# O(1) check if transcript exists for recording
has_matching_transcript() {
    local recording="$1"
    local rec_epoch
    rec_epoch=$(get_file_epoch "$recording")

    if [[ "$rec_epoch" == "0" ]]; then
        # Can't parse timestamp, assume it's matched (don't process)
        return 0
    fi

    # Check if any transcript exists within window after recording
    [[ -n "${TRANSCRIPT_INDEX[$rec_epoch]:-}" ]] && return 0

    return 1  # No matching transcript found
}

# Submit recording to gateway
# Returns: 0 on success, 1 on failure
# Sets: LAST_TRANSCRIPT (text of successful transcription)
LAST_TRANSCRIPT=""

submit_recording() {
    local recording="$1"
    local basename_rec
    basename_rec=$(basename "$recording")

    log "Submitting: $basename_rec"
    log_event "recovery_attempt" "$basename_rec"

    local response http_code body
    response=$(curl -s -w "\n%{http_code}" -F "file=@$recording" "$GATEWAY_URL" 2>&1)
    http_code="${response##*$'\n'}"
    body="${response%$'\n'*}"

    if [[ "$http_code" == "200" ]]; then
        local text
        text=$(echo "$body" | jq -r '.text // empty')
        if [[ -n "$text" ]]; then
            log "SUCCESS: Recovered transcript (${#text} chars)"
            log_event "recovery_success" "$basename_rec"
            clear_failure "$basename_rec"
            LAST_TRANSCRIPT="$text"
            return 0
        fi
    fi

    log "FAILED: HTTP $http_code"
    log_event "recovery_failed" "$basename_rec" "HTTP $http_code"
    update_failure "$basename_rec" "HTTP $http_code"
    return 1
}

main() {
    log "Starting orphan recovery scan"
    init_state

    if [[ ! -d "$RECORDINGS_DIR" ]]; then
        log "Recordings directory not found: $RECORDINGS_DIR"
        exit 0
    fi

    if [[ ! -d "$TRANSCRIPTS_DIR" ]]; then
        log "Transcripts directory not found: $TRANSCRIPTS_DIR"
        exit 0
    fi

    if [[ "$DRY_RUN" == "false" ]]; then
        check_gateway
    fi

    log_event "recovery_scan_started"

    # Build transcript index for O(1) lookup
    build_transcript_index

    local orphan_count=0
    local recovered_count=0
    local skipped_age=0
    local skipped_backoff=0
    local failed_count=0
    local now
    now=$(date +%s)
    local max_age_seconds=$((MAX_AGE_HOURS * 3600))

    # Collect orphans for batch processing
    declare -a orphan_files=()

    for recording in "$RECORDINGS_DIR"/*.wav; do
        [[ -f "$recording" ]] || continue
        local basename_rec
        basename_rec=$(basename "$recording")

        # Skip files still being written (modified < 60s ago)
        local mtime
        mtime=$(stat -c %Y "$recording" 2>/dev/null || echo "0")
        local age=$((now - mtime))
        if [[ $age -lt $MIN_AGE_SECONDS ]]; then
            log_verbose "SKIP (too recent): $basename_rec"
            continue
        fi

        # Skip files older than MAX_AGE_HOURS
        if [[ $age -gt $max_age_seconds ]]; then
            log_verbose "SKIP (too old, ${age}s > ${max_age_seconds}s): $basename_rec"
            skipped_age=$((skipped_age + 1))
            continue
        fi

        # Skip files that are too small (likely empty/corrupted)
        local size
        size=$(stat -c %s "$recording" 2>/dev/null || echo "0")
        if [[ $size -lt $MIN_SIZE_BYTES ]]; then
            log_verbose "SKIP (too small): $basename_rec (${size} bytes)"
            continue
        fi

        # Check backoff for previously failed files
        if should_skip_backoff "$basename_rec"; then
            skipped_backoff=$((skipped_backoff + 1))
            log_event "recovery_skipped" "$basename_rec" "backoff"
            continue
        fi

        # Check if transcript exists (O(1) lookup)
        if has_matching_transcript "$recording"; then
            log_verbose "SKIP (has transcript): $basename_rec"
            continue
        fi

        # Found orphan
        orphan_count=$((orphan_count + 1))
        log "ORPHAN: $basename_rec (${size} bytes, ${age}s old)"
        log_event "recovery_orphan_found" "$basename_rec"

        orphan_files+=("$recording")
    done

    # Process orphans
    if [[ "$DRY_RUN" == "false" ]] && [[ ${#orphan_files[@]} -gt 0 ]]; then
        for recording in "${orphan_files[@]}"; do
            if submit_recording "$recording"; then
                recovered_count=$((recovered_count + 1))
            else
                failed_count=$((failed_count + 1))
            fi
        done
    fi

    # Update scan time
    update_scan_time

    # Summary logging
    log "Scan complete: $orphan_count orphans found, $recovered_count recovered, $failed_count failed"
    [[ $skipped_age -gt 0 ]] && log "Skipped $skipped_age recordings older than ${MAX_AGE_HOURS}h"
    [[ $skipped_backoff -gt 0 ]] && log "Skipped $skipped_backoff recordings in backoff"

    # Batch notification (only if any orphans found/recovered)
    if [[ "$DRY_RUN" == "false" ]] && [[ $orphan_count -gt 0 ]]; then
        if [[ $recovered_count -eq 1 ]] && [[ -n "$LAST_TRANSCRIPT" ]]; then
            # Single recovery: copy to clipboard and notify with text
            echo -n "$LAST_TRANSCRIPT" | wl-copy
            notify-send -t 5000 "Voice Recovery" "Recovered 1 orphan:\n${LAST_TRANSCRIPT:0:100}..."
        elif [[ $recovered_count -gt 1 ]]; then
            # Multiple recoveries: summary only, no clipboard
            notify-send -t 5000 "Voice Recovery" "Recovered $recovered_count orphaned recordings"
        elif [[ $failed_count -gt 0 ]]; then
            # Failures only: brief notification
            notify-send -t 3000 "Voice Recovery" "Found $orphan_count orphans, $failed_count failed (will retry)"
        fi
    fi
}

main "$@"
