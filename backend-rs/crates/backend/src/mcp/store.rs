//! Reading `mcp_servers` rows into [`McpServerConfig`].
//!
//! Both the API layer and the group runtime need the same row → config mapping,
//! and the runtime additionally needs to fetch a specific owner's servers by id
//! in one query, so the queries live here rather than in either caller.

use sqlx::SqlitePool;

use super::config::{
    parse_string_array, parse_string_map, McpServerConfig, McpTransportKind,
    DEFAULT_TIMEOUT_SECONDS,
};

/// The column list every query selects, so [`McpServerRow`] always lines up.
pub const MCP_SERVER_COLUMNS: &str = "id, owner_id, name, description, transport, command, \
     args_json, env_json, cwd, url, headers_json, timeout_seconds, tool_filter_json, status, \
     created_at, updated_at";

/// A row of the `mcp_servers` table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct McpServerRow {
    pub id: String,
    pub owner_id: String,
    pub name: String,
    pub description: Option<String>,
    pub transport: String,
    pub command: Option<String>,
    pub args_json: Option<String>,
    pub env_json: Option<String>,
    pub cwd: Option<String>,
    pub url: Option<String>,
    pub headers_json: Option<String>,
    pub timeout_seconds: i64,
    pub tool_filter_json: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

impl McpServerRow {
    /// Whether the operator has this server switched on.
    pub fn is_active(&self) -> bool {
        self.status == "active"
    }

    /// Parse this row into a connection config.
    ///
    /// An unrecognized `transport` value falls back to stdio rather than
    /// failing: the column is constrained at the API layer, so a bad value means
    /// hand-edited data, and a config that fails validation gives a clearer
    /// error than a row that silently disappears from the list.
    pub fn to_config(&self) -> McpServerConfig {
        McpServerConfig {
            id: self.id.clone(),
            name: self.name.clone(),
            transport: McpTransportKind::parse(&self.transport)
                .unwrap_or(McpTransportKind::Stdio),
            command: self.command.clone(),
            args: parse_string_array(self.args_json.as_deref()),
            env: parse_string_map(self.env_json.as_deref()),
            cwd: self.cwd.clone(),
            url: self.url.clone(),
            headers: parse_string_map(self.headers_json.as_deref()),
            timeout_seconds: if self.timeout_seconds > 0 {
                self.timeout_seconds as u64
            } else {
                DEFAULT_TIMEOUT_SECONDS
            },
            tool_filter: parse_string_array(self.tool_filter_json.as_deref()),
        }
    }
}

/// Load one server owned by `owner_id`.
pub async fn load_server(
    pool: &SqlitePool,
    owner_id: &str,
    server_id: &str,
) -> Result<Option<McpServerRow>, sqlx::Error> {
    sqlx::query_as::<_, McpServerRow>(&format!(
        "SELECT {MCP_SERVER_COLUMNS} FROM mcp_servers WHERE id = ?1 AND owner_id = ?2"
    ))
    .bind(server_id)
    .bind(owner_id)
    .fetch_optional(pool)
    .await
}

/// Load every server owned by `owner_id`, newest first.
pub async fn list_servers(
    pool: &SqlitePool,
    owner_id: &str,
) -> Result<Vec<McpServerRow>, sqlx::Error> {
    sqlx::query_as::<_, McpServerRow>(&format!(
        "SELECT {MCP_SERVER_COLUMNS} FROM mcp_servers WHERE owner_id = ?1 \
         ORDER BY created_at DESC, id DESC"
    ))
    .bind(owner_id)
    .fetch_all(pool)
    .await
}

/// Load the given servers, keeping only the ones owned by `owner_id` and marked
/// active.
///
/// Ids that do not resolve are dropped silently: an agent may reference a server
/// that has since been deleted or disabled, and that must degrade to "no tools
/// from that server" rather than failing the turn.
pub async fn load_active_servers(
    pool: &SqlitePool,
    owner_id: &str,
    server_ids: &[String],
) -> Result<Vec<McpServerRow>, sqlx::Error> {
    let mut rows = Vec::new();
    for server_id in server_ids {
        if let Some(row) = load_server(pool, owner_id, server_id).await? {
            if row.is_active() {
                rows.push(row);
            }
        }
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::McpServerRow;
    use crate::mcp::config::{McpTransportKind, DEFAULT_TIMEOUT_SECONDS};

    fn row(transport: &str, timeout_seconds: i64) -> McpServerRow {
        McpServerRow {
            id: "srv".to_string(),
            owner_id: "owner".to_string(),
            name: "Weather".to_string(),
            description: None,
            transport: transport.to_string(),
            command: Some("node".to_string()),
            args_json: Some(r#"["server.js"]"#.to_string()),
            env_json: Some(r#"{"API_KEY":"secret"}"#.to_string()),
            cwd: None,
            url: None,
            headers_json: None,
            timeout_seconds,
            tool_filter_json: Some(r#"["forecast"]"#.to_string()),
            status: "active".to_string(),
            created_at: "2026-07-26T00:00:00Z".to_string(),
            updated_at: "2026-07-26T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn rows_parse_their_json_columns() {
        let config = row("stdio", 30).to_config();
        assert_eq!(config.transport, McpTransportKind::Stdio);
        assert_eq!(config.args, vec!["server.js"]);
        assert_eq!(config.env.get("API_KEY").map(String::as_str), Some("secret"));
        assert_eq!(config.tool_filter, vec!["forecast"]);
        assert_eq!(config.timeout_seconds, 30);
    }

    #[test]
    fn a_nonpositive_timeout_falls_back_to_the_default() {
        assert_eq!(row("stdio", 0).to_config().timeout_seconds, DEFAULT_TIMEOUT_SECONDS);
        assert_eq!(row("stdio", -5).to_config().timeout_seconds, DEFAULT_TIMEOUT_SECONDS);
    }

    #[test]
    fn an_unknown_transport_falls_back_to_stdio() {
        assert_eq!(
            row("carrier-pigeon", 30).to_config().transport,
            McpTransportKind::Stdio
        );
    }

    #[test]
    fn only_active_rows_count_as_enabled() {
        let mut row = row("stdio", 30);
        assert!(row.is_active());
        row.status = "disabled".to_string();
        assert!(!row.is_active());
    }
}
