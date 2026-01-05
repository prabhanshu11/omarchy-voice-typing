# Project Progress

## Status
- [x] Project Initialization
- [x] Requirements Definition (`project_context.md`)
- [x] Gateway Implementation (Go) - *Verified with tests*
- [x] Renamed project to `voice-gateway`
- [x] Backend integrated with `hyprwhspr` setup
- [x] Docker Support - *Verified*
- [x] Systemd Service - *Installed and active*
- [x] Integration Tests (Python) - *Passed*

## Accomplishments
- Implemented Go gateway with 600s server timeouts for reliability.
- Migrated naming from `omarchy` to `voice-gateway` for cross-platform readiness.
- Successfully configured `hyprwhspr` to use the local AssemblyAI gateway.
- Verified end-to-end transcription workflow.
- Implemented automatic logging of audio recordings (`recordings/`) and transcripts (`transcripts/`).
- Added support for custom word replacements via `config/replacements.json` (e.g., "Dovac" -> "Dvorak").

## Active Tasks
*(None currently active)*

## Next Steps
- Implement streaming (WebSocket) support (future scope).

## Notes
- Replacements config format: List of objects with `from` (list of strings) and `to` (target string). Gateway will flatten this if API requires 1:1.
