# omarchy-voice-typing — Claude Code Context

## Build Contract

**local-bootstrapping calls `./build.sh`. Binary always lands at `./bin/voice-gateway`.**

This is the single source of truth for how to build the gateway. Never hardcode Go or Rust
specifics in local-bootstrapping — update `build.sh` here instead.

### Changing the Gateway Implementation

If you migrate to a new language or restructure the gateway directory:
1. Update `build.sh` detection logic (Rust: `gateway-rs/Cargo.toml`, Go: `gateway/go.mod`)
2. `bin/voice-gateway` must always be the output — the service file never changes
3. Update `CLAUDE.md` (this file) with what changed and why
4. The `bin/` directory is gitignored — it is always rebuilt by `setup-voice-typing.sh`

### Gateway Implementations

| Status | Language | Dir | Binary |
|--------|----------|-----|--------|
| Active | Rust | `gateway-rs/` | `target/release/voice-gateway` → `bin/voice-gateway` |
| Legacy | Go | `gateway/` | `gateway/voice-gateway` → `bin/voice-gateway` |

### Canonical Service Path

The systemd service file in local-bootstrapping always uses:
```
ExecStart=%h/Programs/omarchy-voice-typing/bin/voice-gateway
WorkingDirectory=%h/Programs/omarchy-voice-typing
```

## Architecture

- **hyprwhspr**: Audio capture daemon (AUR package), controlled via `hyprwhspr-toggle`
- **voice-gateway**: HTTP gateway (Rust, port 8765) — receives audio, calls AssemblyAI/Deepgram
- **local-whisper**: Python venv, offline fallback via local Whisper model
- **MicOSD**: GTK4 overlay showing recording state (managed by hyprwhspr-patch)

## Key Files

| File | Purpose |
|------|---------|
| `build.sh` | Build contract — builds binary to `bin/voice-gateway` |
| `.env` | API keys (ASSEMBLYAI_API_KEY, DEEPGRAM_API_KEY) — gitignored |
| `gateway-rs/` | Rust gateway source |
| `gateway/` | Legacy Go gateway source |
| `local-whisper/` | Python offline fallback |
| `logs/` | Runtime logs (gitignored) |

## Deployment

This repo is deployed by `local-bootstrapping/scripts/setup-voice-typing.sh`.
That script:
1. Pulls this repo to master
2. Calls `./build.sh` to build the binary
3. Installs the systemd service file with canonical path
4. Enables and restarts the service

Do not manually run `cargo build` and copy binaries — always let `setup-voice-typing.sh` handle it.
