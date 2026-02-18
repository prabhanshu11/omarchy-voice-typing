use std::path::PathBuf;
use std::sync::Arc;

use crate::config::GatewayConfig;
use crate::logging::latency::LatencyLogger;
use crate::spelling::CustomSpelling;

/// Shared application state, distributed to handlers via axum's State extractor.
///
/// Wrapped in `Arc` once at startup and cloned cheaply into each handler.
#[derive(Clone)]
pub struct AppState {
    pub deepgram_api_key: Option<String>,
    pub assemblyai_api_key: Option<String>,
    pub local_whisper_url: String,
    pub lan_whisper_url: Option<String>,
    pub custom_spelling: Vec<CustomSpelling>,
    pub http_client: reqwest::Client,
    pub latency_logger: Arc<LatencyLogger>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("deepgram_api_key", &self.deepgram_api_key.as_ref().map(|_| "***"))
            .field("assemblyai_api_key", &self.assemblyai_api_key.as_ref().map(|_| "***"))
            .field("local_whisper_url", &self.local_whisper_url)
            .field("lan_whisper_url", &self.lan_whisper_url)
            .field("custom_spelling", &self.custom_spelling)
            .field("latency_logger", &"LatencyLogger { .. }")
            .finish()
    }
}

impl AppState {
    pub fn from_config(config: &GatewayConfig) -> Arc<Self> {
        let latency_dir = latency_log_dir();
        let latency_logger = LatencyLogger::new(&latency_dir)
            .unwrap_or_else(|e| {
                tracing::error!(error = %e, dir = %latency_dir.display(), "Failed to create latency logger, using fallback");
                // Fall back to a temp dir so the gateway still starts
                LatencyLogger::new(&std::env::temp_dir().join("voice-gateway-latency"))
                    .expect("failed to create fallback latency logger")
            });

        Arc::new(Self {
            deepgram_api_key: config.deepgram_api_key.clone(),
            assemblyai_api_key: config.assemblyai_api_key.clone(),
            local_whisper_url: config.local_whisper_url.clone(),
            lan_whisper_url: config.lan_whisper_url.clone(),
            custom_spelling: config.custom_spelling.clone(),
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build HTTP client"),
            latency_logger: Arc::new(latency_logger),
        })
    }
}

/// Resolve latency log directory — consistent with session_log's approach.
fn latency_log_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join("Programs/omarchy-voice-typing/logs/latency")
    } else {
        PathBuf::from("../logs/latency")
    }
}
