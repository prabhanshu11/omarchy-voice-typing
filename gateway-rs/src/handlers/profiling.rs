use std::collections::HashMap;
use std::path::{Path, PathBuf};

use axum::Json;
use axum::extract::Query;
use serde::{Deserialize, Serialize};

use crate::error::GatewayError;

// ---------------------------------------------------------------------------
// Query parameters
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DateRangeQuery {
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SessionsQuery {
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<usize>,
}

// ---------------------------------------------------------------------------
// Response types — latency
// ---------------------------------------------------------------------------

/// A single latency record deserialized from JSONL.
/// Uses Option for fields that may be absent (skip_serializing_if in writer).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyRecord {
    pub request_id: String,
    pub timestamp_unix: u64,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub recording_number: Option<u32>,
    #[serde(default)]
    pub deepgram_connect_ms: Option<i64>,
    #[serde(default)]
    pub transcription_ms: Option<i64>,
    #[serde(default)]
    pub total_commit_ms: Option<i64>,
    #[serde(default)]
    pub upload_ms: Option<i64>,
    #[serde(default)]
    pub total_latency_ms: Option<i64>,
    #[serde(default)]
    pub end_to_end_ms: Option<i64>,
    #[serde(default)]
    pub audio_duration_s: f64,
    #[serde(default)]
    pub audio_bytes: Option<usize>,
    #[serde(default)]
    pub backend: String,
    #[serde(default)]
    pub transcript_id: String,
    #[serde(default)]
    pub transcript_length: usize,
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub error_message: String,
}

// ---------------------------------------------------------------------------
// Response types — session summary
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub filename: String,
    pub session_id: String,
    pub started: String,
    pub duration_s: f64,
    pub status: String,
    pub backend: String,
    pub transcript: String,
    pub dg_connect_ms: i64,
    pub first_audio_ms: i64,
    pub transcribe_ms: i64,
    pub total_ms: i64,
    pub py_chunks: u32,
    pub gw_bytes: u64,
    pub timeline: Vec<TimelineEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wav_filename: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimelineEvent {
    pub elapsed_s: f64,
    pub layer: String,
    pub event: String,
}

// ---------------------------------------------------------------------------
// Response types — aggregated summary
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ProfilingSummary {
    pub total_sessions: usize,
    pub success_count: usize,
    pub failure_count: usize,
    pub success_rate: f64,
    pub total_commit_ms_p50: Option<f64>,
    pub total_commit_ms_p95: Option<f64>,
    pub total_commit_ms_p99: Option<f64>,
    pub transcription_ms_p50: Option<f64>,
    pub transcription_ms_p95: Option<f64>,
    pub transcription_ms_p99: Option<f64>,
    pub by_backend: Vec<BackendStats>,
    pub by_date: Vec<DailyCount>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackendStats {
    pub backend: String,
    pub count: usize,
    pub avg_total_ms: f64,
    pub p50_total_ms: f64,
    pub p95_total_ms: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DailyCount {
    pub date: String,
    pub total: usize,
    pub success: usize,
    pub failure: usize,
}

// ---------------------------------------------------------------------------
// Directory resolution
// ---------------------------------------------------------------------------

fn latency_log_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join("Programs/omarchy-voice-typing/logs/latency")
    } else {
        PathBuf::from("../logs/latency")
    }
}

fn session_log_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join("Programs/omarchy-voice-typing/logs/sessions")
    } else {
        PathBuf::from("../logs/sessions")
    }
}

// ---------------------------------------------------------------------------
// Date filtering helpers
// ---------------------------------------------------------------------------

/// Convert unix timestamp to "YYYY-MM-DD" for date comparison.
fn unix_to_date(ts: u64) -> String {
    let (y, m, d, _, _, _) = crate::audio::timestamp_parts(ts);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Check if a date string falls within the optional [from, to] range (inclusive).
fn date_in_range(date: &str, from: Option<&str>, to: Option<&str>) -> bool {
    if let Some(f) = from {
        if date < f {
            return false;
        }
    }
    if let Some(t) = to {
        if date > t {
            return false;
        }
    }
    true
}

/// Extract date from session log filename like "20260226_190639_rec-003.log" → "2026-02-26".
fn session_filename_date(filename: &str) -> Option<String> {
    let date_part = filename.get(0..8)?;
    if date_part.len() != 8 || !date_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!(
        "{}-{}-{}",
        &date_part[0..4],
        &date_part[4..6],
        &date_part[6..8]
    ))
}

// ---------------------------------------------------------------------------
// JSONL parsing
// ---------------------------------------------------------------------------

/// Read all latency JSONL files from the log dir, returning parsed records.
fn read_latency_records(dir: &Path) -> Result<Vec<LatencyRecord>, GatewayError> {
    let mut records = Vec::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(records),
        Err(e) => return Err(GatewayError::Internal(format!("read latency dir: {e}"))),
    };

    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("latency_") || !name.ends_with(".jsonl") {
            continue;
        }

        let content = std::fs::read_to_string(entry.path()).map_err(|e| {
            GatewayError::Internal(format!("read {name}: {e}"))
        })?;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<LatencyRecord>(line) {
                Ok(rec) => records.push(rec),
                Err(e) => {
                    tracing::warn!(file = %name, error = %e, "skipping malformed latency line");
                }
            }
        }
    }

    Ok(records)
}

// ---------------------------------------------------------------------------
// Session log parsing
// ---------------------------------------------------------------------------

/// Parse a human-readable session log file into a SessionSummary.
pub fn parse_session_log(filename: &str, content: &str) -> Option<SessionSummary> {
    let mut session_id = String::new();
    let mut started = String::new();
    let mut duration_s: f64 = 0.0;
    let mut status = String::new();
    let mut transcript = String::new();
    let mut backend = String::new();
    let mut dg_connect_ms: i64 = -1;
    let mut first_audio_ms: i64 = -1;
    let mut transcribe_ms: i64 = -1;
    let mut total_ms: i64 = -1;
    let mut py_chunks: u32 = 0;
    let mut gw_bytes: u64 = 0;
    let mut timeline = Vec::new();
    let mut wav_filename: Option<String> = None;

    let mut in_timeline = false;
    let mut in_speed = false;
    let mut in_audio_chain = false;
    let mut in_disposition = false;

    for line in content.lines() {
        let line = line.trim();

        // Section headers
        if line.starts_with("=== SESSION ") {
            session_id = line
                .strip_prefix("=== SESSION ")
                .and_then(|s| s.strip_suffix(" ==="))
                .unwrap_or("")
                .to_string();
            in_timeline = false;
            in_speed = false;
            in_audio_chain = false;
            in_disposition = false;
            continue;
        }

        if line == "--- TIMELINE ---" {
            in_timeline = true;
            in_speed = false;
            in_audio_chain = false;
            in_disposition = false;
            continue;
        }
        if line == "--- SPEED PROFILE ---" {
            in_timeline = false;
            in_speed = true;
            in_audio_chain = false;
            in_disposition = false;
            continue;
        }
        if line == "--- AUDIO CHAIN ---" {
            in_timeline = false;
            in_speed = false;
            in_audio_chain = true;
            in_disposition = false;
            continue;
        }
        if line == "--- DATA FLOW ---" || line == "--- DISPOSITION ---" {
            in_timeline = false;
            in_speed = false;
            in_audio_chain = false;
            in_disposition = line == "--- DISPOSITION ---";
            continue;
        }

        // Header fields
        if let Some(val) = line.strip_prefix("Started:") {
            started = val.trim().to_string();
            continue;
        }
        if let Some(val) = line.strip_prefix("Duration:") {
            let val = val.trim();
            if let Some(secs_str) = val.strip_suffix('s') {
                duration_s = secs_str.trim().parse().unwrap_or(0.0);
            }
            continue;
        }

        // Timeline entries: T+0.000s     [GATEWAY     ] event text
        if in_timeline && line.starts_with("T+") {
            if let Some(event) = parse_timeline_line(line) {
                timeline.push(event);
            }
            continue;
        }

        // Speed profile
        if in_speed {
            if let Some(val) = line.strip_prefix("Deepgram connect:") {
                dg_connect_ms = parse_ms_value(val.trim());
            } else if let Some(val) = line.strip_prefix("First audio to GW:") {
                first_audio_ms = parse_ms_value(val.trim());
            } else if let Some(val) = line.strip_prefix("Transcription:") {
                transcribe_ms = parse_ms_value(val.trim());
            } else if let Some(val) = line.strip_prefix("Total (start") {
                // "Total (start→text):   6213ms"
                if let Some(colon_rest) = val.split_once(':') {
                    total_ms = parse_ms_value(colon_rest.1.trim());
                }
            }
            continue;
        }

        // Audio chain
        if in_audio_chain {
            if let Some(val) = line.strip_prefix("Python WS chunks:") {
                py_chunks = val.trim().parse().unwrap_or(0);
            } else if let Some(val) = line.strip_prefix("Gateway bytes:") {
                gw_bytes = val.trim().parse().unwrap_or(0);
            }
            continue;
        }

        // Disposition
        if in_disposition {
            if let Some(val) = line.strip_prefix("Status:") {
                status = val.trim().to_string();
            } else if let Some(val) = line.strip_prefix("Archived WAV:") {
                let v = val.trim();
                if v != "(none)" {
                    wav_filename = Some(v.to_string());
                }
            } else if let Some(val) = line.strip_prefix("Transcript:") {
                transcript = parse_transcript_value(val.trim());
            }
            continue;
        }
    }

    // Determine backend from timeline events
    for ev in &timeline {
        let event_lower = ev.event.to_lowercase();
        if event_lower.contains("backend=deepgram") || event_lower.contains("online") && event_lower.contains("deepgram") {
            backend = "deepgram".to_string();
            break;
        }
        if event_lower.contains("backend=local-whisper") || event_lower.contains("offline") {
            backend = "local-whisper".to_string();
            // Don't break — deepgram might appear later
        }
        if event_lower.contains("backend=none") {
            backend = "none".to_string();
        }
    }

    // Fallback: if backend still empty, check dg_connect_ms
    if backend.is_empty() {
        backend = if dg_connect_ms >= 0 {
            "deepgram".to_string()
        } else {
            "local-whisper".to_string()
        };
    }

    if session_id.is_empty() {
        return None;
    }

    Some(SessionSummary {
        filename: filename.to_string(),
        session_id,
        started,
        duration_s,
        status,
        backend,
        transcript,
        dg_connect_ms,
        first_audio_ms,
        transcribe_ms,
        total_ms,
        py_chunks,
        gw_bytes,
        timeline,
        wav_filename,
    })
}

/// Parse a timeline line like "T+1.707s     [DEEPGRAM    ] connected in 1.706842828s"
fn parse_timeline_line(line: &str) -> Option<TimelineEvent> {
    // Extract elapsed: between "T+" and "s"
    let after_t = line.strip_prefix("T+")?;
    let s_pos = after_t.find('s')?;
    let elapsed_s: f64 = after_t[..s_pos].parse().ok()?;

    // Extract layer: between "[" and "]"
    let bracket_start = line.find('[')?;
    let bracket_end = line.find(']')?;
    let layer = line[bracket_start + 1..bracket_end].trim().to_string();

    // Extract event: after "] "
    let event = line[bracket_end + 1..].trim().to_string();

    Some(TimelineEvent {
        elapsed_s,
        layer,
        event,
    })
}

/// Parse a millisecond value like "1706ms" or "N/A" or "NEVER".
fn parse_ms_value(s: &str) -> i64 {
    if s == "N/A" || s == "NEVER" {
        return -1;
    }
    s.strip_suffix("ms")
        .and_then(|n| n.parse().ok())
        .unwrap_or(-1)
}

/// Parse transcript value from disposition section.
/// Handles: `"some text" (53 chars)` or `(none)`.
fn parse_transcript_value(s: &str) -> String {
    if s == "(none)" {
        return String::new();
    }
    // Strip surrounding quotes and trailing "(N chars)"
    let s = s.trim_start_matches('"');
    if let Some(quote_end) = s.rfind('"') {
        let text = &s[..quote_end];
        // Handle "text..." truncation
        text.strip_suffix("...").unwrap_or(text).to_string()
    } else {
        s.to_string()
    }
}

// ---------------------------------------------------------------------------
// Percentile calculation
// ---------------------------------------------------------------------------

/// Compute percentile from a sorted slice of f64 values.
/// Uses nearest-rank method.
pub fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = (p / 100.0 * sorted.len() as f64).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted.len() - 1);
    Some(sorted[idx])
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/profiling/latency — return all latency records filtered by date range.
pub async fn latency(
    Query(params): Query<DateRangeQuery>,
) -> Result<Json<Vec<LatencyRecord>>, GatewayError> {
    let dir = latency_log_dir();
    let mut records = read_latency_records(&dir)?;

    // Filter by date range
    let from = params.from.as_deref();
    let to = params.to.as_deref();
    records.retain(|r| {
        let date = unix_to_date(r.timestamp_unix);
        date_in_range(&date, from, to)
    });

    // Sort by timestamp ascending
    records.sort_by_key(|r| r.timestamp_unix);

    Ok(Json(records))
}

/// GET /api/profiling/sessions — return parsed session logs.
pub async fn sessions(
    Query(params): Query<SessionsQuery>,
) -> Result<Json<Vec<SessionSummary>>, GatewayError> {
    let dir = session_log_dir();
    let limit = params.limit.unwrap_or(50);

    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Json(vec![])),
        Err(e) => return Err(GatewayError::Internal(format!("read sessions dir: {e}"))),
    };

    let from = params.from.as_deref();
    let to = params.to.as_deref();

    let mut filenames: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".log") {
                return None;
            }
            // Filter by date
            let date = session_filename_date(&name)?;
            if !date_in_range(&date, from, to) {
                return None;
            }
            Some(name)
        })
        .collect();

    // Sort newest first
    filenames.sort_unstable_by(|a, b| b.cmp(a));
    filenames.truncate(limit);

    let mut summaries = Vec::with_capacity(filenames.len());
    for name in filenames {
        let path = dir.join(&name);
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if let Some(summary) = parse_session_log(&name, &content) {
            summaries.push(summary);
        }
    }

    Ok(Json(summaries))
}

/// GET /api/profiling/summary — aggregate statistics.
pub async fn summary(
    Query(params): Query<DateRangeQuery>,
) -> Result<Json<ProfilingSummary>, GatewayError> {
    let dir = latency_log_dir();
    let mut records = read_latency_records(&dir)?;

    // Filter by date range
    let from = params.from.as_deref();
    let to = params.to.as_deref();
    records.retain(|r| {
        let date = unix_to_date(r.timestamp_unix);
        date_in_range(&date, from, to)
    });

    let total = records.len();
    let success_count = records.iter().filter(|r| r.success).count();
    let failure_count = total - success_count;
    let success_rate = if total > 0 {
        success_count as f64 / total as f64
    } else {
        0.0
    };

    // Percentiles for total_commit_ms
    let mut commit_values: Vec<f64> = records
        .iter()
        .filter_map(|r| r.total_commit_ms)
        .filter(|&v| v >= 0)
        .map(|v| v as f64)
        .collect();
    commit_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mut transcription_values: Vec<f64> = records
        .iter()
        .filter_map(|r| r.transcription_ms)
        .filter(|&v| v >= 0)
        .map(|v| v as f64)
        .collect();
    transcription_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // Group by backend
    let mut backend_map: HashMap<String, Vec<f64>> = HashMap::new();
    for r in &records {
        if let Some(ms) = r.total_commit_ms {
            if ms >= 0 {
                backend_map
                    .entry(if r.backend.is_empty() { "unknown".to_string() } else { r.backend.clone() })
                    .or_default()
                    .push(ms as f64);
            }
        }
    }

    let mut by_backend: Vec<BackendStats> = backend_map
        .into_iter()
        .map(|(backend, mut vals)| {
            vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let count = vals.len();
            let avg = vals.iter().sum::<f64>() / count as f64;
            BackendStats {
                backend,
                count,
                avg_total_ms: avg,
                p50_total_ms: percentile(&vals, 50.0).unwrap_or(0.0),
                p95_total_ms: percentile(&vals, 95.0).unwrap_or(0.0),
            }
        })
        .collect();
    by_backend.sort_by(|a, b| b.count.cmp(&a.count));

    // Group by date
    let mut date_map: HashMap<String, (usize, usize, usize)> = HashMap::new();
    for r in &records {
        let date = unix_to_date(r.timestamp_unix);
        let entry = date_map.entry(date).or_insert((0, 0, 0));
        entry.0 += 1;
        if r.success {
            entry.1 += 1;
        } else {
            entry.2 += 1;
        }
    }

    let mut by_date: Vec<DailyCount> = date_map
        .into_iter()
        .map(|(date, (total, success, failure))| DailyCount {
            date,
            total,
            success,
            failure,
        })
        .collect();
    by_date.sort_by(|a, b| a.date.cmp(&b.date));

    Ok(Json(ProfilingSummary {
        total_sessions: total,
        success_count,
        failure_count,
        success_rate,
        total_commit_ms_p50: percentile(&commit_values, 50.0),
        total_commit_ms_p95: percentile(&commit_values, 95.0),
        total_commit_ms_p99: percentile(&commit_values, 99.0),
        transcription_ms_p50: percentile(&transcription_values, 50.0),
        transcription_ms_p95: percentile(&transcription_values, 95.0),
        transcription_ms_p99: percentile(&transcription_values, 99.0),
        by_backend,
        by_date,
    }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_latency_record() {
        let json = r#"{"request_id":"123","timestamp_unix":1771548479,"session_id":"sess_1","recording_number":6,"deepgram_connect_ms":1562,"transcription_ms":1790,"total_commit_ms":6149,"audio_duration_s":4.352,"audio_bytes":208896,"backend":"deepgram","transcript_length":47,"success":true}"#;
        let rec: LatencyRecord = serde_json::from_str(json).unwrap();
        assert_eq!(rec.request_id, "123");
        assert_eq!(rec.timestamp_unix, 1771548479);
        assert_eq!(rec.deepgram_connect_ms, Some(1562));
        assert_eq!(rec.total_commit_ms, Some(6149));
        assert!(rec.success);
        assert_eq!(rec.backend, "deepgram");
    }

    #[test]
    fn test_parse_latency_record_minimal() {
        let json = r#"{"request_id":"x","timestamp_unix":100,"audio_duration_s":1.0,"transcript_length":0,"success":false}"#;
        let rec: LatencyRecord = serde_json::from_str(json).unwrap();
        assert_eq!(rec.deepgram_connect_ms, None);
        assert_eq!(rec.backend, "");
        assert!(!rec.success);
    }

    #[test]
    fn test_parse_session_log_deepgram() {
        let content = r#"=== SESSION rec-003 ===
Started:     2026-02-26 19:06:39.943
Ended:       2026-02-26 19:06:46.156
Duration:    6.2s

--- TIMELINE ---
T+0.000s     [GATEWAY     ] rec-003 started (offline=false)
T+0.000s     [GATEWAY     ] first audio chunk: 3072 bytes
T+1.707s     [DEEPGRAM    ] connected in 1.706842828s
T+4.286s     [GATEWAY     ] commit received
T+4.287s     [GATEWAY     ] taking ONLINE transcription path (Deepgram)
T+6.213s     [GATEWAY     ] commit complete: backend=deepgram, transcript=53 chars

--- SPEED PROFILE ---
Deepgram connect:     1706ms
First audio to GW:    0ms
Transcription:        1926ms
Total (start→text):   6213ms

--- AUDIO CHAIN ---
Python WS chunks:     66
Gateway bytes:        202752

--- DATA FLOW ---
Toggle       -> Python WS    -> Gateway      -> Transcribe
OK              OK              OK              OK

--- DISPOSITION ---
Status:       OK
Archived WAV: (none)
Transcript:   "It must be on one of the external drives at attached." (53 chars)"#;

        let summary = parse_session_log("20260226_190639_rec-003.log", content).unwrap();
        assert_eq!(summary.session_id, "rec-003");
        assert_eq!(summary.started, "2026-02-26 19:06:39.943");
        assert!((summary.duration_s - 6.2).abs() < 0.01);
        assert_eq!(summary.status, "OK");
        assert_eq!(summary.backend, "deepgram");
        assert_eq!(summary.dg_connect_ms, 1706);
        assert_eq!(summary.transcribe_ms, 1926);
        assert_eq!(summary.total_ms, 6213);
        assert_eq!(summary.py_chunks, 66);
        assert_eq!(summary.gw_bytes, 202752);
        assert_eq!(summary.timeline.len(), 6);
        assert_eq!(summary.timeline[2].layer, "DEEPGRAM");
        assert!(summary.transcript.contains("external drives"));
    }

    #[test]
    fn test_parse_session_log_offline() {
        let content = r#"=== SESSION rec-001 ===
Started:     2026-02-17 23:01:29.506
Ended:       2026-02-17 23:01:29.595
Duration:    0.1s

--- TIMELINE ---
T+0.000s     [GATEWAY     ] rec-001 started (offline=false)
T+0.000s     [GATEWAY     ] first audio chunk: 4800 bytes
T+0.040s     [GATEWAY     ] commit received
T+0.040s     [GATEWAY     ] taking OFFLINE transcription path
T+0.089s     [GATEWAY     ] commit complete: backend=local-whisper, transcript=21 chars

--- SPEED PROFILE ---
Deepgram connect:     N/A
First audio to GW:    0ms
Transcription:        49ms
Total (start→text):   89ms

--- AUDIO CHAIN ---
Python WS chunks:     3
Gateway bytes:        14400

--- DATA FLOW ---
Toggle       -> Python WS    -> Gateway      -> Transcribe
OK              OK              OK              OK

--- DISPOSITION ---
Status:       OK
Archived WAV: (none)
Transcript:   "local fallback worked" (21 chars)"#;

        let summary = parse_session_log("20260217_230129_rec-001.log", content).unwrap();
        assert_eq!(summary.session_id, "rec-001");
        assert_eq!(summary.backend, "local-whisper");
        assert_eq!(summary.dg_connect_ms, -1);
        assert_eq!(summary.transcribe_ms, 49);
        assert_eq!(summary.transcript, "local fallback worked");
    }

    #[test]
    fn test_parse_session_log_failed() {
        let content = r#"=== SESSION rec-001 ===
Started:     2026-02-17 23:07:09.162
Duration:    1.0s

--- TIMELINE ---
T+0.999s     [GATEWAY     ] commit received
T+0.999s     [GATEWAY     ] taking OFFLINE transcription path
T+1.041s     [GATEWAY     ] commit complete: backend=none, transcript=0 chars

--- SPEED PROFILE ---
Deepgram connect:     N/A
First audio to GW:    0ms
Transcription:        41ms
Total (start→text):   1041ms

--- AUDIO CHAIN ---
Python WS chunks:     5
Gateway bytes:        24000

--- DATA FLOW ---
Toggle       -> Python WS    -> Gateway      -> Transcribe
OK              OK              OK              FAIL

--- DISPOSITION ---
Status:       FAILED
Archived WAV: (none)
Transcript:   (none)"#;

        let summary = parse_session_log("test.log", content).unwrap();
        assert_eq!(summary.status, "FAILED");
        assert_eq!(summary.backend, "none");
        assert_eq!(summary.transcript, "");
    }

    #[test]
    fn test_parse_timeline_line() {
        let event = parse_timeline_line("T+1.707s     [DEEPGRAM    ] connected in 1.706842828s").unwrap();
        assert!((event.elapsed_s - 1.707).abs() < 0.001);
        assert_eq!(event.layer, "DEEPGRAM");
        assert!(event.event.contains("connected in"));
    }

    #[test]
    fn test_parse_ms_value() {
        assert_eq!(parse_ms_value("1706ms"), 1706);
        assert_eq!(parse_ms_value("N/A"), -1);
        assert_eq!(parse_ms_value("NEVER"), -1);
        assert_eq!(parse_ms_value("0ms"), 0);
    }

    #[test]
    fn test_parse_transcript_value() {
        assert_eq!(
            parse_transcript_value(r#""hello world" (11 chars)"#),
            "hello world"
        );
        assert_eq!(parse_transcript_value("(none)"), "");
        assert_eq!(
            parse_transcript_value(r#""some long text..." (100 chars)"#),
            "some long text"
        );
    }

    #[test]
    fn test_percentile() {
        let vals = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        assert_eq!(percentile(&vals, 50.0), Some(5.0));
        assert_eq!(percentile(&vals, 95.0), Some(10.0));
        assert_eq!(percentile(&vals, 99.0), Some(10.0));
        assert_eq!(percentile(&[], 50.0), None);
    }

    #[test]
    fn test_date_in_range() {
        assert!(date_in_range("2026-02-20", Some("2026-02-18"), Some("2026-02-22")));
        assert!(date_in_range("2026-02-18", Some("2026-02-18"), Some("2026-02-22")));
        assert!(!date_in_range("2026-02-17", Some("2026-02-18"), Some("2026-02-22")));
        assert!(!date_in_range("2026-02-23", Some("2026-02-18"), Some("2026-02-22")));
        assert!(date_in_range("2026-02-20", None, None));
    }

    #[test]
    fn test_session_filename_date() {
        assert_eq!(
            session_filename_date("20260226_190639_rec-003.log"),
            Some("2026-02-26".to_string())
        );
        assert_eq!(session_filename_date("invalid.log"), None);
    }
}
