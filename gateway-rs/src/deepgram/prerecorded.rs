//! Deepgram pre-recorded (batch) transcription of a whole buffered recording.
//!
//! Used by the `local` provider as its automatic fallback when the desktop
//! Whisper is unreachable or slow: the audio is already buffered at commit, so
//! one POST to `/v1/listen` gives the same nova-2 model the streaming path uses.

use std::time::{Duration, Instant};

use crate::audio::build_wav;
use crate::error::GatewayError;

pub async fn transcribe(api_key: &str, audio_data: &[u8], timeout: Duration) -> Result<String, GatewayError> {
    if audio_data.is_empty() {
        return Err(GatewayError::Deepgram("[deepgram-batch] audio data is empty".into()));
    }
    let wav = build_wav(audio_data, 24000)?;
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| GatewayError::Deepgram(format!("[deepgram-batch] client build failed: {e}")))?;
    let t0 = Instant::now();
    let resp = client
        .post("https://api.deepgram.com/v1/listen?model=nova-2&punctuate=true&smart_format=true")
        .header("Authorization", format!("Token {api_key}"))
        .header("Content-Type", "audio/wav")
        .body(wav)
        .send()
        .await
        .map_err(|e| GatewayError::Deepgram(format!("[deepgram-batch] POST failed after {:?}: {e}", t0.elapsed())))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| GatewayError::Deepgram(format!("[deepgram-batch] reading body failed: {e}")))?;
    if !status.is_success() {
        return Err(GatewayError::Deepgram(format!("[deepgram-batch] status {status}: {body}")));
    }
    let v: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| GatewayError::Deepgram(format!("[deepgram-batch] JSON parse error: {e}")))?;
    let text = v["results"]["channels"][0]["alternatives"][0]["transcript"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    tracing::info!(elapsed = ?t0.elapsed(), chars = text.len(), "Deepgram batch OK");
    Ok(text)
}
