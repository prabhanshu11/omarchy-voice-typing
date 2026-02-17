#!/bin/bash
# Voice Gateway startup script
# Loads AssemblyAI API key from pass and starts the gateway

# Load API key from pass (requires GPG passphrase on first access)
export ASSEMBLYAI_API_KEY="$(pass api/assemblyai 2>/dev/null)"

if [[ -z "$ASSEMBLYAI_API_KEY" ]]; then
    echo "Error: Could not retrieve ASSEMBLYAI_API_KEY from pass"
    echo "Make sure pass is initialized and api/assemblyai is set"
    exit 1
fi

# Start the gateway
cd "$(dirname "$0")"
exec ./gateway/voice-gateway
