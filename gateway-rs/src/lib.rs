pub mod assemblyai;
pub mod audio;
pub mod config;
pub mod deepgram;
pub mod error;
pub mod handlers;
pub mod logging;
pub mod secrets;
pub mod spelling;
pub mod state;
pub mod transcription;

use std::sync::Arc;

use axum::Router;

/// Build the full application router with all routes.
///
/// Extracted from main() so integration tests can create the same router.
pub fn build_router(shared_state: Arc<state::AppState>) -> Router {
    Router::new()
        // Core endpoints
        .route("/health", axum::routing::get(handlers::health::health))
        .route(
            "/v1/realtime",
            axum::routing::get(handlers::realtime::realtime),
        )
        .route(
            "/v1/transcribe",
            axum::routing::post(handlers::transcribe::transcribe),
        )
        // Web UI API endpoints
        .route(
            "/api/recordings",
            axum::routing::get(handlers::web::list_recordings),
        )
        .route(
            "/api/transcripts",
            axum::routing::get(handlers::web::list_transcripts),
        )
        .route(
            "/api/transcript/{filename}",
            axum::routing::get(handlers::web::get_transcript)
                .put(handlers::web::update_transcript),
        )
        .route(
            "/api/audio/{filename}",
            axum::routing::get(handlers::web::serve_audio),
        )
        .route("/api/stats", axum::routing::get(handlers::web::stats))
        .route("/api/linked", axum::routing::get(handlers::web::list_linked))
        .with_state(shared_state)
}
