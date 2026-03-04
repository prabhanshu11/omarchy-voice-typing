use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{FromRequest, Multipart, State};
use axum::http::{Request, header};

use crate::assemblyai::client::{Client as AssemblyAIClient, TranscriptResponse};
use crate::error::GatewayError;
use crate::logging::session_log::SessionLog;
use crate::secrets::load_from_pass;
use crate::state::AppState;

/// JSON body fallback when no multipart file is provided.
#[derive(serde::Deserialize)]
struct TranscribeRequest {
    audio_url: String,
}

/// Response returned to the caller on successful transcription.
#[derive(serde::Serialize)]
pub struct TranscribeResponse {
    pub text: String,
    pub raw: TranscriptResponse,
    pub duration_s: f64,
}

/// POST /v1/transcribe
///
/// Accepts either:
/// - Multipart form with a `file` field containing audio data
/// - JSON body with an `audio_url` field pointing to existing audio
///
/// On multipart upload:
/// 1. Reads the full audio into memory
/// 2. Saves it locally to `../recordings/`
/// 3. Uploads to AssemblyAI
/// 4. Creates a transcript job and polls until completion (2s interval, 300s timeout)
/// 5. Saves the transcript text to `../transcripts/`
///
/// The API key is loaded lazily from `pass` on the first request if not already
/// present in the shared state.
pub async fn transcribe(
    State(state): State<Arc<AppState>>,
    request: Request<axum::body::Body>,
) -> Result<Json<TranscribeResponse>, GatewayError> {
    // Assign a session ID from the webrec counter (file uploads always come from web).
    let seq = state.webrec_counter.fetch_add(1, Ordering::SeqCst) + 1;
    let rec_id = format!("webrec-{seq:03}");

    let mut sl = SessionLog::new(rec_id.clone());
    sl.add_event("GATEWAY", &format!("{rec_id} started (assemblyai upload)"));

    tracing::info!(
        method = %request.method(),
        path = %request.uri().path(),
        rec_id = %rec_id,
        "Received transcribe request"
    );

    // Ensure we have an API key (lazy load from pass on first request).
    let api_key = ensure_api_key(&state).await?;

    // Build a dedicated HTTP client with a long timeout for uploads and polling.
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| GatewayError::AssemblyAI(format!("failed to build HTTP client: {e}")))?;

    let aai = AssemblyAIClient::new(api_key, http);

    // Determine the audio source: multipart file upload or JSON audio_url.
    let (audio_url, audio_bytes) = extract_audio_logged(&aai, request, &mut sl).await?;

    if audio_url.is_empty() {
        sl.status = "FAILED".to_string();
        sl.add_event("GATEWAY", "no audio file or URL provided");
        sl.end_time = Some(std::time::SystemTime::now());
        sl.write_to_file();
        return Err(GatewayError::BadRequest(
            "No audio file or URL provided".to_string(),
        ));
    }

    tracing::info!(audio_url = %audio_url, "Processing transcription");

    // Create transcript job.
    sl.add_event("ASSEMBLYAI", "creating transcript job");
    let transcript_id = aai
        .create_transcript(&audio_url, &state.custom_spelling)
        .await?;
    sl.add_event("ASSEMBLYAI", &format!("job created: {transcript_id}"));
    tracing::info!(transcript_id = %transcript_id, "Transcript job created");

    // Poll for completion.
    sl.add_event("ASSEMBLYAI", "polling for completion");
    let final_transcript = poll_transcript(&aai, &transcript_id).await?;

    let text = final_transcript.text.clone().unwrap_or_default();
    let audio_duration = final_transcript.audio_duration.unwrap_or(0.0);

    sl.add_event(
        "ASSEMBLYAI",
        &format!("transcription complete: {} chars", text.len()),
    );
    sl.add_event(
        "GATEWAY",
        &format!(
            "commit complete: backend=assemblyai, transcript={} chars",
            text.len()
        ),
    );

    tracing::info!(
        transcript_id = %transcript_id,
        duration_s = audio_duration,
        "Transcription completed"
    );

    // Save transcript locally (best-effort, don't fail the request).
    save_transcript(&transcript_id, &text);

    // Finalize session log.
    sl.end_time = Some(std::time::SystemTime::now());
    let total_elapsed = sl.start_instant.elapsed();
    sl.total_ms = total_elapsed.as_millis() as i64;
    // transcribe_ms = total minus upload time (upload_ms stored in dg_connect_ms slot)
    if sl.dg_connect_ms >= 0 {
        sl.transcribe_ms = sl.total_ms - sl.dg_connect_ms;
    } else {
        sl.transcribe_ms = sl.total_ms;
    }
    sl.gw_bytes = audio_bytes as u64;
    sl.transcript = text.clone();
    sl.status = if text.is_empty() { "OK_EMPTY".to_string() } else { "OK".to_string() };

    // Archive WAV with session start timestamp for consistency.
    let start_secs = sl.start_time
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, m, d, h, min, s) = crate::audio::timestamp_parts(start_secs);
    let ts = format!("{y:04}{m:02}{d:02}_{h:02}{min:02}{s:02}");
    sl.wav_path = format!("{ts}_audio.wav");

    sl.write_to_file();
    sl.write_last_session("assemblyai");

    tracing::info!(
        rec_id = %rec_id,
        total_ms = sl.total_ms,
        transcript_len = text.len(),
        "Session log written for AssemblyAI transcription"
    );

    Ok(Json(TranscribeResponse {
        text,
        raw: final_transcript,
        duration_s: audio_duration,
    }))
}

/// Ensure the AssemblyAI API key is available, loading from `pass` if needed.
///
/// This is the Rust equivalent of Go's `ensureClient()`. We return the key as a
/// `String` rather than mutating shared state, because in Rust the `AppState` is
/// behind an `Arc` (immutable). For true lazy-init with interior mutability we
/// would need `tokio::sync::OnceCell`, but for now this simple approach mirrors
/// the Go behavior: try state first, then pass, then fail.
async fn ensure_api_key(state: &AppState) -> Result<String, GatewayError> {
    if let Some(ref key) = state.assemblyai_api_key {
        return Ok(key.clone());
    }

    tracing::info!("AssemblyAI API key not in state, loading from pass");

    let key = load_from_pass("api/assemblyai").await.ok_or_else(|| {
        GatewayError::Secret(
            "Failed to load AssemblyAI API key from pass (GPG locked or key not found)".to_string(),
        )
    })?;

    tracing::info!("AssemblyAI API key loaded from pass");
    Ok(key)
}

/// Extract audio from the incoming request, logging events to the session log.
///
/// Returns (audio_url, audio_bytes).
async fn extract_audio_logged(
    aai: &AssemblyAIClient,
    request: Request<axum::body::Body>,
    sl: &mut SessionLog,
) -> Result<(String, usize), GatewayError> {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if content_type.starts_with("multipart/form-data") {
        extract_from_multipart_logged(aai, request, sl).await
    } else {
        let url = extract_from_json(request).await?;
        Ok((url, 0))
    }
}

/// Handle multipart form upload with session logging.
///
/// Returns (upload_url, audio_bytes).
async fn extract_from_multipart_logged(
    aai: &AssemblyAIClient,
    request: Request<axum::body::Body>,
    sl: &mut SessionLog,
) -> Result<(String, usize), GatewayError> {
    let mut multipart: Multipart =
        Multipart::from_request(request, &())
            .await
            .map_err(|e| GatewayError::BadRequest(format!("failed to parse multipart form: {e}")))?;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| GatewayError::BadRequest(format!("failed to read multipart field: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name != "file" {
            continue;
        }

        let filename = field
            .file_name()
            .unwrap_or("audio.wav")
            .to_string();

        let audio_data: Bytes = field
            .bytes()
            .await
            .map_err(|e| GatewayError::BadRequest(format!("failed to read file data: {e}")))?;

        let audio_bytes = audio_data.len();
        sl.add_event("GATEWAY", &format!("received audio file: {} bytes", audio_bytes));

        tracing::info!(
            filename = %filename,
            size_bytes = audio_bytes,
            "Received audio file"
        );

        // Save locally (best-effort).
        save_recording(&filename, &audio_data);

        // Upload to AssemblyAI.
        sl.add_event("ASSEMBLYAI", "uploading audio");
        let upload_url = aai.upload(&audio_data).await?;
        // Record upload time in dg_connect_ms slot (repurposed as "upload_ms").
        sl.dg_connect_ms = sl.start_instant.elapsed().as_millis() as i64;
        sl.add_event("ASSEMBLYAI", "upload complete");
        return Ok((upload_url, audio_bytes));
    }

    Ok((String::new(), 0))
}

/// Handle JSON body with `audio_url` field.
async fn extract_from_json(
    request: Request<axum::body::Body>,
) -> Result<String, GatewayError> {
    let body = axum::body::to_bytes(request.into_body(), 1024 * 1024)
        .await
        .map_err(|e| GatewayError::BadRequest(format!("failed to read request body: {e}")))?;

    if body.is_empty() {
        return Ok(String::new());
    }

    let req: TranscribeRequest = serde_json::from_slice(&body)
        .map_err(|e| GatewayError::BadRequest(format!("invalid JSON body: {e}")))?;

    Ok(req.audio_url)
}

/// Poll AssemblyAI for transcript completion.
///
/// Checks every 2 seconds, gives up after 300 seconds. Matches the Go
/// `pollTranscript()` behavior exactly.
async fn poll_transcript(
    aai: &AssemblyAIClient,
    transcript_id: &str,
) -> Result<TranscriptResponse, GatewayError> {
    let poll_interval = Duration::from_secs(2);
    let timeout = Duration::from_secs(300);
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        tokio::time::sleep(poll_interval).await;

        if tokio::time::Instant::now() >= deadline {
            return Err(GatewayError::Timeout(
                "transcription timed out after 300 seconds".to_string(),
            ));
        }

        let resp = aai.get_transcript(transcript_id).await?;

        match resp.status.as_str() {
            "completed" => return Ok(resp),
            "error" => {
                let err_msg = resp.error.unwrap_or_else(|| "unknown error".to_string());
                return Err(GatewayError::AssemblyAI(format!(
                    "assemblyai transcription error: {err_msg}"
                )));
            }
            status => {
                tracing::debug!(
                    transcript_id = %transcript_id,
                    status = %status,
                    "Transcript still processing"
                );
            }
        }
    }
}

/// Save an uploaded audio file to `../recordings/` (best-effort).
///
/// Uses a timestamp prefix matching the Go format: `YYYYMMDD_HHMMSS_filename`.
fn save_recording(original_filename: &str, data: &[u8]) {
    let dir_str = std::env::var("RECORDINGS_DIR").unwrap_or_else(|_| "../recordings".into());
    let dir = Path::new(&dir_str);
    if let Err(e) = std::fs::create_dir_all(dir) {
        tracing::error!(error = %e, "Failed to create recordings directory");
        return;
    }

    let timestamp = format_timestamp();
    let local_filename = format!("{timestamp}_{original_filename}");
    let path = dir.join(&local_filename);

    match std::fs::write(&path, data) {
        Ok(()) => tracing::info!(path = %path.display(), "Saved audio recording"),
        Err(e) => tracing::error!(error = %e, path = %path.display(), "Failed to save audio recording"),
    }
}

/// Save transcript text to `../transcripts/` (best-effort).
///
/// Uses a timestamp prefix matching the Go format: `YYYYMMDD_HHMMSS_transcriptID.txt`.
fn save_transcript(transcript_id: &str, text: &str) {
    let dir_str = std::env::var("TRANSCRIPTS_DIR").unwrap_or_else(|_| "../transcripts".into());
    let dir = Path::new(&dir_str);
    if let Err(e) = std::fs::create_dir_all(dir) {
        tracing::error!(error = %e, "Failed to create transcripts directory");
        return;
    }

    let timestamp = format_timestamp();
    let filename = format!("{timestamp}_{transcript_id}.txt");
    let path = dir.join(&filename);

    match std::fs::write(&path, text) {
        Ok(()) => tracing::info!(path = %path.display(), "Saved transcript"),
        Err(e) => tracing::error!(error = %e, path = %path.display(), "Failed to save transcript"),
    }
}

/// Generate a timestamp string in `YYYYMMDD_HHMMSS` format without external crates.
///
/// Uses the same algorithm as `crate::audio::chrono_timestamp` (inline here to
/// avoid coupling the handler to the audio module's internals).
fn format_timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let (year, month, day, hour, min, sec) = timestamp_parts(secs);
    format!("{year:04}{month:02}{day:02}_{hour:02}{min:02}{sec:02}")
}

/// Break a Unix timestamp into UTC calendar parts.
fn timestamp_parts(secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    let days = (secs / 86400) as u32;
    let time_of_day = (secs % 86400) as u32;
    let hour = time_of_day / 3600;
    let min = (time_of_day % 3600) / 60;
    let sec = time_of_day % 60;

    let mut y = 1970u32;
    let mut remaining = days;
    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }

    let leap = is_leap(y);
    let month_days: [u32; 12] = [
        31,
        if leap { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];

    let mut m = 0u32;
    for &md in &month_days {
        if remaining < md {
            break;
        }
        remaining -= md;
        m += 1;
    }

    (y, m + 1, remaining + 1, hour, min, sec)
}

fn is_leap(y: u32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}
