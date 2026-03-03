import type { LinkedEntry, Stats, LatencyRecord, SessionSummary, ProfilingSummary } from './types';

const API_BASE = '';

// ── Recording endpoints ──────────────────────────────

export async function fetchLinked(): Promise<LinkedEntry[]> {
  const res = await fetch(`${API_BASE}/api/linked`);
  if (!res.ok) throw new Error('Failed to fetch linked entries');
  return res.json();
}

export async function fetchStats(): Promise<Stats> {
  const res = await fetch(`${API_BASE}/api/stats`);
  if (!res.ok) throw new Error('Failed to fetch stats');
  return res.json();
}

export async function updateTranscript(filename: string, text: string): Promise<void> {
  const res = await fetch(`${API_BASE}/api/transcript/${encodeURIComponent(filename)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ text }),
  });
  if (!res.ok) throw new Error('Failed to update transcript');
}

export function getAudioUrl(filename: string): string {
  return `${API_BASE}/api/audio/${encodeURIComponent(filename)}`;
}

export async function transcribeAudio(audioFilename: string): Promise<void> {
  const audioUrl = getAudioUrl(audioFilename);
  const audioResponse = await fetch(audioUrl);
  if (!audioResponse.ok) throw new Error('Failed to fetch audio file');

  const audioBlob = await audioResponse.blob();
  const formData = new FormData();
  formData.append('file', audioBlob, audioFilename);

  const res = await fetch(`${API_BASE}/v1/transcribe`, {
    method: 'POST',
    body: formData,
  });

  if (!res.ok) {
    const errorText = await res.text();
    throw new Error(`Transcription failed: ${errorText}`);
  }
}

// ── Profiling endpoints ──────────────────────────────

export async function fetchLatency(from?: string, to?: string): Promise<LatencyRecord[]> {
  const params = new URLSearchParams();
  if (from) params.set('from', from);
  if (to) params.set('to', to);
  const qs = params.toString();
  const res = await fetch(`${API_BASE}/api/profiling/latency${qs ? `?${qs}` : ''}`);
  if (!res.ok) throw new Error('Failed to fetch latency data');
  return res.json();
}

export async function fetchSessions(from?: string, to?: string, limit?: number): Promise<SessionSummary[]> {
  const params = new URLSearchParams();
  if (from) params.set('from', from);
  if (to) params.set('to', to);
  if (limit) params.set('limit', limit.toString());
  const qs = params.toString();
  const res = await fetch(`${API_BASE}/api/profiling/sessions${qs ? `?${qs}` : ''}`);
  if (!res.ok) throw new Error('Failed to fetch sessions');
  return res.json();
}

export async function fetchProfilingSummary(from?: string, to?: string): Promise<ProfilingSummary> {
  const params = new URLSearchParams();
  if (from) params.set('from', from);
  if (to) params.set('to', to);
  const qs = params.toString();
  const res = await fetch(`${API_BASE}/api/profiling/summary${qs ? `?${qs}` : ''}`);
  if (!res.ok) throw new Error('Failed to fetch profiling summary');
  return res.json();
}
