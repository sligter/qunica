//! Resolved MCP server connection settings.
//!
//! [`McpServerConfig`] is what the `mcp_servers` table becomes once its JSON
//! columns are parsed. The API layer and the group runtime both build one from a
//! row; the transports consume it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::McpError;

/// Default per-request timeout when a row does not set one.
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 60;

/// Largest per-request timeout an operator may configure.
///
/// A stuck MCP call holds an agent turn open, so the ceiling is deliberately
/// lower than the turn timeout rather than unbounded.
pub const MAX_TIMEOUT_SECONDS: u64 = 600;

/// How a client reaches an MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpTransportKind {
    /// Local child process speaking newline-delimited JSON-RPC over stdio.
    Stdio,
    /// Legacy HTTP+SSE: a `GET` event stream plus a `POST` message endpoint.
    Sse,
    /// Streamable HTTP: one endpoint that answers JSON or an SSE stream.
    StreamableHttp,
}

impl McpTransportKind {
    /// The wire/DB spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            McpTransportKind::Stdio => "stdio",
            McpTransportKind::Sse => "sse",
            McpTransportKind::StreamableHttp => "streamable-http",
        }
    }

    /// Parse the wire/DB spelling, accepting the aliases operators commonly type.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "stdio" | "studio" => Some(McpTransportKind::Stdio),
            "sse" | "http-sse" => Some(McpTransportKind::Sse),
            "streamable-http" | "http" | "streamablehttp" | "streamable" => {
                Some(McpTransportKind::StreamableHttp)
            }
            _ => None,
        }
    }

    /// Whether this transport talks to a remote URL rather than a child process.
    pub fn is_http(self) -> bool {
        matches!(
            self,
            McpTransportKind::Sse | McpTransportKind::StreamableHttp
        )
    }
}

/// Everything needed to open a connection to one configured MCP server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerConfig {
    /// Row id, used as the connection-cache key.
    pub id: String,
    /// Operator-facing name; also the source of the tool-name slug.
    pub name: String,
    /// Chosen transport.
    pub transport: McpTransportKind,
    /// Executable to spawn (stdio only).
    pub command: Option<String>,
    /// Arguments passed to `command` (stdio only).
    pub args: Vec<String>,
    /// Environment overlay applied on top of the inherited env (stdio only).
    pub env: BTreeMap<String, String>,
    /// Working directory for the child (stdio only).
    pub cwd: Option<String>,
    /// Endpoint URL (SSE / streamable HTTP only).
    pub url: Option<String>,
    /// Static headers sent on every HTTP request (SSE / streamable HTTP only).
    pub headers: BTreeMap<String, String>,
    /// Per-request timeout in seconds.
    pub timeout_seconds: u64,
    /// Optional allowlist of server-side tool names; empty means "expose all".
    pub tool_filter: Vec<String>,
}

impl McpServerConfig {
    /// Reject a configuration the transports could not act on, so the failure is
    /// reported at save/connect time rather than as a mystery timeout later.
    pub fn validate(&self) -> Result<(), McpError> {
        match self.transport {
            McpTransportKind::Stdio => {
                let command = self.command.as_deref().unwrap_or("").trim();
                if command.is_empty() {
                    return Err(McpError::Config(
                        "a stdio server needs a command to run".to_string(),
                    ));
                }
            }
            McpTransportKind::Sse | McpTransportKind::StreamableHttp => {
                let url = self.url.as_deref().unwrap_or("").trim();
                if url.is_empty() {
                    return Err(McpError::Config(
                        "an HTTP server needs an endpoint URL".to_string(),
                    ));
                }
                if !(url.starts_with("http://") || url.starts_with("https://")) {
                    return Err(McpError::Config(
                        "the endpoint URL must start with http:// or https://".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }

    /// The timeout clamped into the range the transports honour.
    pub fn effective_timeout(&self) -> u64 {
        self.timeout_seconds.clamp(1, MAX_TIMEOUT_SECONDS)
    }

    /// Whether `tool` passes this server's allowlist.
    pub fn allows_tool(&self, tool: &str) -> bool {
        self.tool_filter.is_empty() || self.tool_filter.iter().any(|name| name == tool)
    }
}

/// Parse a JSON array-of-strings column, tolerating null and malformed values.
pub fn parse_string_array(raw: Option<&str>) -> Vec<String> {
    raw.and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
        .unwrap_or_default()
}

/// Parse a JSON string-map column, tolerating null and malformed values.
///
/// Non-string values are stringified rather than dropped, so a port number typed
/// as `8080` in an env map still reaches the child as `"8080"`.
pub fn parse_string_map(raw: Option<&str>) -> BTreeMap<String, String> {
    let Some(value) = raw.and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
    else {
        return BTreeMap::new();
    };
    let Some(object) = value.as_object() else {
        return BTreeMap::new();
    };
    object
        .iter()
        .map(|(key, value)| {
            let rendered = match value {
                serde_json::Value::String(text) => text.clone(),
                serde_json::Value::Null => String::new(),
                other => other.to_string(),
            };
            (key.clone(), rendered)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        parse_string_array, parse_string_map, McpServerConfig, McpTransportKind,
        DEFAULT_TIMEOUT_SECONDS, MAX_TIMEOUT_SECONDS,
    };
    use std::collections::BTreeMap;

    fn config(transport: McpTransportKind) -> McpServerConfig {
        McpServerConfig {
            id: "server-1".to_string(),
            name: "Test".to_string(),
            transport,
            command: None,
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            url: None,
            headers: BTreeMap::new(),
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
            tool_filter: Vec::new(),
        }
    }

    #[test]
    fn transport_parsing_accepts_the_common_spellings() {
        assert_eq!(
            McpTransportKind::parse("stdio"),
            Some(McpTransportKind::Stdio)
        );
        // Operators routinely type "studio" for stdio; accept it rather than
        // rejecting a server that is otherwise configured correctly.
        assert_eq!(
            McpTransportKind::parse("studio"),
            Some(McpTransportKind::Stdio)
        );
        assert_eq!(McpTransportKind::parse("SSE"), Some(McpTransportKind::Sse));
        assert_eq!(
            McpTransportKind::parse("streamable_http"),
            Some(McpTransportKind::StreamableHttp)
        );
        assert_eq!(McpTransportKind::parse("carrier-pigeon"), None);
    }

    #[test]
    fn stdio_requires_a_command() {
        let mut cfg = config(McpTransportKind::Stdio);
        assert!(cfg.validate().is_err());
        cfg.command = Some("   ".to_string());
        assert!(cfg.validate().is_err());
        cfg.command = Some("npx".to_string());
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn http_requires_an_absolute_url() {
        let mut cfg = config(McpTransportKind::StreamableHttp);
        assert!(cfg.validate().is_err());
        cfg.url = Some("localhost:3000/mcp".to_string());
        assert!(cfg.validate().is_err());
        cfg.url = Some("https://example.com/mcp".to_string());
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn timeout_is_clamped_into_the_supported_range() {
        let mut cfg = config(McpTransportKind::Stdio);
        cfg.timeout_seconds = 0;
        assert_eq!(cfg.effective_timeout(), 1);
        cfg.timeout_seconds = MAX_TIMEOUT_SECONDS * 10;
        assert_eq!(cfg.effective_timeout(), MAX_TIMEOUT_SECONDS);
    }

    #[test]
    fn an_empty_filter_allows_every_tool() {
        let mut cfg = config(McpTransportKind::Stdio);
        assert!(cfg.allows_tool("anything"));
        cfg.tool_filter = vec!["search".to_string()];
        assert!(cfg.allows_tool("search"));
        assert!(!cfg.allows_tool("delete_everything"));
    }

    #[test]
    fn json_columns_tolerate_null_and_garbage() {
        assert!(parse_string_array(None).is_empty());
        assert!(parse_string_array(Some("not json")).is_empty());
        assert_eq!(parse_string_array(Some(r#"["a","b"]"#)), vec!["a", "b"]);

        assert!(parse_string_map(Some("[]")).is_empty());
        let map = parse_string_map(Some(r#"{"A":"1","B":2,"C":null}"#));
        assert_eq!(map.get("A").map(String::as_str), Some("1"));
        assert_eq!(map.get("B").map(String::as_str), Some("2"));
        assert_eq!(map.get("C").map(String::as_str), Some(""));
    }
}
