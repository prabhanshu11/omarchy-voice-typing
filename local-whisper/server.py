#!/usr/bin/env python3
"""Local Whisper HTTP transcription server for offline voice typing fallback.

Loads a faster-whisper model at startup and serves transcription requests.
Designed to run on NVIDIA MX330 (2GB VRAM) as a sidecar to the Go voice gateway.

Endpoints:
    POST /transcribe  - Accept WAV body (PCM16/24kHz), return {"text": "...", "duration": ..., "model": "..."}
                        Optional query: ?hotwords=word1, word2  (faster-whisper hotwords = prompt
                        biasing towards the user's words-to-concentrate-on list)
    GET  /health      - Return {"status": "ready"|"loading", "model": "...", "device": "...",
                        "warm": bool, "long": {...} when the hybrid long model is configured}
    GET  /switch?model=base - Hot-swap model at runtime

Hybrid (2026-11-03, the laptop): base serves short recordings (~3 s from Stop to text);
recordings of WHISPER_LONG_AFTER_S seconds and longer go to the bigger WHISPER_LONG_MODEL
(distil-large-v3, ~21 s per 30 s window on the MX330, more accurate), whole, after Stop. The
long model runs in a child process of this same script on 127.0.0.1:WHISPER_LONG_PORT, loaded
only while ~/.config/voice-typing/stt-provider says one of WHISPER_LONG_WHEN (default
"offline", the popup's "Source: Local"), so while the source is online the GPU holds nothing
and can sleep. A child process, not a second model in this one: freeing a CUDA model leaves
the CUDA context (~300 MB, GPU awake) until the process exits. If the long model is not
ready, or fails, long recordings fall back to base.

Env vars:
    WHISPER_MODEL   - Model name or a CTranslate2 model directory, e.g. the fine-tuned
                      ~/Programs/voice-stt/models/current (see docs/local-stt.md)
    WHISPER_LANGUAGE - Force a language (default: en; empty = auto-detect)
    WHISPER_DEVICE  - cuda (default) | cpu
    WHISPER_COMPUTE - Compute type (default: int8_float32)
    WHISPER_PORT    - Listen port (default: 8766)
    WHISPER_CPU_THREADS - CPU threads when WHISPER_DEVICE=cpu (default 0 = CTranslate2 default)
    WHISPER_BIND    - Listen address (default 0.0.0.0; the long-model child uses 127.0.0.1)
    WHISPER_WARMUP  - 1 (default): one silent pass after loading; 0: none (the long model)
    WHISPER_LONG_MODEL   - The hybrid's bigger model (default empty = no hybrid, e.g. the desktop)
    WHISPER_LONG_DEVICE  - Its device (default cuda)
    WHISPER_LONG_COMPUTE - Its compute type (default int8_float32)
    WHISPER_LONG_AFTER_S - Recordings at least this long go to it (default 25)
    WHISPER_LONG_PORT    - The child's port on 127.0.0.1 (default 8777)
    WHISPER_LONG_WHEN    - Comma-separated stt-provider values that keep it loaded
                           (default "offline"), or "always"
    WHISPER_LONG_WARMUP  - 0 (default): no warm-up pass. distil on the MX330 costs a full
                           30 s window (~20.5 s) per pass, and its first real pass is no slower
                           than later ones (measured 2026-11-03: 20.7 / 20.5 / 20.5 s), so a
                           warm-up only delayed "ready" from ~6 s to ~26 s.
"""

import io
import json
import os
import sys
import threading
import time
import wave
import subprocess
import urllib.request
from http.server import ThreadingHTTPServer, BaseHTTPRequestHandler
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
WHISPER_LANGUAGE = os.environ.get("WHISPER_LANGUAGE", "en") or None
WHISPER_DEVICE = os.environ.get("WHISPER_DEVICE", "cuda")
WHISPER_CPU_THREADS = int(os.environ.get("WHISPER_CPU_THREADS", "0") or 0)
WHISPER_BIND = os.environ.get("WHISPER_BIND", "0.0.0.0")
WHISPER_WARMUP = os.environ.get("WHISPER_WARMUP", "1") != "0"

LONG_MODEL = os.environ.get("WHISPER_LONG_MODEL", "").strip()
LONG_DEVICE = os.environ.get("WHISPER_LONG_DEVICE", "cuda")
LONG_COMPUTE = os.environ.get("WHISPER_LONG_COMPUTE", "int8_float32")
LONG_AFTER_S = float(os.environ.get("WHISPER_LONG_AFTER_S", "25") or 25)
LONG_PORT = int(os.environ.get("WHISPER_LONG_PORT", "8777") or 8777)
LONG_WHEN = os.environ.get("WHISPER_LONG_WHEN", "offline")
PROVIDER_FILE = Path(os.environ.get("VOICE_STT_PROVIDER_FILE",
                                    "~/.config/voice-typing/stt-provider")).expanduser()
LONG_LOAD_DEADLINE_S = 180      # loading + warm-up past this = failed: kill, back off, retry

ALLOWED_MODELS = {"distil-large-v3", "base", "large-v3-turbo"}
MODELS_DIR = Path.home() / "Programs/voice-stt/models"


def model_allowed(name: str) -> bool:
    """Named models, or a directory under ~/Programs/voice-stt/models."""
    if name in ALLOWED_MODELS:
        return True
    p = Path(name).expanduser().resolve()
    return p.is_dir() and MODELS_DIR.resolve() in p.parents

# Global model state
_model = None
_model_name = None
_model_lock = threading.Lock()
_status = "loading"
_warm = False


def load_model(model_name: str, compute_type: str) -> None:
    """Load a faster-whisper model into GPU memory."""
    global _model, _model_name, _status, _warm
    from faster_whisper import WhisperModel

    _status = "loading"
    _warm = False
    print(f"[whisper] Loading model '{model_name}' (compute={compute_type})...")
    t0 = time.perf_counter()

    path = Path(model_name).expanduser()
    new_model = WhisperModel(str(path) if path.is_dir() else model_name,
                             device=WHISPER_DEVICE, compute_type=compute_type,
                             cpu_threads=WHISPER_CPU_THREADS)

    elapsed = time.perf_counter() - t0
    print(f"[whisper] Model '{model_name}' loaded in {elapsed:.1f}s")

    with _model_lock:
        _model = new_model
        _model_name = model_name
        _status = "ready"
    warmup_model()


def wav_duration(wav_bytes: bytes) -> float:
    """Seconds of audio in a WAV body (raw PCM16 24 kHz mono if it has no header)."""
    try:
        with wave.open(io.BytesIO(wav_bytes), "rb") as wf:
            return wf.getnframes() / float(wf.getframerate() or 1)
    except (wave.Error, EOFError):
        return len(wav_bytes) / 48000.0


class LongModel:
    """The hybrid's bigger model, in a child process of this script (see the module doc)."""

    def __init__(self):
        self.proc = None
        self.state = "unloaded"     # unloaded | loading | ready | error
        self.error = ""
        self.started = 0.0
        self.fails = 0
        self.retry_at = 0.0
        self.busy = 0
        self.lock = threading.Lock()

    def wanted(self) -> bool:
        if LONG_WHEN.strip() == "always":
            return True
        try:
            cur = (PROVIDER_FILE.read_text().split() or ["deepgram"])[0]
        except OSError:
            cur = "deepgram"
        return cur in {w.strip() for w in LONG_WHEN.split(",")}

    def info(self) -> dict:
        return {"model": LONG_MODEL, "device": LONG_DEVICE, "after_s": LONG_AFTER_S,
                "state": self.state, "wanted": self.wanted(), "error": self.error}

    def _spawn(self):
        env = dict(os.environ, WHISPER_MODEL=LONG_MODEL, WHISPER_DEVICE=LONG_DEVICE,
                   WHISPER_COMPUTE=LONG_COMPUTE, WHISPER_PORT=str(LONG_PORT),
                   WHISPER_BIND="127.0.0.1", WHISPER_LONG_MODEL="",
                   WHISPER_WARMUP=os.environ.get("WHISPER_LONG_WARMUP", "0"))
        print(f"[whisper] Long model: starting '{LONG_MODEL}' on {LONG_DEVICE} (:{LONG_PORT})")
        self.proc = subprocess.Popen([sys.executable, str(Path(__file__).resolve())], env=env,
                                     stdin=subprocess.DEVNULL)
        self.state, self.error, self.started = "loading", "", time.monotonic()

    def _stop(self, why: str):
        print(f"[whisper] Long model: stopping ({why})")
        self.proc.terminate()
        try:
            self.proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait(timeout=5)
        self.proc = None

    def _child_warm(self) -> bool:
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{LONG_PORT}/health", timeout=1) as r:
                h = json.loads(r.read())
            return h.get("status") == "ready" and bool(h.get("warm"))
        except (OSError, ValueError):
            return False

    def _failed(self, msg: str):
        self.fails += 1
        delay = min(300, 30 * 2 ** (self.fails - 1))
        self.state, self.error = "error", f"{msg}; retrying in {delay} s"
        self.retry_at = time.monotonic() + delay
        print(f"[whisper] Long model: {self.error}")

    def tick(self):
        with self.lock:
            now = time.monotonic()
            want = self.wanted()
            alive = self.proc is not None and self.proc.poll() is None
            if self.proc is not None and not alive:
                code = self.proc.returncode
                self.proc = None
                if want:
                    self._failed(f"exited with code {code}")
                    return
            if want:
                if not alive:
                    if now >= self.retry_at:
                        self._spawn()
                elif self.state == "loading":
                    if self._child_warm():
                        self.state, self.fails = "ready", 0
                        print(f"[whisper] Long model: ready after {now - self.started:.1f}s")
                    elif now - self.started > LONG_LOAD_DEADLINE_S:
                        self._stop("not ready in time")
                        self._failed(f"not ready after {LONG_LOAD_DEADLINE_S} s")
            else:
                if alive and self.busy == 0:
                    self._stop("the source is no longer local")
                if self.proc is None:
                    self.state, self.error, self.fails, self.retry_at = "unloaded", "", 0, 0.0

    def supervise(self):
        while True:
            try:
                self.tick()
            except Exception as e:      # never let the supervisor die
                print(f"[whisper] Long model supervisor: {e!r}")
            time.sleep(1)

    def transcribe(self, wav_bytes: bytes, query: str, duration: float):
        """Forward to the child; None (-> base) if it is not ready or fails."""
        with self.lock:
            if self.state != "ready":
                return None
            self.busy += 1
        try:
            url = f"http://127.0.0.1:{LONG_PORT}/transcribe" + (f"?{query}" if query else "")
            req = urllib.request.Request(url, data=wav_bytes, method="POST",
                                         headers={"Content-Type": "audio/wav"})
            with urllib.request.urlopen(req, timeout=1.2 * duration + 60) as r:
                return json.loads(r.read())
        except (OSError, ValueError) as e:
            print(f"[whisper] Long model failed ({e!r}); using {_model_name}")
            return None
        finally:
            with self.lock:
                self.busy -= 1


_long = LongModel() if LONG_MODEL else None


def transcribe_wav(wav_bytes: bytes, hotwords: str | None = None) -> dict:
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
    # faster-whisper assumes 16 kHz for numpy input; the gateway sends 24 kHz.
    # (Before 2026-09-25 the 24 kHz array went in as-is: speech ran 1.5x slow.)
    if sample_rate != 16000:
        from faster_whisper.audio import decode_audio
        try:
            audio = decode_audio(io.BytesIO(wav_bytes), sampling_rate=16000)
        except Exception:
            n_out = int(len(audio) * 16000 / sample_rate)
            audio = np.interp(np.linspace(0, len(audio), n_out, endpoint=False),
                              np.arange(len(audio)), audio).astype(np.float32)
    segments, info = model.transcribe(audio, beam_size=5, language=WHISPER_LANGUAGE,
                                      hotwords=hotwords or None,
                                      condition_on_previous_text=False)
    text_parts = [seg.text for seg in segments]
    elapsed = time.perf_counter() - t0

    full_text = " ".join(text_parts).strip()

    return {
        "text": full_text,
        "duration": round(audio_duration, 2),
        "transcribe_time": round(elapsed, 2),
        "model": model_name,
        "language": info.language,
        "hotwords": bool(hotwords),
    }


class WhisperHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        parsed = urlparse(self.path)

        if parsed.path == "/health":
            health = {
                "status": _status,
                "model": _model_name or WHISPER_MODEL,
                "device": WHISPER_DEVICE,
                "warm": _warm,
            }
            if _long is not None:
                health["long"] = _long.info()
            self._send_json(health)
            return

        if parsed.path == "/switch":
            params = parse_qs(parsed.query)
            new_model = params.get("model", [None])[0]
            if not new_model:
                self._send_json({"error": "missing ?model= parameter"}, 400)
                return
            if not model_allowed(new_model):
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
        parsed = urlparse(self.path)
        if parsed.path != "/transcribe":
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
        hotwords = parse_qs(parsed.query).get("hotwords", [None])[0]
        result, route = None, ""
        if _long is not None:
            dur = wav_duration(wav_data)
            route = "short"
            if dur >= LONG_AFTER_S:
                result = _long.transcribe(wav_data, parsed.query, dur)
                route = "long" if result is not None else "short (long model not ready)"
        if result is None:
            result = transcribe_wav(wav_data, hotwords)
        if route:
            result["hybrid"] = route

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
    """Warmup: transcribe 1s of silence to JIT-compile CUDA kernels (after every load)."""
    import numpy as np
    global _warm
    if not WHISPER_WARMUP:
        _warm = True
        return
    print("[whisper] Warming up model (JIT kernel compilation)...")
    t0 = time.perf_counter()
    silence = np.zeros(16000, dtype=np.float32)  # 1s at 16kHz
    with _model_lock:
        model = _model
    # max_new_tokens: Whisper "hears" words in silence and decodes to its 448-token limit;
    # uncapped, this warm-up took 41 s for distil on the MX330 (2026-11-03), now a few seconds.
    segments, _ = model.transcribe(silence, beam_size=5, language=WHISPER_LANGUAGE,
                                   max_new_tokens=4, condition_on_previous_text=False)
    _ = list(segments)  # force evaluation
    _warm = True
    print(f"[whisper] Warmup complete in {time.perf_counter() - t0:.1f}s")


def main():
    # Load (then warm up) in background so the server accepts connections immediately.
    threading.Thread(target=load_model, args=(WHISPER_MODEL, WHISPER_COMPUTE), daemon=True).start()
    if _long is not None:
        threading.Thread(target=_long.supervise, daemon=True).start()

    # Threaded: /health answers while a 20 s distil transcription runs (the popup's Source
    # button polls it while the long model loads).
    server = ThreadingHTTPServer((WHISPER_BIND, WHISPER_PORT), WhisperHandler)
    server.daemon_threads = True
    print(f"[whisper] Server listening on {WHISPER_BIND}:{WHISPER_PORT}")
    print(f"[whisper] Model: {WHISPER_MODEL}, Device: {WHISPER_DEVICE}, Compute: {WHISPER_COMPUTE}"
          + (f"; long: {LONG_MODEL} on {LONG_DEVICE} from {LONG_AFTER_S:g} s (when {LONG_WHEN})"
             if LONG_MODEL else ""))
    import signal
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt))
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n[whisper] Shutting down")
    finally:
        if _long is not None and _long.proc is not None:
            _long.proc.terminate()


if __name__ == "__main__":
    main()
