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
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

/// Build the full application router with all routes.
///
/// Extracted from main() so integration tests can create the same router.
pub fn build_router(shared_state: Arc<state::AppState>) -> Router {
    let api = Router::new()
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
        // Profiling endpoints
        .route(
            "/api/profiling/latency",
            axum::routing::get(handlers::profiling::latency),
        )
        .route(
            "/api/profiling/sessions",
            axum::routing::get(handlers::profiling::sessions),
        )
        .route(
            "/api/profiling/summary",
            axum::routing::get(handlers::profiling::summary),
        )
        .with_state(shared_state)
        .layer(CorsLayer::permissive());

    // SPA static file serving: serve web/dist/ with index.html fallback.
    // If the directory doesn't exist (frontend not built), skip gracefully.
    let spa_dir = std::path::Path::new("../web/dist");
    if spa_dir.is_dir() {
        let index = spa_dir.join("index.html");
        api.fallback_service(ServeDir::new(spa_dir).fallback(ServeFile::new(index)))
    } else {
        api
    }
}
