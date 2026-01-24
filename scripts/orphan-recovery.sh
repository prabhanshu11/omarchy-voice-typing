#!/bin/bash
# orphan-recovery.sh - Find and transcribe orphaned voice recordings
#
# A recording is "orphaned" if it reached the gateway but transcription failed.
# This script:
#   1. Scans ~/Programs/recordings/ for .wav files
#   2. Checks if a transcript exists within 5 minutes of the recording timestamp
#   3. Re-submits orphaned recordings to the gateway for transcription
#
# Edge cases handled:
#   - Files still being written (skip if modified < 60s ago)
#   - Empty/corrupted files (skip if < 10KB)
#   - Gateway not running (fail gracefully with notification)
#
# Usage: orphan-recovery.sh [--dry-run]

set -euo pipefail

RECORDINGS_DIR="${HOME}/Programs/recordings"
TRANSCRIPTS_DIR="${HOME}/Programs/transcripts"
GATEWAY_URL="http://localhost:8765/v1/transcribe"
MIN_AGE_SECONDS=60
MIN_SIZE_BYTES=10000  # 10KB
TIME_WINDOW_SECONDS=300  # 5 minutes

DRY_RUN=false
VERBOSE=false

log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $1"
}

usage() {
    echo "Usage: $0 [--dry-run] [--verbose]"
    echo "  --dry-run  Show what would be done without actually doing it"
    echo "  --verbose  Show detailed output"
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
        -h|--help)
            usage
            ;;
        *)
            echo "Unknown option: $1"
            usage
            ;;
    esac
done

# Check gateway health
check_gateway() {
    if ! curl -sf "${GATEWAY_URL%/v1/transcribe}/health" > /dev/null 2>&1; then
        log "ERROR: Gateway is not running at $GATEWAY_URL"
        notify-send -u critical "Voice Recovery" "Gateway not running - cannot recover orphaned recordings"
        exit 1
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

# Check if a transcript exists within TIME_WINDOW_SECONDS of recording
has_matching_transcript() {
    local recording="$1"
    local rec_epoch
    rec_epoch=$(get_file_epoch "$recording")

    if [[ "$rec_epoch" == "0" ]]; then
        # Can't parse timestamp, assume it's matched (don't process)
        return 0
    fi

    local min_epoch=$rec_epoch
    local max_epoch=$((rec_epoch + TIME_WINDOW_SECONDS))

    for transcript in "$TRANSCRIPTS_DIR"/*.txt; do
        [[ -f "$transcript" ]] || continue
        local trans_epoch
        trans_epoch=$(get_file_epoch "$transcript")
        if [[ "$trans_epoch" != "0" ]] && \
           [[ "$trans_epoch" -ge "$min_epoch" ]] && \
           [[ "$trans_epoch" -le "$max_epoch" ]]; then
            return 0  # Found matching transcript
        fi
    done

    return 1  # No matching transcript found
}

# Submit recording to gateway
submit_recording() {
    local recording="$1"
    log "Submitting: $(basename "$recording")"

    local response
    response=$(curl -s -w "\n%{http_code}" -F "file=@$recording" "$GATEWAY_URL")
    local http_code="${response##*$'\n'}"
    local body="${response%$'\n'*}"

    if [[ "$http_code" == "200" ]]; then
        local text
        text=$(echo "$body" | jq -r '.text // empty')
        if [[ -n "$text" ]]; then
            log "SUCCESS: Recovered transcript (${#text} chars)"
            # Copy to clipboard and notify
            echo -n "$text" | wl-copy
            notify-send -t 5000 "Voice Recovery" "Recovered orphaned recording:\n${text:0:100}..."
            return 0
        fi
    fi

    log "FAILED: HTTP $http_code"
    return 1
}

main() {
    log "Starting orphan recovery scan"

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

    local orphan_count=0
    local recovered_count=0
    local now
    now=$(date +%s)

    for recording in "$RECORDINGS_DIR"/*.wav; do
        [[ -f "$recording" ]] || continue

        # Skip files still being written (modified < 60s ago)
        local mtime
        mtime=$(stat -c %Y "$recording" 2>/dev/null || echo "0")
        local age=$((now - mtime))
        if [[ $age -lt $MIN_AGE_SECONDS ]]; then
            [[ "$VERBOSE" == "true" ]] && log "SKIP (too recent): $(basename "$recording")"
            continue
        fi

        # Skip files that are too small (likely empty/corrupted)
        local size
        size=$(stat -c %s "$recording" 2>/dev/null || echo "0")
        if [[ $size -lt $MIN_SIZE_BYTES ]]; then
            [[ "$VERBOSE" == "true" ]] && log "SKIP (too small): $(basename "$recording") (${size} bytes)"
            continue
        fi

        # Check if transcript exists
        if has_matching_transcript "$recording"; then
            [[ "$VERBOSE" == "true" ]] && log "SKIP (has transcript): $(basename "$recording")"
            continue
        fi

        # Found orphan
        orphan_count=$((orphan_count + 1))
        log "ORPHAN: $(basename "$recording") (${size} bytes, ${age}s old)"

        if [[ "$DRY_RUN" == "true" ]]; then
            continue
        fi

        # Submit for transcription
        if submit_recording "$recording"; then
            recovered_count=$((recovered_count + 1))
        fi
    done

    log "Scan complete: $orphan_count orphans found, $recovered_count recovered"
}

main "$@"
