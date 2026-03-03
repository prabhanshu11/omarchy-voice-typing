import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, waitFor, cleanup } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { ProfilingPage } from '../../pages/profiling/ProfilingPage';

afterEach(cleanup);

const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

const mockSessions = [
  {
    filename: '20260226_190639_rec-003.log',
    session_id: 'rec-003',
    started: '2026-02-26 19:06:39.943',
    duration_s: 6.2,
    status: 'OK',
    backend: 'deepgram',
    transcript: 'Test transcript text',
    dg_connect_ms: 1706,
    first_audio_ms: 0,
    transcribe_ms: 1926,
    total_ms: 6213,
    py_chunks: 66,
    gw_bytes: 202752,
    timeline: [],
  },
];

const mockSummary = {
  total_sessions: 180,
  success_count: 165,
  failure_count: 15,
  success_rate: 0.917,
  total_commit_ms_p50: 5200,
  total_commit_ms_p95: 12000,
  total_commit_ms_p99: 28000,
  transcription_ms_p50: 1800,
  transcription_ms_p95: 3200,
  transcription_ms_p99: 5000,
  by_backend: [
    { backend: 'deepgram', count: 120, avg_total_ms: 6000, p50_total_ms: 5500, p95_total_ms: 11000 },
  ],
  by_date: [],
};

beforeEach(() => {
  mockFetch.mockReset();
  mockFetch.mockImplementation((url: string) => {
    if (url.includes('/api/profiling/sessions')) {
      return Promise.resolve({ ok: true, json: () => Promise.resolve(mockSessions) });
    }
    if (url.includes('/api/profiling/summary')) {
      return Promise.resolve({ ok: true, json: () => Promise.resolve(mockSummary) });
    }
    return Promise.resolve({ ok: true, json: () => Promise.resolve({}) });
  });
});

describe('ProfilingPage', () => {
  it('shows loading state initially', () => {
    render(<MemoryRouter><ProfilingPage /></MemoryRouter>);
    expect(screen.getByText('Loading sessions...')).toBeInTheDocument();
  });

  it('renders session list and KPIs after loading', async () => {
    render(<MemoryRouter><ProfilingPage /></MemoryRouter>);

    await waitFor(() => {
      expect(screen.getByText('rec-003')).toBeInTheDocument();
    });

    // KPIs
    const kpiValues = document.querySelectorAll('.kpi-value');
    const texts = Array.from(kpiValues).map((el) => el.textContent);
    expect(texts).toContain('180');
    expect(texts).toContain('91.7%');
  });

  it('renders backend stats', async () => {
    render(<MemoryRouter><ProfilingPage /></MemoryRouter>);

    await waitFor(() => {
      const kpiLabels = document.querySelectorAll('.kpi-label');
      const labels = Array.from(kpiLabels).map((el) => el.textContent);
      expect(labels).toContain('deepgram');
    });
  });
});
