use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use tokio_util::sync::CancellationToken;

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
    pub shutdown: CancellationToken,
    /// Global recording counter for desktop sessions (rec-001, rec-002, ...)
    pub rec_counter: Arc<AtomicU32>,
    /// Global recording counter for web sessions (webrec-001, webrec-002, ...)
    pub webrec_counter: Arc<AtomicU32>,
    /// Directory containing latency_*.jsonl files for profiling endpoints.
    pub latency_log_dir: PathBuf,
    /// Directory containing session .log files for profiling endpoints.
    pub session_log_dir: PathBuf,
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
            .field("shutdown", &self.shutdown.is_cancelled())
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
            shutdown: CancellationToken::new(),
            rec_counter: Arc::new(AtomicU32::new(0)),
            webrec_counter: Arc::new(AtomicU32::new(0)),
            latency_log_dir: latency_dir,
            session_log_dir: session_log_dir(),
        })
    }
}

/// Resolve latency log directory.
/// Checks LATENCY_LOG_DIR env, then tries CWD-relative paths, then HOME-based path.
fn latency_log_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("LATENCY_LOG_DIR") {
        return PathBuf::from(dir);
    }
    // Docker layout: /app/logs/latency
    let cwd_path = PathBuf::from("logs/latency");
    if cwd_path.is_dir() {
        return cwd_path;
    }
    // Local dev layout: ../logs/latency (CWD is gateway-rs/)
    let local_path = PathBuf::from("../logs/latency");
    if local_path.is_dir() {
        return local_path;
    }
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join("Programs/omarchy-voice-typing/logs/latency")
    } else {
        PathBuf::from("../logs/latency")
    }
}

/// Resolve session log directory — same pattern as latency_log_dir.
fn session_log_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SESSION_LOG_DIR") {
        return PathBuf::from(dir);
    }
    let cwd_path = PathBuf::from("logs/sessions");
    if cwd_path.is_dir() {
        return cwd_path;
    }
    let local_path = PathBuf::from("../logs/sessions");
    if local_path.is_dir() {
        return local_path;
    }
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join("Programs/omarchy-voice-typing/logs/sessions")
    } else {
        PathBuf::from("../logs/sessions")
    }
}
