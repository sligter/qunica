//! [`McpManager`]: pooled MCP connections shared across agent turns.
//!
//! Connecting is expensive — a stdio server spawns a process, an SSE server
//! opens a stream and waits out a handshake — and an agent turn may call several
//! tools on the same server. The manager keeps one [`McpClient`] per server id,
//! reconnecting when a pooled connection has died.
//!
//! Connections are keyed by server id only. That is intentional: a row's
//! `owner_id` is checked when the config is loaded from the database, so two
//! owners can never present the same id, and per-turn re-keying would defeat the
//! pool entirely.

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};

use serde_json::Value;
use tokio::sync::Mutex;

use super::{
    config::McpServerConfig, mangle_tool_name, protocol::McpCallOutcome, slugify_server_name,
    McpClient, McpError, McpToolInfo,
};

/// One MCP tool as exposed to a model, with the addressing needed to call it.
#[derive(Debug, Clone, PartialEq)]
pub struct McpToolBinding {
    /// Namespaced name the model sees, e.g. `mcp__github__create_issue`.
    pub exposed_name: String,
    /// Server row id this tool belongs to.
    pub server_id: String,
    /// Operator-facing server name, used in prompts and error text.
    pub server_name: String,
    /// The server-side tool name to send in `tools/call`.
    pub tool_name: String,
    /// Description as the server reported it.
    pub description: String,
    /// Argument schema as the server reported it.
    pub input_schema: Value,
}

/// A cached, live connection plus the config it was opened with.
struct PooledClient {
    client: Arc<McpClient>,
    /// The config as of the last connect, so a row edit forces a reconnect
    /// instead of silently reusing a connection to the old command or URL.
    config: McpServerConfig,
}

/// Pooled MCP connections, one per configured server.
#[derive(Default)]
pub struct McpManager {
    clients: Mutex<HashMap<String, PooledClient>>,
}

/// The process-wide connection pool.
static SHARED: OnceLock<Arc<McpManager>> = OnceLock::new();

impl McpManager {
    /// An empty manager holding no connections.
    pub fn new() -> Self {
        Self::default()
    }

    /// The process-wide pool.
    ///
    /// Connections must outlive a single agent turn — a stdio server costs a
    /// process spawn plus a handshake to open, and the same server is typically
    /// used by several agents across many turns — so the pool is a process
    /// singleton rather than per-request state. Entries are keyed by server row
    /// id, and every config is loaded under an owner check, so sharing the pool
    /// across owners cannot cross-wire two users' servers.
    pub fn shared() -> Arc<McpManager> {
        SHARED.get_or_init(|| Arc::new(McpManager::new())).clone()
    }

    /// Return a live client for `config`, connecting or reconnecting as needed.
    pub async fn client(&self, config: &McpServerConfig) -> Result<Arc<McpClient>, McpError> {
        {
            let mut clients = self.clients.lock().await;
            if let Some(pooled) = clients.get(&config.id) {
                if pooled.client.is_alive() && &pooled.config == config {
                    return Ok(pooled.client.clone());
                }
                // Stale: either the transport died or the row was edited.
                let stale = clients.remove(&config.id);
                if let Some(stale) = stale {
                    drop(clients);
                    stale.client.close().await;
                }
            }
        }

        let client = Arc::new(McpClient::connect(config).await?);
        let mut clients = self.clients.lock().await;
        // Another turn may have connected the same server while this one was
        // waiting on the handshake. Keep the entry that is already pooled and
        // close this one, so the pool never holds two live connections per id.
        if let Some(existing) = clients.get(&config.id) {
            if existing.client.is_alive() && &existing.config == config {
                let winner = existing.client.clone();
                drop(clients);
                client.close().await;
                return Ok(winner);
            }
        }
        // Whatever was pooled under this id is stale (dead, or opened against
        // different settings). Close what it held rather than dropping the
        // handle on the floor, which would orphan a live child process.
        let replaced = clients.insert(
            config.id.clone(),
            PooledClient {
                client: client.clone(),
                config: config.clone(),
            },
        );
        drop(clients);
        if let Some(replaced) = replaced {
            replaced.client.close().await;
        }
        Ok(client)
    }

    /// List `config`'s tools as model-facing bindings.
    pub async fn list_bindings(
        &self,
        config: &McpServerConfig,
    ) -> Result<Vec<McpToolBinding>, McpError> {
        let client = self.client(config).await?;
        let tools = match client.list_tools(config).await {
            Ok(tools) => tools,
            Err(error) => {
                // A listing failure usually means the transport went away
                // mid-call; evict so the next attempt reconnects rather than
                // reusing a connection that is known bad.
                self.evict(&config.id).await;
                return Err(error);
            }
        };
        Ok(bindings_for(config, &tools))
    }

    /// Call `tool_name` on `config`'s server with `arguments`.
    pub async fn call_tool(
        &self,
        config: &McpServerConfig,
        tool_name: &str,
        arguments: &Value,
    ) -> Result<McpCallOutcome, McpError> {
        let client = self.client(config).await?;
        match client.call_tool(tool_name, arguments).await {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                self.evict(&config.id).await;
                Err(error)
            }
        }
    }

    /// Drop and close the pooled connection for `server_id`, if any.
    pub async fn evict(&self, server_id: &str) {
        let pooled = self.clients.lock().await.remove(server_id);
        if let Some(pooled) = pooled {
            pooled.client.close().await;
        }
    }

    /// Close every pooled connection.
    pub async fn shutdown(&self) {
        let pooled: Vec<PooledClient> = self.clients.lock().await.drain().map(|(_, v)| v).collect();
        for entry in pooled {
            entry.client.close().await;
        }
    }
}

/// Turn a server's listed tools into model-facing bindings.
///
/// Two tools that collide after name mangling (possible once truncation kicks
/// in) would leave the model unable to address one of them, so a collision drops
/// the later tool rather than shadowing the earlier one.
pub fn bindings_for(config: &McpServerConfig, tools: &[McpToolInfo]) -> Vec<McpToolBinding> {
    let slug = slugify_server_name(&config.name);
    let mut seen: Vec<String> = Vec::new();
    let mut bindings = Vec::new();

    for tool in tools {
        let exposed_name = mangle_tool_name(&slug, &tool.name);
        if seen.contains(&exposed_name) {
            continue;
        }
        seen.push(exposed_name.clone());
        bindings.push(McpToolBinding {
            exposed_name,
            server_id: config.id.clone(),
            server_name: config.name.clone(),
            tool_name: tool.name.clone(),
            description: tool.description.clone(),
            input_schema: tool.input_schema.clone(),
        });
    }
    bindings
}

#[cfg(test)]
mod tests {
    use super::bindings_for;
    use crate::mcp::{
        config::{McpServerConfig, McpTransportKind, DEFAULT_TIMEOUT_SECONDS},
        McpToolInfo,
    };
    use serde_json::json;
    use std::collections::BTreeMap;

    fn config(name: &str) -> McpServerConfig {
        McpServerConfig {
            id: "server-1".to_string(),
            name: name.to_string(),
            transport: McpTransportKind::Stdio,
            command: Some("node".to_string()),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            url: None,
            headers: BTreeMap::new(),
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
            tool_filter: Vec::new(),
        }
    }

    fn tool(name: &str) -> McpToolInfo {
        McpToolInfo {
            name: name.to_string(),
            description: format!("does {name}"),
            input_schema: json!({"type":"object","properties":{}}),
        }
    }

    #[test]
    fn bindings_namespace_tools_by_server_slug() {
        let bindings = bindings_for(&config("GitHub MCP"), &[tool("create_issue")]);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].exposed_name, "mcp__github_mcp__create_issue");
        assert_eq!(bindings[0].tool_name, "create_issue");
        assert_eq!(bindings[0].server_id, "server-1");
    }

    #[test]
    fn colliding_exposed_names_keep_the_first_tool_only() {
        // Both tool names exceed the length budget and truncate to the same
        // exposed name; the second must be dropped, not silently shadow the first.
        let long_a = format!("{}_alpha", "t".repeat(60));
        let long_b = format!("{}_beta", "t".repeat(60));
        let bindings = bindings_for(&config("srv"), &[tool(&long_a), tool(&long_b)]);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].tool_name, long_a);
    }

    #[test]
    fn distinct_tools_are_all_bound() {
        let bindings = bindings_for(&config("srv"), &[tool("a"), tool("b"), tool("c")]);
        assert_eq!(bindings.len(), 3);
    }
}
