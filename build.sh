#!/bin/bash
# Canonical build script for voice-gateway.
# local-bootstrapping calls this. Binary ALWAYS lands at ./bin/voice-gateway.
# DO NOT hardcode Go or Rust specifics in local-bootstrapping — change this file instead.
#
# Usage: ./build.sh
set -e

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
mkdir -p "$REPO_ROOT/bin"

if [ -f "$REPO_ROOT/gateway-rs/Cargo.toml" ]; then
    echo "[build.sh] Detected Rust gateway"
    cd "$REPO_ROOT/gateway-rs"
    cargo build --release
    cp target/release/voice-gateway "$REPO_ROOT/bin/voice-gateway"
elif [ -f "$REPO_ROOT/gateway/go.mod" ]; then
    echo "[build.sh] Detected Go gateway"
    cd "$REPO_ROOT/gateway"
    go build -o voice-gateway ./cmd/server
    cp voice-gateway "$REPO_ROOT/bin/voice-gateway"
else
    echo "[build.sh] ERROR: No recognizable gateway found (expected gateway-rs/Cargo.toml or gateway/go.mod)" >&2
    exit 1
fi

echo "[build.sh] Built: $REPO_ROOT/bin/voice-gateway"
