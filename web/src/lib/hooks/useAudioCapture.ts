import { useCallback, useRef, useState } from 'react';

interface UseAudioCaptureOptions {
  /** Called with each base64-encoded PCM16 chunk (~85ms of audio) */
  onChunk: (base64Data: string) => void;
  /** Target sample rate for the gateway (default: 24000) */
  targetSampleRate?: number;
  /** Specific audio input device ID (from navigator.mediaDevices.enumerateDevices) */
  deviceId?: string;
}

interface UseAudioCaptureReturn {
  start: () => Promise<void>;
  stop: () => void;
  isRecording: boolean;
  error: string | null;
  /** Get the full recorded audio as a WAV Blob (for AssemblyAI upload mode) */
  getWavBlob: () => Blob | null;
}

const TARGET_RATE = 24000;
const BUFFER_SIZE = 4096; // ScriptProcessorNode buffer size

/** Convert Float32 samples to Int16 PCM bytes (little-endian). */
function float32ToInt16(samples: Float32Array): ArrayBuffer {
  const buf = new ArrayBuffer(samples.length * 2);
  const view = new DataView(buf);
  for (let i = 0; i < samples.length; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(i * 2, s < 0 ? s * 0x8000 : s * 0x7fff, true);
  }
  return buf;
}

/** Downsample from sourceRate to targetRate (simple linear interpolation). */
function downsample(samples: Float32Array, sourceRate: number, targetRate: number): Float32Array {
  if (sourceRate === targetRate) return samples;
  const ratio = sourceRate / targetRate;
  const newLength = Math.round(samples.length / ratio);
  const result = new Float32Array(newLength);
  for (let i = 0; i < newLength; i++) {
    const srcIndex = i * ratio;
    const low = Math.floor(srcIndex);
    const high = Math.min(low + 1, samples.length - 1);
    const frac = srcIndex - low;
    result[i] = samples[low] * (1 - frac) + samples[high] * frac;
  }
  return result;
}

/** Fast ArrayBuffer to base64 using chunked String.fromCharCode. */
function arrayBufferToBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  const chunks: string[] = [];
  // Process in chunks of 8192 to avoid call stack limits on String.fromCharCode
  for (let i = 0; i < bytes.length; i += 8192) {
    const slice = bytes.subarray(i, Math.min(i + 8192, bytes.length));
    chunks.push(String.fromCharCode(...slice));
  }
  return btoa(chunks.join(''));
}

/** Build a WAV file from PCM16 data at the given sample rate. */
function buildWav(pcm16Chunks: ArrayBuffer[], sampleRate: number): Blob {
  const totalBytes = pcm16Chunks.reduce((sum, c) => sum + c.byteLength, 0);
  const header = new ArrayBuffer(44);
  const view = new DataView(header);

  writeString(view, 0, 'RIFF');
  view.setUint32(4, 36 + totalBytes, true);
  writeString(view, 8, 'WAVE');
  writeString(view, 12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  writeString(view, 36, 'data');
  view.setUint32(40, totalBytes, true);

  return new Blob([header, ...pcm16Chunks], { type: 'audio/wav' });
}

function writeString(view: DataView, offset: number, str: string) {
  for (let i = 0; i < str.length; i++) {
    view.setUint8(offset + i, str.charCodeAt(i));
  }
}

export function useAudioCapture({
  onChunk,
  targetSampleRate = TARGET_RATE,
  deviceId,
}: UseAudioCaptureOptions): UseAudioCaptureReturn {
  const [isRecording, setIsRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const streamRef = useRef<MediaStream | null>(null);
  const contextRef = useRef<AudioContext | null>(null);
  const processorRef = useRef<ScriptProcessorNode | null>(null);
  const pcmChunksRef = useRef<ArrayBuffer[]>([]);

  const start = useCallback(async () => {
    setError(null);
    pcmChunksRef.current = [];

    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: {
          channelCount: 1,
          echoCancellation: true,
          noiseSuppression: true,
          ...(deviceId ? { deviceId: { exact: deviceId } } : {}),
        },
      });
      streamRef.current = stream;

      // Let the browser pick its native sample rate — we downsample in software
      const ctx = new AudioContext();
      contextRef.current = ctx;

      // CRITICAL: Resume the AudioContext — browsers create it suspended
      // without a prior user gesture on the exact same AudioContext instance.
      // getUserMedia counts as a gesture, but the context may still need an
      // explicit resume call.
      if (ctx.state === 'suspended') {
        await ctx.resume();
      }

      const source = ctx.createMediaStreamSource(stream);
      const processor = ctx.createScriptProcessor(BUFFER_SIZE, 1, 1);
      processorRef.current = processor;

      const nativeSampleRate = ctx.sampleRate;

      processor.onaudioprocess = (e) => {
        const input = e.inputBuffer.getChannelData(0);
        const downsampled = downsample(input, nativeSampleRate, targetSampleRate);
        const pcm16 = float32ToInt16(downsampled);
        pcmChunksRef.current.push(pcm16);
        onChunk(arrayBufferToBase64(pcm16));
      };

      source.connect(processor);
      // Connect to destination (required for ScriptProcessorNode to fire)
      processor.connect(ctx.destination);
      setIsRecording(true);
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Microphone access denied';
      setError(msg);
    }
  }, [onChunk, targetSampleRate, deviceId]);

  const stop = useCallback(() => {
    processorRef.current?.disconnect();
    processorRef.current = null;

    if (contextRef.current?.state !== 'closed') {
      contextRef.current?.close();
    }
    contextRef.current = null;

    streamRef.current?.getTracks().forEach((t) => t.stop());
    streamRef.current = null;

    setIsRecording(false);
  }, []);

  const getWavBlob = useCallback((): Blob | null => {
    if (pcmChunksRef.current.length === 0) return null;
    return buildWav(pcmChunksRef.current, targetSampleRate);
  }, [targetSampleRate]);

  return { start, stop, isRecording, error, getWavBlob };
}
