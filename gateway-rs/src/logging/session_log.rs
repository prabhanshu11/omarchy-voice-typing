use std::fmt::Write as FmtWrite;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Timestamped event in a recording session timeline.
#[derive(Debug)]
pub struct SessionEvent {
    pub elapsed: Duration,
    pub layer: String,
    pub event: String,
}

/// Accumulates the full timeline for a single recording session.
/// Written to disk as a human-readable log file + compact JSON for waybar tray.
#[derive(Debug)]
pub struct SessionLog {
    pub id: String,
    pub start_instant: Instant,
    pub start_time: SystemTime,
    pub end_time: Option<SystemTime>,
    pub events: Vec<SessionEvent>,

    // Speed profiling (milliseconds, -1 = never measured)
    pub dg_connect_ms: i64,
    pub first_audio_ms: i64,
    pub transcribe_ms: i64,
    pub total_ms: i64,

    // Audio chain counters
    pub py_chunks: u32,
    pub gw_bytes: u64,

    // Disposition
    pub status: String,
    pub transcript: String,
    pub wav_path: String,
}

impl SessionLog {
    pub fn new(id: String) -> Self {
        Self {
            id,
            start_instant: Instant::now(),
            start_time: SystemTime::now(),
            end_time: None,
            events: Vec::new(),
            dg_connect_ms: -1,
            first_audio_ms: -1,
            transcribe_ms: -1,
            total_ms: -1,
            py_chunks: 0,
            gw_bytes: 0,
            status: String::new(),
            transcript: String::new(),
            wav_path: String::new(),
        }
    }

    pub fn add_event(&mut self, layer: &str, event: &str) {
        self.events.push(SessionEvent {
            elapsed: self.start_instant.elapsed(),
            layer: layer.to_string(),
            event: event.to_string(),
        });
    }

    /// Write human-readable session log to disk (async-safe: call from spawn_blocking or tokio::spawn).
    pub fn write_to_file(&self) {
        let dir = session_log_dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::error!(error = %e, "Failed to create session log dir");
            return;
        }

        let ts = format_system_time(&self.start_time);
        let filename = format!("{}_{}.log", ts, self.id);
        let path = dir.join(filename);

        let content = self.format_log();
        match std::fs::write(&path, content) {
            Ok(()) => tracing::info!(path = %path.display(), "Written session log"),
            Err(e) => tracing::error!(error = %e, path = %path.display(), "Failed to write session log"),
        }
    }

    /// Write compact JSON summary for waybar tray tooltip.
    /// Uses atomic tmp+rename to avoid partial reads.
    pub fn write_last_session(&self, backend: &str) {
        let path = last_session_path();

        let preview = if self.transcript.len() > 60 {
            format!("{}...", &self.transcript[..60])
        } else {
            self.transcript.clone()
        };

        let duration = self
            .end_time
            .and_then(|end| end.duration_since(self.start_time).ok())
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);

        let unix_secs = self
            .start_time
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let data = serde_json::json!({
            "id": self.id,
            "timestamp_unix": unix_secs,
            "timestamp_iso": format_system_time_iso(&self.start_time),
            "duration_s": duration,
            "status": self.status,
            "backend": backend,
            "transcript_preview": preview,
            "transcript_chars": self.transcript.len(),
            "dg_connect_ms": self.dg_connect_ms,
            "first_audio_ms": self.first_audio_ms,
            "transcribe_ms": self.transcribe_ms,
            "total_ms": self.total_ms,
            "py_chunks": self.py_chunks,
            "gw_bytes": self.gw_bytes,
        });

        let json_bytes = match serde_json::to_vec(&data) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialize last_session JSON");
                return;
            }
        };

        let tmp_path = format!("{}.tmp", path.display());
        if let Err(e) = std::fs::write(&tmp_path, &json_bytes) {
            tracing::error!(error = %e, "Failed to write {tmp_path}");
            return;
        }
        if let Err(e) = std::fs::rename(&tmp_path, &path) {
            tracing::error!(error = %e, "Failed to rename tmp → last_session.json");
            return;
        }
        tracing::info!(id = %self.id, status = %self.status, "Written last_session.json");
    }

    fn format_log(&self) -> String {
        let mut b = String::with_capacity(2048);

        // Header
        let _ = writeln!(b, "=== SESSION {} ===", self.id);
        let _ = writeln!(
            b,
            "Started:     {}",
            format_system_time_readable(&self.start_time)
        );
        if let Some(end) = self.end_time {
            let note = if self.status == "ABORTED" {
                " (ABORTED by clear)"
            } else {
                ""
            };
            let _ = writeln!(b, "Ended:       {}{note}", format_system_time_readable(&end));
            if let Ok(dur) = end.duration_since(self.start_time) {
                let _ = writeln!(b, "Duration:    {:.1}s", dur.as_secs_f64());
            }
        }
        let _ = writeln!(b);

        // Timeline
        let _ = writeln!(b, "--- TIMELINE ---");
        for e in &self.events {
            let _ = writeln!(
                b,
                "T+{:.3}s     [{:<12}] {}",
                e.elapsed.as_secs_f64(),
                e.layer,
                e.event
            );
        }
        let _ = writeln!(b);

        // Speed profile
        let _ = writeln!(b, "--- SPEED PROFILE ---");
        let _ = writeln!(
            b,
            "Deepgram connect:     {}",
            if self.dg_connect_ms >= 0 {
                format!("{}ms", self.dg_connect_ms)
            } else {
                "N/A".into()
            }
        );
        let _ = writeln!(
            b,
            "First audio to GW:    {}",
            if self.first_audio_ms >= 0 {
                format!("{}ms", self.first_audio_ms)
            } else {
                "NEVER".into()
            }
        );
        let _ = writeln!(
            b,
            "Transcription:        {}",
            if self.transcribe_ms >= 0 {
                format!("{}ms", self.transcribe_ms)
            } else {
                "N/A".into()
            }
        );
        let _ = writeln!(
            b,
            "Total (start→text):   {}",
            if self.total_ms >= 0 {
                format!("{}ms", self.total_ms)
            } else {
                "N/A".into()
            }
        );
        let _ = writeln!(b);

        // Audio chain
        let _ = writeln!(b, "--- AUDIO CHAIN ---");
        let _ = writeln!(b, "Python WS chunks:     {}", self.py_chunks);
        let _ = writeln!(b, "Gateway bytes:        {}", self.gw_bytes);
        let _ = writeln!(b);

        // Data flow diagram
        let _ = writeln!(b, "--- DATA FLOW ---");
        let stages = [
            ("Toggle", true),
            ("Python WS", self.py_chunks > 0),
            ("Gateway", self.gw_bytes > 0),
            ("Transcribe", !self.transcript.is_empty()),
        ];
        let names: Vec<String> = stages.iter().map(|(n, _)| format!("{n:<12}")).collect();
        let checks: Vec<String> = stages
            .iter()
            .map(|(_, ok)| {
                if *ok {
                    format!("{:<12}", "OK")
                } else {
                    format!("{:<12}", "FAIL")
                }
            })
            .collect();
        let _ = writeln!(b, "{}", names.join(" -> "));
        let _ = writeln!(b, "{}", checks.join("    "));
        let _ = writeln!(b);

        // Disposition
        let _ = writeln!(b, "--- DISPOSITION ---");
        let _ = writeln!(b, "Status:       {}", self.status);
        if !self.wav_path.is_empty() {
            let _ = writeln!(b, "Archived WAV: {}", self.wav_path);
        } else {
            let _ = writeln!(b, "Archived WAV: (none)");
        }
        if !self.transcript.is_empty() {
            let preview = if self.transcript.len() > 80 {
                format!("{}...", &self.transcript[..80])
            } else {
                self.transcript.clone()
            };
            let _ = writeln!(
                b,
                "Transcript:   {:?} ({} chars)",
                preview,
                self.transcript.len()
            );
        } else {
            let _ = writeln!(b, "Transcript:   (none)");
        }

        b
    }
}

/// Get session log directory path.
fn session_log_dir() -> PathBuf {
    if let Some(home) = dirs_or_home() {
        home.join("Programs/omarchy-voice-typing/logs/sessions")
    } else {
        PathBuf::from("../logs/sessions")
    }
}

/// Get last_session.json path.
fn last_session_path() -> PathBuf {
    if let Some(home) = dirs_or_home() {
        home.join(".config/hyprwhspr/last_session.json")
    } else {
        PathBuf::from("../last_session.json")
    }
}

fn dirs_or_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

/// Format SystemTime as "YYYYMMDD_HHMMSS" (for filenames).
fn format_system_time(time: &SystemTime) -> String {
    let secs = time.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let (y, m, d, h, min, s) = crate::audio::timestamp_parts(secs);
    format!("{y:04}{m:02}{d:02}_{h:02}{min:02}{s:02}")
}

/// Format SystemTime as "YYYY-MM-DD HH:MM:SS.mmm" (for log readability).
fn format_system_time_readable(time: &SystemTime) -> String {
    let dur = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();
    let (y, m, d, h, min, s) = crate::audio::timestamp_parts(secs);
    format!("{y:04}-{m:02}-{d:02} {h:02}:{min:02}:{s:02}.{millis:03}")
}

/// Format SystemTime as ISO 8601 (for JSON).
fn format_system_time_iso(time: &SystemTime) -> String {
    let secs = time.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let (y, m, d, h, min, s) = crate::audio::timestamp_parts(secs);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:{s:02}Z")
}
