use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use axum::Json;
use axum::body::Body;
use axum::extract::Path as AxumPath;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::error::GatewayError;

// ---------------------------------------------------------------------------
// Directory paths (relative to gateway binary working dir)
// ---------------------------------------------------------------------------

fn recordings_dir() -> &'static str {
    static DIR: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        if let Ok(d) = std::env::var("RECORDINGS_DIR") { return d; }
        if std::path::Path::new("recordings").is_dir() { return "recordings".into(); }
        "../recordings".into()
    })
}

fn transcripts_dir() -> &'static str {
    static DIR: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        if let Ok(d) = std::env::var("TRANSCRIPTS_DIR") { return d; }
        if std::path::Path::new("transcripts").is_dir() { return "transcripts".into(); }
        "../transcripts".into()
    })
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct Recording {
    pub filename: String,
    pub path: String,
    pub size: i64,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Transcript {
    pub filename: String,
    pub path: String,
    pub size: i64,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TranscriptUpdate {
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct Stats {
    pub total_recordings: usize,
    pub total_transcripts: usize,
    pub total_audio_size_mb: f64,
    pub total_transcript_kb: f64,
}

#[derive(Debug, Serialize)]
pub struct LinkedEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording: Option<Recording>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<Transcript>,
}

#[derive(Debug, Deserialize)]
pub struct FilenameParam {
    pub filename: String,
}

// ---------------------------------------------------------------------------
// Timestamp parsing
// ---------------------------------------------------------------------------

/// Extract a timestamp from filenames like "20260105_211652_audio.wav".
///
/// Splits on `_`, takes the first two parts, concatenates them, and parses as
/// "YYYYMMDDHHMMSS". Returns an ISO 8601 string on success, or an empty string
/// if the filename doesn't match the expected pattern.
fn parse_timestamp(filename: &str) -> String {
    let parts: Vec<&str> = filename.split('_').collect();
    if parts.len() < 2 {
        return String::new();
    }

    // Strip any non-digit suffix from the second part (e.g. "211652.wav" -> "211652")
    let time_part: String = parts[1].chars().take_while(|c| c.is_ascii_digit()).collect();
    let date_str = format!("{}{}", parts[0], time_part);

    // Try 14-digit format first: YYYYMMDDHHMMSS
    if date_str.len() == 14 {
        if let Some(ts) = parse_datetime_digits(&date_str) {
            return ts;
        }
    }

    // Try 13-digit format: YYYYMMDDHHMMS (Go's "2006010215405" quirk)
    if date_str.len() == 13 {
        if let Some(ts) = parse_datetime_digits(&format!("{}0", date_str)) {
            return ts;
        }
    }

    String::new()
}

/// Parse a 14-character digit string "YYYYMMDDHHMMSS" into "YYYY-MM-DDTHH:MM:SS".
/// Returns `None` if the string contains non-digits or has out-of-range values.
fn parse_datetime_digits(s: &str) -> Option<String> {
    if s.len() != 14 || !s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    let year: u32 = s[0..4].parse().ok()?;
    let month: u32 = s[4..6].parse().ok()?;
    let day: u32 = s[6..8].parse().ok()?;
    let hour: u32 = s[8..10].parse().ok()?;
    let min: u32 = s[10..12].parse().ok()?;
    let sec: u32 = s[12..14].parse().ok()?;

    // Basic range validation
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || min > 59
        || sec > 59
    {
        return None;
    }

    Some(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        year, month, day, hour, min, sec
    ))
}

/// Extract a "YYYYMMDD_HHMM" key from a filename for linking recordings to transcripts.
/// The key drops seconds so recordings and transcripts from the same minute match.
fn timestamp_key(filename: &str) -> String {
    let parts: Vec<&str> = filename.split('_').collect();
    if parts.len() >= 2 && parts[0].len() == 8 && parts[1].len() >= 4 {
        format!("{}_{}", parts[0], &parts[1][..4])
    } else {
        String::new()
    }
}

// ---------------------------------------------------------------------------
// Filename sanitization helper
// ---------------------------------------------------------------------------

/// Sanitize a user-provided filename to prevent directory traversal.
/// Returns the bare file name component, or `None` if the result is empty.
fn sanitize_filename(raw: &str) -> Option<String> {
    Path::new(raw)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .map(String::from)
}

// ---------------------------------------------------------------------------
// Directory scanning helpers
// ---------------------------------------------------------------------------

fn is_audio_file(name: &str) -> bool {
    name.ends_with(".wav") || name.ends_with(".mp3")
}

fn is_transcript_file(name: &str) -> bool {
    name.ends_with(".txt")
}

/// Read directory entries, filtering by the given predicate on filename.
/// Returns (filename, full_path, size) tuples.
async fn scan_dir(
    dir: &Path,
    filter: fn(&str) -> bool,
) -> Result<Vec<(String, PathBuf, i64)>, GatewayError> {
    let mut reader = tokio::fs::read_dir(dir).await.map_err(|e| {
        tracing::error!(dir = %dir.display(), error = %e, "failed to read directory");
        GatewayError::Internal(format!("failed to read directory: {}", dir.display()))
    })?;

    let mut results = Vec::new();
    while let Some(entry) = reader.next_entry().await? {
        let file_type = entry.file_type().await?;
        if file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if filter(&name) {
            let metadata = entry.metadata().await?;
            let path = entry.path();
            results.push((name, path, metadata.len() as i64));
        }
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/recordings — list all .wav/.mp3 files, sorted newest first.
pub async fn list_recordings() -> Result<Json<Vec<Recording>>, GatewayError> {
    let dir = Path::new(recordings_dir());
    let entries = scan_dir(dir, is_audio_file).await?;

    let mut recordings: Vec<Recording> = entries
        .into_iter()
        .map(|(name, path, size)| {
            let timestamp = parse_timestamp(&name);
            Recording {
                filename: name,
                path: path.to_string_lossy().into_owned(),
                size,
                timestamp,
                duration: None,
            }
        })
        .collect();

    // Sort newest first (reverse lexicographic on ISO timestamp string)
    recordings.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    Ok(Json(recordings))
}

/// GET /api/transcripts — list all .txt files, sorted newest first.
pub async fn list_transcripts() -> Result<Json<Vec<Transcript>>, GatewayError> {
    let dir = Path::new(transcripts_dir());
    let entries = scan_dir(dir, is_transcript_file).await?;

    let mut transcripts: Vec<Transcript> = entries
        .into_iter()
        .map(|(name, path, size)| {
            let timestamp = parse_timestamp(&name);
            Transcript {
                filename: name,
                path: path.to_string_lossy().into_owned(),
                size,
                timestamp,
                text: None,
                recording_id: None,
            }
        })
        .collect();

    transcripts.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    Ok(Json(transcripts))
}

/// GET /api/transcript/:filename — return a single transcript with its text content.
pub async fn get_transcript(
    AxumPath(raw_filename): AxumPath<String>,
) -> Result<Json<Transcript>, GatewayError> {
    let filename = sanitize_filename(&raw_filename)
        .ok_or_else(|| GatewayError::BadRequest("invalid filename".into()))?;

    let path = Path::new(transcripts_dir()).join(&filename);

    let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            GatewayError::NotFound("transcript not found".into())
        } else {
            GatewayError::Internal("failed to read transcript".into())
        }
    })?;

    let metadata = tokio::fs::metadata(&path).await?;

    Ok(Json(Transcript {
        filename: filename.clone(),
        path: path.to_string_lossy().into_owned(),
        size: metadata.len() as i64,
        timestamp: parse_timestamp(&filename),
        text: Some(content),
        recording_id: None,
    }))
}

/// PUT /api/transcript/:filename — update a transcript's text on disk.
pub async fn update_transcript(
    AxumPath(raw_filename): AxumPath<String>,
    Json(body): Json<TranscriptUpdate>,
) -> Result<impl IntoResponse, GatewayError> {
    let filename = sanitize_filename(&raw_filename)
        .ok_or_else(|| GatewayError::BadRequest("invalid filename".into()))?;

    let path = Path::new(transcripts_dir()).join(&filename);

    // Verify the file exists before writing
    if !path.exists() {
        return Err(GatewayError::NotFound("transcript not found".into()));
    }

    tokio::fs::write(&path, body.text.as_bytes()).await.map_err(|e| {
        tracing::error!(path = %path.display(), error = %e, "failed to write transcript");
        GatewayError::Internal("failed to save transcript".into())
    })?;

    Ok(Json(serde_json::json!({ "status": "saved" })))
}

/// GET /api/audio/:filename — serve an audio file with correct Content-Type and
/// HTTP range request support for seeking in the browser.
pub async fn serve_audio(
    AxumPath(raw_filename): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Response, GatewayError> {
    let filename = sanitize_filename(&raw_filename)
        .ok_or_else(|| GatewayError::BadRequest("invalid filename".into()))?;

    let path = Path::new(recordings_dir()).join(&filename);

    let metadata = tokio::fs::metadata(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            GatewayError::NotFound("audio file not found".into())
        } else {
            GatewayError::Internal("failed to open audio file".into())
        }
    })?;

    let file_size = metadata.len();
    let content_type = if filename.ends_with(".mp3") {
        "audio/mpeg"
    } else {
        "audio/wav"
    };

    // Parse Range header for partial content support
    if let Some(range_value) = headers.get(header::RANGE) {
        if let Ok(range_str) = range_value.to_str() {
            if let Some((start, end)) = parse_range_header(range_str, file_size) {
                let length = end - start + 1;

                let mut file = tokio::fs::File::open(&path).await?;
                file.seek(std::io::SeekFrom::Start(start)).await?;
                let reader = tokio::io::BufReader::new(file.take(length));
                let stream = tokio_util::io::ReaderStream::new(reader);

                return Ok(Response::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(header::CONTENT_TYPE, content_type)
                    .header(header::CONTENT_LENGTH, length.to_string())
                    .header(
                        header::CONTENT_RANGE,
                        format!("bytes {}-{}/{}", start, end, file_size),
                    )
                    .header(header::ACCEPT_RANGES, "bytes")
                    .body(Body::from_stream(stream))
                    .unwrap());
            }
        }
    }

    // Full file response
    let file = tokio::fs::File::open(&path).await?;
    let reader = tokio::io::BufReader::new(file);
    let stream = tokio_util::io::ReaderStream::new(reader);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, file_size.to_string())
        .header(header::ACCEPT_RANGES, "bytes")
        .body(Body::from_stream(stream))
        .unwrap())
}

/// GET /api/stats — count recordings and transcripts, sum their sizes.
pub async fn stats() -> Result<Json<Stats>, GatewayError> {
    let rec_dir = Path::new(recordings_dir());
    let tx_dir = Path::new(transcripts_dir());

    let mut total_recordings = 0usize;
    let mut total_audio_bytes = 0u64;
    let mut total_transcripts = 0usize;
    let mut total_transcript_bytes = 0u64;

    // Scan recordings
    if let Ok(entries) = scan_dir(rec_dir, is_audio_file).await {
        for (_name, _path, size) in &entries {
            total_recordings += 1;
            total_audio_bytes += *size as u64;
        }
    }

    // Scan transcripts
    if let Ok(entries) = scan_dir(tx_dir, is_transcript_file).await {
        for (_name, _path, size) in &entries {
            total_transcripts += 1;
            total_transcript_bytes += *size as u64;
        }
    }

    Ok(Json(Stats {
        total_recordings,
        total_transcripts,
        total_audio_size_mb: total_audio_bytes as f64 / (1024.0 * 1024.0),
        total_transcript_kb: total_transcript_bytes as f64 / 1024.0,
    }))
}

/// GET /api/linked — match recordings and transcripts by timestamp prefix
/// (YYYYMMDD_HHMM), returning linked pairs sorted newest first.
pub async fn list_linked() -> Result<Json<Vec<LinkedEntry>>, GatewayError> {
    let rec_dir = Path::new(recordings_dir());
    let tx_dir = Path::new(transcripts_dir());

    // Build recordings map keyed by minute-resolution timestamp
    let mut recordings: BTreeMap<String, Recording> = BTreeMap::new();
    if let Ok(entries) = scan_dir(rec_dir, is_audio_file).await {
        for (name, path, size) in entries {
            let key = timestamp_key(&name);
            if !key.is_empty() {
                recordings.insert(
                    key,
                    Recording {
                        timestamp: parse_timestamp(&name),
                        filename: name,
                        path: path.to_string_lossy().into_owned(),
                        size,
                        duration: None,
                    },
                );
            }
        }
    }

    // Build transcripts map, reading text content for each
    let mut transcripts: BTreeMap<String, Transcript> = BTreeMap::new();
    if let Ok(entries) = scan_dir(tx_dir, is_transcript_file).await {
        for (name, path, size) in entries {
            let key = timestamp_key(&name);
            if !key.is_empty() {
                let text = tokio::fs::read_to_string(&path).await.unwrap_or_default();
                transcripts.insert(
                    key,
                    Transcript {
                        timestamp: parse_timestamp(&name),
                        filename: name,
                        path: path.to_string_lossy().into_owned(),
                        size,
                        text: Some(text),
                        recording_id: None,
                    },
                );
            }
        }
    }

    // Collect all keys
    let mut all_keys = BTreeSet::new();
    all_keys.extend(recordings.keys().cloned());
    all_keys.extend(transcripts.keys().cloned());

    // Merge into linked entries
    let mut entries: Vec<LinkedEntry> = all_keys
        .into_iter()
        .map(|key| LinkedEntry {
            recording: recordings.remove(&key),
            transcript: transcripts.remove(&key),
        })
        .collect();

    // Sort newest first: pick the timestamp from whichever side is present
    entries.sort_by(|a, b| {
        let ts_a = a
            .recording
            .as_ref()
            .map(|r| &r.timestamp)
            .or_else(|| a.transcript.as_ref().map(|t| &t.timestamp))
            .cloned()
            .unwrap_or_default();
        let ts_b = b
            .recording
            .as_ref()
            .map(|r| &r.timestamp)
            .or_else(|| b.transcript.as_ref().map(|t| &t.timestamp))
            .cloned()
            .unwrap_or_default();
        ts_b.cmp(&ts_a)
    });

    Ok(Json(entries))
}

// ---------------------------------------------------------------------------
// Range header parsing
// ---------------------------------------------------------------------------

/// Parse an HTTP Range header value like "bytes=0-1023" into (start, end) inclusive.
/// Only supports a single byte range. Returns `None` for malformed values.
fn parse_range_header(range: &str, file_size: u64) -> Option<(u64, u64)> {
    let range = range.strip_prefix("bytes=")?;
    let mut parts = range.splitn(2, '-');
    let start_str = parts.next()?.trim();
    let end_str = parts.next()?.trim();

    if start_str.is_empty() {
        // Suffix range: "bytes=-500" means last 500 bytes
        let suffix_len: u64 = end_str.parse().ok()?;
        let start = file_size.saturating_sub(suffix_len);
        Some((start, file_size - 1))
    } else {
        let start: u64 = start_str.parse().ok()?;
        let end = if end_str.is_empty() {
            file_size - 1
        } else {
            end_str.parse().ok()?
        };
        if start > end || start >= file_size {
            None
        } else {
            Some((start, end.min(file_size - 1)))
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_timestamp_standard() {
        let ts = parse_timestamp("20260105_211652_audio.wav");
        assert_eq!(ts, "2026-01-05T21:16:52");
    }

    #[test]
    fn test_parse_timestamp_short_name() {
        let ts = parse_timestamp("20260105_211652.wav");
        assert_eq!(ts, "2026-01-05T21:16:52");
    }

    #[test]
    fn test_parse_timestamp_invalid() {
        let ts = parse_timestamp("readme.txt");
        assert_eq!(ts, "");
    }

    #[test]
    fn test_parse_timestamp_empty() {
        let ts = parse_timestamp("");
        assert_eq!(ts, "");
    }

    #[test]
    fn test_timestamp_key() {
        let key = timestamp_key("20260105_211652_audio.wav");
        assert_eq!(key, "20260105_2116");
    }

    #[test]
    fn test_timestamp_key_invalid() {
        let key = timestamp_key("readme.txt");
        assert_eq!(key, "");
    }

    #[test]
    fn test_sanitize_filename_traversal() {
        assert_eq!(
            sanitize_filename("../../etc/passwd"),
            Some("passwd".into())
        );
    }

    #[test]
    fn test_sanitize_filename_normal() {
        assert_eq!(
            sanitize_filename("20260105_211652_audio.wav"),
            Some("20260105_211652_audio.wav".into())
        );
    }

    #[test]
    fn test_sanitize_filename_empty() {
        assert_eq!(sanitize_filename(""), None);
    }

    #[test]
    fn test_parse_range_full() {
        assert_eq!(parse_range_header("bytes=0-499", 1000), Some((0, 499)));
    }

    #[test]
    fn test_parse_range_open_end() {
        assert_eq!(parse_range_header("bytes=500-", 1000), Some((500, 999)));
    }

    #[test]
    fn test_parse_range_suffix() {
        assert_eq!(parse_range_header("bytes=-200", 1000), Some((800, 999)));
    }

    #[test]
    fn test_parse_range_out_of_bounds() {
        assert_eq!(parse_range_header("bytes=1000-2000", 1000), None);
    }

    #[test]
    fn test_parse_range_clamp_end() {
        assert_eq!(
            parse_range_header("bytes=900-2000", 1000),
            Some((900, 999))
        );
    }

    #[test]
    fn test_parse_range_invalid() {
        assert_eq!(parse_range_header("invalid", 1000), None);
    }
}
