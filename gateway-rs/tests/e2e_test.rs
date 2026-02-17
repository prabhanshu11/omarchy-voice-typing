//! End-to-end integration tests for the voice gateway.
//!
//! These tests start the full axum server on a random port and exercise
//! the API through real HTTP and WebSocket connections. External services
//! (Deepgram, AssemblyAI, Whisper) are mocked with wiremock.

mod common;

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ─────────────────────────────────────────────
// Helper: start server with wiremock whisper
// ─────────────────────────────────────────────

async fn setup_with_mock_whisper() -> (std::net::SocketAddr, MockServer) {
    let mock_whisper = MockServer::start().await;

    // Mock the /transcribe endpoint to return a transcript
    Mock::given(method("POST"))
        .and(path("/transcribe"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "text": "hello world from Dovac",
            "model": "test-whisper",
            "duration": 2.5,
            "transcribe_time": 0.3,
        })))
        .mount(&mock_whisper)
        .await;

    let state = common::test_state(&mock_whisper.uri(), None);
    let addr = common::start_test_server(state).await;
    (addr, mock_whisper)
}

/// Connect a WebSocket client to the gateway's /v1/realtime endpoint.
async fn ws_connect(
    addr: std::net::SocketAddr,
) -> (
    futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) {
    let url = format!("ws://{addr}/v1/realtime");
    let (ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("WS connect failed");
    ws.split()
}

/// Read the next text message, with a timeout.
async fn read_json(
    stream: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) -> serde_json::Value {
    let msg = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("timeout reading WS message")
        .expect("stream ended")
        .expect("WS read error");

    match msg {
        Message::Text(t) => serde_json::from_str(&t).expect("invalid JSON from gateway"),
        other => panic!("expected text message, got: {other:?}"),
    }
}

/// Send a JSON message over WS.
async fn send_json(
    sink: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    value: &serde_json::Value,
) {
    let text = serde_json::to_string(value).unwrap();
    sink.send(Message::Text(text.into())).await.unwrap();
}

// ═══════════════════════════════════════════════
// Test 1: GET /health returns 200
// ═══════════════════════════════════════════════

#[tokio::test]
async fn health_returns_200() {
    let state = common::test_state("http://localhost:9999", None);
    let addr = common::start_test_server(state).await;

    let resp = reqwest::get(format!("http://{addr}/health"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
}

// ═══════════════════════════════════════════════
// Test 2: WS connect → receive session.created
// ═══════════════════════════════════════════════

#[tokio::test]
async fn ws_connect_receives_session_created() {
    let (addr, _mock) = setup_with_mock_whisper().await;

    let (_sink, mut stream) = ws_connect(addr).await;
    let msg = read_json(&mut stream).await;

    assert_eq!(msg["type"], "session.created");
    assert!(msg["session"]["id"].as_str().unwrap().starts_with("sess_"));
    assert_eq!(msg["session"]["model"], "nova-2");
}

// ═══════════════════════════════════════════════
// Test 3: session.update → receive session.updated
// ═══════════════════════════════════════════════

#[tokio::test]
async fn session_update_returns_session_updated() {
    let (addr, _mock) = setup_with_mock_whisper().await;

    let (mut sink, mut stream) = ws_connect(addr).await;

    // Consume session.created
    let _ = read_json(&mut stream).await;

    // Send session.update
    send_json(
        &mut sink,
        &serde_json::json!({"type": "session.update", "session": {}}),
    )
    .await;

    let msg = read_json(&mut stream).await;
    assert_eq!(msg["type"], "session.updated");
    // offline_mode is lazy (set on first failed connect, not at session.update time)
    // With no Deepgram key, the gateway still reports "nova-2" optimistically here.
    // Actual offline fallback happens at commit time when connect fails.
    assert_eq!(msg["session"]["model"], "nova-2");
}

// ═══════════════════════════════════════════════
// Test 4: Full offline recording session
//   Send audio → commit → receive transcript via mock whisper
// ═══════════════════════════════════════════════

#[tokio::test]
async fn offline_recording_returns_transcript_via_whisper() {
    let (addr, _mock) = setup_with_mock_whisper().await;

    let (mut sink, mut stream) = ws_connect(addr).await;

    // Consume session.created
    let _ = read_json(&mut stream).await;

    // session.update
    send_json(
        &mut sink,
        &serde_json::json!({"type": "session.update", "session": {}}),
    )
    .await;
    let _ = read_json(&mut stream).await; // session.updated

    // Send 5 audio chunks (~0.5s of silence)
    let chunk = common::silence_pcm16(0.1); // 100ms of audio
    let b64_chunk = BASE64.encode(&chunk);
    for _ in 0..5 {
        send_json(
            &mut sink,
            &serde_json::json!({
                "type": "input_audio_buffer.append",
                "audio": b64_chunk,
            }),
        )
        .await;
    }

    // Commit
    send_json(
        &mut sink,
        &serde_json::json!({"type": "input_audio_buffer.commit"}),
    )
    .await;

    // Should receive transcript — spelling replacement applied: "Dovac" → "Dvorak"
    let msg = read_json(&mut stream).await;
    assert_eq!(
        msg["type"],
        "conversation.item.input_audio_transcription.completed"
    );
    let transcript = msg["transcript"].as_str().unwrap();
    assert!(
        transcript.contains("Dvorak"),
        "Expected 'Dvorak' (spelling replacement applied), got: {transcript}"
    );
    assert!(
        !transcript.contains("Dovac"),
        "Spelling replacement not applied: {transcript}"
    );
}

// ═══════════════════════════════════════════════
// Test 5: Clear with buffered audio → no transcript
//   (discard, not auto-commit)
// ═══════════════════════════════════════════════

#[tokio::test]
async fn clear_with_audio_does_not_produce_transcript() {
    let (addr, _mock) = setup_with_mock_whisper().await;

    let (mut sink, mut stream) = ws_connect(addr).await;

    // Consume session.created
    let _ = read_json(&mut stream).await;

    // session.update
    send_json(
        &mut sink,
        &serde_json::json!({"type": "session.update", "session": {}}),
    )
    .await;
    let _ = read_json(&mut stream).await; // session.updated

    // Send audio
    let chunk = common::silence_pcm16(0.1);
    let b64_chunk = BASE64.encode(&chunk);
    send_json(
        &mut sink,
        &serde_json::json!({
            "type": "input_audio_buffer.append",
            "audio": b64_chunk,
        }),
    )
    .await;

    // Clear (not commit)
    send_json(
        &mut sink,
        &serde_json::json!({"type": "input_audio_buffer.clear"}),
    )
    .await;

    // Give the gateway a moment to process
    tokio::time::sleep(Duration::from_millis(200)).await;

    // There should be NO transcript message. Try reading with a short timeout.
    let result = tokio::time::timeout(Duration::from_millis(500), stream.next()).await;
    assert!(
        result.is_err(),
        "Expected no message after clear, but got one"
    );
}

// ═══════════════════════════════════════════════
// Test 6: Audio before session.update → ignored
// ═══════════════════════════════════════════════

#[tokio::test]
async fn audio_before_session_update_is_ignored() {
    let (addr, _mock) = setup_with_mock_whisper().await;

    let (mut sink, mut stream) = ws_connect(addr).await;

    // Consume session.created
    let _ = read_json(&mut stream).await;

    // Send audio WITHOUT session.update first
    let chunk = common::silence_pcm16(0.1);
    let b64_chunk = BASE64.encode(&chunk);
    send_json(
        &mut sink,
        &serde_json::json!({
            "type": "input_audio_buffer.append",
            "audio": b64_chunk,
        }),
    )
    .await;

    // Commit
    send_json(
        &mut sink,
        &serde_json::json!({"type": "input_audio_buffer.commit"}),
    )
    .await;

    // Should NOT receive a transcript (audio was ignored)
    // Give it a moment
    tokio::time::sleep(Duration::from_millis(200)).await;

    let result = tokio::time::timeout(Duration::from_millis(500), stream.next()).await;
    // Could get empty transcript or nothing — both are acceptable
    // The key assertion: no panic, no crash
    if let Ok(Some(Ok(Message::Text(t)))) = result {
        let msg: serde_json::Value = serde_json::from_str(&t).unwrap();
        if msg["type"] == "conversation.item.input_audio_transcription.completed" {
            // Empty transcript is fine (no audio was buffered)
            let transcript = msg["transcript"].as_str().unwrap_or("");
            assert!(
                transcript.is_empty(),
                "Expected empty transcript for pre-session audio, got: {transcript}"
            );
        }
    }
}

// ═══════════════════════════════════════════════
// Test 7: Back-to-back recordings
// ═══════════════════════════════════════════════

#[tokio::test]
async fn back_to_back_recordings_both_succeed() {
    let (addr, _mock) = setup_with_mock_whisper().await;

    let (mut sink, mut stream) = ws_connect(addr).await;

    // Consume session.created
    let _ = read_json(&mut stream).await;

    // session.update
    send_json(
        &mut sink,
        &serde_json::json!({"type": "session.update", "session": {}}),
    )
    .await;
    let _ = read_json(&mut stream).await; // session.updated

    let chunk = common::silence_pcm16(0.1);
    let b64_chunk = BASE64.encode(&chunk);

    // Recording 1
    for _ in 0..3 {
        send_json(
            &mut sink,
            &serde_json::json!({
                "type": "input_audio_buffer.append",
                "audio": b64_chunk,
            }),
        )
        .await;
    }
    send_json(
        &mut sink,
        &serde_json::json!({"type": "input_audio_buffer.commit"}),
    )
    .await;

    let msg1 = read_json(&mut stream).await;
    assert_eq!(
        msg1["type"],
        "conversation.item.input_audio_transcription.completed"
    );

    // Recording 2 (immediately after)
    for _ in 0..3 {
        send_json(
            &mut sink,
            &serde_json::json!({
                "type": "input_audio_buffer.append",
                "audio": b64_chunk,
            }),
        )
        .await;
    }
    send_json(
        &mut sink,
        &serde_json::json!({"type": "input_audio_buffer.commit"}),
    )
    .await;

    let msg2 = read_json(&mut stream).await;
    assert_eq!(
        msg2["type"],
        "conversation.item.input_audio_transcription.completed"
    );
}

// ═══════════════════════════════════════════════
// Test 8: Whisper returns empty → empty transcript
// ═══════════════════════════════════════════════

#[tokio::test]
async fn whisper_empty_response_returns_empty_transcript() {
    let mock_whisper = MockServer::start().await;

    // Mock whisper to return empty text
    Mock::given(method("POST"))
        .and(path("/transcribe"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "text": "",
            "model": "test-whisper",
            "duration": 1.0,
            "transcribe_time": 0.1,
        })))
        .mount(&mock_whisper)
        .await;

    let state = common::test_state(&mock_whisper.uri(), None);
    let addr = common::start_test_server(state).await;

    let (mut sink, mut stream) = ws_connect(addr).await;
    let _ = read_json(&mut stream).await; // session.created

    send_json(
        &mut sink,
        &serde_json::json!({"type": "session.update", "session": {}}),
    )
    .await;
    let _ = read_json(&mut stream).await; // session.updated

    let chunk = common::silence_pcm16(0.1);
    let b64_chunk = BASE64.encode(&chunk);
    send_json(
        &mut sink,
        &serde_json::json!({
            "type": "input_audio_buffer.append",
            "audio": b64_chunk,
        }),
    )
    .await;
    send_json(
        &mut sink,
        &serde_json::json!({"type": "input_audio_buffer.commit"}),
    )
    .await;

    let msg = read_json(&mut stream).await;
    assert_eq!(
        msg["type"],
        "conversation.item.input_audio_transcription.completed"
    );
    let transcript = msg["transcript"].as_str().unwrap();
    assert!(
        transcript.is_empty(),
        "Expected empty transcript, got: {transcript}"
    );
}

// ═══════════════════════════════════════════════
// Test 9: All routes respond (no 404 for valid paths)
// ═══════════════════════════════════════════════

#[tokio::test]
async fn all_routes_respond() {
    let state = common::test_state("http://localhost:9999", None);
    let addr = common::start_test_server(state).await;
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    // Health
    let r = client.get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(r.status(), 200);

    // API endpoints (should return 200 with empty arrays, not 404)
    let r = client
        .get(format!("{base}/api/recordings"))
        .send()
        .await
        .unwrap();
    assert_ne!(r.status().as_u16(), 404, "/api/recordings should not 404");

    let r = client
        .get(format!("{base}/api/transcripts"))
        .send()
        .await
        .unwrap();
    assert_ne!(r.status().as_u16(), 404, "/api/transcripts should not 404");

    let r = client
        .get(format!("{base}/api/stats"))
        .send()
        .await
        .unwrap();
    assert_ne!(r.status().as_u16(), 404, "/api/stats should not 404");

    let r = client
        .get(format!("{base}/api/linked"))
        .send()
        .await
        .unwrap();
    assert_ne!(r.status().as_u16(), 404, "/api/linked should not 404");
}

// ═══════════════════════════════════════════════
// Test 10: LAN whisper fallback → local whisper
// ═══════════════════════════════════════════════

#[tokio::test]
async fn lan_whisper_fails_falls_back_to_local() {
    let mock_local = MockServer::start().await;

    // Mock local whisper
    Mock::given(method("POST"))
        .and(path("/transcribe"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "text": "local fallback worked",
            "model": "local-whisper",
            "duration": 1.0,
            "transcribe_time": 0.2,
        })))
        .mount(&mock_local)
        .await;

    // LAN whisper points to a dead URL
    let state = common::test_state(
        &mock_local.uri(),
        Some("http://127.0.0.1:1"), // unreachable
    );
    let addr = common::start_test_server(state).await;

    let (mut sink, mut stream) = ws_connect(addr).await;
    let _ = read_json(&mut stream).await; // session.created

    send_json(
        &mut sink,
        &serde_json::json!({"type": "session.update", "session": {}}),
    )
    .await;
    let _ = read_json(&mut stream).await; // session.updated

    let chunk = common::silence_pcm16(0.1);
    let b64_chunk = BASE64.encode(&chunk);
    for _ in 0..3 {
        send_json(
            &mut sink,
            &serde_json::json!({
                "type": "input_audio_buffer.append",
                "audio": b64_chunk,
            }),
        )
        .await;
    }
    send_json(
        &mut sink,
        &serde_json::json!({"type": "input_audio_buffer.commit"}),
    )
    .await;

    let msg = read_json(&mut stream).await;
    assert_eq!(
        msg["type"],
        "conversation.item.input_audio_transcription.completed"
    );
    let transcript = msg["transcript"].as_str().unwrap();
    assert_eq!(transcript, "local fallback worked");
}
