import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fetchLinked, fetchStats, getAudioUrl, fetchLatency, fetchSessions, fetchProfilingSummary } from '../../lib/api';

const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

beforeEach(() => {
  mockFetch.mockReset();
});

describe('fetchLinked', () => {
  it('fetches linked entries', async () => {
    const data = [{ recording: { filename: 'test.wav' } }];
    mockFetch.mockResolvedValue({ ok: true, json: () => Promise.resolve(data) });

    const result = await fetchLinked();
    expect(result).toEqual(data);
    expect(mockFetch).toHaveBeenCalledWith('/api/linked');
  });

  it('throws on error', async () => {
    mockFetch.mockResolvedValue({ ok: false });
    await expect(fetchLinked()).rejects.toThrow('Failed to fetch linked entries');
  });
});

describe('fetchStats', () => {
  it('fetches stats', async () => {
    const stats = { total_recordings: 10, total_transcripts: 8, total_audio_size_mb: 50.5, total_transcript_kb: 12.3 };
    mockFetch.mockResolvedValue({ ok: true, json: () => Promise.resolve(stats) });

    const result = await fetchStats();
    expect(result.total_recordings).toBe(10);
  });
});

describe('getAudioUrl', () => {
  it('constructs URL with encoded filename', () => {
    expect(getAudioUrl('20260105_211652_audio.wav')).toBe('/api/audio/20260105_211652_audio.wav');
  });

  it('encodes special characters', () => {
    expect(getAudioUrl('file with spaces.wav')).toBe('/api/audio/file%20with%20spaces.wav');
  });
});

describe('fetchLatency', () => {
  it('builds URL with date params', async () => {
    mockFetch.mockResolvedValue({ ok: true, json: () => Promise.resolve([]) });

    await fetchLatency('2026-02-18', '2026-02-22');
    expect(mockFetch).toHaveBeenCalledWith('/api/profiling/latency?from=2026-02-18&to=2026-02-22');
  });

  it('builds URL without params', async () => {
    mockFetch.mockResolvedValue({ ok: true, json: () => Promise.resolve([]) });

    await fetchLatency();
    expect(mockFetch).toHaveBeenCalledWith('/api/profiling/latency');
  });
});

describe('fetchSessions', () => {
  it('includes limit param', async () => {
    mockFetch.mockResolvedValue({ ok: true, json: () => Promise.resolve([]) });

    await fetchSessions('2026-02-18', undefined, 100);
    expect(mockFetch).toHaveBeenCalledWith('/api/profiling/sessions?from=2026-02-18&limit=100');
  });
});

describe('fetchProfilingSummary', () => {
  it('fetches summary', async () => {
    const summary = { total_sessions: 100, success_rate: 0.95 };
    mockFetch.mockResolvedValue({ ok: true, json: () => Promise.resolve(summary) });

    const result = await fetchProfilingSummary();
    expect(result.total_sessions).toBe(100);
  });
});
