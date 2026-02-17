mod config;
mod error;
mod handlers;
mod secrets;
mod spelling;
mod state;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use tokio::net::TcpListener;
use tokio::signal;

use config::GatewayConfig;
use state::AppState;

#[tokio::main]
async fn main() {
    // Initialize tracing (structured logging)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "voice_gateway=info,tower_http=info".into()),
        )
        .init();

    // Load configuration
    let config = GatewayConfig::load().await;
    let port = config.port;
    let shared_state: Arc<AppState> = AppState::from_config(&config);

    // Build router (Phase 1: health only; later phases add /v1/*, /api/*, etc.)
    let app = Router::new()
        .route("/health", axum::routing::get(handlers::health::health))
        .with_state(shared_state);

    // Bind and serve
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await.expect("failed to bind port");
    tracing::info!(%addr, "Starting gateway server");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
}

/// Wait for SIGTERM or SIGINT (Ctrl+C) for graceful shutdown.
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => tracing::info!("Received Ctrl+C, shutting down"),
        () = terminate => tracing::info!("Received SIGTERM, shutting down"),
    }
}
