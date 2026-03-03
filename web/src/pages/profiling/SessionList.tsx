import type { SessionSummary } from '../../lib/types';
import { formatMs } from '../../lib/utils';

interface SessionListProps {
  sessions: SessionSummary[];
  selectedId: string | null;
  onSelect: (session: SessionSummary) => void;
  statusFilter: string;
  onStatusFilterChange: (status: string) => void;
  backendFilter: string;
  onBackendFilterChange: (backend: string) => void;
}

function formatSessionTime(started: string): string {
  const d = new Date(started);
  if (isNaN(d.getTime())) return started;
  return d.toLocaleDateString('en-US', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    hour12: true,
  });
}

function truncate(text: string, max: number): string {
  if (text.length <= max) return text;
  return text.slice(0, max).trimEnd() + '…';
}

export function SessionList({
  sessions,
  selectedId,
  onSelect,
  statusFilter,
  onStatusFilterChange,
  backendFilter,
  onBackendFilterChange,
}: SessionListProps) {
  const filtered = sessions.filter((s) => {
    if (statusFilter && statusFilter !== 'all' && s.status !== statusFilter) return false;
    if (backendFilter && backendFilter !== 'all' && s.backend !== backendFilter) return false;
    return true;
  });

  return (
    <aside className="session-list">
      <div className="session-list-header">
        <h2>Sessions</h2>
        <span className="count">{filtered.length}</span>
      </div>
      <div className="session-filters">
        <select value={statusFilter} onChange={(e) => onStatusFilterChange(e.target.value)}>
          <option value="all">All status</option>
          <option value="OK">OK</option>
          <option value="FAILED">Failed</option>
          <option value="SILENCE">Silence</option>
          <option value="ABORTED">Aborted</option>
        </select>
        <select value={backendFilter} onChange={(e) => onBackendFilterChange(e.target.value)}>
          <option value="all">All backends</option>
          <option value="deepgram">Deepgram</option>
          <option value="local-whisper">Local Whisper</option>
          <option value="none">None</option>
        </select>
      </div>
      <div className="session-scroll">
        {filtered.length === 0 ? (
          <div className="empty-list"><p>No sessions match filters</p></div>
        ) : (
          filtered.map((session) => (
            <div
              key={session.filename}
              className={`session-card ${selectedId === session.filename ? 'selected' : ''} ${session.status === 'OK' ? 'status-ok' : session.status === 'FAILED' ? 'status-fail' : 'status-other'}`}
              onClick={() => onSelect(session)}
            >
              <div className="session-card-top">
                <span className="session-time">{formatSessionTime(session.started)}</span>
                <span className={`session-status ${session.status.toLowerCase()}`}>
                  {session.status}
                </span>
              </div>
              <div className="session-card-id">{session.session_id}</div>
              {session.transcript && (
                <div className="session-card-transcript">
                  {truncate(session.transcript, 72)}
                </div>
              )}
              <div className="session-card-bottom">
                <span className="session-duration">{formatMs(session.total_ms)}</span>
                <span className="session-card-sep">·</span>
                <span className={`session-backend backend-${session.backend}`}>
                  {session.backend}
                </span>
                {session.transcript && (
                  <>
                    <span className="session-card-sep">·</span>
                    <span className="session-chars">{session.transcript.length} ch</span>
                  </>
                )}
              </div>
            </div>
          ))
        )}
      </div>
    </aside>
  );
}
