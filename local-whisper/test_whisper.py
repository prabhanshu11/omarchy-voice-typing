#!/usr/bin/env python3
"""Test local Whisper transcription on NVIDIA MX330 (2GB VRAM).

Usage:
    uv run python test_whisper.py [audio_file] [--model tiny|base|small] [--compare]

Examples:
    uv run python test_whisper.py ../recordings/20260101_185000_Harvard_speech100.wav
    uv run python test_whisper.py ../recordings/20260101_185000_Harvard_speech100.wav --model base
    uv run python test_whisper.py ../recordings/20260101_185000_Harvard_speech100.wav --compare
"""

import argparse
import os
import sys
import time
from pathlib import Path

# Auto-configure LD_LIBRARY_PATH for pip-installed NVIDIA libs (must be set before ctranslate2 loads)
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
        # Re-exec to pick up LD_LIBRARY_PATH before native libs load
        os.execvp(sys.executable, [sys.executable] + sys.argv)


def get_gpu_info():
    """Get GPU memory usage via nvidia-smi."""
    import subprocess
    try:
        result = subprocess.run(
            ["nvidia-smi", "--query-gpu=memory.used,memory.total,name",
             "--format=csv,noheader,nounits"],
            capture_output=True, text=True, timeout=5
        )
        if result.returncode == 0:
            parts = result.stdout.strip().split(", ")
            return {
                "used_mb": int(parts[0]),
                "total_mb": int(parts[1]),
                "name": parts[2],
            }
    except (subprocess.TimeoutExpired, FileNotFoundError, IndexError, ValueError):
        pass
    return None


def transcribe(audio_path: str, model_size: str = "small",
               force_device: str = None, force_compute: str = None):
    """Run transcription and return results with timing."""
    from faster_whisper import WhisperModel

    gpu_before = get_gpu_info()

    print(f"\n{'='*60}")
    print(f"Model: {model_size}")
    print(f"Audio: {audio_path}")
    if force_device:
        print(f"Forced: {force_device}/{force_compute}")
    print(f"{'='*60}")

    # Load model
    t0 = time.perf_counter()
    device_used = "unknown"

    if force_device and force_compute:
        # Use exactly what was requested
        try:
            model = WhisperModel(model_size, device=force_device, compute_type=force_compute)
            device_used = f"{force_device}/{force_compute}"
        except Exception as e:
            print(f"  {force_device}/{force_compute} failed: {e}")
            return {"model": model_size, "error": str(e)}
    else:
        # Auto-detect: try CUDA with different compute types, fall back to CPU
        for device, compute in [("cuda", "float16"), ("cuda", "float32"), ("cuda", "int8"), ("cpu", "int8")]:
            try:
                model = WhisperModel(model_size, device=device, compute_type=compute)
                device_used = f"{device}/{compute}"
                break
            except Exception as e:
                print(f"  {device}/{compute} failed: {e}")

    print(f"Using: {device_used}")
    load_time = time.perf_counter() - t0
    print(f"Model loaded in {load_time:.2f}s")

    gpu_after_load = get_gpu_info()

    # Transcribe
    print("Transcribing...")
    t1 = time.perf_counter()
    segments, info = model.transcribe(audio_path, beam_size=5)
    text_parts = []
    for segment in segments:
        text_parts.append(segment.text)
    transcribe_time = time.perf_counter() - t1

    gpu_after_transcribe = get_gpu_info()

    full_text = " ".join(text_parts).strip()

    # Results
    print(f"\n--- Results ---")
    print(f"Language: {info.language} (prob: {info.language_probability:.2f})")
    print(f"Duration: {info.duration:.1f}s audio")
    print(f"Load time: {load_time:.2f}s")
    print(f"Transcribe time: {transcribe_time:.2f}s")
    print(f"Real-time factor: {transcribe_time / info.duration:.2f}x")

    if gpu_before:
        print(f"\nGPU: {gpu_before['name']}")
        print(f"VRAM before: {gpu_before['used_mb']}MB / {gpu_before['total_mb']}MB")
    if gpu_after_load:
        print(f"VRAM after load: {gpu_after_load['used_mb']}MB / {gpu_after_load['total_mb']}MB")
    if gpu_after_transcribe:
        print(f"VRAM after transcribe: {gpu_after_transcribe['used_mb']}MB / {gpu_after_transcribe['total_mb']}MB")

    print(f"\nTranscription:\n{full_text}")
    print(f"{'='*60}\n")

    return {
        "model": model_size,
        "load_time": load_time,
        "transcribe_time": transcribe_time,
        "duration": info.duration,
        "rtf": transcribe_time / info.duration,
        "text": full_text,
        "vram_after": gpu_after_transcribe["used_mb"] if gpu_after_transcribe else None,
    }


def main():
    parser = argparse.ArgumentParser(description="Test local Whisper transcription")
    parser.add_argument("audio", nargs="?",
                        default="../recordings/20260101_185000_Harvard_speech100.wav",
                        help="Path to audio file")
    parser.add_argument("--model", default="small",
                        help="Whisper model size (default: small). Options: tiny, base, small, medium, large-v3, distil-small.en, distil-medium.en, distil-large-v3")
    parser.add_argument("--device", choices=["cuda", "cpu"],
                        help="Force device (default: auto-detect)")
    parser.add_argument("--compute", choices=["float16", "float32", "int8", "int8_float32"],
                        help="Force compute type (default: auto-detect)")
    parser.add_argument("--compare", action="store_true",
                        help="Compare tiny, base, and small models")
    parser.add_argument("--compare-compute", action="store_true",
                        help="Compare compute types (float32, int8) on GPU vs CPU for one model")
    args = parser.parse_args()

    audio = Path(args.audio)
    if not audio.exists():
        print(f"Error: Audio file not found: {audio}")
        sys.exit(1)

    print(f"Audio file: {audio} ({audio.stat().st_size / 1024 / 1024:.1f}MB)")

    gpu = get_gpu_info()
    if gpu:
        print(f"GPU: {gpu['name']} ({gpu['total_mb']}MB VRAM, {gpu['used_mb']}MB used)")
    else:
        print("Warning: nvidia-smi not available, GPU info unavailable")

    if args.compare_compute:
        # Compare different compute types for one model size
        configs = [
            ("cuda", "float32"),
            ("cuda", "int8"),
            ("cuda", "int8_float32"),
            ("cpu", "int8"),
            ("cpu", "float32"),
        ]
        results = []
        for device, compute in configs:
            label = f"{device}/{compute}"
            try:
                r = transcribe(str(audio), args.model,
                               force_device=device, force_compute=compute)
                r["label"] = label
                results.append(r)
            except Exception as e:
                print(f"Error with {label}: {e}")
                results.append({"label": label, "model": args.model, "error": str(e)})

        print("\n" + "="*60)
        print(f"COMPUTE TYPE COMPARISON — model: {args.model}")
        print("="*60)
        print(f"{'Config':<22} {'Load(s)':<9} {'Trans(s)':<10} {'RTF':<6} {'VRAM(MB)':<10}")
        print("-"*60)
        for r in results:
            if "error" in r:
                print(f"{r['label']:<22} ERROR: {r['error']}")
            else:
                vram = str(r['vram_after']) if r['vram_after'] else "N/A"
                print(f"{r['label']:<22} {r['load_time']:<9.2f} {r['transcribe_time']:<10.2f} {r['rtf']:<6.2f} {vram:<10}")

    elif args.compare:
        results = []
        for size in ["tiny", "base", "small"]:
            try:
                r = transcribe(str(audio), size,
                               force_device=args.device, force_compute=args.compute)
                results.append(r)
            except Exception as e:
                print(f"Error with {size}: {e}")
                results.append({"model": size, "error": str(e)})

        print("\n" + "="*60)
        print("COMPARISON SUMMARY")
        print("="*60)
        print(f"{'Model':<8} {'Load(s)':<9} {'Trans(s)':<10} {'RTF':<6} {'VRAM(MB)':<10}")
        print("-"*50)
        for r in results:
            if "error" in r:
                print(f"{r['model']:<8} ERROR: {r['error']}")
            else:
                vram = str(r['vram_after']) if r['vram_after'] else "N/A"
                print(f"{r['model']:<8} {r['load_time']:<9.2f} {r['transcribe_time']:<10.2f} {r['rtf']:<6.2f} {vram:<10}")
    else:
        transcribe(str(audio), args.model,
                   force_device=args.device, force_compute=args.compute)


if __name__ == "__main__":
    main()
