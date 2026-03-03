use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::signal;

use voice_type::config::GatewayConfig;
use voice_type::state::AppState;

#[tokio::main]
async fn main() {
    // Initialize tracing (structured logging)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "voice_type=info,tower_http=info".into()),
        )
        .init();

    // Load configuration
    let config = GatewayConfig::load().await;
    let port = config.port;
    let shared_state: Arc<AppState> = AppState::from_config(&config);
    let shutdown_token = shared_state.shutdown.clone();

    // Build router with all routes
    let app = voice_type::build_router(shared_state);

    // Bind and serve
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await.expect("failed to bind port");
    tracing::info!(%addr, "Starting gateway server");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(shutdown_token))
        .await
        .expect("server error");
}

/// Wait for SIGTERM or SIGINT (Ctrl+C), then cancel the token so in-flight
/// WebSocket sessions can drain before the process exits.
async fn shutdown_signal(token: tokio_util::sync::CancellationToken) {
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

    // Signal all active sessions to wrap up
    token.cancel();

    // Brief grace period for in-flight recordings to finish
    tracing::info!("Waiting up to 3s for in-flight sessions to drain");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
}
