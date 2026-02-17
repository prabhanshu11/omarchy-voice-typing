use std::path::Path;

use crate::secrets::load_from_pass;
use crate::spelling::{CustomSpelling, load_replacements};

/// Gateway configuration loaded from environment variables and config files.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub assemblyai_api_key: Option<String>,
    pub deepgram_api_key: Option<String>,
    pub local_whisper_url: String,
    pub lan_whisper_url: Option<String>,
    pub port: u16,
    pub custom_spelling: Vec<CustomSpelling>,
}

impl GatewayConfig {
    /// Load configuration from environment variables, `pass`, and config files.
    pub async fn load() -> Self {
        // AssemblyAI: env only at startup (lazy pass load on first REST request)
        let assemblyai_api_key = std::env::var("ASSEMBLYAI_API_KEY")
            .or_else(|_| std::env::var("ASSEMBLY_API_KEY"))
            .ok();

        if assemblyai_api_key.is_some() {
            tracing::info!("AssemblyAI API key loaded from environment");
        } else {
            tracing::info!(
                "No AssemblyAI API key in environment — will load from pass on first REST request"
            );
        }

        // Deepgram: try env first, then pass
        let deepgram_api_key = std::env::var("DEEPGRAM_API_KEY").ok();
        let deepgram_api_key = match deepgram_api_key {
            Some(key) => {
                tracing::info!("Deepgram API key loaded from environment");
                Some(key)
            }
            None => match load_from_pass("api/deepgram").await {
                Some(key) => {
                    tracing::info!("Deepgram API key loaded from pass");
                    Some(key)
                }
                None => {
                    tracing::warn!(
                        "No Deepgram API key found — streaming endpoint will not work"
                    );
                    None
                }
            },
        };

        // Whisper URLs
        let local_whisper_url = std::env::var("LOCAL_WHISPER_URL")
            .unwrap_or_else(|_| "http://localhost:8767".to_string());
        tracing::info!(url = %local_whisper_url, "Local whisper fallback URL");

        let lan_whisper_url = std::env::var("LAN_WHISPER_URL").ok();
        match &lan_whisper_url {
            Some(url) => tracing::info!(url = %url, "LAN whisper fallback URL"),
            None => tracing::info!("LAN whisper fallback: not configured (set LAN_WHISPER_URL)"),
        }

        // Port
        let port: u16 = std::env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8765);

        // Custom spelling replacements
        let replacements_path = Path::new("../config/replacements.json");
        let custom_spelling = match load_replacements(replacements_path) {
            Ok(spellings) => {
                tracing::info!(count = spellings.len(), "Loaded custom spelling replacements");
                spellings
            }
            Err(e) => {
                tracing::warn!(
                    path = %replacements_path.display(),
                    error = %e,
                    "Failed to load replacements"
                );
                Vec::new()
            }
        };

        Self {
            assemblyai_api_key,
            deepgram_api_key,
            local_whisper_url,
            lan_whisper_url,
            port,
            custom_spelling,
        }
    }
}
