use std::time::Duration;

use crate::transcription::whisper;

/// How long the `local` provider waits for the desktop before falling back:
/// 2.5 s + 0.15 s per second of audio, capped at 20 s (desktop GPU takes ~0.5 s
/// for a typical clip when idle).
pub fn lan_timeout(audio_bytes: usize) -> Duration {
    let audio_secs = audio_bytes as f64 / 48000.0;
    Duration::from_secs_f64((2.5 + 0.15 * audio_secs).min(20.0))
}

/// `local` provider chain: desktop Whisper (short timeout) -> Deepgram batch
/// (automatic fallback when the desktop is unreachable or slow) -> laptop Whisper.
pub async fn transcribe_local_provider(
    audio_data: &[u8],
    lan_whisper_url: Option<&str>,
    local_whisper_url: &str,
    deepgram_api_key: Option<&str>,
) -> (String, String) {
    if let Some(lan_url) = lan_whisper_url {
        let timeout = lan_timeout(audio_data.len());
        match whisper::transcribe_lan_timeout(lan_url, audio_data, timeout).await {
            Ok(resp) if !resp.text.is_empty() => return (resp.text, "lan-whisper".to_string()),
            Ok(_) => tracing::warn!("LAN whisper returned empty transcript"),
            Err(e) => tracing::warn!(error = %e, ?timeout, "LAN whisper failed or slow, falling back to Deepgram"),
        }
    }
    if let Some(key) = deepgram_api_key {
        match crate::deepgram::prerecorded::transcribe(key, audio_data, Duration::from_secs(30)).await {
            Ok(text) if !text.is_empty() => return (text, "deepgram-batch".to_string()),
            Ok(_) => tracing::warn!("Deepgram batch returned empty transcript"),
            Err(e) => tracing::warn!(error = %e, "Deepgram batch failed, trying laptop whisper"),
        }
    }
    transcribe_offline(audio_data, None, local_whisper_url).await
}

/// Transcribe audio using the fallback chain: LAN whisper → local whisper.
///
/// Returns (transcript_text, backend_name). Empty transcript if all fail.
/// Matches the Go offline path in handleAudioCommit.
pub async fn transcribe_offline(
    audio_data: &[u8],
    lan_whisper_url: Option<&str>,
    local_whisper_url: &str,
) -> (String, String) {
    // Try LAN whisper first (e.g., desktop GPU via Tailscale)
    if let Some(lan_url) = lan_whisper_url {
        match whisper::transcribe_lan(lan_url, audio_data).await {
            Ok(resp) if !resp.text.is_empty() => {
                return (resp.text, "lan-whisper".to_string());
            }
            Ok(_) => {
                tracing::warn!("LAN whisper returned empty transcript");
            }
            Err(e) => {
                tracing::warn!(error = %e, "LAN whisper failed, trying local");
            }
        }
    }

    // Fall back to local whisper
    if !local_whisper_url.is_empty() {
        match whisper::transcribe_local(local_whisper_url, audio_data).await {
            Ok(resp) if !resp.text.is_empty() => {
                return (resp.text, "local-whisper".to_string());
            }
            Ok(_) => {
                tracing::warn!("Local whisper returned empty transcript");
            }
            Err(e) => {
                tracing::error!(error = %e, "Local whisper also failed");
            }
        }
    } else {
        tracing::error!("No local whisper URL configured");
    }

    (String::new(), "none".to_string())
}
