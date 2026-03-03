import type { SessionSummary, TimelineEvent } from '../../../lib/types';
import type { Participant as ParticipantType, DiagramArrow } from './types';
import { Participant } from './Participant';
import { Arrow } from './Arrow';
import { TimeMarker } from './TimeMarker';

interface SequenceDiagramProps {
  session: SessionSummary;
}

const COLORS = {
  hyprwhspr: '#9277ff',
  gateway: '#7c5dfa',
  deepgram: '#ff8f00',
  whisper: '#33d69f',
  recording: '#33d69f',
};

const LEFT_MARGIN = 55;
const COL_GAP = 160;
const TOP_OFFSET = 50;
const ROW_HEIGHT = 40;

interface ActivationBar {
  participantId: string;
  startRow: number;
  endRow: number;
  color: string;
  label: string;
}

function buildParticipants(backend: string): ParticipantType[] {
  const participants: ParticipantType[] = [
    { id: 'hyprwhspr', label: 'hyprwhspr', color: COLORS.hyprwhspr },
    { id: 'gateway', label: 'Gateway', color: COLORS.gateway },
  ];

  if (backend === 'local-whisper' || backend === 'none') {
    participants.push({ id: 'whisper', label: 'Local Whisper', color: COLORS.whisper });
  } else {
    participants.push({ id: 'deepgram', label: 'Deepgram', color: COLORS.deepgram });
  }

  return participants;
}

interface MappedResult {
  arrows: DiagramArrow[];
  activations: ActivationBar[];
}

function mapEventsToArrows(events: TimelineEvent[], backend: string): MappedResult {
  const arrows: DiagramArrow[] = [];
  const activations: ActivationBar[] = [];
  const transcriptionTarget = backend === 'local-whisper' || backend === 'none' ? 'whisper' : 'deepgram';

  let commitRowIndex = -1;
  let firstAudioRowIndex = -1;
  let connectedRowIndex = -1;
  let commitCompleteRowIndex = -1;
  let rowCounter = 0;

  for (const ev of events) {
    const lower = ev.event.toLowerCase();

    // Session started
    if (lower.includes('started')) {
      const offlineMatch = ev.event.match(/offline=(true|false)/);
      const mode = offlineMatch?.[1] === 'true' ? 'offline' : 'online';
      arrows.push({
        fromId: 'gateway',
        toId: 'gateway',
        label: `session start (${mode})`,
        timestamp: ev.elapsed_s,
      });
      rowCounter++;
      continue;
    }

    // First audio chunk: hyprwhspr → gateway
    if (lower.includes('first audio chunk')) {
      firstAudioRowIndex = rowCounter;
      const bytesMatch = ev.event.match(/(\d+)\s*bytes/);
      arrows.push({
        fromId: 'hyprwhspr',
        toId: 'gateway',
        label: `audio (${bytesMatch ? bytesMatch[1] + 'B' : ''})`,
        timestamp: ev.elapsed_s,
      });
      rowCounter++;
      continue;
    }

    // Deepgram connected: response comes FROM deepgram back to gateway
    if (ev.layer === 'DEEPGRAM' && lower.includes('connected in')) {
      connectedRowIndex = rowCounter;
      const msMatch = ev.event.match(/([\d.]+)s/);
      const ms = msMatch ? Math.round(parseFloat(msMatch[1]) * 1000) : undefined;
      arrows.push({
        fromId: 'deepgram',
        toId: 'gateway',
        label: `connected (${ms ?? '?'}ms)`,
        timestamp: ev.elapsed_s,
        durationMs: ms,
        style: 'dashed',
      });
      rowCounter++;
      continue;
    }

    // Deepgram connect failed
    if (ev.layer === 'DEEPGRAM' && lower.includes('connect failed')) {
      arrows.push({
        fromId: 'deepgram',
        toId: 'gateway',
        label: 'connect FAILED',
        timestamp: ev.elapsed_s,
        style: 'dashed',
      });
      rowCounter++;
      continue;
    }

    // Silence detected
    if (lower.includes('silence detected')) {
      arrows.push({
        fromId: 'gateway',
        toId: 'gateway',
        label: 'silence detected — skipped',
        timestamp: ev.elapsed_s,
      });
      rowCounter++;
      continue;
    }

    // Commit received (user stopped recording)
    if (lower.includes('commit received')) {
      commitRowIndex = rowCounter;
      arrows.push({
        fromId: 'hyprwhspr',
        toId: 'gateway',
        label: 'stop recording (commit)',
        timestamp: ev.elapsed_s,
      });
      rowCounter++;
      continue;
    }

    // Taking online/offline path
    if (lower.includes('taking online') || lower.includes('taking offline')) {
      arrows.push({
        fromId: 'gateway',
        toId: transcriptionTarget,
        label: lower.includes('online') ? 'finalize (online)' : 'transcribe (offline)',
        timestamp: ev.elapsed_s,
      });
      rowCounter++;
      continue;
    }

    // Commit complete
    if (lower.includes('commit complete')) {
      commitCompleteRowIndex = rowCounter;
      const charsMatch = ev.event.match(/transcript=(\d+)\s*chars/);
      arrows.push({
        fromId: transcriptionTarget,
        toId: 'gateway',
        label: `transcript (${charsMatch ? charsMatch[1] : '?'} chars)`,
        timestamp: ev.elapsed_s,
        style: 'dashed',
      });
      rowCounter++;

      arrows.push({
        fromId: 'gateway',
        toId: 'hyprwhspr',
        label: 'result → typing',
        timestamp: ev.elapsed_s + 0.01,
      });
      rowCounter++;
      continue;
    }

    // WebSocket closed
    if (lower.includes('websocket closed') || lower.includes('session ended')) {
      arrows.push({
        fromId: 'gateway',
        toId: 'gateway',
        label: 'session ended',
        timestamp: ev.elapsed_s,
      });
      rowCounter++;
      continue;
    }
  }

  // Activation bars
  // Recording bar on hyprwhspr: from first audio to commit
  if (firstAudioRowIndex >= 0 && commitRowIndex >= 0) {
    activations.push({
      participantId: 'hyprwhspr',
      startRow: firstAudioRowIndex,
      endRow: commitRowIndex,
      color: COLORS.recording,
      label: 'Recording',
    });
  }

  // Gateway active: relaying audio from first chunk to commit
  if (firstAudioRowIndex >= 0 && commitRowIndex >= 0) {
    activations.push({
      participantId: 'gateway',
      startRow: firstAudioRowIndex,
      endRow: commitRowIndex,
      color: COLORS.gateway,
      label: '',
    });
  }

  // Deepgram active: receiving audio from connection until transcript returned
  if (connectedRowIndex >= 0 && commitCompleteRowIndex >= 0 && backend === 'deepgram') {
    activations.push({
      participantId: 'deepgram',
      startRow: connectedRowIndex,
      endRow: commitCompleteRowIndex,
      color: COLORS.deepgram,
      label: '',
    });
  }

  return { arrows, activations };
}

export function SequenceDiagram({ session }: SequenceDiagramProps) {
  const participants = buildParticipants(session.backend);
  const { arrows, activations } = mapEventsToArrows(session.timeline, session.backend);

  if (arrows.length === 0) {
    return null;
  }

  const colX: Record<string, number> = {};
  participants.forEach((p, i) => {
    colX[p.id] = LEFT_MARGIN + i * COL_GAP + COL_GAP / 2;
  });

  const diagramHeight = TOP_OFFSET + arrows.length * ROW_HEIGHT + 30;
  const svgWidth = LEFT_MARGIN + participants.length * COL_GAP + 20;

  return (
    <div className="sequence-diagram-section">
      <h3>Sequence Diagram</h3>
      <div className="sequence-diagram-container">
        <svg
          width={svgWidth}
          height={diagramHeight}
          viewBox={`0 0 ${svgWidth} ${diagramHeight}`}
          className="sequence-diagram-svg"
        >
          {/* Participants */}
          {participants.map((p) => (
            <Participant
              key={p.id}
              x={colX[p.id]}
              label={p.label}
              color={p.color}
              height={diagramHeight}
            />
          ))}

          {/* Activation bars (behind arrows) */}
          {activations.map((act, i) => {
            const x = colX[act.participantId];
            if (x === undefined) return null;
            const yStart = TOP_OFFSET + act.startRow * ROW_HEIGHT - 8;
            const yEnd = TOP_OFFSET + act.endRow * ROW_HEIGHT + 8;
            const barWidth = 12;

            return (
              <g key={`act-${i}`}>
                <rect
                  x={x - barWidth / 2}
                  y={yStart}
                  width={barWidth}
                  height={yEnd - yStart}
                  rx={3}
                  fill={act.color}
                  fillOpacity={0.25}
                  stroke={act.color}
                  strokeWidth={1.5}
                  strokeOpacity={0.6}
                />
                {act.label && (
                  <text
                    x={x + barWidth / 2 + 5}
                    y={(yStart + yEnd) / 2 + 3}
                    fill={act.color}
                    fontSize={9}
                    fontWeight={600}
                    fontFamily="'Inter', sans-serif"
                  >
                    {act.label}
                  </text>
                )}
              </g>
            );
          })}

          {/* Arrows */}
          {arrows.map((arrow, i) => {
            const y = TOP_OFFSET + i * ROW_HEIGHT;
            const fromX = colX[arrow.fromId];
            const toX = colX[arrow.toId];

            if (fromX === undefined || toX === undefined) return null;

            // Self-arrows (notes)
            if (arrow.fromId === arrow.toId) {
              return (
                <g key={i}>
                  <TimeMarker y={y} label={`T+${arrow.timestamp.toFixed(1)}s`} />
                  <text
                    x={fromX + 14}
                    y={y + 3}
                    fill="#9898a0"
                    fontSize={10}
                    fontFamily="'Inter', sans-serif"
                    fontStyle="italic"
                  >
                    {arrow.label}
                  </text>
                </g>
              );
            }

            const sourceP = participants.find((p) => p.id === arrow.fromId);
            const color = sourceP?.color ?? '#9898a0';

            return (
              <g key={i}>
                <TimeMarker y={y} label={`T+${arrow.timestamp.toFixed(1)}s`} />
                <Arrow
                  x1={fromX}
                  x2={toX}
                  y={y}
                  label={arrow.label}
                  color={color}
                  dashed={arrow.style === 'dashed'}
                />
              </g>
            );
          })}
        </svg>
      </div>
    </div>
  );
}
