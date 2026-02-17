import type { Recording, Transcript, LinkedEntry, Stats } from './types';

const API_BASE = '';

export async function fetchRecordings(): Promise<Recording[]> {
  const res = await fetch(`${API_BASE}/api/recordings`);
  if (!res.ok) throw new Error('Failed to fetch recordings');
  return res.json();
}

export async function fetchTranscripts(): Promise<Transcript[]> {
  const res = await fetch(`${API_BASE}/api/transcripts`);
  if (!res.ok) throw new Error('Failed to fetch transcripts');
  return res.json();
}

export async function fetchLinked(): Promise<LinkedEntry[]> {
  const res = await fetch(`${API_BASE}/api/linked`);
  if (!res.ok) throw new Error('Failed to fetch linked entries');
  return res.json();
}

export async function fetchTranscript(filename: string): Promise<Transcript> {
  const res = await fetch(`${API_BASE}/api/transcript?filename=${encodeURIComponent(filename)}`);
  if (!res.ok) throw new Error('Failed to fetch transcript');
  return res.json();
}

export async function updateTranscript(filename: string, text: string): Promise<void> {
  const res = await fetch(`${API_BASE}/api/transcript?filename=${encodeURIComponent(filename)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ text }),
  });
  if (!res.ok) throw new Error('Failed to update transcript');
}

export async function fetchStats(): Promise<Stats> {
  const res = await fetch(`${API_BASE}/api/stats`);
  if (!res.ok) throw new Error('Failed to fetch stats');
  return res.json();
}

export function getAudioUrl(filename: string): string {
  return `${API_BASE}/api/audio?filename=${encodeURIComponent(filename)}`;
}

export async function transcribeAudio(audioFilename: string): Promise<void> {
  // Fetch the audio file first
  const audioUrl = getAudioUrl(audioFilename);
  const audioResponse = await fetch(audioUrl);
  if (!audioResponse.ok) throw new Error('Failed to fetch audio file');

  const audioBlob = await audioResponse.blob();

  // Create form data with the audio file
  const formData = new FormData();
  formData.append('file', audioBlob, audioFilename);

  // Send to transcription endpoint
  const res = await fetch(`${API_BASE}/v1/transcribe`, {
    method: 'POST',
    body: formData,
  });

  if (!res.ok) {
    const errorText = await res.text();
    throw new Error(`Transcription failed: ${errorText}`);
  }
}
