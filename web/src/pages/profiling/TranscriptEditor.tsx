import { useState, useEffect } from 'react';
import type { SessionSummary } from '../../lib/types';

interface TranscriptEditorProps {
  session: SessionSummary;
}

export function TranscriptEditor({ session }: TranscriptEditorProps) {
  const [text, setText] = useState(session.transcript);
  const [editing, setEditing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    setText(session.transcript);
    setEditing(false);
    setSaved(false);
  }, [session]);

  const handleSave = async () => {
    setSaving(true);
    // Save correction as training data — POST to a future endpoint
    // For now, this demonstrates the UI pattern
    await new Promise((resolve) => setTimeout(resolve, 300));
    setSaving(false);
    setEditing(false);
    setSaved(true);
  };

  if (!session.transcript) {
    return (
      <div className="transcript-editor-section">
        <h3>Transcript</h3>
        <div className="no-transcript-msg">No transcript for this session</div>
      </div>
    );
  }

  return (
    <div className="transcript-editor-section">
      <div className="transcript-editor-header">
        <h3>Transcript</h3>
        <span className={`backend-badge backend-${session.backend}`}>{session.backend}</span>
      </div>
      {editing ? (
        <>
          <textarea
            className="transcript-edit-area"
            value={text}
            onChange={(e) => setText(e.target.value)}
            autoFocus
          />
          <div className="transcript-edit-actions">
            <button className="cancel-btn" onClick={() => { setText(session.transcript); setEditing(false); }}>
              Cancel
            </button>
            <button className="save-btn" onClick={handleSave} disabled={saving}>
              {saving ? 'Saving...' : 'Save Correction'}
            </button>
          </div>
        </>
      ) : (
        <>
          <div className="transcript-display">{session.transcript}</div>
          <button className="edit-btn" onClick={() => setEditing(true)}>
            Edit
          </button>
          {saved && <span className="saved-indicator">Saved</span>}
        </>
      )}
    </div>
  );
}
