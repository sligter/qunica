//! Model Context Protocol (MCP) client.
//!
//! An MCP server exposes tools that an agent may call. This module speaks the
//! client half of the protocol over the three standard transports — a local
//! child process (`stdio`), the legacy HTTP+SSE pair, and streamable HTTP — and
//! reduces each to the same two operations the runtime needs: list the server's
//! tools, and call one of them.
//!
//! Only the tool surface is implemented. MCP also defines resources, prompts,
//! sampling and roots; none of those reach the agent today, so `initialize`
//! advertises no client capabilities beyond the baseline and any server-initiated
//! request is answered with method-not-found.
//!
//! # Naming
//!
//! Tool names are namespaced before they reach a model, because two servers may
//! each expose a `search` and a provider tool list has one flat namespace. The
//! wire name is `mcp__<server slug>__<tool>`, matching the convention Claude Code
//! uses. [`mangle_tool_name`] builds it and [`parse_tool_name`] takes it apart
//! again when a tool call comes back.

pub mod client;
pub mod config;
pub mod manager;
pub mod protocol;
pub mod store;
pub mod transport;

pub use client::McpClient;
pub use config::{McpServerConfig, McpTransportKind};
pub use manager::{McpManager, McpToolBinding};
pub use protocol::McpToolInfo;
pub use store::{McpServerRow, MCP_SERVER_COLUMNS};

use thiserror::Error;

/// Prefix every MCP tool name carries once it is exposed to a model.
pub const MCP_TOOL_PREFIX: &str = "mcp__";

/// Separator between the server slug and the server-side tool name.
const MCP_TOOL_SEPARATOR: &str = "__";

/// Longest tool name providers accept (OpenAI and Anthropic both cap at 64).
pub const MAX_TOOL_NAME_CHARS: usize = 64;

/// A failure talking to an MCP server.
///
/// Every variant renders through `Display` into text that is safe to hand back
/// to a model: no absolute paths, no header values, no API keys.
#[derive(Debug, Error)]
pub enum McpError {
    /// The server configuration is unusable (missing URL, missing command, …).
    #[error("MCP server configuration is invalid: {0}")]
    Config(String),
    /// The transport could not be established or has since died.
    #[error("MCP server is unreachable: {0}")]
    Transport(String),
    /// The server returned a JSON-RPC error object.
    #[error("MCP server rejected the request ({code}): {message}")]
    Rpc {
        /// JSON-RPC error code.
        code: i64,
        /// JSON-RPC error message.
        message: String,
    },
    /// A response arrived but did not have the shape the protocol requires.
    #[error("MCP server returned a malformed response: {0}")]
    Malformed(String),
    /// The server did not answer within the configured timeout.
    #[error("MCP server did not respond within {0}s")]
    Timeout(u64),
}

/// Reduce a server name to the `[a-z0-9_]` alphabet allowed inside a tool name.
///
/// Runs of rejected characters collapse into a single `_` so `My Server (v2)`
/// and `My-Server-v2` do not both become `my_server_v2___`. An empty result
/// falls back to `server`, because a tool name may not contain an empty segment.
pub fn slugify_server_name(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut pending_separator = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_separator && !slug.is_empty() {
                slug.push('_');
            }
            pending_separator = false;
            slug.push(ch.to_ascii_lowercase());
        } else {
            pending_separator = true;
        }
    }
    if slug.is_empty() {
        "server".to_string()
    } else {
        slug
    }
}

/// Build the provider-facing name for `tool` served by `server_slug`.
///
/// The result is truncated to [`MAX_TOOL_NAME_CHARS`]; callers that need the
/// mapping back must go through the binding table rather than re-deriving it,
/// since truncation is not reversible.
pub fn mangle_tool_name(server_slug: &str, tool: &str) -> String {
    let sanitized_tool: String = tool
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let full = format!("{MCP_TOOL_PREFIX}{server_slug}{MCP_TOOL_SEPARATOR}{sanitized_tool}");
    if full.chars().count() <= MAX_TOOL_NAME_CHARS {
        return full;
    }
    full.chars().take(MAX_TOOL_NAME_CHARS).collect()
}

/// Whether `name` is a mangled MCP tool name rather than a built-in tool.
pub fn is_mcp_tool_name(name: &str) -> bool {
    name.starts_with(MCP_TOOL_PREFIX)
}

/// Split a mangled name back into `(server slug, tool name)`.
///
/// Returns `None` for names that are not MCP names or that lack a tool segment.
/// Because a server slug never contains `__` (it is `[a-z0-9_]` with single
/// underscores between runs), the first `__` after the prefix is the boundary.
pub fn parse_tool_name(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix(MCP_TOOL_PREFIX)?;
    let (server, tool) = rest.split_once(MCP_TOOL_SEPARATOR)?;
    if server.is_empty() || tool.is_empty() {
        return None;
    }
    Some((server, tool))
}

#[cfg(test)]
mod tests {
    use super::{mangle_tool_name, parse_tool_name, slugify_server_name, MAX_TOOL_NAME_CHARS};

    #[test]
    fn slugify_collapses_runs_of_rejected_characters() {
        assert_eq!(slugify_server_name("My Server (v2)"), "my_server_v2");
        assert_eq!(slugify_server_name("github---mcp"), "github_mcp");
        assert_eq!(slugify_server_name("文件服务"), "server");
        assert_eq!(slugify_server_name(""), "server");
    }

    #[test]
    fn slug_never_leaves_a_trailing_separator() {
        assert_eq!(slugify_server_name("weather!"), "weather");
        assert_eq!(slugify_server_name("!weather"), "weather");
    }

    #[test]
    fn mangled_names_round_trip() {
        let name = mangle_tool_name("github", "create_issue");
        assert_eq!(name, "mcp__github__create_issue");
        assert_eq!(parse_tool_name(&name), Some(("github", "create_issue")));
    }

    #[test]
    fn mangled_names_stay_within_the_provider_limit() {
        let name = mangle_tool_name("a_very_long_server_slug_indeed", &"t".repeat(80));
        assert_eq!(name.chars().count(), MAX_TOOL_NAME_CHARS);
    }

    #[test]
    fn parse_rejects_non_mcp_and_incomplete_names() {
        assert_eq!(parse_tool_name("Read"), None);
        assert_eq!(parse_tool_name("mcp__github"), None);
        assert_eq!(parse_tool_name("mcp____tool"), None);
    }
}
