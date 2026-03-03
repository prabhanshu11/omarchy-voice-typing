use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;

/// Start the full gateway router on a random port.
/// Returns the address to connect to.
pub async fn start_test_server(state: Arc<voice_type::state::AppState>) -> SocketAddr {
    let app = voice_type::build_router(state);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind test server");
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Give the server a moment to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    addr
}

/// Create an AppState for testing with given overrides.
pub fn test_state(
    local_whisper_url: &str,
    lan_whisper_url: Option<&str>,
) -> Arc<voice_type::state::AppState> {
    let latency_logger = voice_type::logging::latency::LatencyLogger::new(
        &std::env::temp_dir().join("voice-gateway-test-latency"),
    )
    .expect("failed to create test latency logger");

    Arc::new(voice_type::state::AppState {
        deepgram_api_key: None, // offline by default in tests
        assemblyai_api_key: None,
        local_whisper_url: local_whisper_url.to_string(),
        lan_whisper_url: lan_whisper_url.map(|s| s.to_string()),
        custom_spelling: vec![voice_type::spelling::CustomSpelling {
            from: "Dovac".to_string(),
            to: "Dvorak".to_string(),
        }],
        http_client: reqwest::Client::new(),
        latency_logger: std::sync::Arc::new(latency_logger),
        shutdown: tokio_util::sync::CancellationToken::new(),
    })
}

/// Generate some fake PCM16 silence (zeros) of the given duration in seconds.
pub fn silence_pcm16(seconds: f64) -> Vec<u8> {
    let sample_rate = 24000;
    let num_samples = (sample_rate as f64 * seconds) as usize;
    vec![0u8; num_samples * 2] // 2 bytes per sample (PCM16)
}
