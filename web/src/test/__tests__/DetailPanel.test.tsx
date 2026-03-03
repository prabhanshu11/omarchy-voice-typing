import { describe, it, expect, afterEach } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { DetailPanel } from '../../pages/recordings/DetailPanel';
import type { LinkedEntry } from '../../lib/types';

afterEach(cleanup);

const mockEntry: LinkedEntry = {
  recording: {
    filename: '20260220_143000_audio.wav',
    path: '/recordings/20260220_143000_audio.wav',
    size: 204800,
    timestamp: '2026-02-20T14:30:00',
  },
  transcript: {
    filename: '20260220_143000_transcript.txt',
    path: '/transcripts/20260220_143000_transcript.txt',
    size: 150,
    timestamp: '2026-02-20T14:30:00',
    text: 'This is the transcript text.',
  },
};

describe('DetailPanel', () => {
  it('shows empty state when no entry', () => {
    render(<DetailPanel entry={null} onTranscriptSaved={() => {}} />);
    expect(screen.getByText('Select a recording')).toBeInTheDocument();
  });

  it('displays transcript text and edit button', () => {
    render(<DetailPanel entry={mockEntry} onTranscriptSaved={() => {}} />);
    expect(screen.getByText('This is the transcript text.')).toBeInTheDocument();
    expect(screen.getByText('Edit')).toBeInTheDocument();
  });

  it('enters edit mode on Edit click', async () => {
    const user = userEvent.setup();
    render(<DetailPanel entry={mockEntry} onTranscriptSaved={() => {}} />);

    await user.click(screen.getByText('Edit'));
    expect(screen.getByText('Save')).toBeInTheDocument();
    expect(screen.getByText('Cancel')).toBeInTheDocument();
  });

  it('shows audio element', () => {
    const { container } = render(<DetailPanel entry={mockEntry} onTranscriptSaved={() => {}} />);
    expect(container.querySelector('audio')).toBeTruthy();
  });

  it('shows Transcribe button when no transcript', () => {
    const entryNoText: LinkedEntry = { recording: mockEntry.recording };
    render(<DetailPanel entry={entryNoText} onTranscriptSaved={() => {}} />);
    expect(screen.getByText('Transcribe')).toBeInTheDocument();
  });
});
