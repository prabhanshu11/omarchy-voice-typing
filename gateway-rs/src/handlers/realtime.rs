use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;

use crate::state::AppState;

use super::realtime_session::RealtimeSession;

/// Incoming event from hyprwhspr (OpenAI Realtime protocol subset).
#[derive(Debug, serde::Deserialize)]
pub struct RealtimeEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub session: Option<serde_json::Value>,
    #[serde(default)]
    pub audio: Option<String>,
    // session.event telemetry fields (Python → Gateway)
    #[serde(default)]
    pub layer: Option<String>,
    #[serde(default)]
    pub event: Option<String>,
}

/// GET /v1/realtime — WebSocket upgrade handler.
///
/// Upgrades the HTTP connection to a WebSocket and hands off to the
/// message dispatch loop. Matches Go's `RealtimeHandler`.
pub async fn realtime(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    tracing::info!("New WebSocket connection request");
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

/// Main WebSocket dispatch loop.
///
/// Sends `session.created`, then reads messages from the client and dispatches
/// them to the appropriate `RealtimeSession` method. Runs until the client
/// disconnects or an error occurs.
async fn handle_ws(socket: WebSocket, state: Arc<AppState>) {
    let (ws_sink, mut ws_stream) = socket.split();
    let session_id = format!("sess_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos());

    let mut session = RealtimeSession::new(ws_sink, &state, session_id.clone());

    // Send session.created
    let created = serde_json::json!({
        "type": "session.created",
        "session": {
            "id": session_id,
            "model": "nova-2",
            "object": "realtime.session",
        },
    });
    if let Err(e) = session.send_json(&created).await {
        tracing::error!(error = %e, "Failed to send session.created");
        return;
    }

    // Read messages from hyprwhspr
    use futures_util::StreamExt;
    while let Some(msg_result) = ws_stream.next().await {
        let msg = match msg_result {
            Ok(msg) => msg,
            Err(e) => {
                tracing::info!(error = %e, "WebSocket read error");
                break;
            }
        };

        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => {
                tracing::info!("Client disconnected normally");
                break;
            }
            _ => continue,
        };

        let event: RealtimeEvent = match serde_json::from_str(&text) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(error = %e, "Invalid JSON from client");
                continue;
            }
        };

        match event.event_type.as_str() {
            "session.update" => session.handle_session_update().await,
            "input_audio_buffer.append" => session.handle_audio_append(&event).await,
            "input_audio_buffer.commit" => session.handle_audio_commit().await,
            "input_audio_buffer.clear" => session.handle_audio_clear().await,
            "session.event" => session.handle_session_event(&event),
            other => {
                tracing::warn!(event_type = %other, "Unknown event type");
            }
        }
    }

    // Client disconnected — run cleanup
    session.cleanup().await;
}
