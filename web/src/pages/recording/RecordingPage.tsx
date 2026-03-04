import { useCallback, useEffect, useRef, useState } from 'react';
import { useAudioCapture } from '../../lib/hooks/useAudioCapture';
import { useRealtimeSession } from '../../lib/hooks/useRealtimeSession';
import './RecordingPage.css';

type Backend = 'deepgram' | 'assemblyai';
type RecordMode = 'hold' | 'toggle';

interface AudioDevice {
  deviceId: string;
  label: string;
}

export function RecordingPage() {
  const [backend, setBackend] = useState<Backend>('deepgram');
  const [recordMode, setRecordMode] = useState<RecordMode>('toggle');
  const [copied, setCopied] = useState(false);
  const [aaiStatus, setAaiStatus] = useState<'idle' | 'uploading' | 'done' | 'error'>('idle');
  const [aaiTranscript, setAaiTranscript] = useState('');
  const [aaiError, setAaiError] = useState<string | null>(null);
  const [recordingDuration, setRecordingDuration] = useState(0);
  const [audioDevices, setAudioDevices] = useState<AudioDevice[]>([]);
  const [selectedDeviceId, setSelectedDeviceId] = useState<string>('');

  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const isHoldingRef = useRef(false);

  // Enumerate audio input devices
  useEffect(() => {
    async function loadDevices() {
      try {
        // Need a brief getUserMedia to get labeled devices (browser requires permission first)
        const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
        stream.getTracks().forEach((t) => t.stop());

        const devices = await navigator.mediaDevices.enumerateDevices();
        const inputs = devices
          .filter((d) => d.kind === 'audioinput' && d.deviceId !== 'default')
          .map((d) => ({
            deviceId: d.deviceId,
            label: d.label || `Microphone ${d.deviceId.slice(0, 8)}`,
          }));
        setAudioDevices(inputs);
      } catch {
        // Permission denied — will be caught again when recording starts
      }
    }
    loadDevices();
  }, []);

  const session = useRealtimeSession();

  const audioCapture = useAudioCapture({
    onChunk: useCallback(
      (base64: string) => {
        if (backend === 'deepgram') {
          session.sendAudio(base64);
        }
      },
      [backend, session.sendAudio]
    ),
    deviceId: selectedDeviceId || undefined,
  });

  // Duration timer
  useEffect(() => {
    if (audioCapture.isRecording) {
      setRecordingDuration(0);
      timerRef.current = setInterval(() => {
        setRecordingDuration((d) => d + 0.1);
      }, 100);
    } else {
      if (timerRef.current) {
        clearInterval(timerRef.current);
        timerRef.current = null;
      }
    }
    return () => {
      if (timerRef.current) clearInterval(timerRef.current);
    };
  }, [audioCapture.isRecording]);

  const startRecording = useCallback(async () => {
    setCopied(false);
    setAaiStatus('idle');
    setAaiTranscript('');
    setAaiError(null);

    if (backend === 'deepgram') {
      const ready = await session.connect();
      if (!ready) return; // connect() already set the error state
    }
    await audioCapture.start();
  }, [backend, session, audioCapture]);

  const stopRecording = useCallback(async () => {
    audioCapture.stop();

    if (backend === 'deepgram') {
      session.commit();
    } else {
      // AssemblyAI: upload the recorded WAV
      const blob = audioCapture.getWavBlob();
      if (!blob) {
        setAaiError('No audio recorded');
        return;
      }

      setAaiStatus('uploading');
      try {
        const formData = new FormData();
        formData.append('file', blob, 'recording.wav');

        const base = import.meta.env.BASE_URL || '/';
        const res = await fetch(`${base}v1/transcribe`, {
          method: 'POST',
          body: formData,
        });

        if (!res.ok) {
          const text = await res.text();
          throw new Error(text || `HTTP ${res.status}`);
        }

        const data = await res.json();
        setAaiTranscript(data.text || '');
        setAaiStatus('done');
      } catch (err) {
        setAaiError(err instanceof Error ? err.message : 'Upload failed');
        setAaiStatus('error');
      }
    }
  }, [backend, audioCapture, session]);

  // Hold-to-record: use global pointerup so releasing anywhere stops recording
  const handlePointerDown = useCallback(() => {
    if (recordMode !== 'hold') return;
    isHoldingRef.current = true;
    startRecording();
  }, [recordMode, startRecording]);

  useEffect(() => {
    if (recordMode !== 'hold') return;

    const handleGlobalPointerUp = () => {
      if (!isHoldingRef.current) return;
      isHoldingRef.current = false;
      stopRecording();
    };

    window.addEventListener('pointerup', handleGlobalPointerUp);
    return () => window.removeEventListener('pointerup', handleGlobalPointerUp);
  }, [recordMode, stopRecording]);

  // Toggle mode handler
  const handleToggleClick = useCallback(() => {
    if (recordMode !== 'toggle') return;
    if (audioCapture.isRecording) {
      stopRecording();
    } else {
      startRecording();
    }
  }, [recordMode, audioCapture.isRecording, startRecording, stopRecording]);

  // Copy to clipboard
  const handleCopy = useCallback(() => {
    const text = backend === 'deepgram' ? session.transcript : aaiTranscript;
    if (!text) return;
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  }, [backend, session.transcript, aaiTranscript]);

  // Determine current transcript and status
  const transcript = backend === 'deepgram' ? session.transcript : aaiTranscript;
  const isProcessing =
    (backend === 'deepgram' && session.state === 'processing') ||
    (backend === 'assemblyai' && aaiStatus === 'uploading');
  const isDone =
    (backend === 'deepgram' && session.state === 'done') ||
    (backend === 'assemblyai' && aaiStatus === 'done');
  const displayError = audioCapture.error || session.error || aaiError;

  const formatDuration = (s: number) => {
    const mins = Math.floor(s / 60);
    const secs = Math.floor(s % 60);
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  };

  return (
    <div className="recording-page">
      {/* Settings bar */}
      <div className="recording-settings">
        <div className="setting-group">
          <label className="setting-label">Backend</label>
          <div className="setting-pills">
            <button
              className={`pill ${backend === 'deepgram' ? 'active' : ''}`}
              onClick={() => setBackend('deepgram')}
              disabled={audioCapture.isRecording}
            >
              Deepgram
              <span className="pill-tag">streaming</span>
            </button>
            <button
              className={`pill ${backend === 'assemblyai' ? 'active' : ''}`}
              onClick={() => setBackend('assemblyai')}
              disabled={audioCapture.isRecording}
            >
              AssemblyAI
              <span className="pill-tag">upload</span>
            </button>
          </div>
        </div>
        <div className="setting-group">
          <label className="setting-label">Mode</label>
          <div className="setting-pills">
            <button
              className={`pill ${recordMode === 'hold' ? 'active' : ''}`}
              onClick={() => setRecordMode('hold')}
              disabled={audioCapture.isRecording}
            >
              Hold
            </button>
            <button
              className={`pill ${recordMode === 'toggle' ? 'active' : ''}`}
              onClick={() => setRecordMode('toggle')}
              disabled={audioCapture.isRecording}
            >
              Toggle
            </button>
          </div>
        </div>
      </div>

      {/* Mic selector */}
      {audioDevices.length > 1 && (
        <div className="recording-settings">
          <div className="setting-group">
            <label className="setting-label">Mic</label>
            <select
              className="mic-select"
              value={selectedDeviceId}
              onChange={(e) => setSelectedDeviceId(e.target.value)}
              disabled={audioCapture.isRecording}
            >
              <option value="">System default</option>
              {audioDevices.map((d) => (
                <option key={d.deviceId} value={d.deviceId}>
                  {d.label}
                </option>
              ))}
            </select>
          </div>
        </div>
      )}

      {/* Record area */}
      <div className="recording-area">
        <div className="record-button-container">
          {audioCapture.isRecording && (
            <div className="record-ring" />
          )}
          <button
            className={`record-button ${audioCapture.isRecording ? 'active' : ''}`}
            onPointerDown={recordMode === 'hold' ? handlePointerDown : undefined}
            onClick={recordMode === 'toggle' ? handleToggleClick : undefined}
            disabled={isProcessing}
          >
            <div className="record-icon">
              {audioCapture.isRecording ? (
                <div className="recording-bars">
                  <span /><span /><span /><span /><span />
                </div>
              ) : (
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
                  <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
                  <line x1="12" y1="19" x2="12" y2="22" />
                </svg>
              )}
            </div>
          </button>
        </div>

        <div className="record-status">
          {audioCapture.isRecording ? (
            <>
              <span className="status-dot recording" />
              <span className="status-text">Recording {formatDuration(recordingDuration)}</span>
            </>
          ) : isProcessing ? (
            <>
              <span className="status-dot processing" />
              <span className="status-text">
                {backend === 'deepgram' ? 'Transcribing...' : 'Uploading & transcribing...'}
              </span>
            </>
          ) : (
            <span className="status-text hint">
              {recordMode === 'hold' ? 'Hold to record' : 'Click to start'}
            </span>
          )}
        </div>

        {session.backend && (
          <div className="backend-badge">
            {session.backend}
          </div>
        )}
      </div>

      {/* Transcript output */}
      {(transcript || displayError) && (
        <div className="transcript-output">
          {displayError ? (
            <div className="transcript-error">
              <span className="error-icon">!</span>
              {displayError}
            </div>
          ) : (
            <>
              <div className="transcript-result">
                <p>{transcript}</p>
              </div>
              {isDone && transcript && (
                <button className="copy-button" onClick={handleCopy}>
                  {copied ? 'Copied!' : 'Copy'}
                </button>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
