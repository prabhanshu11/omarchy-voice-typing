use crate::transcription::whisper;

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
