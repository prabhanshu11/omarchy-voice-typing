import { useCallback, useRef, useState } from 'react';

export type SessionState =
  | 'idle'
  | 'connecting'
  | 'ready'
  | 'recording'
  | 'processing'
  | 'done'
  | 'error';

interface UseRealtimeSessionReturn {
  connect: () => void;
  disconnect: () => void;
  sendAudio: (base64Chunk: string) => void;
  commit: () => void;
  state: SessionState;
  transcript: string;
  error: string | null;
  /** Model/backend reported by the gateway (e.g. "nova-2", "local-whisper") */
  backend: string | null;
}

/** Construct the WebSocket URL relative to the current page. */
function buildWsUrl(): string {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  const base = import.meta.env.BASE_URL || '/';
  return `${proto}//${location.host}${base}v1/realtime`;
}

export function useRealtimeSession(): UseRealtimeSessionReturn {
  const [state, setState] = useState<SessionState>('idle');
  const [transcript, setTranscript] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [backend, setBackend] = useState<string | null>(null);

  const wsRef = useRef<WebSocket | null>(null);
  const stateRef = useRef<SessionState>('idle');

  // Keep ref in sync with state
  const setStateTracked = useCallback((s: SessionState) => {
    stateRef.current = s;
    setState(s);
  }, []);

  const disconnect = useCallback(() => {
    if (wsRef.current) {
      wsRef.current.close();
      wsRef.current = null;
    }
    setStateTracked('idle');
  }, [setStateTracked]);

  const connect = useCallback(() => {
    // Clean up any existing connection
    if (wsRef.current) {
      wsRef.current.close();
      wsRef.current = null;
    }

    setError(null);
    setTranscript('');
    setBackend(null);
    setStateTracked('connecting');

    const url = buildWsUrl();
    const ws = new WebSocket(url);
    wsRef.current = ws;

    ws.onopen = () => {
      // Wait for session.created from server before sending anything
    };

    ws.onmessage = (event) => {
      let msg: { type: string; session?: { model?: string }; transcript?: string };
      try {
        msg = JSON.parse(event.data);
      } catch {
        return;
      }

      switch (msg.type) {
        case 'session.created':
          // Send session.update to signal readiness
          ws.send(JSON.stringify({ type: 'session.update', session: {} }));
          break;

        case 'session.updated':
          if (msg.session?.model) {
            setBackend(msg.session.model);
          }
          setStateTracked('ready');
          break;

        case 'conversation.item.input_audio_transcription.completed':
          setTranscript(msg.transcript ?? '');
          setStateTracked('done');
          break;
      }
    };

    ws.onerror = () => {
      setError('WebSocket connection failed');
      setStateTracked('error');
    };

    ws.onclose = (event) => {
      // Use ref to avoid stale closure over state
      const currentState = stateRef.current;
      if (currentState !== 'done' && currentState !== 'idle') {
        if (event.code !== 1000) {
          setError(`Connection closed (code ${event.code})`);
          setStateTracked('error');
        }
      }
      wsRef.current = null;
    };
  }, [setStateTracked]);

  const sendAudio = useCallback((base64Chunk: string) => {
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return;

    ws.send(
      JSON.stringify({
        type: 'input_audio_buffer.append',
        audio: base64Chunk,
      })
    );
  }, []);

  const commit = useCallback(() => {
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return;

    setStateTracked('processing');
    ws.send(JSON.stringify({ type: 'input_audio_buffer.commit' }));
  }, []);

  return {
    connect,
    disconnect,
    sendAudio,
    commit,
    state,
    transcript,
    error,
    backend,
  };
}
