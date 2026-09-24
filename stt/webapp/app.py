#!/usr/bin/env python3
"""Voice labels: fix wrongly transcribed words, keep a words-to-concentrate-on list.

Stdlib only (runs with /usr/bin/python3). Bound to 127.0.0.1 by default.

  GET  /                       the page (static/index.html)
  GET  /api/recordings?before=TS&limit=N   recent recordings + transcript + any correction
  GET  /audio/<TS>             the WAV (Range supported, so seeking works)
  POST /api/correction         {"ts","text","edits":[...],"confirmed":bool}
  GET  /api/vocab              {"words":[...]}
  POST /api/vocab              {"add":"word"} | {"remove":"word"}
  GET  /api/status             provider in use, counts

Labels live in $STT_HOME/labels (default ~/Programs/voice-stt/labels):
  corrections.jsonl  append-only; the last row per clip wins
  vocab.txt          one word/phrase per line
After every save the two files are copied to the desktop (best effort), where
training runs; the gateway reads vocab.txt live and sends it as Whisper hotwords.
"""
from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import threading
import time
from datetime import datetime
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

HOME = Path.home()
STT_HOME = Path(os.environ.get("STT_HOME", HOME / "Programs/voice-stt"))
LABELS = STT_HOME / "labels"
REC_DIR = Path(os.environ.get("RECORDINGS_DIR", HOME / "Programs/recordings"))
TXT_DIR = Path(os.environ.get("TRANSCRIPTS_DIR", HOME / "Programs/transcripts"))
PROVIDER_FILE = HOME / ".config/voice-typing/stt-provider"
SYNC_HOST = os.environ.get("VOICE_LABELS_SYNC_HOST", "desktop")
STATIC = Path(__file__).parent / "static"
HOST = os.environ.get("VOICE_LABELS_HOST", "127.0.0.1")
PORT = int(os.environ.get("VOICE_LABELS_PORT", "8771"))

RE_AUDIO = re.compile(r"^(\d{8}_\d{6})_audio\.wav$")
RE_TEXT = re.compile(r"^(\d{8}_\d{6})_(.+)\.txt$")
_lock = threading.Lock()
_sha_cache: dict[str, tuple[float, str]] = {}


def clip_id(p: Path) -> str:
    """Same id the training manifest uses: first 16 hex of the WAV's sha1."""
    st = p.stat().st_mtime
    hit = _sha_cache.get(str(p))
    if hit and hit[0] == st:
        return hit[1]
    h = hashlib.sha1(p.read_bytes()).hexdigest()[:16]
    _sha_cache[str(p)] = (st, h)
    return h


def transcripts_by_ts() -> dict[str, tuple[str, Path]]:
    out: dict[str, tuple[str, Path]] = {}
    rank = {"deepgram": 1, "lan-whisper": 2, "local-whisper": 2, "recovered": 0}
    if TXT_DIR.is_dir():
        for p in TXT_DIR.iterdir():
            m = RE_TEXT.match(p.name)
            if not m:
                continue
            ts, backend = m.groups()
            if ts not in out or rank.get(backend, 0) >= rank.get(out[ts][0], 0):
                out[ts] = (backend, p)
    return out


def load_corrections() -> dict[str, dict]:
    out: dict[str, dict] = {}
    f = LABELS / "corrections.jsonl"
    if f.exists():
        for line in f.read_text().splitlines():
            if line.strip():
                r = json.loads(line)
                out[r["clip_id"]] = r
    return out


def read_vocab() -> list[str]:
    f = LABELS / "vocab.txt"
    if not f.exists():
        return []
    return [w.strip() for w in f.read_text().splitlines() if w.strip() and not w.startswith("#")]


def write_vocab(words: list[str]) -> None:
    LABELS.mkdir(parents=True, exist_ok=True)
    body = ("# Words the voice model should concentrate on (one per line).\n"
            "# Used live as Whisper hotwords and in the next training round.\n"
            "# Edited from the voice-labels web app.\n")
    tmp = LABELS / "vocab.txt.tmp"
    tmp.write_text(body + "".join(w + "\n" for w in words))
    tmp.replace(LABELS / "vocab.txt")


def push_to_desktop() -> None:
    """Best-effort copy of the labels to the training machine."""
    def run():
        files = [str(LABELS / n) for n in ("corrections.jsonl", "vocab.txt") if (LABELS / n).exists()]
        if not files or not SYNC_HOST:
            return
        subprocess.run(["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", SYNC_HOST,
                        "mkdir -p ~/Programs/voice-stt/labels"], capture_output=True, timeout=20)
        subprocess.run(["scp", "-q", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", *files,
                        f"{SYNC_HOST}:Programs/voice-stt/labels/"], capture_output=True, timeout=60)
    threading.Thread(target=run, daemon=True).start()


def list_recordings(before: str | None, limit: int) -> list[dict]:
    names = sorted((p.name for p in REC_DIR.iterdir() if RE_AUDIO.match(p.name)), reverse=True) \
        if REC_DIR.is_dir() else []
    txt = transcripts_by_ts()
    corr = load_corrections()
    out = []
    for name in names:
        ts = RE_AUDIO.match(name).group(1)
        if before and ts >= before:
            continue
        if ts not in txt:
            continue  # nothing to correct (silence, failed)
        p = REC_DIR / name
        backend, tp = txt[ts]
        text = tp.read_text(errors="replace").strip()
        if not text:
            continue
        cid = clip_id(p)
        c = corr.get(cid)
        size = p.stat().st_size
        out.append({
            "ts": ts, "when": datetime.strptime(ts, "%Y%m%d_%H%M%S").strftime("%a %d %b %H:%M"),
            "secs": round(max(size - 44, 0) / 48000, 1), "backend": backend, "clip_id": cid,
            "transcript": text, "corrected": c["text"] if c else None,
            "confirmed": bool(c and c.get("confirmed")), "edits": c.get("edits", []) if c else [],
        })
        if len(out) >= limit:
            break
    return out


class H(BaseHTTPRequestHandler):
    server_version = "voice-labels/1"

    def log_message(self, fmt, *args):
        print(f"[voice-labels] {self.address_string()} {fmt % args}", flush=True)

    def _json(self, obj, status=200):
        b = json.dumps(obj).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(b)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(b)

    def _body(self) -> dict:
        n = int(self.headers.get("Content-Length") or 0)
        return json.loads(self.rfile.read(n) or b"{}")

    def do_GET(self):
        u = urlparse(self.path)
        q = parse_qs(u.query)
        if u.path in ("/", "/index.html"):
            b = (STATIC / "index.html").read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(b)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(b)
        elif u.path == "/api/recordings":
            limit = min(int(q.get("limit", ["25"])[0]), 100)
            self._json({"recordings": list_recordings(q.get("before", [None])[0], limit)})
        elif u.path == "/api/vocab":
            self._json({"words": read_vocab()})
        elif u.path == "/api/status":
            prov = PROVIDER_FILE.read_text().split()[0] if PROVIDER_FILE.exists() else \
                os.environ.get("STT_PROVIDER", "deepgram")
            self._json({"provider": prov, "corrections": len(load_corrections()),
                        "vocab": len(read_vocab()), "labels_dir": str(LABELS)})
        elif u.path.startswith("/audio/"):
            ts = u.path.rsplit("/", 1)[-1]
            p = REC_DIR / f"{ts}_audio.wav"
            if not re.fullmatch(r"\d{8}_\d{6}", ts) or not p.exists():
                return self._json({"error": "not found"}, 404)
            self._send_file(p)
        else:
            self._json({"error": "not found"}, 404)

    def _send_file(self, p: Path):
        size = p.stat().st_size
        rng = self.headers.get("Range")
        start, end = 0, size - 1
        status = HTTPStatus.OK
        if rng and rng.startswith("bytes="):
            a, _, b = rng[6:].partition("-")
            start = int(a) if a else max(size - int(b), 0)
            end = int(b) if (a and b) else size - 1
            status = HTTPStatus.PARTIAL_CONTENT
        with open(p, "rb") as f:
            f.seek(start)
            data = f.read(end - start + 1)
        self.send_response(status)
        self.send_header("Content-Type", "audio/wav")
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Length", str(len(data)))
        if status == HTTPStatus.PARTIAL_CONTENT:
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        u = urlparse(self.path)
        try:
            body = self._body()
        except Exception:
            return self._json({"error": "bad json"}, 400)
        if u.path == "/api/correction":
            ts = str(body.get("ts", ""))
            p = REC_DIR / f"{ts}_audio.wav"
            if not re.fullmatch(r"\d{8}_\d{6}", ts) or not p.exists():
                return self._json({"error": "unknown recording"}, 404)
            txt = transcripts_by_ts().get(ts)
            row = {"clip_id": clip_id(p), "ts": ts, "audio": str(p),
                   "backend": txt[0] if txt else None,
                   "original": txt[1].read_text(errors="replace").strip() if txt else "",
                   "text": str(body.get("text", "")).strip(),
                   "edits": body.get("edits", []), "confirmed": bool(body.get("confirmed")),
                   "when": datetime.now().isoformat(timespec="seconds"), "source": "voice-labels"}
            if not row["text"]:
                return self._json({"error": "empty text"}, 400)
            with _lock:
                LABELS.mkdir(parents=True, exist_ok=True)
                with open(LABELS / "corrections.jsonl", "a") as f:
                    f.write(json.dumps(row, ensure_ascii=False) + "\n")
            push_to_desktop()
            return self._json({"ok": True, "clip_id": row["clip_id"]})
        if u.path == "/api/vocab":
            with _lock:
                words = read_vocab()
                if body.get("add"):
                    w = " ".join(str(body["add"]).split())[:60]
                    if w and w.lower() not in (x.lower() for x in words):
                        words.append(w)
                if body.get("remove"):
                    words = [x for x in words if x != body["remove"]]
                write_vocab(words)
            push_to_desktop()
            return self._json({"words": words})
        self._json({"error": "not found"}, 404)


def main():
    LABELS.mkdir(parents=True, exist_ok=True)
    srv = ThreadingHTTPServer((HOST, PORT), H)
    print(f"[voice-labels] http://{HOST}:{PORT}/  labels={LABELS}", flush=True)
    srv.serve_forever()


if __name__ == "__main__":
    main()
