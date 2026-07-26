//! MCP transports.
//!
//! Each transport reduces its wire format to the same three operations:
//! issue a request and await its response, fire a notification, and shut down.
//! [`McpClient`](crate::mcp::McpClient) drives that interface and never knows
//! which transport it holds.

pub mod http;
pub mod sse;
pub mod stdio;

use async_trait::async_trait;
use serde_json::Value;

use super::McpError;

/// A live connection to one MCP server.
#[async_trait]
pub trait McpTransport: Send + Sync {
    /// Send a JSON-RPC request and await its response envelope.
    ///
    /// The returned value is the full envelope, not the `result`: the caller
    /// decodes it so a JSON-RPC error keeps its code.
    async fn request(&self, method: &str, params: Value) -> Result<Value, McpError>;

    /// Send a fire-and-forget JSON-RPC notification.
    async fn notify(&self, method: &str, params: Value) -> Result<(), McpError>;

    /// Whether the connection is still usable.
    ///
    /// The manager checks this before handing a pooled connection to a caller,
    /// so a server that died between turns is reconnected instead of returning
    /// transport errors for the rest of the process's life.
    fn is_alive(&self) -> bool;

    /// Release the transport's resources. Safe to call more than once.
    async fn close(&self);
}
