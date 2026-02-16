# Backlog

## Resolved

### Python 3.14 migration (2026-02-16)

**Problem:** Arch Linux upgraded system Python from 3.13 to 3.14. The hyprwhspr service crashed in a loop (98 restarts) with `ModuleNotFoundError: No module named 'sounddevice'`, then `No module named 'websocket'`.

**Root cause:** System packages `python-sounddevice` and `python-websocket-client` were built for Python 3.13 and installed to `/usr/lib/python3.13/site-packages/`. Python 3.14 doesn't look there.

**Fix:** Recreated the hyprwhspr venv with Python 3.14, installed `sounddevice` and `websocket-client` via pip in the venv, and added the venv's site-packages to the service's `PYTHONPATH` in the systemd override.

See `docs/python-upgrade-guide.md` for the full procedure.

---

## Mic OSD color based on transcription backend

Change the mic overlay color to indicate which backend is being used (Deepgram vs local-whisper vs offline/no connection). Gives immediate visual feedback on connectivity state while recording.

**Complexity notes:**
- Involves gtk4-layer-shell OSD (LD_PRELOAD quirks, see `local-bootstrapping/docs/mic-osd-lessons.md`)
- Must work identically on both desktop and laptop
- Notification/OSD changes historically don't work on first try — budget time for cross-device testing
