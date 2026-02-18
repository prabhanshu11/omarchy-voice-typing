use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::audio::timestamp_parts;

/// Timing information for a single transcription request (batch or realtime).
///
/// Serialized as one JSON line per entry. Matches Go's `LatencyMetrics`.
#[derive(Debug, serde::Serialize)]
pub struct LatencyMetrics {
    pub request_id: String,
    pub timestamp_unix: u64,

    // Session context
    #[serde(skip_serializing_if = "String::is_empty")]
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording_number: Option<u32>,

    // Server-side timing (milliseconds, -1 = not measured)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deepgram_connect_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcription_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_commit_ms: Option<i64>,

    // Legacy aliases (batch endpoint compatibility)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_latency_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_to_end_ms: Option<i64>,

    // Metadata
    pub audio_duration_s: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_bytes: Option<usize>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub backend: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub transcript_id: String,
    pub transcript_length: usize,
    pub success: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error_message: String,
}

impl LatencyMetrics {
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            request_id: format!("{}", now.as_nanos()),
            timestamp_unix: now.as_secs(),
            session_id: String::new(),
            recording_number: None,
            deepgram_connect_ms: None,
            transcription_ms: None,
            total_commit_ms: None,
            upload_ms: None,
            total_latency_ms: None,
            end_to_end_ms: None,
            audio_duration_s: 0.0,
            audio_bytes: None,
            backend: String::new(),
            transcript_id: String::new(),
            transcript_length: 0,
            success: false,
            error_message: String::new(),
        }
    }
}

/// JSONL latency logger with daily file rotation.
///
/// Thread-safe via `std::sync::Mutex`. File writes are fast enough
/// that async isn't needed.
pub struct LatencyLogger {
    log_dir: PathBuf,
    inner: Mutex<LoggerInner>,
}

struct LoggerInner {
    file: Option<File>,
    current_date: String,
}

impl LatencyLogger {
    /// Create a new logger writing to the given directory.
    /// Creates the directory if it doesn't exist.
    pub fn new(log_dir: &Path) -> Result<Self, std::io::Error> {
        fs::create_dir_all(log_dir)?;

        let date = today_string();
        let path = log_dir.join(format!("latency_{date}.jsonl"));

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        tracing::info!(path = %path.display(), "Latency logging to");

        Ok(Self {
            log_dir: log_dir.to_path_buf(),
            inner: Mutex::new(LoggerInner {
                file: Some(file),
                current_date: date,
            }),
        })
    }

    /// Log a latency metrics entry as a JSONL line.
    /// Rotates to a new file if the date has changed.
    pub fn log(&self, metrics: &LatencyMetrics) {
        let mut inner = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => return,
        };

        let today = today_string();

        // Rotate if date changed
        if inner.current_date != today {
            if let Some(f) = inner.file.take() {
                drop(f);
            }

            let path = self.log_dir.join(format!("latency_{today}.jsonl"));
            match OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                Ok(f) => {
                    tracing::info!(path = %path.display(), "Rotated latency log");
                    inner.file = Some(f);
                    inner.current_date = today;
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to rotate latency log");
                    return;
                }
            }
        }

        let file = match &mut inner.file {
            Some(f) => f,
            None => return,
        };

        match serde_json::to_string(metrics) {
            Ok(json) => {
                let _ = writeln!(file, "{json}");
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialize latency metrics");
            }
        }
    }
}

/// Get today's date as "YYYY-MM-DD".
fn today_string() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, m, d, _, _, _) = timestamp_parts(secs);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latency_metrics_new() {
        let m = LatencyMetrics::new();
        assert!(!m.request_id.is_empty());
        assert!(m.timestamp_unix > 0);
        assert!(!m.success);
    }

    #[test]
    fn test_latency_metrics_serialization() {
        let mut m = LatencyMetrics::new();
        m.success = true;
        m.transcript_length = 42;
        m.total_commit_ms = Some(1500);
        m.backend = "deepgram".to_string();

        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"total_commit_ms\":1500"));
        assert!(json.contains("\"backend\":\"deepgram\""));
        // Empty strings should be skipped
        assert!(!json.contains("error_message"));
        // None options should be skipped
        assert!(!json.contains("upload_ms"));
    }

    #[test]
    fn test_today_string_format() {
        let s = today_string();
        // Should be YYYY-MM-DD format
        assert_eq!(s.len(), 10);
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
    }
}
