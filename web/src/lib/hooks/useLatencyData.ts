import { useState, useEffect } from 'react';
import type { LatencyRecord } from '../types';
import { fetchLatency } from '../api';

export function useLatencyData(from?: string, to?: string) {
  const [records, setRecords] = useState<LatencyRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);

    fetchLatency(from, to)
      .then((data) => {
        if (!cancelled) {
          setRecords(data);
          setError(null);
        }
      })
      .catch((err) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load latency data');
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => { cancelled = true; };
  }, [from, to]);

  return { records, loading, error };
}
