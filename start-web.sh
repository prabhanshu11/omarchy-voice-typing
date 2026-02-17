#!/bin/bash
# Start the Voice Gateway with Web UI

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Check for API key
if [ -z "$ASSEMBLYAI_API_KEY" ] && [ -z "$ASSEMBLY_API_KEY" ]; then
    echo "Warning: ASSEMBLYAI_API_KEY or ASSEMBLY_API_KEY not set"
    echo "Transcription will not work, but you can still view existing recordings/transcripts"
fi

# Set default paths for recordings/transcripts (global directories)
export RECORDINGS_DIR="${RECORDINGS_DIR:-$HOME/Programs/recordings}"
export TRANSCRIPTS_DIR="${TRANSCRIPTS_DIR:-$HOME/Programs/transcripts}"

# Build the web UI if needed
if [ ! -d "web/dist" ]; then
    echo "Building web UI..."
    cd web
    npm install
    npm run build
    cd ..
fi

# Build the gateway if needed
if [ ! -f "gateway/voice-gateway" ]; then
    echo "Building gateway..."
    cd gateway
    go build -o voice-gateway ./cmd/server/
    cd ..
fi

# Start the gateway
echo "============================================"
echo "  Voice Gateway Web UI"
echo "============================================"
echo "  URL: http://localhost:8765"
echo "  Recordings: $RECORDINGS_DIR"
echo "  Transcripts: $TRANSCRIPTS_DIR"
echo "============================================"
cd gateway
exec ./voice-gateway
