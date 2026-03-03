import type { SessionSummary } from '../../lib/types';
import { formatMs } from '../../lib/utils';

interface SpeedProfileProps {
  session: SessionSummary;
}

export function SpeedProfile({ session }: SpeedProfileProps) {
  const maxMs = Math.max(
    session.dg_connect_ms > 0 ? session.dg_connect_ms : 0,
    session.transcribe_ms > 0 ? session.transcribe_ms : 0,
    session.total_ms > 0 ? session.total_ms : 1,
  );

  const bars = [
    {
      label: 'DG connect',
      ms: session.dg_connect_ms,
      color: 'var(--warning)',
      show: session.dg_connect_ms > 0,
    },
    {
      label: 'Transcribe',
      ms: session.transcribe_ms,
      color: 'var(--success)',
      show: session.transcribe_ms > 0,
    },
    {
      label: 'Total',
      ms: session.total_ms,
      color: 'var(--accent-primary)',
      show: session.total_ms > 0,
    },
  ];

  return (
    <div className="speed-profile">
      <h3>Speed Profile</h3>
      <div className="speed-bars">
        {bars.filter((b) => b.show).map((bar) => (
          <div key={bar.label} className="speed-bar-row">
            <span className="speed-bar-label">{bar.label}</span>
            <div className="speed-bar-track">
              <div
                className="speed-bar-fill"
                style={{
                  width: `${Math.max((bar.ms / maxMs) * 100, 2)}%`,
                  backgroundColor: bar.color,
                }}
              />
            </div>
            <span className="speed-bar-value">{formatMs(bar.ms)}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
