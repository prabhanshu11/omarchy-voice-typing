import { useState } from 'react';
import type { SessionSummary } from '../../lib/types';
import { useSessionData } from '../../lib/hooks/useSessionData';
import { useProfilingSummary } from '../../lib/hooks/useProfilingSummary';
import { formatMs } from '../../lib/utils';
import { SessionList } from './SessionList';
import { SessionDetail } from './SessionDetail';
import './ProfilingPage.css';

export function ProfilingPage() {
  const [selectedSession, setSelectedSession] = useState<SessionSummary | null>(null);
  const [statusFilter, setStatusFilter] = useState('all');
  const [backendFilter, setBackendFilter] = useState('all');

  const { sessions, loading, error } = useSessionData(undefined, undefined, 500);
  const { summary } = useProfilingSummary();

  const handleSelect = (session: SessionSummary) => {
    setSelectedSession(session);
  };

  return (
    <main className="profiling-main">
      {/* KPI summary bar */}
      {summary && (
        <div className="profiling-kpis">
          <div className="kpi">
            <span className="kpi-value">{summary.total_sessions}</span>
            <span className="kpi-label">Sessions</span>
          </div>
          <div className="kpi">
            <span className="kpi-value">{(summary.success_rate * 100).toFixed(1)}%</span>
            <span className="kpi-label">Success Rate</span>
          </div>
          <div className="kpi">
            <span className="kpi-value">{summary.total_commit_ms_p50 != null ? formatMs(summary.total_commit_ms_p50) : 'N/A'}</span>
            <span className="kpi-label">P50</span>
          </div>
          <div className="kpi">
            <span className="kpi-value">{summary.total_commit_ms_p95 != null ? formatMs(summary.total_commit_ms_p95) : 'N/A'}</span>
            <span className="kpi-label">P95</span>
          </div>
          {summary.by_backend.map((b) => (
            <div key={b.backend} className="kpi">
              <span className="kpi-value">{b.count}</span>
              <span className="kpi-label">{b.backend}</span>
            </div>
          ))}
        </div>
      )}

      <div className="profiling-content">
        {loading ? (
          <div className="loading">
            <div className="spinner"></div>
            <p>Loading sessions...</p>
          </div>
        ) : error ? (
          <div className="error">
            <p>{error}</p>
          </div>
        ) : (
          <>
            <SessionList
              sessions={sessions}
              selectedId={selectedSession?.filename ?? null}
              onSelect={handleSelect}
              statusFilter={statusFilter}
              onStatusFilterChange={setStatusFilter}
              backendFilter={backendFilter}
              onBackendFilterChange={setBackendFilter}
            />
            <SessionDetail session={selectedSession} />
          </>
        )}
      </div>
    </main>
  );
}
