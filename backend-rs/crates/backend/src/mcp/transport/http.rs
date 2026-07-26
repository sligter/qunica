//! Streamable HTTP transport.
//!
//! # Wire framing
//!
//! One endpoint handles everything. The client `POST`s a JSON-RPC message with
//! `Accept: application/json, text/event-stream`; the server answers either with
//! a single `application/json` body or with a `text/event-stream` on which the
//! response arrives as one or more SSE `data:` events. Notifications get a
//! `202 Accepted` with no body.
//!
//! If the server assigns a session it returns an `Mcp-Session-Id` header on the
//! `initialize` response; every later request must echo it back. That id is
//! captured on the first response that carries one and replayed from then on.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Arc,
    },
    time::Duration,
};

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE},
    Client, Response, StatusCode,
};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::mcp::{
    config::McpServerConfig,
    protocol::{notification_envelope, request_envelope},
    McpError,
};

/// Header carrying the server-assigned session id.
pub const SESSION_HEADER: &str = "mcp-session-id";

/// Header carrying the negotiated protocol revision on later requests.
pub const PROTOCOL_VERSION_HEADER: &str = "mcp-protocol-version";

/// `Accept` value announcing that both response forms are understood.
const ACCEPT_BOTH: &str = "application/json, text/event-stream";

/// Cap on bytes read from one SSE response, so a server that never closes its
/// stream cannot grow the buffer without bound.
const MAX_STREAM_BYTES: usize = 8 * 1024 * 1024;

/// A streamable-HTTP MCP connection.
pub struct HttpTransport {
    client: Client,
    url: String,
    headers: HeaderMap,
    next_id: Arc<AtomicI64>,
    timeout_seconds: u64,
    session_id: Mutex<Option<String>>,
    alive: Arc<AtomicBool>,
}

impl HttpTransport {
    /// Build a transport for `config`. No request is issued yet; the first
    /// `initialize` establishes the session.
    pub fn connect(config: &McpServerConfig) -> Result<Self, McpError> {
        let url = config
            .url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .ok_or_else(|| McpError::Config("an HTTP server needs an endpoint URL".to_string()))?
            .to_string();

        let timeout_seconds = config.effective_timeout();
        let client = Client::builder()
            // The per-request timeout is applied around the whole call, but an
            // SSE response stays open past the response headers, so the client's
            // own timeout must cover connection setup only.
            .connect_timeout(Duration::from_secs(timeout_seconds.min(30)))
            // No idle keep-alive pooling. JSON-RPC over HTTP is low volume here
            // — a handful of requests per agent turn — and a pooled connection
            // the peer has already closed surfaces as a send failure on the next
            // request. Retrying is not safe (`tools/call` may not be idempotent,
            // so a retry could double-execute a side effect), and a fresh
            // connection costs far less than a spurious tool failure.
            .pool_max_idle_per_host(0)
            .build()
            .map_err(|err| McpError::Transport(format!("could not build an HTTP client: {err}")))?;

        Ok(Self {
            client,
            url,
            headers: build_headers(&config.headers)?,
            next_id: Arc::new(AtomicI64::new(1)),
            timeout_seconds,
            session_id: Mutex::new(None),
            alive: Arc::new(AtomicBool::new(true)),
        })
    }

    /// Issue one POST carrying `payload`, with the session header when known.
    async fn post(&self, payload: &Value) -> Result<Response, McpError> {
        let mut request = self
            .client
            .post(&self.url)
            .headers(self.headers.clone())
            .header(ACCEPT, ACCEPT_BOTH)
            .header(CONTENT_TYPE, "application/json")
            .json(payload);

        if let Some(session_id) = self.session_id.lock().await.clone() {
            request = request.header(SESSION_HEADER, session_id);
        }

        request
            .send()
            .await
            .map_err(|err| McpError::Transport(describe_request_error(&err)))
    }

    /// Record the session id if the response carries one and none is held yet.
    async fn capture_session_id(&self, response: &Response) {
        let Some(session_id) = response
            .headers()
            .get(SESSION_HEADER)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
        else {
            return;
        };
        let mut slot = self.session_id.lock().await;
        if slot.as_deref() != Some(session_id) {
            *slot = Some(session_id.to_string());
        }
    }
}

#[async_trait]
impl super::McpTransport for HttpTransport {
    async fn request(&self, method: &str, params: Value) -> Result<Value, McpError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let payload = request_envelope(id, method, params);

        let response = tokio::time::timeout(
            Duration::from_secs(self.timeout_seconds),
            self.request_inner(&payload, id),
        )
        .await
        .map_err(|_| McpError::Timeout(self.timeout_seconds))??;

        Ok(response)
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), McpError> {
        let payload = notification_envelope(method, params);
        let response = tokio::time::timeout(
            Duration::from_secs(self.timeout_seconds),
            self.post(&payload),
        )
        .await
        .map_err(|_| McpError::Timeout(self.timeout_seconds))??;

        self.capture_session_id(&response).await;
        // A notification has no response body to read; anything but a 2xx means
        // the server rejected it.
        if !response.status().is_success() {
            return Err(status_error(response.status(), method));
        }
        Ok(())
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    async fn close(&self) {
        self.alive.store(false, Ordering::SeqCst);
        // The spec has the client DELETE the endpoint to end a session. Best
        // effort: a server that does not implement it just answers 405.
        let session_id = self.session_id.lock().await.clone();
        if let Some(session_id) = session_id {
            let _ = tokio::time::timeout(
                Duration::from_secs(5),
                self.client
                    .delete(&self.url)
                    .headers(self.headers.clone())
                    .header(SESSION_HEADER, session_id)
                    .send(),
            )
            .await;
        }
    }
}

impl HttpTransport {
    /// The body of [`request`](super::McpTransport::request), wrapped by the
    /// caller in the overall timeout.
    async fn request_inner(&self, payload: &Value, id: i64) -> Result<Value, McpError> {
        let response = self.post(payload).await?;
        self.capture_session_id(&response).await;

        let status = response.status();
        if !status.is_success() {
            let method = payload
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("request");
            return Err(status_error_with_body(status, method, response).await);
        }

        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();

        if content_type.contains("text/event-stream") {
            read_response_from_stream(response, id).await
        } else {
            response
                .json::<Value>()
                .await
                .map_err(|err| McpError::Malformed(format!("response was not JSON: {err}")))
        }
    }
}

/// Read an SSE response stream until the event matching `id` arrives.
///
/// The server may interleave notifications and progress events on the same
/// stream; anything that is not the awaited response is skipped.
pub(crate) async fn read_response_from_stream(
    response: Response,
    id: i64,
) -> Result<Value, McpError> {
    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    let mut total = 0usize;

    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|err| McpError::Transport(format!("the stream failed: {err}")))?;
        total += chunk.len();
        if total > MAX_STREAM_BYTES {
            return Err(McpError::Malformed(
                "the response stream exceeded the size limit".to_string(),
            ));
        }
        buffer.extend_from_slice(&chunk);

        while let Some(position) = buffer.iter().position(|byte| *byte == b'\n') {
            let line_bytes: Vec<u8> = buffer.drain(..=position).collect();
            let line = String::from_utf8_lossy(&line_bytes);
            let Some(data) = sse_data(line.trim_end_matches(['\r', '\n'])) else {
                continue;
            };
            let Ok(message) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            if message.get("id").and_then(Value::as_i64) == Some(id) {
                return Ok(message);
            }
        }
    }

    Err(McpError::Transport(
        "the response stream ended before the reply arrived".to_string(),
    ))
}

/// Extract the JSON payload of an SSE `data:` line, ignoring other fields.
pub(crate) fn sse_data(line: &str) -> Option<&str> {
    let data = line.strip_prefix("data:")?.trim();
    (!data.is_empty()).then_some(data)
}

/// Turn the configured static headers into a `HeaderMap`.
///
/// Header names and values are operator-supplied, so an invalid one is reported
/// as a configuration error rather than silently dropped.
pub(crate) fn build_headers(
    configured: &std::collections::BTreeMap<String, String>,
) -> Result<HeaderMap, McpError> {
    let mut headers = HeaderMap::new();
    for (key, value) in configured {
        let name = HeaderName::from_bytes(key.trim().as_bytes())
            .map_err(|_| McpError::Config(format!("'{key}' is not a valid header name")))?;
        let value = HeaderValue::from_str(value.trim())
            .map_err(|_| McpError::Config(format!("the value for header '{key}' is not valid")))?;
        headers.insert(name, value);
    }
    Ok(headers)
}

/// Describe a `reqwest` failure without leaking the full error chain.
pub(crate) fn describe_request_error(err: &reqwest::Error) -> String {
    if err.is_connect() {
        "could not connect to the endpoint".to_string()
    } else if err.is_timeout() {
        "the endpoint did not respond in time".to_string()
    } else {
        format!("the request failed: {err}")
    }
}

/// Map a non-2xx status to a readable error.
pub(crate) fn status_error(status: StatusCode, method: &str) -> McpError {
    let hint = match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            " — check the configured authorization header"
        }
        StatusCode::NOT_FOUND => " — check the endpoint URL",
        _ => "",
    };
    McpError::Transport(format!("'{method}' returned HTTP {status}{hint}"))
}

/// Map a non-2xx status to a readable error, including a bounded body excerpt.
async fn status_error_with_body(status: StatusCode, method: &str, response: Response) -> McpError {
    let body = response.text().await.unwrap_or_default();
    let excerpt: String = body.trim().chars().take(300).collect();
    match status_error(status, method) {
        McpError::Transport(message) if !excerpt.is_empty() => {
            McpError::Transport(format!("{message}: {excerpt}"))
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::{build_headers, sse_data, status_error};
    use reqwest::StatusCode;
    use std::collections::BTreeMap;

    #[test]
    fn sse_data_only_matches_data_lines() {
        assert_eq!(sse_data("data: {\"id\":1}"), Some("{\"id\":1}"));
        assert_eq!(sse_data("event: message"), None);
        assert_eq!(sse_data(": comment"), None);
        assert_eq!(sse_data("data:"), None);
        assert_eq!(sse_data(""), None);
    }

    #[test]
    fn headers_are_built_from_the_configured_map() {
        let mut configured = BTreeMap::new();
        configured.insert("Authorization".to_string(), "Bearer token".to_string());
        let headers = build_headers(&configured).unwrap();
        assert_eq!(headers.get("authorization").unwrap(), "Bearer token");
    }

    #[test]
    fn invalid_header_names_are_configuration_errors() {
        let mut configured = BTreeMap::new();
        configured.insert("bad header".to_string(), "value".to_string());
        assert!(build_headers(&configured).is_err());
    }

    #[test]
    fn auth_failures_point_at_the_authorization_header() {
        let message = status_error(StatusCode::UNAUTHORIZED, "tools/list").to_string();
        assert!(message.contains("authorization header"), "{message}");
        let message = status_error(StatusCode::NOT_FOUND, "tools/list").to_string();
        assert!(message.contains("endpoint URL"), "{message}");
    }
}
