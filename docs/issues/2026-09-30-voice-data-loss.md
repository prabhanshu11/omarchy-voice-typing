# 2026-09-30: voice typing lost two messages. How it ran so long, and the fix

Lane: voice-typing data-loss investigation and fix (laptop, ThinkPad T14).
Written 2026-09-30 03:00–03:30 IST.

## Sources read

- `forensics/2026-09-30-hyprwhspr-deadlock/README.md`: the incident timeline and the core-dump recovery.
- This repo: `CLAUDE.md`, `ARCHITECTURE_CONSTRAINTS.md`, `progress.md`, `status.md`, `docs/backlog.md`,
  `docs/streaming-architecture.md`, and the full `git log`. Also `scripts/orphan-recovery.sh`,
  `logs/orphan-recovery.log` (692 MB), `logs/sessions/` (307 session logs) and `logs/latency/`.
- Gateway source: `gateway-rs/src/handlers/{realtime,realtime_session,transcribe}.rs`,
  `deepgram/{streaming,prerecorded}.rs`, `audio.rs`.
- Upstream hyprwhspr 1.12.0: `/usr/lib/hyprwhspr/lib/main.py`, and in `src/` the files
  `audio_capture.py`, `realtime_client.py` and `whisper_manager.py`.
- local-bootstrapping: `dotfiles/local-lib/hyprwhspr-patch/main.py` and its history (`git log -S`), the
  `hyprwhspr.service.d/override.conf`, `voice-orphan-recovery.{service,timer}`,
  `scripts/setup-voice-typing.sh`, and `configs/keystroke-log/README.md`.
- Journals on the laptop: `journalctl --user -u hyprwhspr` and `-u voice-gateway`. User journal history
  starts 2026-08-29 (3.5 GB retained). On the desktop, read-only: `logs/sessions/` (411 logs, 2026-02-17 → 2026-05-01).
- His past messages. A reader subagent searched the datalake (`find-conversations`, `get-messages-from-current`);
  the quotes it found are below.

## His words that bind this work

- 2026-09-30 ~02:24 IST, voice-typed. The audio was recovered from the core dump and then cut off by the deadlock:
  "This is really bad because I fixed any kind of such data loss issues a long time ago. It's policy across many apps,
  including the philosophy behind the data lake that no data should be deleted. And also in local bootstrapping, I'm
  trying to log everything. So investigation needs to be started as to how we were running on voice typing for so long
  even though that project is one of the most developed and changed aspect of…"
- 2026-09-30: he said **yes** to "make the gateway always save the audio, even when the text isn't delivered".
- 2026-02-18 (session 134444b2): "Right now, make sure we never loose the recording again… Implement detailed session
  based logs, we have to log everthing… In any case we shouldn't have lost the recording". Then: "I lost a recording again.."
- 2026-02-13 (session 11ed0c7f): "Can you tell me if all the recordings are getting saved even if the Python process
  is crashing?" The answer he got was "Recordings ARE being saved", but it covered only the overlay (MicOSD) crashing.
  hyprwhspr itself crashing was never checked.
- 2026-01-10 (session b36a41ec): "All the recordings should be saved onto a database… Or perhaps the Omarci voice typing
  project just hijacks hypervisper recording mechanism."
- 2026-01-25 (session 8994e7d4): his pasted plan named the gap itself: "hyprwhspr holds audio in RAM and only sends to
  gateway when recording completes normally… If process crashes or mic disconnects, buffered audio is lost."

Rule taken from these: **no audio may exist only in memory, and nothing gets deleted.** Old files are archived, never removed.

## What happened (IST)

| Time | Event |
|---|---|
| 02:17:42 | Recording rec-008 (32 s). The gateway streamed it to Deepgram, and the finals arrived up to 02:18:17. |
| 02:18:15 | Commit. `transcribe_deepgram()` sends Finalize, then `close_deepgram()` waits for the Deepgram read loop to end. **That wait had no time limit.** The Deepgram connection had gone dead without closing. |
| 02:18:15 → 02:35:46 | The gateway sits in that wait for **1050 s**, until the kernel's TCP retransmission timeout: `Deepgram read error … Connection timed out (os error 110)`. Its WebSocket loop handles one message at a time, so for 17.5 min it **read nothing from hyprwhspr**. |
| 02:18:45 | hyprwhspr's 30 s `realtime_timeout` expires: "No transcription generated". The text is lost from his view. Nothing was on disk at this point. |
| 02:24:28 | Next recording. Upstream `AudioCapture.audio_callback` holds `AudioCapture.lock` while the streaming callback runs `ws.send()`. The gateway was not reading, so the TCP send buffer filled (2.6 MB) and `ws.send()` blocked forever **inside the lock**. The stop key, the record thread and the main thread all waited on that lock: **deadlock**. The audio existed only in process memory. It was recovered from a core dump (`forensics/…`). |
| 02:35:46 | The gateway finally gets the TCP error. It then saves `recordings/20260929_204742_audio.wav` and `transcripts/20260929_204742_deepgram.txt` (338 chars), and tries to send the text to a socket that is already dead. **Correction to the forensics README:** the 02:17:42 WAV and text *were* saved in the end, 17.5 min late. Nothing was saved *at the time*. |
| 02:35:47 | The gateway processes the queued messages from the killed socket. rec-009 (the 02:24 recording) had 1.9 s buffered. `cleanup()` **dropped it**. Only the core dump kept it. |

## Why the earlier fix did not cover this

The "fix from a long time ago" was **orphan recovery** (2026-01-25, commits `19a9d20` here and `29b27a6` in local-bootstrapping).
Some February work was added on top of it.

1. **Orphan recovery covers only audio that is already on disk.** Its header says: "A recording is 'orphaned' if it
   reached the gateway but transcription failed." It re-submits `~/Programs/recordings/*.wav` files that have no
   transcript. The gateway wrote a WAV only **after** transcription finished, on commit, clear or silence. So a
   stalled transcription, a dropped socket, a crash or a deadlock left no file for it to find. The 2026-01-25 plan
   named this gap, but no client-side persistence was ever built.
2. **Orphan recovery never actually ran, from 2026-02-20 to 2026-09-04.** `logs/orphan-recovery.log` has
   5,322 "Starting orphan recovery scan" lines but only 329 "Scan complete" lines, and the first of those is on 2026-09-04.
   Under `set -e`, a false `[[ ]]` in `log_verbose` aborted every scan at its first skipped file (fixed in `43a13fc`, 2026-09-04).
   Nothing reported the failure, so it went unnoticed for 6.5 months.
3. **After 2026-09-04 it made copies instead of recovering.** Each re-submission was saved again under a longer name.
   That produced 27,131 copies in `~/Programs/recordings` (fixed in `f6aa387` on 2026-09-25; the copies were left in place),
   43,754 failed submissions (HTTP 500) and 2.4 M "ORPHAN" log lines. **The whole 692 MB log is a symptom of this.**
   In its entire history the log records **one** successful recovery: a 66-char transcript on 2026-09-25 03:33.
   It is healthy now: about 11 lines and about 3 min per 15-minute run. The remaining 500s are AssemblyAI answering
   "no spoken audio" for silent WAVs.
4. **The 2026-02-18 client guards were removed in April.** Fix 1 (skip `clear_audio_buffer`) and Fix 2 (a double-start
   guard) were deleted in local-bootstrapping `0fc00f6` (2026-04-26), because they referenced modules that hyprwhspr
   1.12 does not have. Fix 3 is on the gateway (save audio on clear) and it survived.
5. **Nobody ever looked at the lock.** The deadlock is in upstream hyprwhspr code: network I/O inside
   `AudioCapture.lock`. The patch never touched `audio_capture.py`. The lock only bites once the gateway stops reading,
   and 2026-09-30 is the first recorded case of that (path A below).
6. **The "log everything" work does not cover dictated text.** The keystroke log deliberately skips virtual injectors
   (ydotoold/hyprwhspr), so dictated text never reaches it either.

## How long each loss path existed

| Path | What was lost | Since | Until |
|---|---|---|---|
| A. Unbounded Deepgram close/finalize wait blocks the gateway's socket loop | text from the user's view (audio and text saved late) | 2026-02-18 (Rust gateway, `6484160`/`bd5e0fb`) | `dda6186` |
| B. hyprwhspr `ws.send()` inside `AudioCapture.lock` (upstream) → deadlock when the gateway stops reading | the whole recording, plus every recording until restart | since hyprwhspr's realtime-ws backend was adopted (2026-02-13). Only reachable through A or a stopped gateway | local-bootstrapping `a6eaec5` |
| C. Gateway `cleanup()` drops the buffer of an uncommitted recording when the socket closes | the whole recording | 2026-02-18 (Rust port) | `dda6186` |
| D. Client (hyprwhspr) holds audio only in RAM: crash, OOM kill, restart, `is_processing` skip, "Not connected" | the whole recording | forever (upstream design) | `a6eaec5` (client spool) |
| E. Gateway slower than the client's 30 s wait (for example local-whisper fallback at 43 s) | text from the user's view | 2026-02 (offline fallback) | rescue in `a6eaec5` (batch re-transcription after the timeout) |

## How many recordings were lost

Evidence is laptop session logs from 2026-02-18, desktop session logs from 2026-02-17 to 2026-05-01, laptop latency
JSONL, and the laptop hyprwhspr journal from 2026-08-29. Earlier journals are gone, so drops that happened only on the
hyprwhspr side before 2026-08-29 **cannot be counted**.

**Audio dropped by the gateway when the socket closed mid-recording (path C). This audio is gone:**

| Machine | < 2 s (accidental taps, double-start) | 2 s – 10 min (real dictation) | > 10 min (probably forgotten-on recordings) |
|---|---|---|---|
| Laptop | 5 (6.5 s total) | **1: 2026-07-26 08:37 UTC, 234 s** | 1: 2026-02-20, 11 h |
| Desktop | 9 (8.1 s total) | **11 (119 s total)**: 2026-02-19 (15.5, 41.8, 9.2, 9.1 s), 2026-03-04 (11.3, 2.6, 3.4, 10.2, 7.5, 3.8 s), 2026-03-19 (4.2 s) | 3: 2026-02-20 (26 min), 2026-04-04 (73 min), 2026-04-09 (39 min) |

**Text not delivered, audio kept (paths A and E):** laptop 2026-09-14 20:29 IST (3 chars, arrived 14 s after the
timeout) and 2026-09-30 02:18 (338 chars, now in `transcripts/20260929_204742_deepgram.txt`).

**Whole recording lost in the client (path B/D):** 2026-09-30 02:24 (42 s, recovered from the core dump). No other
case appears in the laptop journal since 2026-08-29. Of about 61 recordings in that window: 2 timeouts, 1 deadlock,
10 empty transcripts. The empty ones are Deepgram returning nothing for audio the gateway did save. Four of them were
the 2026-09-25 00:44 no-mic BT device, which was fixed in `77a41f9`.

**Not recoverable by any storage:** the 2026-02-20 BT SCO case ("lost 2 recordings", `docs/backlog.md`), where the mic
itself captured silence.

**Bottom line:** at least **12 real dictations lost outright** (1 laptop at 234 s, 11 desktop at 2–42 s), **2 texts lost
from his view**, and 1 recording that survived only through the core dump. There are also 14 sub-2-second fragments
and 4 long recordings that were probably left on by mistake but may have held speech.

## The fix: no recording is ever lost

### Gateway (`dda6186`, this repo)
- Every Deepgram write (audio, Finalize, CloseStream) has a 3 s limit. After CloseStream the read loop gets 3 s to exit,
  then it is aborted.
- If the stream stalls or fails mid-recording, it is **not reconnected** (reconnecting cleared the finals). The **full
  buffer** is re-transcribed in one Deepgram batch call, with a 10 s budget, as backend `deepgram-batch-rescue`.
  If that fails, the stream's finals are used.
- **The WAV is written before transcription starts.** A WAV without a transcript is exactly what orphan recovery retries.
- `cleanup()` saves the uncommitted audio when the socket closes.
- `save_audio` never overwrites a WAV from the same second.
- Writes to hyprwhspr have a 5 s limit.

### Client (local-bootstrapping `a6eaec5`, `2caac5a`: `dotfiles/local-lib/hyprwhspr-patch/voice_safety.py`)
- **Spool:** each chunk goes into a queue that never blocks. A writer thread appends it to
  `~/Programs/recordings/client/<utc>_client.wav` while recording, and the header is rewritten on every write, so the
  file is valid even after a kill. A `<stem>.json` sidecar records `source`, `pipeline`, `created`, `status`, `pid` and `rms`.
- Network sends run on their own thread, **never inside `AudioCapture.lock`**. A send watchdog shuts the socket down
  if a send is blocked for more than 5 s.
- The send queue is drained before commit. An interrupted stream never commits a partial buffer, and chunks from an
  earlier recording are never sent into a new one.
- If no text arrives and the spool holds speech, the spool WAV is re-transcribed with Deepgram batch. The text is typed
  through the normal path, into the window where the recording started, and a notification says so. If the rescue itself
  fails, a **critical notification names the file and gives the retry command**. If Deepgram hears no words, no alarm is raised.
- At startup, spool files left by a crash are recovered in the background, to the clipboard with a notification.
  The gateway socket is reconnected at recording start if it is down.

### Recovery tools (`35b5ce4`, this repo)
- `voice-retranscribe FILE.wav [--copy] [--notify]`, `--pending` and `--list`. It is linked into `~/.local/bin` by
  `setup-voice-typing.sh`. The key comes from `$DEEPGRAM_API_KEY`, then `.env`, then `pass show api/deepgram`.
  It never moves or deletes audio.
- `orphan-recovery.sh` runs `voice-retranscribe --pending --notify` on every 15-minute pass.

## Verification

- `gateway-rs/tests/stall_test.rs` uses a fake Deepgram server that sends one final and then never closes. The commit
  was answered in **4.5 s** with the batch transcript (before the fix: 1050 s). When the client socket was dropped
  mid-recording, the 1.5 s of buffered audio was **saved as a WAV**. The full suite passes, except the 4 known
  silence-gate e2e failures listed in the backlog.
- `tests/client_safety/repro.py` drives the real upstream hyprwhspr classes, using upstream's lock pattern, against fake gateways:

  | Mode | Stop latency | Result |
  |---|---|---|
  | `baseline-stall` (upstream only, gateway stops reading) | could not take the lock in 10 s | **DEADLOCK**: the 02:24 incident reproduced |
  | `stall` (patched) | 0.0 s | the spool held all 93 s; the text was rescued in 2.9 s; status `recovered` |
  | `no-reply` (patched, gateway never answers commit) | 0.0 s | the spool held all 93 s; the text was rescued after the 4 s timeout; status `recovered` |
  | `ok` (patched) | 0.0 s | the gateway text was typed; the spool held 96 s; status `transcribed`; no notifications |
- Live on the laptop at 03:13–03:14 after restarting both services: the tray went ready → recording → ready. The spool
  WAV and JSON were written. The gateway logged `Saved audio` **before** `Taking ONLINE transcription path`.
  Room noise produced 0 words, which was recorded as `silent` with no alarm. The first live run raised a false critical
  alarm for noise; that was fixed in `2caac5a`.
- `voice-retranscribe` on the audio recovered from the core dump gave 437 chars in 2.8 s.

## Remaining risks and follow-ups

- **Late transcript race (upstream):** if the gateway answers a timed-out commit *after* the next recording's commit
  clears `response_event`, the old text could be typed for the new recording. This was rarer before; the bounded gateway
  now keeps commits well under 30 s.
- The gateway still processes a commit inline, which can take up to about 20 s in the worst case. During that time
  hyprwhspr's appends wait in TCP buffers. That is harmless now (off-lock sender plus spool), but a fully asynchronous
  commit would be cleaner.
- **The desktop is not deployed.** Both repos are only committed locally. Pushing local-bootstrapping triggers
  `sync-from-master` on every machine, which re-runs `setup-voice-typing.sh` (pull, build, restart the voice services).
- `logs/orphan-recovery.log` (692 MB, 99.9 % cascade noise) and the 27,131 copies are for him to decide on. Archive
  (compress or move to the Elements drive), do not delete.
