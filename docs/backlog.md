# Backlog

## Mic OSD color based on transcription backend

Change the mic overlay color to indicate which backend is being used (Deepgram vs local-whisper vs offline/no connection). Gives immediate visual feedback on connectivity state while recording.

**Complexity notes:**
- Involves gtk4-layer-shell OSD (LD_PRELOAD quirks, see `local-bootstrapping/docs/mic-osd-lessons.md`)
- Must work identically on both desktop and laptop
- Notification/OSD changes historically don't work on first try — budget time for cross-device testing
