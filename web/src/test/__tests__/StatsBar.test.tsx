import { describe, it, expect } from 'vitest';
import { render, cleanup } from '@testing-library/react';
import { StatsBar } from '../../components/layout/StatsBar';

const stats = {
  total_recordings: 361,
  total_transcripts: 299,
  total_audio_size_mb: 892.4,
  total_transcript_kb: 45.2,
};

describe('StatsBar', () => {
  it('renders all stat values and labels', () => {
    const { container } = render(<StatsBar stats={stats} />);

    const values = container.querySelectorAll('.stat-value');
    const labels = container.querySelectorAll('.stat-label');

    expect(values).toHaveLength(3);
    expect(values[0].textContent).toBe('361');
    expect(values[1].textContent).toBe('299');
    expect(values[2].textContent).toBe('892.4');

    expect(labels).toHaveLength(3);
    expect(labels[0].textContent).toBe('recordings');
    expect(labels[1].textContent).toBe('transcripts');
    expect(labels[2].textContent).toBe('MB audio');

    cleanup();
  });
});
