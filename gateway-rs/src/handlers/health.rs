use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::State;

use crate::state::AppState;

#[derive(serde::Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub backend: &'static str,
    pub whisper_ready: bool,
    pub whisper_url: String,
    pub lan_whisper_ready: bool,
    pub lan_whisper_url: Option<String>,
}

/// GET /health — probes local and LAN whisper backends, reports gateway status.
///
/// This is the Rust equivalent of Go's `healthHandler()`. It creates a short-lived
/// HTTP client with a 2s timeout to probe each whisper endpoint, matching the Go behavior.
pub async fn health(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let probe_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    // Probe local whisper
    let whisper_ready = probe_health(&probe_client, &state.local_whisper_url).await;

    // Probe LAN whisper
    let lan_whisper_ready = match &state.lan_whisper_url {
        Some(url) => probe_health(&probe_client, url).await,
        None => false,
    };

    Json(HealthResponse {
        status: "ok",
        backend: "deepgram",
        whisper_ready,
        whisper_url: state.local_whisper_url.clone(),
        lan_whisper_ready,
        lan_whisper_url: state.lan_whisper_url.clone(),
    })
}

/// Probe a whisper backend's /health endpoint. Returns true if it responds 200 OK.
async fn probe_health(client: &reqwest::Client, base_url: &str) -> bool {
    let url = format!("{}/health", base_url);
    match client.get(&url).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}
