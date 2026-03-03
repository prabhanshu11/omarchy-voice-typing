import { useState, useEffect } from 'react';
import type { SessionSummary } from '../types';
import { fetchSessions } from '../api';

export function useSessionData(from?: string, to?: string, limit?: number) {
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);

    fetchSessions(from, to, limit)
      .then((data) => {
        if (!cancelled) {
          setSessions(data);
          setError(null);
        }
      })
      .catch((err) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load sessions');
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => { cancelled = true; };
  }, [from, to, limit]);

  return { sessions, loading, error, refetch: () => {
    setLoading(true);
    fetchSessions(from, to, limit)
      .then(setSessions)
      .catch(() => {})
      .finally(() => setLoading(false));
  }};
}
