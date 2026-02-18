#!/usr/bin/env python3
"""Local Whisper HTTP transcription server for offline voice typing fallback.

Loads a faster-whisper model at startup and serves transcription requests.
Designed to run on NVIDIA MX330 (2GB VRAM) as a sidecar to the Go voice gateway.

Endpoints:
    POST /transcribe  - Accept WAV body (PCM16/24kHz), return {"text": "...", "duration": ..., "model": "..."}
    GET  /health      - Return {"status": "ready"|"loading", "model": "..."}
    GET  /switch?model=base - Hot-swap model at runtime

Env vars:
    WHISPER_MODEL   - Model name (default: distil-large-v3)
    WHISPER_COMPUTE - Compute type (default: int8_float32)
    WHISPER_PORT    - Listen port (default: 8766)
"""

import io
import json
import os
import sys
import threading
import time
import wave
from http.server import HTTPServer, BaseHTTPRequestHandler
from pathlib import Path
from urllib.parse import parse_qs, urlparse

# Auto-configure LD_LIBRARY_PATH for pip-installed NVIDIA libs
# Must happen before ctranslate2 is imported
_venv = Path(__file__).parent / ".venv/lib"
_nvidia_paths = []
for _pydir in _venv.glob("python*/site-packages/nvidia"):
    for _libdir in _pydir.glob("*/lib"):
        if _libdir.is_dir():
            _nvidia_paths.append(str(_libdir))
if _nvidia_paths:
    _new_path = ":".join(_nvidia_paths)
    _existing = os.environ.get("LD_LIBRARY_PATH", "")
    if _new_path not in _existing:
        os.environ["LD_LIBRARY_PATH"] = _new_path + (":" + _existing if _existing else "")
        os.execvp(sys.executable, [sys.executable] + sys.argv)


WHISPER_MODEL = os.environ.get("WHISPER_MODEL", "base")
WHISPER_COMPUTE = os.environ.get("WHISPER_COMPUTE", "int8_float32")
WHISPER_PORT = int(os.environ.get("WHISPER_PORT", "8767"))

ALLOWED_MODELS = {"distil-large-v3", "base"}

# Global model state
_model = None
_model_name = None
_model_lock = threading.Lock()
_status = "loading"


def load_model(model_name: str, compute_type: str) -> None:
    """Load a faster-whisper model into GPU memory."""
    global _model, _model_name, _status
    from faster_whisper import WhisperModel

    _status = "loading"
    print(f"[whisper] Loading model '{model_name}' (compute={compute_type})...")
    t0 = time.perf_counter()

    new_model = WhisperModel(model_name, device="cuda", compute_type=compute_type)

    elapsed = time.perf_counter() - t0
    print(f"[whisper] Model '{model_name}' loaded in {elapsed:.1f}s")

    with _model_lock:
        _model = new_model
        _model_name = model_name
        _status = "ready"


def transcribe_wav(wav_bytes: bytes) -> dict:
    """Transcribe WAV audio bytes using the loaded model."""
    with _model_lock:
        if _model is None:
            return {"error": "model not loaded"}
        model = _model
        model_name = _model_name

    # Parse WAV to get raw audio for faster-whisper
    # faster-whisper accepts file paths or numpy arrays
    import numpy as np

    buf = io.BytesIO(wav_bytes)
    try:
        with wave.open(buf, "rb") as wf:
            sample_rate = wf.getframerate()
            n_channels = wf.getnchannels()
            sample_width = wf.getsampwidth()
            frames = wf.readframes(wf.getnframes())
    except wave.Error:
        # If not valid WAV, try treating as raw PCM16 24kHz mono
        frames = wav_bytes
        sample_rate = 24000
        n_channels = 1
        sample_width = 2

    # Convert to float32 numpy array (what faster-whisper expects)
    audio = np.frombuffer(frames, dtype=np.int16).astype(np.float32) / 32768.0
    if n_channels > 1:
        audio = audio.reshape(-1, n_channels).mean(axis=1)

    audio_duration = len(audio) / sample_rate

    t0 = time.perf_counter()
    segments, info = model.transcribe(audio, beam_size=5)
    text_parts = [seg.text for seg in segments]
    elapsed = time.perf_counter() - t0

    full_text = " ".join(text_parts).strip()

    return {
        "text": full_text,
        "duration": round(audio_duration, 2),
        "transcribe_time": round(elapsed, 2),
        "model": model_name,
        "language": info.language,
    }


class WhisperHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        parsed = urlparse(self.path)

        if parsed.path == "/health":
            self._send_json({
                "status": _status,
                "model": _model_name or WHISPER_MODEL,
            })
            return

        if parsed.path == "/switch":
            params = parse_qs(parsed.query)
            new_model = params.get("model", [None])[0]
            if not new_model:
                self._send_json({"error": "missing ?model= parameter"}, 400)
                return
            if new_model not in ALLOWED_MODELS:
                self._send_json({
                    "error": f"unknown model: {new_model}",
                    "allowed": sorted(ALLOWED_MODELS),
                }, 400)
                return
            if new_model == _model_name:
                self._send_json({"status": "already_loaded", "model": new_model})
                return

            # Load in background to not block the HTTP response
            threading.Thread(
                target=load_model,
                args=(new_model, WHISPER_COMPUTE),
                daemon=True,
            ).start()
            self._send_json({"status": "switching", "model": new_model})
            return

        self._send_json({"error": "not found"}, 404)

    def do_POST(self):
        if self.path != "/transcribe":
            self._send_json({"error": "not found"}, 404)
            return

        if _status != "ready":
            self._send_json({"error": "model not ready", "status": _status}, 503)
            return

        content_length = int(self.headers.get("Content-Length", 0))
        if content_length == 0:
            self._send_json({"error": "empty body"}, 400)
            return

        wav_data = self.rfile.read(content_length)
        result = transcribe_wav(wav_data)

        if "error" in result:
            self._send_json(result, 500)
        else:
            self._send_json(result)

    def _send_json(self, data: dict, status: int = 200):
        body = json.dumps(data).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format, *args):
        print(f"[whisper-http] {args[0]}")


def warmup_model():
    """Warmup: transcribe 1s of silence to JIT-compile CUDA kernels."""
    import numpy as np
    global _status
    while _status != "ready":
        time.sleep(0.5)
    print("[whisper] Warming up model (JIT kernel compilation)...")
    t0 = time.perf_counter()
    silence = np.zeros(16000, dtype=np.float32)  # 1s at 16kHz
    with _model_lock:
        segments, _ = _model.transcribe(silence, beam_size=5)
        _ = list(segments)  # force evaluation
    print(f"[whisper] Warmup complete in {time.perf_counter() - t0:.1f}s")


def main():
    # Load model in background so server starts accepting connections immediately
    threading.Thread(target=load_model, args=(WHISPER_MODEL, WHISPER_COMPUTE), daemon=True).start()
    # Warmup after model loads to JIT-compile CUDA kernels
    threading.Thread(target=warmup_model, daemon=True).start()

    server = HTTPServer(("0.0.0.0", WHISPER_PORT), WhisperHandler)
    print(f"[whisper] Server listening on :{WHISPER_PORT}")
    print(f"[whisper] Model: {WHISPER_MODEL}, Compute: {WHISPER_COMPUTE}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n[whisper] Shutting down")
        server.shutdown()


if __name__ == "__main__":
    main()
