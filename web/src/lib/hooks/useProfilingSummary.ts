import { useState, useEffect } from 'react';
import type { ProfilingSummary } from '../types';
import { fetchProfilingSummary } from '../api';

export function useProfilingSummary(from?: string, to?: string) {
  const [summary, setSummary] = useState<ProfilingSummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);

    fetchProfilingSummary(from, to)
      .then((data) => {
        if (!cancelled) {
          setSummary(data);
          setError(null);
        }
      })
      .catch((err) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load summary');
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => { cancelled = true; };
  }, [from, to]);

  return { summary, loading, error };
}
