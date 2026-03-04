import { useState, useEffect } from 'react';
import type { SessionSummary } from '../../lib/types';
import { getAudioUrl } from '../../lib/api';
import { formatMs } from '../../lib/utils';
import { TranscriptEditor } from './TranscriptEditor';
import { SequenceDiagram } from './sequence-diagram/SequenceDiagram';
import { TimelineWaterfall } from './TimelineWaterfall';

interface SessionDetailProps {
  session: SessionSummary | null;
}

export function SessionDetail({ session }: SessionDetailProps) {
  const [audioError, setAudioError] = useState(false);

  // Reset audio error state when session changes
  useEffect(() => {
    setAudioError(false);
  }, [session?.filename]);

  if (!session) {
    return (
      <div className="session-detail empty">
        <div className="empty-state">
          <div className="empty-icon">chart</div>
          <h3>Select a session</h3>
          <p>Choose a session from the list to view its profiling data</p>
        </div>
      </div>
    );
  }

  const audioFilename = session.wav_filename ?? null;

  return (
    <div className="session-detail">
      <div className="session-detail-header">
        <div>
          <h2>{session.session_id}</h2>
          <div className="session-meta">
            <span>{session.started}</span>
            <span>{formatMs(session.total_ms)} total</span>
            <span className={`session-status ${session.status.toLowerCase()}`}>{session.status}</span>
            <span className={`backend-badge backend-${session.backend}`}>{session.backend}</span>
          </div>
        </div>
      </div>

      {/* Audio player — only shown when wav_filename is known */}
      {audioFilename && !audioError ? (
        <div className="audio-section">
          <audio
            controls
            src={getAudioUrl(audioFilename)}
            onError={() => setAudioError(true)}
          />
        </div>
      ) : (
        <div className="audio-section audio-unavailable">
          <span>{audioFilename ? 'Audio file not found' : 'No audio linked to this session'}</span>
        </div>
      )}

      <TranscriptEditor session={session} />

      <SequenceDiagram session={session} />

      <TimelineWaterfall session={session} />
    </div>
  );
}
