use std::sync::Arc;

use crate::config::GatewayConfig;
use crate::spelling::CustomSpelling;

/// Shared application state, distributed to handlers via axum's State extractor.
///
/// Wrapped in `Arc` once at startup and cloned cheaply into each handler.
#[derive(Debug, Clone)]
pub struct AppState {
    pub deepgram_api_key: Option<String>,
    pub assemblyai_api_key: Option<String>,
    pub local_whisper_url: String,
    pub lan_whisper_url: Option<String>,
    pub custom_spelling: Vec<CustomSpelling>,
    pub http_client: reqwest::Client,
}

impl AppState {
    pub fn from_config(config: &GatewayConfig) -> Arc<Self> {
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
        })
    }
}
