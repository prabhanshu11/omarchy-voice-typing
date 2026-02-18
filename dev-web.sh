#!/bin/bash
# Development mode - run gateway and Vite dev server in parallel

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Check for API key
if [ -z "$ASSEMBLYAI_API_KEY" ] && [ -z "$ASSEMBLY_API_KEY" ]; then
    echo "Warning: ASSEMBLYAI_API_KEY or ASSEMBLY_API_KEY not set"
fi

# Set default paths for recordings/transcripts (global directories)
export RECORDINGS_DIR="${RECORDINGS_DIR:-$HOME/Programs/recordings}"
export TRANSCRIPTS_DIR="${TRANSCRIPTS_DIR:-$HOME/Programs/transcripts}"

# Build the gateway if needed
if [ ! -f "gateway/voice-gateway" ]; then
    echo "Building gateway..."
    cd gateway
    go build -o voice-gateway ./cmd/server/
    cd ..
fi

# Install web dependencies if needed
if [ ! -d "web/node_modules" ]; then
    echo "Installing web dependencies..."
    cd web
    npm install
    cd ..
fi

# Function to cleanup background processes
cleanup() {
    echo "Stopping servers..."
    kill $GATEWAY_PID 2>/dev/null || true
    kill $VITE_PID 2>/dev/null || true
    exit 0
}

trap cleanup SIGINT SIGTERM

# Start the gateway in the background
echo "Starting gateway on port 8765..."
cd gateway
./voice-gateway &
GATEWAY_PID=$!
cd ..

# Wait a bit for the gateway to start
sleep 1

# Start Vite dev server
echo "Starting Vite dev server on http://localhost:3000"
cd web
npm run dev &
VITE_PID=$!
cd ..

echo ""
echo "============================================"
echo "  Voice Gateway Development Mode"
echo "============================================"
echo "  Frontend: http://localhost:3000"
echo "  API:      http://localhost:8765"
echo "  Recordings: $RECORDINGS_DIR"
echo "  Transcripts: $TRANSCRIPTS_DIR"
echo "============================================"
echo "  Press Ctrl+C to stop both servers"
echo ""

# Wait for either process to exit
wait
