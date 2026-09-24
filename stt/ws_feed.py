# /// script
# dependencies = ["websockets>=12"]
# requires-python = ">=3.11"
# ///
"""Feed recorded WAVs to the gateway's realtime WebSocket, like hyprwhspr does,
without a microphone and without typing anywhere.

  uv run stt/ws_feed.py ws://127.0.0.1:18765/v1/realtime clip1.wav [clip2.wav ...]
WAVs must be 24 kHz mono PCM16 (what ~/Programs/recordings holds since Feb 2026).
Prints per clip: transcript and the commit -> transcript latency.
"""
import asyncio
import base64
import json
import sys
import time
import wave

import websockets


async def one(ws, path):
    with wave.open(path, "rb") as w:
        assert w.getframerate() == 24000 and w.getnchannels() == 1 and w.getsampwidth() == 2, path
        pcm = w.readframes(w.getnframes())
    chunk = 4800  # 100 ms
    for i in range(0, len(pcm), chunk):
        await ws.send(json.dumps({"type": "input_audio_buffer.append",
                                  "audio": base64.b64encode(pcm[i:i + chunk]).decode()}))
    t0 = time.perf_counter()
    await ws.send(json.dumps({"type": "input_audio_buffer.commit"}))
    while True:
        msg = json.loads(await asyncio.wait_for(ws.recv(), 60))
        if msg.get("type") == "conversation.item.input_audio_transcription.completed":
            return msg.get("transcript", ""), time.perf_counter() - t0, len(pcm) / 48000


async def main(url, paths):
    async with websockets.connect(url, max_size=None) as ws:
        print("<-", json.loads(await ws.recv()).get("type"))
        await ws.send(json.dumps({"type": "session.update", "session": {"source": "test-feed"}}))
        print("<-", json.dumps(json.loads(await ws.recv()).get("session")))
        for p in paths:
            text, dt, dur = await one(ws, p)
            print(json.dumps({"clip": p.rsplit("/", 1)[-1], "audio_s": round(dur, 1),
                              "commit_to_text_s": round(dt, 2), "text": text}))


asyncio.run(main(sys.argv[1], sys.argv[2:]))
