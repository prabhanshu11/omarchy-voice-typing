import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { EntryCard } from '../../pages/recordings/EntryCard';
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
    text: 'This is a test transcript with some content for preview testing.',
  },
};

describe('EntryCard', () => {
  it('renders timestamp and badges', () => {
    const { container } = render(<EntryCard entry={mockEntry} isSelected={false} onSelect={() => {}} />);
    expect(container.querySelector('.entry-time')?.textContent).toContain('Feb');
    expect(container.querySelector('.badge.audio')).toBeTruthy();
    expect(container.querySelector('.badge.text')).toBeTruthy();
  });

  it('renders pending badge when no transcript', () => {
    const noTranscript: LinkedEntry = { recording: mockEntry.recording };
    const { container } = render(<EntryCard entry={noTranscript} isSelected={false} onSelect={() => {}} />);
    expect(container.querySelector('.badge.missing')?.textContent).toBe('pending');
  });

  it('renders preview text', () => {
    const { container } = render(<EntryCard entry={mockEntry} isSelected={false} onSelect={() => {}} />);
    expect(container.querySelector('.entry-preview')?.textContent).toContain('test transcript');
  });

  it('calls onSelect when clicked', async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    const { container } = render(<EntryCard entry={mockEntry} isSelected={false} onSelect={onSelect} />);

    await user.click(container.querySelector('.entry-card')!);
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it('applies selected class', () => {
    const { container } = render(<EntryCard entry={mockEntry} isSelected={true} onSelect={() => {}} />);
    expect(container.querySelector('.entry-card.selected')).toBeTruthy();
  });
});
