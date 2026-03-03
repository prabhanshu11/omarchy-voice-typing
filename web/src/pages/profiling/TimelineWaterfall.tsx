import type { SessionSummary, TimelineEvent } from '../../lib/types';

interface TimelineWaterfallProps {
  session: SessionSummary;
}

interface Phase {
  label: string;
  startS: number;
  endS: number;
  color: string;
  detail?: string;
}

function derivePhases(events: TimelineEvent[], backend: string): Phase[] {
  const phases: Phase[] = [];

  let commitT: number | null = null;
  let connectedT: number | null = null;
  let transcribeEndT: number | null = null;
  let silenceDetected = false;
  let connectFailed = false;

  for (const ev of events) {
    const lower = ev.event.toLowerCase();
    if (lower.includes('commit received')) commitT = ev.elapsed_s;
    if (ev.layer === 'DEEPGRAM' && lower.includes('connected in')) connectedT = ev.elapsed_s;
    if (ev.layer === 'DEEPGRAM' && lower.includes('connect failed')) {
      connectFailed = true;
      connectedT = ev.elapsed_s;
    }
    if (lower.includes('commit complete')) transcribeEndT = ev.elapsed_s;
    if (lower.includes('silence detected')) silenceDetected = true;
  }

  // 1. User recording: how long the user was speaking
  if (commitT !== null && commitT > 0.01) {
    phases.push({
      label: 'User recording',
      startS: 0,
      endS: commitT,
      color: '#33d69f',
      detail: `${commitT.toFixed(1)}s`,
    });
  }

  // 2. DG connection: parallel setup during recording
  if (connectedT !== null && backend === 'deepgram') {
    phases.push({
      label: connectFailed ? 'DG connect (failed)' : 'DG connect',
      startS: 0,
      endS: connectedT,
      color: connectFailed ? '#f87171' : '#ff8f00',
      detail: connectFailed ? 'failed' : `${(connectedT * 1000).toFixed(0)}ms`,
    });
  }

  // 3. Finalize wait: time user waits AFTER stopping recording until result arrives
  if (commitT !== null && transcribeEndT !== null && transcribeEndT > commitT) {
    const waitS = transcribeEndT - commitT;
    phases.push({
      label: 'Finalize wait',
      startS: commitT,
      endS: transcribeEndT,
      color: '#9277ff',
      detail: `${(waitS * 1000).toFixed(0)}ms`,
    });
  }

  // Silence fallback
  if (silenceDetected && phases.length === 0) {
    phases.push({
      label: 'Silence detected',
      startS: 0,
      endS: commitT ?? 0.05,
      color: '#606068',
      detail: 'skipped',
    });
  }

  return phases;
}

export function TimelineWaterfall({ session }: TimelineWaterfallProps) {
  const phases = derivePhases(session.timeline, session.backend);

  if (phases.length === 0) {
    return null;
  }

  const maxS = Math.max(...phases.map((p) => p.endS), 0.1);

  return (
    <div className="timeline-waterfall-section">
      <h3>Timing Breakdown</h3>
      <div className="timeline-waterfall">
        {phases.map((phase, i) => {
          const leftPct = (phase.startS / maxS) * 100;
          const widthPct = Math.max(((phase.endS - phase.startS) / maxS) * 100, 1.5);

          return (
            <div key={i} className="timeline-row">
              <span className="timeline-label">{phase.label}</span>
              <div className="timeline-track">
                <div
                  className="timeline-bar"
                  style={{
                    left: `${leftPct}%`,
                    width: `${widthPct}%`,
                    backgroundColor: phase.color,
                  }}
                  title={`${phase.label}: ${phase.startS.toFixed(2)}s → ${phase.endS.toFixed(2)}s`}
                />
              </div>
              <span className="timeline-detail">{phase.detail}</span>
            </div>
          );
        })}
        {/* Time axis */}
        <div className="timeline-axis">
          <span className="timeline-label" />
          <div className="timeline-axis-track">
            {Array.from({ length: Math.min(Math.ceil(maxS) + 1, 12) }, (_, i) => {
              const pct = (i / maxS) * 100;
              if (pct > 100) return null;
              return (
                <span key={i} className="timeline-tick" style={{ left: `${pct}%` }}>
                  {i}s
                </span>
              );
            })}
          </div>
          <span className="timeline-detail" />
        </div>
      </div>
    </div>
  );
}
