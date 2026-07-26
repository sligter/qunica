//! Legacy HTTP+SSE transport.
//!
//! # Wire framing
//!
//! Two channels. The client opens a long-lived `GET` on the configured URL with
//! `Accept: text/event-stream`; the server's first event is
//! `event: endpoint` whose `data` is the URL to `POST` messages to (usually
//! relative, so it is resolved against the `GET` URL). From then on the client
//! `POST`s JSON-RPC requests — the server answers `202 Accepted` with no body —
//! and every response and notification comes back as a `message` event on the
//! open `GET` stream.
//!
//! Because responses are demultiplexed off a single shared stream, a background
//! reader task owns it and routes each response to the matching pending request
//! by JSON-RPC id, the same arrangement the stdio transport uses.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Arc,
    },
    time::Duration,
};

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{
    header::{HeaderMap, ACCEPT, CONTENT_TYPE},
    Client,
};
use serde_json::Value;
use tokio::{
    sync::{oneshot, Mutex},
    task::JoinHandle,
    time::timeout,
};

use crate::mcp::{
    config::McpServerConfig,
    protocol::{notification_envelope, request_envelope},
    McpError,
};

use super::http::{build_headers, describe_request_error, status_error};

/// How long to wait for the `endpoint` event before giving up on the handshake.
const ENDPOINT_TIMEOUT: Duration = Duration::from_secs(30);

/// Cap on bytes read from the event stream, so a server that streams forever
/// cannot grow the buffer without bound.
const MAX_STREAM_BYTES: usize = 32 * 1024 * 1024;

type PendingMap = Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>;

/// A legacy HTTP+SSE MCP connection.
pub struct SseTransport {
    client: Client,
    headers: HeaderMap,
    /// The `POST` endpoint announced by the server's `endpoint` event.
    message_url: String,
    pending: PendingMap,
    next_id: Arc<AtomicI64>,
    timeout_seconds: u64,
    alive: Arc<AtomicBool>,
    reader: Mutex<Option<JoinHandle<()>>>,
}

impl SseTransport {
    /// Open the event stream and complete the endpoint handshake.
    ///
    /// Returns once the server has announced its message endpoint, so a caller
    /// that gets an `Ok` can immediately `initialize`.
    pub async fn connect(config: &McpServerConfig) -> Result<Self, McpError> {
        let url = config
            .url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .ok_or_else(|| McpError::Config("an SSE server needs an endpoint URL".to_string()))?
            .to_string();

        let timeout_seconds = config.effective_timeout();
        let headers = build_headers(&config.headers)?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(timeout_seconds.min(30)))
            .build()
            .map_err(|err| McpError::Transport(format!("could not build an HTTP client: {err}")))?;

        let response = client
            .get(&url)
            .headers(headers.clone())
            .header(ACCEPT, "text/event-stream")
            .send()
            .await
            .map_err(|err| McpError::Transport(describe_request_error(&err)))?;

        if !response.status().is_success() {
            return Err(status_error(response.status(), "GET"));
        }

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let alive = Arc::new(AtomicBool::new(true));
        let (endpoint_tx, endpoint_rx) = oneshot::channel::<String>();

        let reader_pending = pending.clone();
        let reader_alive = alive.clone();
        let reader = tokio::spawn(async move {
            read_event_stream(response, reader_pending.clone(), endpoint_tx).await;
            // The stream ended: the server is gone. Release every waiter.
            reader_alive.store(false, Ordering::SeqCst);
            reader_pending.lock().await.clear();
        });

        let endpoint = match timeout(ENDPOINT_TIMEOUT, endpoint_rx).await {
            Ok(Ok(endpoint)) => endpoint,
            // The reader ended before announcing an endpoint.
            Ok(Err(_)) => {
                reader.abort();
                return Err(McpError::Transport(
                    "the event stream closed before the server announced its message endpoint"
                        .to_string(),
                ));
            }
            Err(_) => {
                reader.abort();
                return Err(McpError::Transport(
                    "the server did not announce a message endpoint — it may not speak the SSE transport"
                        .to_string(),
                ));
            }
        };

        let message_url = resolve_endpoint(&url, &endpoint)?;

        Ok(Self {
            client,
            headers,
            message_url,
            pending,
            next_id: Arc::new(AtomicI64::new(1)),
            timeout_seconds,
            alive,
            reader: Mutex::new(Some(reader)),
        })
    }

    /// The resolved `POST` endpoint, exposed for diagnostics.
    pub fn message_url(&self) -> &str {
        &self.message_url
    }

    /// `POST` one payload to the message endpoint.
    async fn post(&self, payload: &Value) -> Result<(), McpError> {
        let response = self
            .client
            .post(&self.message_url)
            .headers(self.headers.clone())
            .header(CONTENT_TYPE, "application/json")
            .json(payload)
            .send()
            .await
            .map_err(|err| McpError::Transport(describe_request_error(&err)))?;

        if !response.status().is_success() {
            let method = payload
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("request");
            return Err(status_error(response.status(), method));
        }
        Ok(())
    }
}

#[async_trait]
impl super::McpTransport for SseTransport {
    async fn request(&self, method: &str, params: Value) -> Result<Value, McpError> {
        if !self.alive.load(Ordering::SeqCst) {
            return Err(McpError::Transport("the event stream closed".to_string()));
        }

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        // Register the waiter before posting: on a fast server the response can
        // land on the event stream before the POST call even returns.
        if let Err(err) = self.post(&request_envelope(id, method, params)).await {
            self.pending.lock().await.remove(&id);
            return Err(err);
        }

        match timeout(Duration::from_secs(self.timeout_seconds), rx).await {
            Ok(Ok(message)) => Ok(message),
            Ok(Err(_)) => Err(McpError::Transport(
                "the event stream closed before the reply arrived".to_string(),
            )),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(McpError::Timeout(self.timeout_seconds))
            }
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), McpError> {
        self.post(&notification_envelope(method, params)).await
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    async fn close(&self) {
        self.alive.store(false, Ordering::SeqCst);
        // The legacy transport has no shutdown message; dropping the GET stream
        // is what tells the server the client is gone.
        if let Some(reader) = self.reader.lock().await.take() {
            reader.abort();
        }
        self.pending.lock().await.clear();
    }
}

/// Drive the `GET` event stream, announcing the endpoint and routing responses.
async fn read_event_stream(
    response: reqwest::Response,
    pending: PendingMap,
    endpoint_tx: oneshot::Sender<String>,
) {
    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    let mut total = 0usize;
    let mut endpoint_tx = Some(endpoint_tx);
    let mut event = SseEvent::default();

    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else { break };
        total += chunk.len();
        if total > MAX_STREAM_BYTES {
            break;
        }
        buffer.extend_from_slice(&chunk);

        while let Some(position) = buffer.iter().position(|byte| *byte == b'\n') {
            let line_bytes: Vec<u8> = buffer.drain(..=position).collect();
            let line = String::from_utf8_lossy(&line_bytes);
            let line = line.trim_end_matches(['\r', '\n']);

            // A blank line terminates the event; dispatch what was accumulated.
            if line.is_empty() {
                let finished = std::mem::take(&mut event);
                dispatch_event(finished, &pending, &mut endpoint_tx).await;
                continue;
            }
            event.absorb(line);
        }
    }

    // Flush a final event that arrived without its terminating blank line.
    dispatch_event(event, &pending, &mut endpoint_tx).await;
}

/// One accumulating SSE event.
#[derive(Default)]
struct SseEvent {
    name: Option<String>,
    data: String,
}

impl SseEvent {
    /// Fold one non-blank SSE line into this event.
    fn absorb(&mut self, line: &str) {
        if let Some(name) = line.strip_prefix("event:") {
            self.name = Some(name.trim().to_string());
        } else if let Some(data) = line.strip_prefix("data:") {
            // Multiple `data:` lines in one event join with newlines.
            if !self.data.is_empty() {
                self.data.push('\n');
            }
            self.data.push_str(data.trim_start());
        }
        // `id:`, `retry:` and comment lines carry nothing this client needs.
    }
}

/// Route a completed SSE event.
async fn dispatch_event(
    event: SseEvent,
    pending: &PendingMap,
    endpoint_tx: &mut Option<oneshot::Sender<String>>,
) {
    let data = event.data.trim();
    if data.is_empty() {
        return;
    }

    // The endpoint announcement is a bare URL, not JSON.
    if event.name.as_deref() == Some("endpoint") {
        if let Some(tx) = endpoint_tx.take() {
            let _ = tx.send(data.to_string());
        }
        return;
    }

    let Ok(message) = serde_json::from_str::<Value>(data) else {
        return;
    };
    // Server-initiated requests and notifications carry a `method`; neither is
    // acted on. `initialize` advertises no client capabilities, so a conforming
    // server has nothing to ask for and will not be left waiting on a reply.
    if message.get("method").is_some() {
        return;
    }
    let Some(id) = message.get("id").and_then(Value::as_i64) else {
        return;
    };
    if let Some(tx) = pending.lock().await.remove(&id) {
        let _ = tx.send(message);
    }
}

/// Resolve the announced endpoint against the stream URL.
///
/// Servers announce either an absolute URL or a path (`/messages?sessionId=…`).
/// Both forms are common, so both are handled.
pub(crate) fn resolve_endpoint(stream_url: &str, endpoint: &str) -> Result<String, McpError> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        return Err(McpError::Malformed(
            "the server announced an empty message endpoint".to_string(),
        ));
    }
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        return Ok(endpoint.to_string());
    }

    // Split the stream URL into scheme+authority and drop its path, then join.
    let scheme_end = stream_url
        .find("://")
        .ok_or_else(|| McpError::Config("the endpoint URL has no scheme".to_string()))?
        + 3;
    let authority_end = stream_url[scheme_end..]
        .find('/')
        .map(|offset| scheme_end + offset)
        .unwrap_or(stream_url.len());
    let origin = &stream_url[..authority_end];

    if endpoint.starts_with('/') {
        return Ok(format!("{origin}{endpoint}"));
    }

    // A relative path resolves against the stream URL's directory.
    let path = &stream_url[authority_end..];
    let directory = match path.rfind('/') {
        Some(index) => &path[..=index],
        None => "/",
    };
    Ok(format!("{origin}{directory}{endpoint}"))
}

#[cfg(test)]
mod tests {
    use super::{resolve_endpoint, SseEvent};

    #[test]
    fn an_absolute_endpoint_is_used_as_is() {
        let resolved =
            resolve_endpoint("https://example.com/sse", "https://other.test/messages").unwrap();
        assert_eq!(resolved, "https://other.test/messages");
    }

    #[test]
    fn a_rooted_endpoint_resolves_against_the_origin() {
        let resolved = resolve_endpoint(
            "https://example.com/mcp/sse",
            "/messages?sessionId=abc",
        )
        .unwrap();
        assert_eq!(resolved, "https://example.com/messages?sessionId=abc");
    }

    #[test]
    fn a_relative_endpoint_resolves_against_the_stream_directory() {
        let resolved =
            resolve_endpoint("https://example.com/mcp/sse", "messages?sessionId=abc").unwrap();
        assert_eq!(resolved, "https://example.com/mcp/messages?sessionId=abc");
    }

    #[test]
    fn a_stream_url_without_a_path_still_resolves() {
        let resolved = resolve_endpoint("http://127.0.0.1:9000", "/messages").unwrap();
        assert_eq!(resolved, "http://127.0.0.1:9000/messages");
    }

    #[test]
    fn an_empty_endpoint_is_rejected() {
        assert!(resolve_endpoint("https://example.com/sse", "   ").is_err());
    }

    #[test]
    fn events_accumulate_name_and_multiline_data() {
        let mut event = SseEvent::default();
        event.absorb("event: message");
        event.absorb("data: {\"id\":1,");
        event.absorb("data: \"result\":{}}");
        event.absorb("id: 42");
        assert_eq!(event.name.as_deref(), Some("message"));
        assert_eq!(event.data, "{\"id\":1,\n\"result\":{}}");
    }
}
