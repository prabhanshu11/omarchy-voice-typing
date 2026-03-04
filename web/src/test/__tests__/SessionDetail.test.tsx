import { describe, it, expect, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';
import { SessionDetail } from '../../pages/profiling/SessionDetail';
import type { SessionSummary } from '../../lib/types';

afterEach(cleanup);

const okSession: SessionSummary = {
  filename: '20260226_190639_rec-003.log',
  session_id: 'rec-003',
  started: '2026-02-26 19:06:39.943',
  duration_s: 6.2,
  status: 'OK',
  backend: 'deepgram',
  transcript: 'Hello world this is a test',
  dg_connect_ms: 1706,
  first_audio_ms: 0,
  transcribe_ms: 1926,
  total_ms: 6213,
  py_chunks: 66,
  gw_bytes: 202752,
  timeline: [
    { elapsed_s: 0.0, layer: 'GATEWAY', event: 'rec-003 started (offline=false)' },
    { elapsed_s: 0.0, layer: 'GATEWAY', event: 'first audio chunk: 3072 bytes' },
    { elapsed_s: 1.707, layer: 'DEEPGRAM', event: 'connected in 1.706842828s' },
    { elapsed_s: 4.286, layer: 'GATEWAY', event: 'commit received' },
    { elapsed_s: 4.287, layer: 'GATEWAY', event: 'taking ONLINE transcription path (Deepgram)' },
    { elapsed_s: 6.213, layer: 'GATEWAY', event: 'commit complete: backend=deepgram, transcript=53 chars' },
  ],
  wav_filename: '20260226_190639_audio.wav',
};

describe('SessionDetail', () => {
  it('shows empty state when no session selected', () => {
    render(<SessionDetail session={null} />);
    expect(screen.getByText('Select a session')).toBeInTheDocument();
  });

  it('renders session header with metadata', () => {
    render(<SessionDetail session={okSession} />);
    expect(screen.getByText('rec-003')).toBeInTheDocument();
    expect(screen.getByText('OK')).toBeInTheDocument();
    // "deepgram" appears in header badge and sequence diagram
    const badges = document.querySelectorAll('.backend-badge');
    expect(Array.from(badges).some(b => b.textContent === 'deepgram')).toBe(true);
  });

  it('renders audio element pointing to correct file', () => {
    render(<SessionDetail session={okSession} />);
    const audio = document.querySelector('audio');
    expect(audio).toBeTruthy();
    // Filename: 20260226_190639_rec-003.log → 20260226_190639_audio.wav
    expect(audio?.src).toContain('20260226_190639_audio.wav');
  });

  it('shows "no audio" when audio element fires error', () => {
    render(<SessionDetail session={okSession} />);
    const audio = document.querySelector('audio');
    expect(audio).toBeTruthy();

    // Simulate the audio element failing to load
    fireEvent.error(audio!);

    expect(screen.getByText('Audio file not found')).toBeInTheDocument();
    // Audio element should be gone
    expect(document.querySelector('audio')).toBeNull();
  });

  it('renders sequence diagram for session with timeline', () => {
    render(<SessionDetail session={okSession} />);
    expect(screen.getByText('Sequence Diagram')).toBeInTheDocument();
  });

  it('renders timing breakdown waterfall', () => {
    render(<SessionDetail session={okSession} />);
    expect(screen.getByText('Timing Breakdown')).toBeInTheDocument();
    // Should show finalize wait (the only actual user-felt latency)
    expect(screen.getByText('Finalize wait')).toBeInTheDocument();
  });

  it('renders transcript editor', () => {
    render(<SessionDetail session={okSession} />);
    expect(screen.getByText('Hello world this is a test')).toBeInTheDocument();
  });
});
