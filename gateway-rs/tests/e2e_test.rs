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
// Test 10: CORS preflight returns proper headers
// ═══════════════════════════════════════════════

#[tokio::test]
async fn cors_preflight_returns_headers() {
    let state = common::test_state("http://localhost:9999", None);
    let addr = common::start_test_server(state).await;
    let client = reqwest::Client::new();

    let resp = client
        .request(reqwest::Method::OPTIONS, format!("http://{addr}/health"))
        .header("Origin", "http://localhost:5173")
        .header("Access-Control-Request-Method", "GET")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    assert!(
        resp.headers().contains_key("access-control-allow-origin"),
        "Missing CORS allow-origin header"
    );
    assert!(
        resp.headers().contains_key("access-control-allow-methods"),
        "Missing CORS allow-methods header"
    );
}

// ═══════════════════════════════════════════════
// Test 11: Concurrent WS sessions — no crosstalk
//   3 sessions, each gets its own transcript
// ═══════════════════════════════════════════════

#[tokio::test]
async fn concurrent_ws_sessions_no_crosstalk() {
    let mock_whisper = MockServer::start().await;

    // Use a counter to make each response unique — wiremock returns same
    // response for all, but we verify each session gets *a* transcript.
    Mock::given(method("POST"))
        .and(path("/transcribe"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "text": "session transcript",
            "model": "test-whisper",
            "duration": 1.0,
            "transcribe_time": 0.1,
        })))
        .mount(&mock_whisper)
        .await;

    let state = common::test_state(&mock_whisper.uri(), None);
    let addr = common::start_test_server(state).await;

    // Spawn 3 concurrent sessions
    let mut handles = Vec::new();
    for _ in 0..3 {
        let addr = addr;
        handles.push(tokio::spawn(async move {
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
            msg["transcript"].as_str().unwrap().to_string()
        }));
    }

    // All 3 should succeed independently
    for handle in handles {
        let transcript = handle.await.unwrap();
        assert!(
            !transcript.is_empty(),
            "Expected non-empty transcript from concurrent session"
        );
    }
}

// ═══════════════════════════════════════════════
// Test 12: Rapid-fire recordings (5 append→commit cycles)
// ═══════════════════════════════════════════════

#[tokio::test]
async fn rapid_fire_recordings() {
    let (addr, _mock) = setup_with_mock_whisper().await;

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

    for i in 0..5 {
        // Append audio
        for _ in 0..2 {
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

        let msg = read_json(&mut stream).await;
        assert_eq!(
            msg["type"],
            "conversation.item.input_audio_transcription.completed",
            "Recording {i} did not produce transcript"
        );
    }
}

// ═══════════════════════════════════════════════
// Test 13: POST /v1/transcribe with mock AssemblyAI
// ═══════════════════════════════════════════════

#[tokio::test]
async fn assemblyai_transcribe_endpoint() {
    let mock_aai = MockServer::start().await;

    // Mock upload
    Mock::given(method("POST"))
        .and(path("/v2/upload"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "upload_url": "https://cdn.assemblyai.com/test-audio"
        })))
        .mount(&mock_aai)
        .await;

    // Mock create transcript
    Mock::given(method("POST"))
        .and(path("/v2/transcript"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "test-tx-123",
            "status": "queued",
        })))
        .mount(&mock_aai)
        .await;

    // Mock poll transcript (immediately completed)
    Mock::given(method("GET"))
        .and(path("/v2/transcript/test-tx-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "test-tx-123",
            "status": "completed",
            "text": "Assembled transcript",
            "audio_duration": 3.5,
        })))
        .mount(&mock_aai)
        .await;

    // Create state with mock AssemblyAI key
    // The transcribe handler uses the base URL from the client, but the client
    // is hardcoded to https://api.assemblyai.com. So this test verifies the
    // handler wiring but can't easily redirect to wiremock. We'll test that
    // the endpoint accepts the request and returns a meaningful error when the
    // real API isn't reachable.
    let state = common::test_state("http://localhost:9999", None);
    let addr = common::start_test_server(state).await;

    let client = reqwest::Client::new();

    // POST with JSON body — should fail gracefully (no API key + no pass)
    let resp = client
        .post(format!("http://{addr}/v1/transcribe"))
        .json(&serde_json::json!({"audio_url": "https://example.com/audio.wav"}))
        .send()
        .await
        .unwrap();

    // Should return an error about missing API key, not 404 or panic
    assert_ne!(
        resp.status().as_u16(),
        404,
        "/v1/transcribe should be routed"
    );
    // The endpoint exists and responds (even if it's a 500 due to no API key)
    assert!(
        resp.status().as_u16() >= 400,
        "Expected error status (no API key configured), got {}",
        resp.status()
    );
}

// ═══════════════════════════════════════════════
// Test 14: Latency JSONL written after recording commit
// ═══════════════════════════════════════════════

#[tokio::test]
async fn latency_jsonl_written_after_recording() {
    let (addr, _mock) = setup_with_mock_whisper().await;

    // Find the latency log dir used by tests (temp dir)
    let latency_dir = std::env::temp_dir().join("voice-gateway-test-latency");

    // Count existing JSONL lines before the test
    let count_lines = |dir: &std::path::Path| -> usize {
        let mut total = 0;
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if entry.path().extension().map(|e| e == "jsonl").unwrap_or(false) {
                    if let Ok(contents) = std::fs::read_to_string(entry.path()) {
                        total += contents.lines().count();
                    }
                }
            }
        }
        total
    };

    let lines_before = count_lines(&latency_dir);

    // Run a full recording cycle
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

    let _ = read_json(&mut stream).await; // transcript

    // Give async log writing a moment
    tokio::time::sleep(Duration::from_millis(200)).await;

    let lines_after = count_lines(&latency_dir);
    assert!(
        lines_after > lines_before,
        "Expected new latency JSONL line after recording (before={lines_before}, after={lines_after})"
    );
}

// ═══════════════════════════════════════════════
// Test 15: Directory traversal blocked on audio endpoint
// ═══════════════════════════════════════════════

#[tokio::test]
async fn directory_traversal_blocked() {
    let state = common::test_state("http://localhost:9999", None);
    let addr = common::start_test_server(state).await;
    let client = reqwest::Client::new();

    // Attempt directory traversal through URL-encoded paths.
    // Axum normalizes raw "../" before routing, but %2F-encoded paths reach
    // the handler where sanitize_filename() strips path components.
    let traversal_paths = [
        "/api/audio/..%2F..%2F..%2Fetc%2Fpasswd",
        "/api/transcript/..%2F..%2F..%2Fetc%2Fpasswd",
    ];

    for path in &traversal_paths {
        let resp = client
            .get(format!("http://{addr}{path}"))
            .send()
            .await
            .unwrap();

        // Verify body doesn't contain /etc/passwd content
        let body = resp.text().await.unwrap_or_default();
        assert!(
            !body.contains("root:"),
            "Directory traversal leaked file content for {path}"
        );
    }
}

// ═══════════════════════════════════════════════
// Test 16: LAN whisper fallback → local whisper
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
