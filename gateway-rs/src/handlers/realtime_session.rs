use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use futures_util::stream::SplitSink;
use futures_util::SinkExt;
use tokio::sync::Mutex as TokioMutex;

use crate::audio;
use crate::deepgram::streaming::StreamingClient;
use crate::logging::latency::{LatencyLogger, LatencyMetrics};
use crate::logging::session_log::SessionLog;
use crate::spelling::{self, CustomSpelling};
use crate::state::AppState;
use crate::transcription::fallback;

use super::realtime::RealtimeEvent;

const DEEPGRAM_RETRY_INTERVAL: Duration = Duration::from_secs(5);

/// Per-recording structured log (single summary line).
struct RecordingLog {
    id: String,
    start_time: Instant,
    audio_chunks: u32,
    audio_duration: f64,
    offline_at_start: bool,
    reconnect_attempts: u32,
    reconnect_success: bool,
    backend: String,
    connect_time: Duration,
    transcribe_time: Duration,
    total_time: Duration,
    transcript_len: usize,
    err: String,
}

impl RecordingLog {
    fn summary(&self) -> String {
        let mut parts = vec![
            format!("[{}] DONE", self.id),
            format!("backend={}", self.backend),
            format!("audio={:.1}s", self.audio_duration),
            format!("chunks={}", self.audio_chunks),
            format!("connect={:?}", self.connect_time),
            format!("transcribe={:?}", self.transcribe_time),
            format!("total={:?}", self.total_time),
            format!("transcript_len={}", self.transcript_len),
            format!("offline_at_start={}", self.offline_at_start),
            format!("reconnects={}", self.reconnect_attempts),
        ];
        if self.reconnect_attempts > 0 {
            parts.push(format!("reconnect_success={}", self.reconnect_success));
        }
        if !self.err.is_empty() {
            parts.push(format!("error={:?}", self.err));
        }
        parts.join(" ")
    }
}

/// State machine for a single WebSocket session.
///
/// Manages: client WS sink, Deepgram streaming connection, audio buffer,
/// transcript accumulation, offline/reconnection state, and session logging.
pub struct RealtimeSession {
    client_sink: Arc<TokioMutex<SplitSink<WebSocket, Message>>>,
    deepgram_api_key: Option<String>,
    local_whisper_url: String,
    lan_whisper_url: Option<String>,
    spellings: Vec<CustomSpelling>,

    /// Deepgram streaming client. Wrapped in Arc<TokioMutex<Option>> because:
    /// 1. The read_loop task sets it to None when the connection dies (fix from 8d5cb9a)
    /// 2. Multiple async tasks may need to check/use it
    deepgram_client: Arc<TokioMutex<Option<StreamingClient>>>,

    /// Signal from the Deepgram read loop that it has exited.
    read_done: Option<tokio::sync::oneshot::Receiver<()>>,

    /// Accumulated raw PCM16 audio bytes.
    audio_buffer: Vec<u8>,

    /// Accumulated Deepgram final transcript segments.
    /// Uses std::sync::Mutex (not tokio) because the on_final callback is sync.
    finals: Arc<std::sync::Mutex<Vec<String>>>,

    /// Whether session.update has been received.
    session_ready: bool,

    /// Deepgram unavailable — use local whisper.
    offline_mode: Arc<AtomicBool>,
    last_deepgram_try: Instant,
    reconnecting: Arc<AtomicBool>,

    /// Per-recording structured log.
    current_rec: Option<RecordingLog>,
    recording_seq: u32,

    /// Per-recording session timeline log.
    current_sess_log: Option<SessionLog>,

    /// JSONL latency logger (shared across all sessions via Arc).
    latency_logger: Arc<LatencyLogger>,

    /// WebSocket session identifier (for latency log correlation).
    ws_session_id: String,
}

impl RealtimeSession {
    pub fn new(
        ws_sink: SplitSink<WebSocket, Message>,
        state: &AppState,
        session_id: String,
    ) -> Self {
        Self {
            client_sink: Arc::new(TokioMutex::new(ws_sink)),
            deepgram_api_key: state.deepgram_api_key.clone(),
            local_whisper_url: state.local_whisper_url.clone(),
            lan_whisper_url: state.lan_whisper_url.clone(),
            spellings: state.custom_spelling.clone(),
            deepgram_client: Arc::new(TokioMutex::new(None)),
            read_done: None,
            audio_buffer: Vec::new(),
            finals: Arc::new(std::sync::Mutex::new(Vec::new())),
            session_ready: false,
            offline_mode: Arc::new(AtomicBool::new(false)),
            last_deepgram_try: Instant::now() - DEEPGRAM_RETRY_INTERVAL,
            reconnecting: Arc::new(AtomicBool::new(false)),
            current_rec: None,
            recording_seq: 0,
            current_sess_log: None,
            latency_logger: Arc::clone(&state.latency_logger),
            ws_session_id: session_id,
        }
    }

    /// Send a JSON value to the client over the WebSocket.
    pub async fn send_json(&self, value: &serde_json::Value) -> Result<(), String> {
        let text = serde_json::to_string(value).map_err(|e| e.to_string())?;
        let mut sink = self.client_sink.lock().await;
        sink.send(Message::Text(text.into()))
            .await
            .map_err(|e| format!("send to client failed: {e}"))
    }

    async fn send_to_client(&self, value: &serde_json::Value) {
        if let Err(e) = self.send_json(value).await {
            tracing::error!(error = %e, "Failed to send to client");
        }
    }

    // ──────────────────────────────────────────────
    // Deepgram connection management
    // ──────────────────────────────────────────────

    /// Open a new Deepgram streaming connection and start the read loop.
    async fn connect_deepgram(&mut self) -> Result<(), String> {
        // Close existing connection if any
        self.close_deepgram().await;

        let api_key = match &self.deepgram_api_key {
            Some(k) => k.clone(),
            None => {
                self.offline_mode.store(true, Ordering::SeqCst);
                return Err("no Deepgram API key".into());
            }
        };

        let connect_start = Instant::now();
        let result = StreamingClient::connect(&api_key, 24000).await;
        let connect_elapsed = connect_start.elapsed();

        if let Some(rec) = &mut self.current_rec {
            rec.connect_time += connect_elapsed;
        }

        match result {
            Err(e) => {
                self.offline_mode.store(true, Ordering::SeqCst);
                self.last_deepgram_try = Instant::now();
                tracing::warn!(error = %e, "Deepgram unavailable, switching to offline mode");
                if let Some(sl) = &mut self.current_sess_log {
                    sl.add_event("DEEPGRAM", &format!("connect FAILED after {connect_elapsed:?}: {e}"));
                    sl.dg_connect_ms = -1;
                }
                Err(e.to_string())
            }
            Ok((client, read_stream)) => {
                let was_offline = self.offline_mode.swap(false, Ordering::SeqCst);
                if was_offline {
                    tracing::info!("Deepgram connectivity restored, back online");
                }

                if let Some(sl) = &mut self.current_sess_log {
                    sl.add_event("DEEPGRAM", &format!("connected in {connect_elapsed:?}"));
                    sl.dg_connect_ms = connect_elapsed.as_millis() as i64;
                }

                // Reset finals for new utterance
                {
                    let mut finals = self.finals.lock().unwrap();
                    finals.clear();
                }

                // Store client
                {
                    let mut dg = self.deepgram_client.lock().await;
                    *dg = Some(client);
                }

                // Start read loop in a spawned task.
                // When it exits, it sets deepgram_client to None (matching Go fix 8d5cb9a).
                let (done_tx, done_rx) = tokio::sync::oneshot::channel();
                self.read_done = Some(done_rx);

                let finals_clone = Arc::clone(&self.finals);
                let dg_client_clone = Arc::clone(&self.deepgram_client);

                tokio::spawn(async move {
                    crate::deepgram::streaming::read_loop(
                        read_stream,
                        |text| {
                            tracing::info!(text = %text, "Deepgram interim");
                        },
                        {
                            let finals = Arc::clone(&finals_clone);
                            move |text| {
                                tracing::info!(text = %text, "Deepgram final");
                                let mut f = finals.lock().unwrap();
                                f.push(text);
                            }
                        },
                    )
                    .await;

                    // Mark connection as dead (Go fix 8d5cb9a)
                    let mut dg = dg_client_clone.lock().await;
                    *dg = None;
                    tracing::debug!("Deepgram read loop exited, client set to None");

                    let _ = done_tx.send(());
                });

                Ok(())
            }
        }
    }

    fn is_offline(&self) -> bool {
        self.offline_mode.load(Ordering::SeqCst)
    }

    fn should_retry_deepgram(&self) -> bool {
        self.is_offline() && self.last_deepgram_try.elapsed() >= DEEPGRAM_RETRY_INTERVAL
    }

    /// Kick off a background Deepgram reconnection probe.
    /// Non-blocking: audio processing continues during the attempt.
    fn try_async_reconnect(&mut self) {
        if !self.is_offline() {
            return;
        }
        if self.last_deepgram_try.elapsed() < DEEPGRAM_RETRY_INTERVAL {
            return;
        }
        if self.reconnecting.load(Ordering::SeqCst) {
            return;
        }

        self.reconnecting.store(true, Ordering::SeqCst);
        self.last_deepgram_try = Instant::now();

        if let Some(rec) = &mut self.current_rec {
            rec.reconnect_attempts += 1;
        }

        let api_key = match &self.deepgram_api_key {
            Some(k) => k.clone(),
            None => return,
        };
        let reconnecting = Arc::clone(&self.reconnecting);
        let offline_mode = Arc::clone(&self.offline_mode);

        tokio::spawn(async move {
            tracing::info!("Async reconnect probe started");
            match StreamingClient::connect(&api_key, 24000).await {
                Err(e) => {
                    tracing::info!(error = %e, "Async reconnect probe failed");
                }
                Ok((client, _stream)) => {
                    // Connected! Close probe — next recording will use a fresh connection.
                    client.close().await;
                    offline_mode.store(false, Ordering::SeqCst);
                    tracing::info!("Async reconnect probe succeeded — next recording will be online");
                }
            }
            reconnecting.store(false, Ordering::SeqCst);
        });
    }

    /// Cleanly shut down the current Deepgram connection.
    async fn close_deepgram(&mut self) {
        let client = {
            let mut dg = self.deepgram_client.lock().await;
            dg.take()
        };
        if let Some(client) = client {
            client.close().await;
            // Wait for read loop to exit
            if let Some(done_rx) = self.read_done.take() {
                let _ = done_rx.await;
            }
        }
    }

    // ──────────────────────────────────────────────
    // Message handlers
    // ──────────────────────────────────────────────

    pub async fn handle_session_update(&mut self) {
        tracing::info!(offline = %self.is_offline(), "session.update received");
        self.session_ready = true;

        // Do NOT eagerly connect to Deepgram here. The connection would sit idle
        // and Deepgram closes idle connections after ~12s. Connect lazily on first
        // audio chunk in handle_audio_append.

        let backend = if self.is_offline() {
            "local-whisper"
        } else {
            "nova-2"
        };

        self.send_to_client(&serde_json::json!({
            "type": "session.updated",
            "session": {
                "model": backend,
                "input_audio_format": "pcm16",
            },
        }))
        .await;
    }

    pub async fn handle_audio_append(&mut self, event: &RealtimeEvent) {
        let audio_b64 = match &event.audio {
            Some(a) if !a.is_empty() => a,
            _ => return,
        };

        if !self.session_ready {
            tracing::warn!("Audio received before session.update — ignoring");
            return;
        }

        // Decode base64 audio
        let pcm = match audio::decode_base64_audio(audio_b64) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "Base64 decode failed");
                return;
            }
        };

        // Accumulate audio
        let was_empty = self.audio_buffer.is_empty();
        self.audio_buffer.extend_from_slice(&pcm);
        let buf_len = self.audio_buffer.len();

        // Start new recording log on first chunk
        if was_empty {
            self.recording_seq += 1;
            let rec_id = format!("rec-{:03}", self.recording_seq);
            let offline = self.is_offline();

            self.current_rec = Some(RecordingLog {
                id: rec_id.clone(),
                start_time: Instant::now(),
                audio_chunks: 0,
                audio_duration: 0.0,
                offline_at_start: offline,
                reconnect_attempts: 0,
                reconnect_success: false,
                backend: String::new(),
                connect_time: Duration::ZERO,
                transcribe_time: Duration::ZERO,
                total_time: Duration::ZERO,
                transcript_len: 0,
                err: String::new(),
            });

            let mut sl = SessionLog::new(rec_id.clone());
            sl.add_event("GATEWAY", &format!("{rec_id} started (offline={offline})"));
            self.current_sess_log = Some(sl);

            tracing::info!(rec_id = %rec_id, offline = %offline, "Recording started");
        }

        // Count chunks and track audio flow
        if let Some(rec) = &mut self.current_rec {
            rec.audio_chunks += 1;
        }
        if let Some(sl) = &mut self.current_sess_log {
            sl.py_chunks += 1;
            sl.gw_bytes += pcm.len() as u64;
            if sl.first_audio_ms == -1 {
                sl.first_audio_ms = sl.start_instant.elapsed().as_millis() as i64;
                sl.add_event("GATEWAY", &format!("first audio chunk: {} bytes", pcm.len()));
            }
        }

        // If offline, try async reconnection and accumulate for whisper fallback
        if self.is_offline() {
            self.try_async_reconnect();
            // Log periodically (~1s of audio = 48000 bytes)
            if buf_len % (48000 * 2) < pcm.len() {
                tracing::info!(
                    buf_bytes = buf_len,
                    audio_secs = format_args!("{:.1}", buf_len as f64 / 48000.0),
                    "Offline: accumulating audio"
                );
            }
            return;
        }

        // Lazily connect to Deepgram on first audio chunk (or after previous died)
        let needs_connect = {
            let dg = self.deepgram_client.lock().await;
            dg.is_none()
        };
        if needs_connect {
            tracing::info!("Connecting to Deepgram for new utterance");
            if let Err(e) = self.connect_deepgram().await {
                tracing::warn!(error = %e, "Failed to connect to Deepgram (will use offline fallback at commit)");
                return;
            }
        }

        // Forward audio to Deepgram
        let dg = self.deepgram_client.lock().await;
        if let Some(client) = dg.as_ref() {
            if let Err(e) = client.send_audio(&pcm).await {
                tracing::warn!(error = %e, "Failed to send audio to Deepgram");
            }
        }
    }

    pub async fn handle_audio_commit(&mut self) {
        let offline = self.is_offline();
        let dg_nil = {
            let dg = self.deepgram_client.lock().await;
            dg.is_none()
        };
        tracing::info!(offline = %offline, dg_nil = %dg_nil, "input_audio_buffer.commit received");

        if let Some(sl) = &mut self.current_sess_log {
            sl.add_event("GATEWAY", "commit received");
        }

        // Snapshot audio data
        let audio_data = self.audio_buffer.clone();
        let audio_duration = audio_data.len() as f64 / 48000.0;
        tracing::info!(
            buf_bytes = audio_data.len(),
            audio_secs = format_args!("{:.1}", audio_duration),
            "Audio buffer at commit"
        );

        if let Some(rec) = &mut self.current_rec {
            rec.audio_duration = audio_duration;
        }

        let transcribe_start = Instant::now();
        let (full_transcript, backend);

        if offline || dg_nil {
            // OFFLINE PATH
            tracing::info!(
                offline = %offline,
                dg_nil = %dg_nil,
                lan_url = ?self.lan_whisper_url,
                local_url = %self.local_whisper_url,
                "Taking OFFLINE transcription path"
            );
            if let Some(sl) = &mut self.current_sess_log {
                sl.add_event("GATEWAY", "taking OFFLINE transcription path");
            }
            self.close_deepgram().await;

            let (text, be) = fallback::transcribe_offline(
                &audio_data,
                self.lan_whisper_url.as_deref(),
                &self.local_whisper_url,
            )
            .await;
            full_transcript = text;
            backend = be;
        } else {
            // ONLINE PATH (Deepgram)
            tracing::info!("Taking ONLINE transcription path (Deepgram)");
            if let Some(sl) = &mut self.current_sess_log {
                sl.add_event("GATEWAY", "taking ONLINE transcription path (Deepgram)");
            }
            let (text, be) = self.transcribe_deepgram().await;
            full_transcript = text;
            backend = be;
        }

        let transcribe_elapsed = transcribe_start.elapsed();

        // Apply custom spelling replacements
        let full_transcript = spelling::apply_replacements(&full_transcript, &self.spellings);
        tracing::info!(backend = %backend, transcript = %full_transcript, "Full transcript");

        // Send transcription result to hyprwhspr
        self.send_to_client(&serde_json::json!({
            "type": "conversation.item.input_audio_transcription.completed",
            "item_id": format!("item_{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()),
            "content_index": 0,
            "transcript": full_transcript,
        }))
        .await;

        // Emit structured recording summary
        if let Some(rec) = &mut self.current_rec {
            rec.backend = backend.clone();
            rec.transcribe_time = transcribe_elapsed;
            rec.total_time = rec.start_time.elapsed();
            rec.transcript_len = full_transcript.len();
            tracing::info!("{}", rec.summary());
        }

        // Log latency metrics to JSONL
        {
            let mut metrics = LatencyMetrics::new();
            metrics.session_id = self.ws_session_id.clone();
            metrics.recording_number = Some(self.recording_seq);
            metrics.audio_duration_s = audio_duration;
            metrics.audio_bytes = Some(audio_data.len());
            metrics.backend = backend.clone();
            metrics.transcript_length = full_transcript.len();
            metrics.transcription_ms = Some(transcribe_elapsed.as_millis() as i64);
            metrics.success = !full_transcript.is_empty();

            if let Some(rec) = &self.current_rec {
                metrics.deepgram_connect_ms = Some(rec.connect_time.as_millis() as i64);
                metrics.total_commit_ms = Some(rec.total_time.as_millis() as i64);
            }

            self.latency_logger.log(&metrics);
        }

        // Write session log
        if let Some(sl) = &mut self.current_sess_log {
            sl.end_time = Some(std::time::SystemTime::now());
            sl.transcribe_ms = transcribe_elapsed.as_millis() as i64;
            sl.total_ms = sl.start_instant.elapsed().as_millis() as i64;
            sl.transcript = full_transcript.clone();
            sl.status = if !full_transcript.is_empty() {
                "OK".to_string()
            } else {
                "FAILED".to_string()
            };
            sl.add_event(
                "GATEWAY",
                &format!("commit complete: backend={backend}, transcript={} chars", full_transcript.len()),
            );
            sl.write_last_session(&backend);
        }
        // Write full log file in background
        if let Some(sl) = self.current_sess_log.take() {
            tokio::task::spawn_blocking(move || sl.write_to_file());
        }

        // Clear state
        self.audio_buffer.clear();
        self.current_rec = None;

        // Archive audio and transcript in background
        let audio_for_archive = audio_data;
        let transcript_for_archive = full_transcript;
        let backend_for_archive = backend.clone();
        tokio::task::spawn_blocking(move || {
            audio::archive_recording(&audio_for_archive, &transcript_for_archive, &backend_for_archive);
        });

        // If offline, try async reconnection for next recording
        if self.is_offline() {
            self.try_async_reconnect();
        }
    }

    /// Finalize Deepgram stream and collect transcript.
    async fn transcribe_deepgram(&mut self) -> (String, String) {
        // Send Finalize to flush remaining audio
        {
            let dg = self.deepgram_client.lock().await;
            if let Some(client) = dg.as_ref() {
                if let Err(e) = client.finalize().await {
                    tracing::warn!(error = %e, "Finalize failed");
                }
            }
        }

        // Wait for Deepgram to send back remaining finals
        tokio::time::sleep(Duration::from_millis(1500)).await;

        // Close connection and wait for read loop
        self.close_deepgram().await;

        // Collect full transcript
        let full_transcript = {
            let mut finals = self.finals.lock().unwrap();
            let text = finals.join(" ");
            finals.clear();
            text
        };

        (full_transcript, "deepgram".to_string())
    }

    pub async fn handle_audio_clear(&mut self) {
        let buf_len = self.audio_buffer.len();
        tracing::info!(buf_bytes = buf_len, "input_audio_buffer.clear received");

        // Early-clear diagnostic (< 2s after recording start = likely double-start bug)
        if let Some(sl) = &self.current_sess_log {
            let elapsed = sl.start_instant.elapsed();
            if elapsed < Duration::from_secs(2) {
                tracing::warn!(
                    elapsed_secs = format_args!("{:.1}", elapsed.as_secs_f64()),
                    "Clear shortly after start — likely double-start bug"
                );
                if let Some(sl) = &mut self.current_sess_log {
                    sl.add_event(
                        "GATEWAY",
                        &format!(
                            "WARNING: early clear at T+{:.1}s — likely double-start race condition",
                            elapsed.as_secs_f64()
                        ),
                    );
                }
            }
        }

        // Do NOT auto-commit on clear. Discard + archive for debugging + log as ABORTED.
        if buf_len > 0 {
            let audio_duration = buf_len as f64 / 48000.0;
            tracing::warn!(
                buf_bytes = buf_len,
                audio_secs = format_args!("{:.1}", audio_duration),
                "Discarding audio on clear — NOT auto-committing"
            );

            // Archive audio for debugging (no transcription)
            let audio_data = self.audio_buffer.clone();
            tokio::task::spawn_blocking(move || {
                audio::archive_recording(&audio_data, "", "cleared");
            });

            if let Some(sl) = &mut self.current_sess_log {
                sl.add_event(
                    "GATEWAY",
                    &format!("clear: discarded {buf_len} bytes / {audio_duration:.1}s (not auto-committed)"),
                );
            }
        }

        // Write session log as ABORTED
        if let Some(sl) = &mut self.current_sess_log {
            sl.end_time = Some(std::time::SystemTime::now());
            sl.status = "ABORTED".to_string();
            if buf_len > 0 {
                sl.add_event(
                    "GATEWAY",
                    &format!("clear: discarded {buf_len} bytes (not auto-committed)"),
                );
            } else {
                sl.add_event("GATEWAY", "clear received with empty buffer");
            }
            sl.write_last_session("none");
        }
        if let Some(sl) = self.current_sess_log.take() {
            tokio::task::spawn_blocking(move || sl.write_to_file());
        }

        // Clear all state
        self.audio_buffer.clear();
        self.current_rec = None;

        // Close Deepgram (will reconnect lazily on first append)
        self.close_deepgram().await;

        // Clear finals
        {
            let mut finals = self.finals.lock().unwrap();
            finals.clear();
        }
    }

    pub fn handle_session_event(&mut self, event: &RealtimeEvent) {
        if let Some(sl) = &mut self.current_sess_log {
            let layer = event
                .layer
                .as_deref()
                .unwrap_or("PYTHON");
            let event_text = event
                .event
                .as_deref()
                .unwrap_or("");
            sl.add_event(layer, event_text);
        }
    }

    /// Cleanup on WebSocket disconnect.
    pub async fn cleanup(&mut self) {
        if let Some(sl) = &mut self.current_sess_log {
            sl.end_time = Some(std::time::SystemTime::now());
            if sl.status.is_empty() {
                sl.status = "ABORTED".to_string();
            }
            sl.add_event("GATEWAY", "WebSocket closed — session ended");
        }
        if let Some(sl) = self.current_sess_log.take() {
            // Write synchronously during cleanup to ensure it completes
            sl.write_to_file();
        }
        self.close_deepgram().await;
    }
}
