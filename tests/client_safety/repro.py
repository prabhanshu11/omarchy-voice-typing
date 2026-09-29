#!/usr/bin/python3
"""Reproduce the 2026-09-30 hyprwhspr failures against fake gateways.

Drives the REAL upstream hyprwhspr classes (/usr/lib/hyprwhspr/lib/src:
RealtimeClient, WhisperManager) and the real safety module
(local-bootstrapping/dotfiles/local-lib/hyprwhspr-patch/voice_safety.py).
The microphone is emulated with the exact locking of upstream
AudioCapture.audio_callback: the streaming callback is called while holding
the capture lock, and stop_recording() must take that same lock.

Modes (one per process, because the safety module monkey-patches classes):
  baseline-stall  upstream only; the gateway stops reading -> expect DEADLOCK
  stall           with voice_safety; gateway stops reading
  no-reply        with voice_safety; gateway reads everything, never answers commit
  ok              with voice_safety; gateway answers normally

Run: /usr/bin/python3 tests/client_safety/repro.py <mode>   (system python: it
needs the same numpy/scipy/websocket-client the hyprwhspr daemon uses).
Prints one JSON result line. No network, no microphone, no notifications
(notify-send and voice-retranscribe are replaced by fakes in a temp dir).
"""

import base64
import hashlib
import json
import os
import socket
import struct
import sys
import tempfile
import threading
import time
from pathlib import Path

MODE = sys.argv[1] if len(sys.argv) > 1 else 'stall'
TMP = Path(tempfile.mkdtemp(prefix='voice-safety-repro-'))
SPOOL = TMP / 'spool'
FAKE_BIN = TMP / 'bin'
FAKE_BIN.mkdir()
(FAKE_BIN / 'notify-send').write_text(f'#!/bin/sh\necho "$@" >> {TMP}/notifications.log\n')
(FAKE_BIN / 'notify-send').chmod(0o755)
fake_rt = TMP / 'fake-retranscribe'
fake_rt.write_text('import json,sys\nprint(json.dumps({"file": sys.argv[-1], "text": "rescued by batch"}))\n')
os.environ['PATH'] = f'{FAKE_BIN}:{os.environ["PATH"]}'
os.environ['VOICE_SPOOL_DIR'] = str(SPOOL)
os.environ['VOICE_RETRANSCRIBE'] = str(fake_rt)

sys.path.insert(0, '/usr/lib/hyprwhspr/lib/src')
sys.path.insert(0, str(Path.home() / 'Programs/local-bootstrapping/dotfiles/local-lib/hyprwhspr-patch'))
import numpy as np  # noqa: E402
from realtime_client import RealtimeClient  # noqa: E402
from whisper_manager import WhisperManager  # noqa: E402

if MODE != 'baseline-stall':
    import voice_safety  # noqa: E402

    class _App:  # install() only needs a class object for the app
        pass
    voice_safety.install(_App, WhisperManager)


# ───────────── fake gateway (raw WebSocket server) ─────────────

GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11'


def ws_frame(text):
    data = text.encode()
    n = len(data)
    if n < 126:
        hdr = struct.pack('!BB', 0x81, n)
    elif n < 65536:
        hdr = struct.pack('!BBH', 0x81, 126, n)
    else:
        hdr = struct.pack('!BBQ', 0x81, 127, n)
    return hdr + data


def read_exact(conn, n):
    buf = b''
    while len(buf) < n:
        chunk = conn.recv(n - len(buf))
        if not chunk:
            raise ConnectionError
        buf += chunk
    return buf


def read_frame(conn):
    b1, b2 = read_exact(conn, 2)
    n = b2 & 0x7F
    if n == 126:
        n = struct.unpack('!H', read_exact(conn, 2))[0]
    elif n == 127:
        n = struct.unpack('!Q', read_exact(conn, 8))[0]
    mask = read_exact(conn, 4) if b2 & 0x80 else b'\0\0\0\0'
    payload = bytearray(read_exact(conn, n))
    for i in range(n):
        payload[i] ^= mask[i % 4]
    return b1 & 0x0F, bytes(payload)


def serve(mode, ready):
    ls = socket.socket()
    ls.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    ls.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 4096)   # stall fills fast
    ls.bind(('127.0.0.1', 0))
    ls.listen(5)
    ready.append(ls.getsockname()[1])
    while True:
        conn, _ = ls.accept()
        threading.Thread(target=handle, args=(conn, mode), daemon=True).start()


def handle(conn, mode):
    req = b''
    while b'\r\n\r\n' not in req:
        req += conn.recv(4096)
    key = [l.split(b':', 1)[1].strip() for l in req.split(b'\r\n') if l.lower().startswith(b'sec-websocket-key')][0]
    accept = base64.b64encode(hashlib.sha1(key + GUID.encode()).digest()).decode()
    conn.sendall(('HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n'
                  f'Sec-WebSocket-Accept: {accept}\r\n\r\n').encode())
    conn.sendall(ws_frame(json.dumps({'type': 'session.created'})))
    SERVER['connections'] += 1
    try:
        while True:
            if mode == 'stall' and SERVER['stall'].is_set():
                time.sleep(3600)          # stop reading: like the gateway at 02:18:20
            op, payload = read_frame(conn)
            if op == 8:
                return
            ev = json.loads(payload)
            t = ev.get('type')
            SERVER['events'].append(t)
            if t == 'session.update':
                conn.sendall(ws_frame(json.dumps({'type': 'session.updated'})))
            elif t == 'input_audio_buffer.commit' and mode == 'ok':
                conn.sendall(ws_frame(json.dumps({
                    'type': 'conversation.item.input_audio_transcription.completed',
                    'transcript': 'hello from the fake gateway'})))
    except (ConnectionError, OSError):
        return


SERVER = {'events': [], 'connections': 0, 'stall': threading.Event()}


class FakeConfig:
    def __init__(self, d):
        self.d = d

    def get_setting(self, k, default=None):
        return self.d.get(k, default)


def main():
    ready = []
    gw_mode = {'baseline-stall': 'stall', 'stall': 'stall', 'no-reply': 'noreply', 'ok': 'ok'}[MODE]
    threading.Thread(target=serve, args=(gw_mode, ready), daemon=True).start()
    while not ready:
        time.sleep(0.01)

    rc = RealtimeClient()
    assert rc.connect(f'ws://127.0.0.1:{ready[0]}/v1/realtime', 'x', 'nova-2'), 'connect failed'

    wm = WhisperManager.__new__(WhisperManager)
    wm.config = FakeConfig({'transcription_backend': 'realtime-ws', 'realtime_timeout': 4})
    wm.ready = True
    wm._realtime_client = rc

    def _resample_and_send(audio_chunk):   # copied from upstream whisper_manager.initialize
        from scipy import signal
        resampled = signal.resample(audio_chunk, int(len(audio_chunk) * 1.5))
        rc.append_audio(resampled.astype(np.float32))
    wm._realtime_streaming_callback = _resample_and_send

    if MODE != 'baseline-stall':
        voice_safety.STATE.rc = rc
        voice_safety.STATE.begin(16000)
    callback = wm.get_realtime_streaming_callback()

    # ── emulated AudioCapture: upstream audio_callback locking ──
    lock = threading.Lock()
    chunks, stop = [], threading.Event()
    tone = (0.3 * np.sin(2 * np.pi * 440 * np.arange(1024) / 16000)).astype(np.float32)

    def capture():
        while not stop.is_set():
            with lock:                    # upstream holds AudioCapture.lock here ...
                chunks.append(tone.copy())
                callback(tone.copy())     # ... while calling the streaming callback
            time.sleep(0.002)             # 32x real time: fills socket buffers quickly
    t_cap = threading.Thread(target=capture, daemon=True)
    t_cap.start()
    time.sleep(0.5)
    if gw_mode == 'stall':
        SERVER['stall'].set()
    time.sleep(3.0)                       # keep "talking" while the gateway is stalled

    # ── stop press: upstream stop_recording() takes the same lock ──
    t0 = time.monotonic()
    stop.set()
    got = lock.acquire(timeout=10)
    stop_s = time.monotonic() - t0
    result = {'mode': MODE, 'stop_lock_acquired': got, 'stop_latency_s': round(stop_s, 3),
              'captured_s': round(len(chunks) * 1024 / 16000, 2)}
    if not got:
        result['verdict'] = 'DEADLOCK: stop key could not take the capture lock (the 02:24 incident)'
        print(json.dumps(result), flush=True)
        os._exit(0)
    lock.release()
    audio = np.concatenate(chunks)

    # ── transcription (patched in safety modes) ──
    t1 = time.monotonic()
    out = {}
    th = threading.Thread(target=lambda: out.update(text=wm.transcribe_audio(audio)), daemon=True)
    th.start()
    th.join(60)
    result['transcribe_s'] = round(time.monotonic() - t1, 2)
    result['text'] = out.get('text', '<still blocked after 60 s>')
    if MODE != 'baseline-stall':
        voice_safety.STATE.end('stopped')
        wavs = sorted(SPOOL.glob('*.wav'))
        if wavs:
            import wave
            with wave.open(str(wavs[0])) as w:
                result['spool_wav_s'] = round(w.getnframes() / w.getframerate(), 2)
            result['spool_status'] = json.loads(wavs[0].with_suffix('.json').read_text())['status']
        notes = TMP / 'notifications.log'
        result['notifications'] = notes.read_text().strip().splitlines() if notes.exists() else []
    result['gateway_events'] = {e: SERVER['events'].count(e) for e in set(SERVER['events'])}
    result['spool_dir'] = str(SPOOL)
    print(json.dumps(result), flush=True)
    os._exit(0)


if __name__ == '__main__':
    main()
