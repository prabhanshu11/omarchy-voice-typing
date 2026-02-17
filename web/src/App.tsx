import { useState, useEffect, useRef } from 'react';
import type { LinkedEntry, Stats } from './types';
import { fetchLinked, fetchStats, updateTranscript, getAudioUrl, transcribeAudio } from './api';
import './App.css';

function formatBytes(bytes: number): string {
  if (bytes < 1024) return bytes + ' B';
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
  return (bytes / (1024 * 1024)).toFixed(2) + ' MB';
}

function formatDate(timestamp: string): string {
  if (!timestamp || timestamp === '0001-01-01T00:00:00Z') return 'Unknown';
  const date = new Date(timestamp);
  return date.toLocaleDateString('en-US', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function formatDuration(seconds: number): string {
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins}:${secs.toString().padStart(2, '0')}`;
}

interface EntryCardProps {
  entry: LinkedEntry;
  isSelected: boolean;
  onSelect: () => void;
}

function EntryCard({ entry, isSelected, onSelect }: EntryCardProps) {
  const hasRecording = !!entry.recording;
  const hasTranscript = !!entry.transcript;
  const timestamp = entry.recording?.timestamp || entry.transcript?.timestamp;
  const preview = entry.transcript?.text?.slice(0, 100) || '';

  return (
    <div
      className={`entry-card ${isSelected ? 'selected' : ''} ${!hasRecording ? 'no-audio' : ''} ${!hasTranscript ? 'no-transcript' : ''}`}
      onClick={onSelect}
    >
      <div className="entry-header">
        <span className="entry-time">{formatDate(timestamp || '')}</span>
        <div className="entry-badges">
          {hasRecording && (
            <span className="badge audio" title={formatBytes(entry.recording!.size)}>
              audio
            </span>
          )}
          {hasTranscript && (
            <span className="badge text" title={formatBytes(entry.transcript!.size)}>
              text
            </span>
          )}
          {!hasTranscript && hasRecording && (
            <span className="badge missing" title="No transcript">
              pending
            </span>
          )}
        </div>
      </div>
      {preview && <p className="entry-preview">{preview}{entry.transcript?.text && entry.transcript.text.length > 100 ? '...' : ''}</p>}
      {!preview && hasRecording && (
        <p className="entry-preview empty">No transcript - {entry.recording?.filename}</p>
      )}
    </div>
  );
}

interface DetailPanelProps {
  entry: LinkedEntry | null;
  onTranscriptSaved: () => void;
}

function DetailPanel({ entry, onTranscriptSaved }: DetailPanelProps) {
  const [editedText, setEditedText] = useState('');
  const [isEditing, setIsEditing] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [isTranscribing, setIsTranscribing] = useState(false);
  const [transcribeError, setTranscribeError] = useState<string | null>(null);
  const [audioDuration, setAudioDuration] = useState<number | null>(null);
  const audioRef = useRef<HTMLAudioElement>(null);

  useEffect(() => {
    if (entry?.transcript?.text) {
      setEditedText(entry.transcript.text);
    } else {
      setEditedText('');
    }
    setIsEditing(false);
    setAudioDuration(null);
    setTranscribeError(null);
  }, [entry]);

  const handleSave = async () => {
    if (!entry?.transcript?.filename) return;
    setIsSaving(true);
    try {
      await updateTranscript(entry.transcript.filename, editedText);
      setIsEditing(false);
      onTranscriptSaved();
    } catch (error) {
      console.error('Failed to save:', error);
    }
    setIsSaving(false);
  };

  const handleLoadedMetadata = () => {
    if (audioRef.current) {
      setAudioDuration(audioRef.current.duration);
    }
  };

  const handleTranscribe = async () => {
    if (!entry?.recording?.filename) return;
    setIsTranscribing(true);
    setTranscribeError(null);
    try {
      await transcribeAudio(entry.recording.filename);
      onTranscriptSaved(); // Refresh the list
    } catch (error) {
      console.error('Transcription failed:', error);
      setTranscribeError(error instanceof Error ? error.message : 'Transcription failed');
    }
    setIsTranscribing(false);
  };

  if (!entry) {
    return (
      <div className="detail-panel empty">
        <div className="empty-state">
          <div className="empty-icon">mic</div>
          <h3>Select a recording</h3>
          <p>Choose an entry from the list to view details and play audio</p>
        </div>
      </div>
    );
  }

  return (
    <div className="detail-panel">
      <div className="detail-header">
        <h2>{formatDate(entry.recording?.timestamp || entry.transcript?.timestamp || '')}</h2>
        <div className="detail-meta">
          {entry.recording && <span>{formatBytes(entry.recording.size)}</span>}
          {audioDuration && <span>{formatDuration(audioDuration)}</span>}
        </div>
      </div>

      {entry.recording && (
        <div className="audio-section">
          <audio
            ref={audioRef}
            controls
            src={getAudioUrl(entry.recording.filename)}
            onLoadedMetadata={handleLoadedMetadata}
          />
          <div className="audio-filename">{entry.recording.filename}</div>
        </div>
      )}

      <div className="transcript-section">
        <div className="transcript-header">
          <h3>Transcript</h3>
          {entry.transcript && !isEditing && (
            <button className="edit-btn" onClick={() => setIsEditing(true)}>
              Edit
            </button>
          )}
          {isEditing && (
            <div className="edit-actions">
              <button className="cancel-btn" onClick={() => {
                setEditedText(entry.transcript?.text || '');
                setIsEditing(false);
              }}>
                Cancel
              </button>
              <button className="save-btn" onClick={handleSave} disabled={isSaving}>
                {isSaving ? 'Saving...' : 'Save'}
              </button>
            </div>
          )}
        </div>

        {entry.transcript ? (
          isEditing ? (
            <textarea
              className="transcript-editor"
              value={editedText}
              onChange={(e) => setEditedText(e.target.value)}
              autoFocus
            />
          ) : (
            <div className="transcript-text">
              {entry.transcript.text || <em className="no-text">Empty transcript</em>}
            </div>
          )
        ) : entry.recording ? (
          <div className="no-transcript">
            <p>No transcript available for this recording</p>
            <button
              className="transcribe-btn"
              onClick={handleTranscribe}
              disabled={isTranscribing}
            >
              {isTranscribing ? 'Transcribing...' : 'Transcribe'}
            </button>
            {transcribeError && (
              <p className="transcribe-error">{transcribeError}</p>
            )}
          </div>
        ) : (
          <div className="no-transcript">
            <p>No transcript or recording available</p>
          </div>
        )}

        {entry.transcript && (
          <div className="transcript-filename">{entry.transcript.filename}</div>
        )}
      </div>
    </div>
  );
}

function App() {
  const [entries, setEntries] = useState<LinkedEntry[]>([]);
  const [stats, setStats] = useState<Stats | null>(null);
  const [selectedEntry, setSelectedEntry] = useState<LinkedEntry | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadData = async () => {
    try {
      const [linkedData, statsData] = await Promise.all([
        fetchLinked(),
        fetchStats(),
      ]);
      setEntries(linkedData || []);
      setStats(statsData);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load data');
    }
    setLoading(false);
  };

  useEffect(() => {
    loadData();
  }, []);

  const handleTranscriptSaved = () => {
    loadData();
  };

  return (
    <div className="app">
      <header className="app-header">
        <div className="logo">
          <span className="logo-icon">mic</span>
          <h1>Voice Gateway</h1>
        </div>
        {stats && (
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
        )}
      </header>

      <main className="app-main">
        {loading ? (
          <div className="loading">
            <div className="spinner"></div>
            <p>Loading recordings...</p>
          </div>
        ) : error ? (
          <div className="error">
            <p>{error}</p>
            <button onClick={loadData}>Retry</button>
          </div>
        ) : (
          <>
            <aside className="entries-list">
              <div className="list-header">
                <h2>Recordings</h2>
                <span className="count">{entries.length}</span>
              </div>
              <div className="entries-scroll">
                {entries.length === 0 ? (
                  <div className="empty-list">
                    <p>No recordings yet</p>
                  </div>
                ) : (
                  entries.map((entry, idx) => (
                    <EntryCard
                      key={entry.recording?.filename || entry.transcript?.filename || idx}
                      entry={entry}
                      isSelected={selectedEntry === entry}
                      onSelect={() => setSelectedEntry(entry)}
                    />
                  ))
                )}
              </div>
            </aside>
            <DetailPanel entry={selectedEntry} onTranscriptSaved={handleTranscriptSaved} />
          </>
        )}
      </main>
    </div>
  );
}

export default App;
