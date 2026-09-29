//! Reproductions of the 2026-09-30 voice-typing data loss (see
//! docs/issues/2026-09-30-voice-data-loss.md).
//!
//! 1. Deepgram stream stalls after commit (sends finals, then never closes):
//!    the gateway used to wait 1050 s for the kernel TCP timeout, stop reading
//!    hyprwhspr's socket, and never deliver the transcript. Now the commit must
//!    answer within a few seconds, via a batch re-transcription of the audio.
//! 2. The client socket dies mid-recording: the buffered audio used to be
//!    dropped in cleanup(). Now it must be saved as a WAV.
//!
//! Own test binary: it sets process-wide env vars (Deepgram URL overrides,
//! RECORDINGS_DIR), so it must not share a process with e2e_test.rs.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 1 s of a 440 Hz tone, PCM16 @ 24 kHz, loud enough to pass the silence gate.
fn tone_pcm16(seconds: f64) -> Vec<u8> {
    let n = (24000.0 * seconds) as usize;
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n {
        let v = (3000.0 * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / 24000.0).sin()) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Fake Deepgram streaming server: sends one final after the first audio,
/// then ignores Finalize/CloseStream and never closes (a stalled connection).
async fn start_stalling_deepgram() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (tcp, _) = match listener.accept().await {
                Ok(x) => x,
                Err(_) => return,
            };
            tokio::spawn(async move {
                let ws = match tokio_tungstenite::accept_async(tcp).await {
                    Ok(ws) => ws,
                    Err(_) => return,
                };
                let (mut sink, mut stream) = ws.split();
                let mut sent_final = false;
                // Read until the peer goes away, but never answer a close.
                while let Some(Ok(msg)) = stream.next().await {
                    if let Message::Binary(_) = msg {
                        if !sent_final {
                            sent_final = true;
                            let r = serde_json::json!({
                                "type": "Results", "is_final": true,
                                "channel": {"alternatives": [{"transcript": "partial stream words", "confidence": 0.9}]}
                            });
                            let _ = sink.send(Message::Text(r.to_string().into())).await;
                        }
                    }
                    if let Message::Close(_) = msg {
                        // Stall: do not reply, keep the TCP connection open.
                        tokio::time::sleep(Duration::from_secs(3600)).await;
                    }
                }
                tokio::time::sleep(Duration::from_secs(3600)).await;
            });
        }
    });
    addr
}

fn state_with_key() -> Arc<voice_type::state::AppState> {
    let base = common::test_state("http://127.0.0.1:1/unused", None);
    let mut s = (*base).clone();
    s.deepgram_api_key = Some("test-key".into());
    Arc::new(s)
}

fn wavs_in(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default();
    v.retain(|p| p.extension().map(|e| e == "wav").unwrap_or(false));
    v.sort();
    v
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stalled_deepgram_and_dropped_client_never_lose_audio() {
    let rec_dir = tempfile::tempdir().unwrap();
    let txt_dir = tempfile::tempdir().unwrap();
    let dg_addr = start_stalling_deepgram().await;
    let batch = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "results": {"channels": [{"alternatives": [{"transcript": "the full rescued transcript"}]}]}
        })))
        .mount(&batch)
        .await;
    // SAFETY: single test in this binary; set before any gateway task reads them.
    unsafe {
        std::env::set_var("DEEPGRAM_STREAM_URL_OVERRIDE", format!("ws://{dg_addr}/v1/listen"));
        std::env::set_var("DEEPGRAM_BATCH_URL_OVERRIDE", format!("{}/v1/listen", batch.uri()));
        std::env::set_var("RECORDINGS_DIR", rec_dir.path());
        std::env::set_var("TRANSCRIPTS_DIR", txt_dir.path());
        std::env::set_var("STT_PROVIDER", "deepgram"); // used when ~/.config/voice-typing/stt-provider is absent
    }

    let addr = common::start_test_server(state_with_key()).await;
    let url = format!("ws://{addr}/v1/realtime");

    // ── Case 1: Deepgram stalls after commit ────────────────────────────
    let (ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let (mut sink, mut stream) = ws.split();
    let _created = stream.next().await; // session.created
    sink.send(Message::Text(r#"{"type":"session.update","session":{}}"#.into())).await.unwrap();
    let _updated = stream.next().await; // session.updated

    let pcm = tone_pcm16(2.0);
    for chunk in pcm.chunks(3072) {
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, chunk);
        let ev = serde_json::json!({"type": "input_audio_buffer.append", "audio": b64});
        sink.send(Message::Text(ev.to_string().into())).await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let t0 = Instant::now();
    sink.send(Message::Text(r#"{"type":"input_audio_buffer.commit"}"#.into())).await.unwrap();

    let transcript = loop {
        let msg = tokio::time::timeout(Duration::from_secs(20), stream.next())
            .await
            .expect("gateway never answered the commit (stalled like 2026-09-30)")
            .expect("stream ended")
            .expect("ws error");
        if let Message::Text(t) = msg {
            let v: serde_json::Value = serde_json::from_str(&t).unwrap();
            if v["type"] == "conversation.item.input_audio_transcription.completed" {
                break v["transcript"].as_str().unwrap_or("").to_string();
            }
        }
    };
    let elapsed = t0.elapsed();
    eprintln!("case 1: commit answered in {elapsed:?}: {transcript:?}");
    assert!(elapsed < Duration::from_secs(12), "commit took {elapsed:?}");
    assert_eq!(transcript, "the full rescued transcript");
    let wavs = wavs_in(rec_dir.path());
    assert_eq!(wavs.len(), 1, "audio must be saved: {wavs:?}");

    // ── Case 2: client socket dies mid-recording ─────────────────────────
    for chunk in tone_pcm16(1.5).chunks(3072) {
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, chunk);
        let ev = serde_json::json!({"type": "input_audio_buffer.append", "audio": b64});
        sink.send(Message::Text(ev.to_string().into())).await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    drop(sink);
    drop(stream); // abrupt close, no commit

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let wavs = wavs_in(rec_dir.path());
        if wavs.len() >= 2 {
            let size = std::fs::metadata(&wavs[wavs.len() - 1]).unwrap().len();
            eprintln!("case 2: saved {wavs:?} (last {size} bytes)");
            break;
        }
        assert!(Instant::now() < deadline, "uncommitted audio was not saved on disconnect");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let total: u64 = wavs_in(rec_dir.path()).iter().map(|p| std::fs::metadata(p).unwrap().len()).sum();
    // 2.0 s + 1.5 s of PCM16 @ 24 kHz plus headers
    assert!(total >= (3.5 * 48000.0) as u64, "saved audio too small: {total}");
}
