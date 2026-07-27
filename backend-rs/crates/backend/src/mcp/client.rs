//! [`McpClient`]: an initialized MCP session over any transport.
//!
//! Construction performs the handshake — `initialize`, then the
//! `notifications/initialized` acknowledgement — so a client that exists is a
//! client that is ready to list and call tools.

use std::sync::Arc;

use serde_json::{json, Value};

use super::{
    config::{McpServerConfig, McpTransportKind},
    protocol::{
        decode_response, decode_tool_call, decode_tools_list, initialize_params, next_cursor,
        tools_call_params, McpCallOutcome, McpToolInfo, METHOD_INITIALIZE, METHOD_INITIALIZED,
        METHOD_TOOLS_CALL, METHOD_TOOLS_LIST, MAX_TOOLS_PER_SERVER,
    },
    transport::{http::HttpTransport, sse::SseTransport, stdio::StdioTransport, McpTransport},
    McpError,
};

/// Client identity sent in the `initialize` handshake.
const CLIENT_NAME: &str = "ag-swarmer";
/// Client version sent in the `initialize` handshake.
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Most `tools/list` pages to follow before giving up on a paginating server.
const MAX_TOOL_PAGES: usize = 10;

/// An initialized MCP session.
pub struct McpClient {
    transport: Arc<dyn McpTransport>,
    /// What the server called itself in the handshake, when it said.
    server_label: Option<String>,
}

// The transport holds live pipes and sockets and cannot derive `Debug`; the
// label plus liveness is what a caller actually wants to see.
impl std::fmt::Debug for McpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpClient")
            .field("server_label", &self.server_label)
            .field("alive", &self.transport.is_alive())
            .finish()
    }
}

impl McpClient {
    /// Open a transport for `config` and complete the MCP handshake.
    pub async fn connect(config: &McpServerConfig) -> Result<Self, McpError> {
        config.validate()?;

        let transport: Arc<dyn McpTransport> = match config.transport {
            McpTransportKind::Stdio => Arc::new(StdioTransport::connect(config)?),
            McpTransportKind::StreamableHttp => Arc::new(HttpTransport::connect(config)?),
            McpTransportKind::Sse => Arc::new(SseTransport::connect(config).await?),
        };

        match handshake(transport.as_ref()).await {
            Ok(server_label) => Ok(Self {
                transport,
                server_label,
            }),
            Err(error) => {
                // A half-open transport must not leak: the child process or the
                // SSE reader task would otherwise outlive this failed attempt.
                transport.close().await;
                Err(error)
            }
        }
    }

    /// The server's self-reported `name@version`, when it supplied one.
    pub fn server_label(&self) -> Option<&str> {
        self.server_label.as_deref()
    }

    /// List every tool the server exposes, following pagination.
    ///
    /// The result is filtered by the config's allowlist and capped at
    /// [`MAX_TOOLS_PER_SERVER`], so one server cannot flood an agent's tool list.
    pub async fn list_tools(&self, config: &McpServerConfig) -> Result<Vec<McpToolInfo>, McpError> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;

        for _ in 0..MAX_TOOL_PAGES {
            let params = match &cursor {
                Some(cursor) => json!({ "cursor": cursor }),
                None => json!({}),
            };
            let envelope = self.transport.request(METHOD_TOOLS_LIST, params).await?;
            let result = decode_response(&envelope)?;
            tools.extend(decode_tools_list(&result)?);

            if tools.len() >= MAX_TOOLS_PER_SERVER {
                tools.truncate(MAX_TOOLS_PER_SERVER);
                break;
            }
            match next_cursor(&result) {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }

        tools.retain(|tool| config.allows_tool(&tool.name));
        Ok(tools)
    }

    /// Call `tool` with `arguments`.
    ///
    /// A JSON-RPC error is converted into a failed [`McpCallOutcome`] rather
    /// than an `Err`, because "the tool rejected your arguments" is information
    /// the model should see and retry on, not a transport failure. Transport and
    /// timeout failures still surface as `Err`.
    pub async fn call_tool(
        &self,
        tool: &str,
        arguments: &Value,
    ) -> Result<McpCallOutcome, McpError> {
        let envelope = self
            .transport
            .request(METHOD_TOOLS_CALL, tools_call_params(tool, arguments))
            .await?;

        match decode_response(&envelope) {
            Ok(result) => Ok(decode_tool_call(&result)),
            Err(McpError::Rpc { code, message }) => Ok(McpCallOutcome {
                text: format!("The MCP server rejected the call ({code}): {message}"),
                is_error: true,
            }),
            Err(other) => Err(other),
        }
    }

    /// Whether the underlying transport is still usable.
    pub fn is_alive(&self) -> bool {
        self.transport.is_alive()
    }

    /// Release the transport.
    pub async fn close(&self) {
        self.transport.close().await;
    }
}

/// Run `initialize` + `notifications/initialized` against a fresh transport,
/// returning the server's self-reported name and version when it supplies them.
async fn handshake(transport: &dyn McpTransport) -> Result<Option<String>, McpError> {
    let envelope = transport
        .request(
            METHOD_INITIALIZE,
            initialize_params(CLIENT_NAME, CLIENT_VERSION),
        )
        .await?;
    let result = decode_response(&envelope)?;

    // The acknowledgement is required by the spec before any other request.
    transport.notify(METHOD_INITIALIZED, json!({})).await?;

    Ok(server_label(&result))
}

/// Render the server's `serverInfo` as `name@version`, when present.
fn server_label(result: &Value) -> Option<String> {
    let info = result.get("serverInfo")?;
    let name = info
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())?;
    match info.get("version").and_then(Value::as_str) {
        Some(version) if !version.is_empty() => Some(format!("{name}@{version}")),
        _ => Some(name.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::server_label;
    use serde_json::json;

    #[test]
    fn server_label_combines_name_and_version() {
        let result = json!({"serverInfo":{"name":"weather","version":"1.2.0"}});
        assert_eq!(server_label(&result).as_deref(), Some("weather@1.2.0"));
    }

    #[test]
    fn server_label_tolerates_a_missing_version() {
        let result = json!({"serverInfo":{"name":"weather"}});
        assert_eq!(server_label(&result).as_deref(), Some("weather"));
    }

    #[test]
    fn server_label_is_absent_when_the_server_says_nothing() {
        assert_eq!(server_label(&json!({})), None);
        assert_eq!(server_label(&json!({"serverInfo":{"name":"  "}})), None);
    }
}
