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
  /** Connect and wait for session to be ready. Resolves true if ready, false on error. */
  connect: () => Promise<boolean>;
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
  const readyResolveRef = useRef<((ready: boolean) => void) | null>(null);

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

  const connect = useCallback((): Promise<boolean> => {
    // Clean up any existing connection
    if (wsRef.current) {
      wsRef.current.close();
      wsRef.current = null;
    }
    // Resolve any pending ready promise as failed
    readyResolveRef.current?.(false);

    setError(null);
    setTranscript('');
    setBackend(null);
    setStateTracked('connecting');

    return new Promise<boolean>((resolve) => {
      readyResolveRef.current = resolve;

      const url = buildWsUrl();
      const ws = new WebSocket(url);
      wsRef.current = ws;

      // Timeout: if not ready in 5s, give up
      const timeout = setTimeout(() => {
        if (readyResolveRef.current === resolve) {
          readyResolveRef.current = null;
          setError('Connection timed out');
          setStateTracked('error');
          ws.close();
          resolve(false);
        }
      }, 5000);

      ws.onmessage = (event) => {
        let msg: { type: string; session?: { model?: string }; transcript?: string };
        try {
          msg = JSON.parse(event.data);
        } catch {
          return;
        }

        switch (msg.type) {
          case 'session.created':
            ws.send(JSON.stringify({ type: 'session.update', session: { source: 'web' } }));
            break;

          case 'session.updated':
            if (msg.session?.model) {
              setBackend(msg.session.model);
            }
            setStateTracked('ready');
            clearTimeout(timeout);
            if (readyResolveRef.current === resolve) {
              readyResolveRef.current = null;
              resolve(true);
            }
            break;

          case 'conversation.item.input_audio_transcription.completed':
            setTranscript(msg.transcript ?? '');
            setStateTracked('done');
            break;
        }
      };

      ws.onerror = () => {
        clearTimeout(timeout);
        setError('WebSocket connection failed');
        setStateTracked('error');
        if (readyResolveRef.current === resolve) {
          readyResolveRef.current = null;
          resolve(false);
        }
      };

      ws.onclose = (event) => {
        clearTimeout(timeout);
        const currentState = stateRef.current;
        // Only show error if connection closed unexpectedly during active use
        if (currentState !== 'done' && currentState !== 'idle' && currentState !== 'error') {
          if (event.code !== 1000) {
            setError(`Connection closed unexpectedly`);
            setStateTracked('error');
          }
        }
        wsRef.current = null;
        if (readyResolveRef.current === resolve) {
          readyResolveRef.current = null;
          resolve(false);
        }
      };
    });
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
