use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use crate::error::GatewayError;

const DEEPGRAM_HOST: &str = "api.deepgram.com";

/// Cached DNS resolution for api.deepgram.com.
/// Cleared on connection failure so the next attempt re-resolves.
static DNS_CACHE: tokio::sync::Mutex<Option<SocketAddr>> = tokio::sync::Mutex::const_new(None);

/// Resolve api.deepgram.com with DNS caching.
/// Prefers IPv4 addresses to avoid NAT64 TLS hostname mismatch issues.
/// Cache is invalidated on connection failure via `invalidate_dns_cache()`.
async fn resolve_deepgram() -> Result<SocketAddr, GatewayError> {
    {
        let cache = DNS_CACHE.lock().await;
        if let Some(addr) = *cache {
            return Ok(addr);
        }
    }

    let addrs: Vec<SocketAddr> = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::net::lookup_host(format!("{DEEPGRAM_HOST}:443")),
    )
    .await
    .map_err(|_| GatewayError::Deepgram(format!("DNS timeout for {DEEPGRAM_HOST}")))?
    .map_err(|e| GatewayError::Deepgram(format!("DNS lookup failed for {DEEPGRAM_HOST}: {e}")))?
    .collect();

    if addrs.is_empty() {
        return Err(GatewayError::Deepgram(format!(
            "No addresses for {DEEPGRAM_HOST}"
        )));
    }

    // Prefer IPv4 to avoid NAT64 (64:ff9b::/96) TLS hostname mismatch.
    // NAT64 translates IPv6→IPv4 at the network layer, but TLS SNI sees the
    // IPv6 address and the certificate is for api.deepgram.com → mismatch.
    let addr = addrs
        .iter()
        .find(|a| a.is_ipv4())
        .copied()
        .unwrap_or(addrs[0]);

    {
        let mut cache = DNS_CACHE.lock().await;
        *cache = Some(addr);
    }

    tracing::info!(%addr, "DNS resolved {DEEPGRAM_HOST} (cached)");
    Ok(addr)
}

/// Clear the DNS cache so the next connect does a fresh lookup.
async fn invalidate_dns_cache() {
    let mut cache = DNS_CACHE.lock().await;
    *cache = None;
}

/// Deepgram streaming API response.
#[derive(Debug, serde::Deserialize)]
pub struct Response {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub channel: Option<Channel>,
    pub is_final: Option<bool>,
}

#[derive(Debug, serde::Deserialize)]
pub struct Channel {
    pub alternatives: Vec<Alternative>,
}

#[derive(Debug, serde::Deserialize)]
pub struct Alternative {
    pub transcript: String,
    pub confidence: f64,
}

/// Write half of the Deepgram WebSocket, wrapped for shared access.
type WsSink = Arc<
    Mutex<
        futures_util::stream::SplitSink<
            WebSocketStream<tokio_native_tls::TlsStream<TcpStream>>,
            Message,
        >,
    >,
>;

/// Read half of the Deepgram WebSocket.
type WsStream =
    futures_util::stream::SplitStream<WebSocketStream<tokio_native_tls::TlsStream<TcpStream>>>;

/// A streaming connection to Deepgram's Nova-2 API.
///
/// Holds the write half; the read half is consumed by `read_loop()`.
/// The Go equivalent is `StreamingClient` in `deepgram/streaming.go`.
pub struct StreamingClient {
    sink: WsSink,
    closed: Arc<std::sync::atomic::AtomicBool>,
}

impl StreamingClient {
    /// Connect to Deepgram's streaming API.
    ///
    /// Uses cached DNS, explicit TLS SNI (always `api.deepgram.com`), and
    /// prefers IPv4 to avoid NAT64 hostname mismatch.
    /// Returns the client (write half) and the read stream (must be consumed by `read_loop`).
    pub async fn connect(
        api_key: &str,
        sample_rate: u32,
    ) -> Result<(Self, WsStream), GatewayError> {
        let sample_rate = if sample_rate == 0 { 24000 } else { sample_rate };

        let addr = resolve_deepgram().await?;

        let params = format!(
            "model=nova-2&encoding=linear16&sample_rate={sample_rate}&channels=1\
             &interim_results=true&punctuate=true&smart_format=true&endpointing=300"
        );

        // Use the real hostname in the URI so tungstenite sends correct Host
        // header and SNI. We connect TCP to the resolved IP manually below.
        let url = format!("wss://{DEEPGRAM_HOST}/v1/listen?{params}");

        let request = tokio_tungstenite::tungstenite::http::Request::builder()
            .uri(&url)
            .header("Authorization", format!("Token {api_key}"))
            .header("Host", DEEPGRAM_HOST)
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header(
                "Sec-WebSocket-Key",
                tokio_tungstenite::tungstenite::handshake::client::generate_key(),
            )
            .body(())
            .map_err(|e| GatewayError::Deepgram(format!("failed to build request: {e}")))?;

        // Establish TCP to the resolved IP, then wrap with TLS using the real
        // hostname for SNI. This is the Rust equivalent of Go's
        //   TLSClientConfig{ServerName: "api.deepgram.com"}
        // and avoids the NAT64 hostname mismatch where native-tls would try to
        // verify the certificate against an IPv6 address instead of the domain.
        let tls_connector = native_tls::TlsConnector::new()
            .map_err(|e| GatewayError::Deepgram(format!("TLS connector failed: {e}")))?;
        let tls_connector = tokio_native_tls::TlsConnector::from(tls_connector);

        let tcp_stream = tokio::time::timeout(
            Duration::from_secs(3),
            TcpStream::connect(addr),
        )
        .await
        .map_err(|_| GatewayError::Deepgram(format!("TCP connect timeout to {addr}")))?
        .map_err(|e| {
            tokio::spawn(async { invalidate_dns_cache().await });
            GatewayError::Deepgram(format!("TCP connect failed to {addr}: {e}"))
        })?;

        let tls_stream = tokio::time::timeout(
            Duration::from_secs(3),
            tls_connector.connect(DEEPGRAM_HOST, tcp_stream),
        )
        .await
        .map_err(|_| GatewayError::Deepgram("TLS handshake timeout".into()))?
        .map_err(|e| {
            tokio::spawn(async { invalidate_dns_cache().await });
            GatewayError::Deepgram(format!("TLS handshake failed: {e}"))
        })?;

        let (ws_stream, _response) = tokio::time::timeout(
            Duration::from_secs(3),
            tokio_tungstenite::client_async(request, tls_stream),
        )
        .await
        .map_err(|_| GatewayError::Deepgram("WebSocket handshake timeout".into()))?
        .map_err(|e| {
            tokio::spawn(async { invalidate_dns_cache().await });
            GatewayError::Deepgram(format!("WebSocket handshake failed: {e}"))
        })?;

        tracing::info!(sample_rate, "Connected to Deepgram streaming API");

        let (sink, stream) = ws_stream.split();
        let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));

        Ok((
            Self {
                sink: Arc::new(Mutex::new(sink)),
                closed,
            },
            stream,
        ))
    }

    /// Send raw PCM16 audio bytes to Deepgram.
    pub async fn send_audio(&self, pcm16: &[u8]) -> Result<(), GatewayError> {
        if self.is_closed() {
            return Err(GatewayError::Deepgram("connection closed".into()));
        }
        let mut sink = self.sink.lock().await;
        sink.send(Message::Binary(pcm16.to_vec().into()))
            .await
            .map_err(|e| GatewayError::Deepgram(format!("send audio failed: {e}")))
    }

    /// Send Finalize message to flush remaining audio through Deepgram's pipeline.
    pub async fn finalize(&self) -> Result<(), GatewayError> {
        if self.is_closed() {
            return Err(GatewayError::Deepgram("connection closed".into()));
        }
        let msg = serde_json::json!({"type": "Finalize"});
        let mut sink = self.sink.lock().await;
        sink.send(Message::Text(msg.to_string().into()))
            .await
            .map_err(|e| GatewayError::Deepgram(format!("finalize failed: {e}")))
    }

    /// Send CloseStream and close the WebSocket connection.
    pub async fn close(&self) {
        if self
            .closed
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return; // already closed
        }
        let mut sink = self.sink.lock().await;
        // Best-effort CloseStream message
        let msg = serde_json::json!({"type": "CloseStream"});
        let _ = sink.send(Message::Text(msg.to_string().into())).await;
        let _ = sink.close().await;
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Read loop for Deepgram responses.
///
/// Calls `on_interim` for partial results and `on_final` for final transcript segments.
/// Returns when the connection closes or an error occurs.
///
/// The caller should spawn this as a task. When it returns, the Deepgram connection
/// is dead and `deepgram_client` should be set to `None` (matching the Go fix where
/// the ReadLoop goroutine sets `s.deepgramClient = nil` on exit).
pub async fn read_loop<FI, FF>(mut stream: WsStream, on_interim: FI, on_final: FF)
where
    FI: Fn(String) + Send,
    FF: Fn(String) + Send,
{
    while let Some(msg_result) = stream.next().await {
        let msg = match msg_result {
            Ok(msg) => msg,
            Err(e) => {
                // Check if this is a normal close
                if matches!(
                    e,
                    tokio_tungstenite::tungstenite::Error::ConnectionClosed
                        | tokio_tungstenite::tungstenite::Error::AlreadyClosed
                ) {
                    tracing::debug!("Deepgram connection closed normally");
                } else {
                    tracing::warn!(error = %e, "Deepgram read error");
                }
                return;
            }
        };

        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => return,
            _ => continue,
        };

        let resp: Response = match serde_json::from_str(&text) {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!(error = %e, "Failed to parse Deepgram response");
                continue;
            }
        };

        if resp.msg_type != "Results" {
            continue;
        }

        let channel = match resp.channel {
            Some(c) => c,
            None => continue,
        };

        if channel.alternatives.is_empty() {
            continue;
        }

        let transcript = channel.alternatives[0].transcript.trim().to_string();
        if transcript.is_empty() {
            continue;
        }

        if resp.is_final.unwrap_or(false) {
            on_final(transcript);
        } else {
            on_interim(transcript);
        }
    }
}
