use std::time::Duration;

use crate::audio::build_wav;
use crate::error::GatewayError;

/// Response from a whisper server's /transcribe endpoint.
#[derive(Debug, serde::Deserialize)]
pub struct WhisperResponse {
    pub text: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub transcribe_time: f64,
}

/// Transcribe audio using a local whisper server.
///
/// POSTs a WAV file to `{base_url}/transcribe` with a 120s timeout.
/// Matches Go's `transcribeLocal()`.
pub async fn transcribe_local(
    base_url: &str,
    audio_data: &[u8],
) -> Result<WhisperResponse, GatewayError> {
    transcribe_whisper(base_url, audio_data, Duration::from_secs(120), "local-whisper").await
}

/// Transcribe audio using a LAN whisper server (e.g., desktop GPU via Tailscale).
///
/// POSTs a WAV file to `{base_url}/transcribe` with a 30s timeout.
/// Matches Go's `transcribeLAN()`.
pub async fn transcribe_lan(
    base_url: &str,
    audio_data: &[u8],
) -> Result<WhisperResponse, GatewayError> {
    transcribe_whisper(base_url, audio_data, Duration::from_secs(30), "lan-whisper").await
}

async fn transcribe_whisper(
    base_url: &str,
    audio_data: &[u8],
    timeout: Duration,
    label: &str,
) -> Result<WhisperResponse, GatewayError> {
    if audio_data.is_empty() {
        return Err(GatewayError::Whisper(format!(
            "[{label}] audio data is empty"
        )));
    }

    let wav_buf = build_wav(audio_data, 24000)?;
    let audio_duration = audio_data.len() as f64 / 48000.0;
    tracing::info!(
        label,
        wav_bytes = wav_buf.len(),
        pcm_bytes = audio_data.len(),
        audio_secs = format_args!("{audio_duration:.1}"),
        "Built WAV for whisper"
    );

    let url = format!("{base_url}/transcribe");
    tracing::info!(label, %url, "POSTing to whisper");

    let t0 = std::time::Instant::now();

    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| GatewayError::Whisper(format!("[{label}] client build failed: {e}")))?;

    let resp = client
        .post(&url)
        .header("Content-Type", "audio/wav")
        .body(wav_buf)
        .send()
        .await
        .map_err(|e| {
            let elapsed = t0.elapsed();
            tracing::error!(label, ?elapsed, error = %e, "POST to whisper failed");
            GatewayError::Whisper(format!("[{label}] POST failed after {elapsed:?}: {e}"))
        })?;

    let elapsed = t0.elapsed();
    let status = resp.status();

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        tracing::error!(label, %status, %body, ?elapsed, "Non-200 from whisper");
        return Err(GatewayError::Whisper(format!(
            "[{label}] status {status} (body={body}, roundtrip={elapsed:?})"
        )));
    }

    let body = resp.text().await.map_err(|e| {
        GatewayError::Whisper(format!("[{label}] reading response body failed: {e}"))
    })?;

    let result: WhisperResponse = serde_json::from_str(&body).map_err(|e| {
        tracing::error!(label, error = %e, %body, "JSON parse error");
        GatewayError::Whisper(format!("[{label}] JSON parse error: {e}"))
    })?;

    tracing::info!(
        label,
        text = %result.text,
        model = %result.model,
        audio_secs = format_args!("{:.1}", result.duration),
        transcribe_secs = format_args!("{:.2}", result.transcribe_time),
        ?elapsed,
        "Whisper OK"
    );

    Ok(result)
}
