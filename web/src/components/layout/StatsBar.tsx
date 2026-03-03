import type { Stats } from '../../lib/types';

interface StatsBarProps {
  stats: Stats;
}

export function StatsBar({ stats }: StatsBarProps) {
  return (
    <div className="stats-bar">
      <div className="stat">
        <span className="stat-value">{stats.total_recordings}</span>
        <span className="stat-label">recordings</span>
      </div>
      <div className="stat">
        <span className="stat-value">{stats.total_transcripts}</span>
        <span className="stat-label">transcripts</span>
      </div>
      <div className="stat">
        <span className="stat-value">{stats.total_audio_size_mb.toFixed(1)}</span>
        <span className="stat-label">MB audio</span>
      </div>
    </div>
  );
}
