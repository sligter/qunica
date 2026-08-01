//! `/api/v2/mcp-servers`: CRUD for the MCP servers an owner has configured,
//! plus a live connection test that reports the tools a server exposes.
//!
//! Rows are soft-deleted (`status = 'deleted'`) like LLM providers, so an agent
//! whose tool config still references a removed server degrades to "no tools
//! from that server" instead of hitting a dangling foreign key.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Deserializer, Serialize};
use sqlx::SqlitePool;
use std::collections::BTreeMap;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::api::{auth::current_user_id, error::ApiError, AppState};
use crate::mcp::{
    config::{MAX_TIMEOUT_SECONDS, DEFAULT_TIMEOUT_SECONDS},
    manager::bindings_for,
    slugify_server_name,
    store::{McpServerRow, MCP_SERVER_COLUMNS},
    McpClient, McpServerConfig, McpTransportKind,
};

/// Longest command, URL or working directory accepted, so a paste accident
/// cannot write an unbounded string into the row.
const MAX_TEXT_CHARS: usize = 2_000;

/// Most arguments, env entries, headers or filter entries accepted per server.
const MAX_COLLECTION_ENTRIES: usize = 100;

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    name: String,
    transport: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Option<Vec<String>>,
    #[serde(default)]
    env: Option<BTreeMap<String, String>>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    url: Option<String>,
    /// `Some(value)` sets the header, `None` keeps whatever is stored. See
    /// [`resolve_headers`].
    #[serde(default)]
    headers: Option<BTreeMap<String, Option<String>>>,
    #[serde(default)]
    timeout_seconds: Option<i64>,
    #[serde(default)]
    tool_filter: Option<Vec<String>>,
    #[serde(default)]
    enabled: Option<bool>,
    /// Only meaningful on `POST /mcp-servers/test`: the saved row whose stored
    /// header values a `None` entry should resolve against.
    #[serde(default)]
    server_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    transport: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    description: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    command: Option<Option<String>>,
    #[serde(default)]
    args: Option<Vec<String>>,
    #[serde(default)]
    env: Option<BTreeMap<String, String>>,
    #[serde(default, deserialize_with = "double_option")]
    cwd: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    url: Option<Option<String>>,
    /// `Some(value)` sets the header, `None` keeps whatever is stored, and a
    /// key that is absent entirely is deleted. See [`resolve_headers`].
    #[serde(default)]
    headers: Option<BTreeMap<String, Option<String>>>,
    #[serde(default)]
    timeout_seconds: Option<i64>,
    #[serde(default)]
    tool_filter: Option<Vec<String>>,
    #[serde(default)]
    enabled: Option<bool>,
}

/// An MCP server as returned to the client.
///
/// Header values are masked: they routinely carry bearer tokens, and the list
/// endpoint is fetched on every settings visit.
#[derive(Debug, Serialize)]
pub struct McpServerResponse {
    id: String,
    name: String,
    description: Option<String>,
    transport: String,
    /// The slug used to namespace this server's tools, e.g. `mcp__<slug>__<tool>`.
    slug: String,
    command: Option<String>,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    cwd: Option<String>,
    url: Option<String>,
    headers_masked: BTreeMap<String, String>,
    timeout_seconds: i64,
    tool_filter: Vec<String>,
    enabled: bool,
    status: String,
    created_at: String,
    updated_at: String,
}

impl From<McpServerRow> for McpServerResponse {
    fn from(row: McpServerRow) -> Self {
        let config = row.to_config();
        Self {
            slug: slugify_server_name(&row.name),
            id: row.id,
            name: row.name,
            description: row.description,
            transport: config.transport.as_str().to_string(),
            command: config.command,
            args: config.args,
            env: config.env,
            cwd: config.cwd,
            url: config.url,
            headers_masked: config
                .headers
                .into_iter()
                .map(|(key, value)| (key, mask_secret(&value)))
                .collect(),
            timeout_seconds: config.timeout_seconds as i64,
            tool_filter: config.tool_filter,
            enabled: row.status == "active",
            status: row.status,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// One tool discovered by a connection test.
#[derive(Debug, Serialize)]
pub struct DiscoveredToolResponse {
    /// Server-side tool name.
    name: String,
    /// Namespaced name an agent would call.
    exposed_name: String,
    description: String,
}

/// Result of a connection test.
#[derive(Debug, Serialize)]
pub struct TestConnectionResponse {
    ok: bool,
    /// The server's self-reported `name@version`, when it supplied one.
    server_label: Option<String>,
    tools: Vec<DiscoveredToolResponse>,
    /// Why the test failed, when it did.
    error: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateRequest>,
) -> Result<(StatusCode, Json<McpServerResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    Ok((
        StatusCode::CREATED,
        Json(create_inner(&state, &owner_id, body).await?),
    ))
}

/// The body of [`create`] without the axum extractors.
///
/// Approved app-actions call this, so a staged proposal runs exactly the
/// validation the UI path does. A second implementation would drift.
pub(crate) async fn create_inner(
    state: &AppState,
    owner_id: &str,
    body: CreateRequest,
) -> Result<McpServerResponse, ApiError> {
    let owner_id = owner_id.to_string();

    let name = validate_name(&body.name)?;
    ensure_slug_is_free(state.db.pool(), &owner_id, &name, None).await?;
    let transport = validate_transport(&body.transport)?;
    let command = normalize_text(body.command.as_deref(), "command")?;
    let args = validate_collection(body.args.unwrap_or_default(), "args")?;
    let env = validate_map(body.env.unwrap_or_default(), "env")?;
    let cwd = normalize_text(body.cwd.as_deref(), "cwd")?;
    let url = normalize_text(body.url.as_deref(), "url")?;
    // Nothing is stored yet, so a "keep" entry has nothing to keep and drops out.
    let headers_map = resolve_headers(
        body.headers.unwrap_or_default(),
        &BTreeMap::new(),
        "headers",
    )?;
    let timeout_seconds = validate_timeout(body.timeout_seconds)?;
    let tool_filter = validate_collection(body.tool_filter.unwrap_or_default(), "tool_filter")?;
    let status = if body.enabled.unwrap_or(true) {
        "active"
    } else {
        "disabled"
    };

    let draft = McpServerConfig {
        id: String::new(),
        name: name.clone(),
        transport,
        command: command.clone(),
        args: args.clone(),
        env: env.clone(),
        cwd: cwd.clone(),
        url: url.clone(),
        headers: headers_map.clone(),
        timeout_seconds: timeout_seconds as u64,
        tool_filter: tool_filter.clone(),
    };
    draft
        .validate()
        .map_err(|error| ApiError::invalid_input(error.to_string()))?;

    let id = Uuid::new_v4().to_string();
    let now = now_rfc3339();

    sqlx::query(
        "INSERT INTO mcp_servers \
         (id, owner_id, name, description, transport, command, args_json, env_json, cwd, \
          url, headers_json, timeout_seconds, tool_filter_json, status, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&owner_id)
    .bind(&name)
    .bind(normalize_text(body.description.as_deref(), "description")?)
    .bind(transport.as_str())
    .bind(&command)
    .bind(to_json_array(&args)?)
    .bind(to_json_map(&env)?)
    .bind(&cwd)
    .bind(&url)
    .bind(to_json_map(&headers_map)?)
    .bind(timeout_seconds)
    .bind(to_json_array(&tool_filter)?)
    .bind(status)
    .bind(&now)
    .bind(&now)
    .execute(state.db.pool())
    .await
    .map_err(map_insert_error)?;

    let row = fetch_row(state.db.pool(), &id)
        .await?
        .ok_or_else(|| ApiError::internal("MCP server vanished after insert"))?;
    Ok(row.into())
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<McpServerResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let sql = format!(
        "SELECT {MCP_SERVER_COLUMNS} FROM mcp_servers \
         WHERE owner_id = ? AND status != 'deleted' \
         ORDER BY created_at DESC, id DESC"
    );
    let rows = sqlx::query_as::<_, McpServerRow>(&sql)
        .bind(&owner_id)
        .fetch_all(state.db.pool())
        .await
        .map_err(|_| ApiError::internal("database error"))?;

    Ok(Json(rows.into_iter().map(McpServerResponse::from).collect()))
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
) -> Result<Json<McpServerResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let server_id = validate_uuid(&server_id)?;
    let row = load_owned(state.db.pool(), &server_id, &owner_id).await?;
    Ok(Json(row.into()))
}

pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
    Json(body): Json<UpdateRequest>,
) -> Result<Json<McpServerResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    Ok(Json(update_inner(&state, &owner_id, &server_id, body).await?))
}

/// The body of [`update`] without the axum extractors. See [`create_inner`].
pub(crate) async fn update_inner(
    state: &AppState,
    owner_id: &str,
    server_id: &str,
    body: UpdateRequest,
) -> Result<McpServerResponse, ApiError> {
    let owner_id = owner_id.to_string();
    let server_id = validate_uuid(server_id)?;
    let existing = load_owned(state.db.pool(), &server_id, &owner_id).await?;
    let current = existing.to_config();

    let name = match body.name {
        Some(ref raw) => validate_name(raw)?,
        None => existing.name.clone(),
    };
    ensure_slug_is_free(state.db.pool(), &owner_id, &name, Some(&server_id)).await?;
    let transport = match body.transport {
        Some(ref raw) => validate_transport(raw)?,
        None => current.transport,
    };
    let description = match body.description {
        Some(ref provided) => normalize_text(provided.as_deref(), "description")?,
        None => existing.description.clone(),
    };
    let command = match body.command {
        Some(ref provided) => normalize_text(provided.as_deref(), "command")?,
        None => current.command.clone(),
    };
    let args = match body.args {
        Some(args) => validate_collection(args, "args")?,
        None => current.args.clone(),
    };
    let env = match body.env {
        Some(env) => validate_map(env, "env")?,
        None => current.env.clone(),
    };
    let cwd = match body.cwd {
        Some(ref provided) => normalize_text(provided.as_deref(), "cwd")?,
        None => current.cwd.clone(),
    };
    let url = match body.url {
        Some(ref provided) => normalize_text(provided.as_deref(), "url")?,
        None => current.url.clone(),
    };
    let headers_map = match body.headers {
        Some(headers) => resolve_headers(headers, &current.headers, "headers")?,
        None => current.headers.clone(),
    };
    let timeout_seconds = match body.timeout_seconds {
        Some(value) => validate_timeout(Some(value))?,
        None => current.timeout_seconds as i64,
    };
    let tool_filter = match body.tool_filter {
        Some(filter) => validate_collection(filter, "tool_filter")?,
        None => current.tool_filter.clone(),
    };
    let status = match body.enabled {
        Some(true) => "active",
        Some(false) => "disabled",
        None => existing.status.as_str(),
    };

    let updated = McpServerConfig {
        id: server_id.clone(),
        name: name.clone(),
        transport,
        command: command.clone(),
        args: args.clone(),
        env: env.clone(),
        cwd: cwd.clone(),
        url: url.clone(),
        headers: headers_map.clone(),
        timeout_seconds: timeout_seconds as u64,
        tool_filter: tool_filter.clone(),
    };
    updated
        .validate()
        .map_err(|error| ApiError::invalid_input(error.to_string()))?;

    let now = now_rfc3339();
    sqlx::query(
        "UPDATE mcp_servers SET \
         name = ?, description = ?, transport = ?, command = ?, args_json = ?, env_json = ?, \
         cwd = ?, url = ?, headers_json = ?, timeout_seconds = ?, tool_filter_json = ?, \
         status = ?, updated_at = ? \
         WHERE id = ? AND owner_id = ?",
    )
    .bind(&name)
    .bind(&description)
    .bind(transport.as_str())
    .bind(&command)
    .bind(to_json_array(&args)?)
    .bind(to_json_map(&env)?)
    .bind(&cwd)
    .bind(&url)
    .bind(to_json_map(&headers_map)?)
    .bind(timeout_seconds)
    .bind(to_json_array(&tool_filter)?)
    .bind(status)
    .bind(&now)
    .bind(&server_id)
    .bind(&owner_id)
    .execute(state.db.pool())
    .await
    .map_err(map_insert_error)?;

    // The pooled connection was opened against the old settings; drop it so the
    // next turn dials the edited command or URL.
    state.mcp.evict(&server_id).await;

    let row = fetch_row(state.db.pool(), &server_id)
        .await?
        .ok_or_else(|| ApiError::internal("MCP server vanished after update"))?;
    Ok(row.into())
}

pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let server_id = validate_uuid(&server_id)?;
    load_owned(state.db.pool(), &server_id, &owner_id).await?;

    let now = now_rfc3339();
    sqlx::query(
        "UPDATE mcp_servers SET status = 'deleted', updated_at = ? WHERE id = ? AND owner_id = ?",
    )
    .bind(&now)
    .bind(&server_id)
    .bind(&owner_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to delete MCP server"))?;

    state.mcp.evict(&server_id).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Connect to a stored server and list its tools.
///
/// A connection failure is a `200` with `ok: false` rather than an HTTP error:
/// the caller is a settings screen showing the operator what is wrong with their
/// configuration, and the reason is the useful part of the response.
pub async fn test_connection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
) -> Result<Json<TestConnectionResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let server_id = validate_uuid(&server_id)?;
    let row = load_owned(state.db.pool(), &server_id, &owner_id).await?;
    Ok(Json(probe(&row.to_config()).await))
}

/// Connect to a not-yet-saved configuration and list its tools.
///
/// Lets the settings form verify a server before the row exists, which is the
/// difference between "save and hope" and knowing the command is right.
pub async fn test_draft(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateRequest>,
) -> Result<Json<TestConnectionResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;

    // Testing an edit to a saved server sends "keep" for every header the
    // operator did not retype, and the client cannot fill those in because it
    // only ever saw the mask. Resolve them against the row so the probe dials
    // with the real credentials instead of reporting a spurious 401.
    let stored = match body.server_id.as_deref() {
        Some(server_id) => {
            let server_id = validate_uuid(server_id)?;
            load_owned(state.db.pool(), &server_id, &owner_id)
                .await?
                .to_config()
                .headers
        }
        None => BTreeMap::new(),
    };

    let config = McpServerConfig {
        id: Uuid::new_v4().to_string(),
        name: validate_name(&body.name)?,
        transport: validate_transport(&body.transport)?,
        command: normalize_text(body.command.as_deref(), "command")?,
        args: validate_collection(body.args.unwrap_or_default(), "args")?,
        env: validate_map(body.env.unwrap_or_default(), "env")?,
        cwd: normalize_text(body.cwd.as_deref(), "cwd")?,
        url: normalize_text(body.url.as_deref(), "url")?,
        headers: resolve_headers(body.headers.unwrap_or_default(), &stored, "headers")?,
        timeout_seconds: validate_timeout(body.timeout_seconds)? as u64,
        tool_filter: validate_collection(body.tool_filter.unwrap_or_default(), "tool_filter")?,
    };
    config
        .validate()
        .map_err(|error| ApiError::invalid_input(error.to_string()))?;

    Ok(Json(probe(&config).await))
}

/// Open a one-off connection, list the tools, and close it again.
///
/// A draft test must not join the shared pool — the config has no stable row id
/// and would evict or shadow the saved server's connection — so this connects
/// directly and always closes.
async fn probe(config: &McpServerConfig) -> TestConnectionResponse {
    let client = match McpClient::connect(config).await {
        Ok(client) => client,
        Err(error) => {
            return TestConnectionResponse {
                ok: false,
                server_label: None,
                tools: Vec::new(),
                error: Some(error.to_string()),
            }
        }
    };

    let server_label = client.server_label().map(str::to_string);
    let outcome = client.list_tools(config).await;
    client.close().await;

    match outcome {
        Ok(tools) => {
            let bindings = bindings_for(config, &tools);
            TestConnectionResponse {
                ok: true,
                server_label,
                tools: bindings
                    .into_iter()
                    .map(|binding| DiscoveredToolResponse {
                        name: binding.tool_name,
                        exposed_name: binding.exposed_name,
                        description: binding.description,
                    })
                    .collect(),
                error: None,
            }
        }
        Err(error) => TestConnectionResponse {
            ok: false,
            server_label,
            tools: Vec::new(),
            error: Some(error.to_string()),
        },
    }
}

/// Reject a name whose slug already belongs to another of this owner's servers.
///
/// Tool names are namespaced by the slug, not the name, and slugification is
/// lossy: `Notion (work)` and `Notion-work` both become `notion_work`. Two such
/// servers would produce identical `mcp__notion_work__*` tool names, leaving the
/// model unable to address one of them. The unique index is on the raw name, so
/// it does not catch this — hence the explicit check.
async fn ensure_slug_is_free(
    pool: &SqlitePool,
    owner_id: &str,
    name: &str,
    exclude_id: Option<&str>,
) -> Result<(), ApiError> {
    let slug = slugify_server_name(name);
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT id, name FROM mcp_servers WHERE owner_id = ?1 AND status != 'deleted'")
            .bind(owner_id)
            .fetch_all(pool)
            .await
            .map_err(|_| ApiError::internal("database error"))?;

    for (id, existing_name) in rows {
        if exclude_id == Some(id.as_str()) {
            continue;
        }
        if slugify_server_name(&existing_name) == slug {
            return Err(ApiError::invalid_input(format!(
                "'{name}' produces the same tool prefix (mcp__{slug}__) as '{existing_name}'. \
                 Pick a name that differs by more than punctuation or case."
            )));
        }
    }
    Ok(())
}

async fn load_owned(
    pool: &SqlitePool,
    server_id: &str,
    owner_id: &str,
) -> Result<McpServerRow, ApiError> {
    let row = fetch_row(pool, server_id)
        .await?
        .ok_or_else(|| ApiError::not_found("MCP server not found"))?;
    if row.status == "deleted" {
        return Err(ApiError::not_found("MCP server not found"));
    }
    if row.owner_id != owner_id {
        return Err(ApiError::permission_denied(
            "MCP server belongs to another user",
        ));
    }
    Ok(row)
}

async fn fetch_row(pool: &SqlitePool, server_id: &str) -> Result<Option<McpServerRow>, ApiError> {
    let sql = format!("SELECT {MCP_SERVER_COLUMNS} FROM mcp_servers WHERE id = ?");
    sqlx::query_as::<_, McpServerRow>(&sql)
        .bind(server_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}

/// A duplicate name trips the `(owner_id, name)` unique index; report that as a
/// user error rather than an opaque internal failure.
fn map_insert_error(error: sqlx::Error) -> ApiError {
    let message = error.to_string();
    if message.contains("UNIQUE constraint failed") {
        ApiError::invalid_input("an MCP server with that name already exists")
    } else {
        ApiError::internal("failed to save the MCP server")
    }
}

fn validate_name(raw: &str) -> Result<String, ApiError> {
    let name = raw.trim().to_string();
    let len = name.chars().count();
    if !(1..=100).contains(&len) {
        return Err(ApiError::invalid_input(
            "name must be between 1 and 100 characters",
        ));
    }
    Ok(name)
}

fn validate_transport(raw: &str) -> Result<McpTransportKind, ApiError> {
    McpTransportKind::parse(raw).ok_or_else(|| {
        ApiError::invalid_input("transport must be one of: stdio, sse, streamable-http")
    })
}

fn validate_timeout(raw: Option<i64>) -> Result<i64, ApiError> {
    let Some(value) = raw else {
        return Ok(DEFAULT_TIMEOUT_SECONDS as i64);
    };
    if value < 1 || value > MAX_TIMEOUT_SECONDS as i64 {
        return Err(ApiError::invalid_input(format!(
            "timeout_seconds must be between 1 and {MAX_TIMEOUT_SECONDS}"
        )));
    }
    Ok(value)
}

/// Trim a text field, treat blank as absent, and reject anything oversized.
fn normalize_text(raw: Option<&str>, field: &str) -> Result<Option<String>, ApiError> {
    let Some(value) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if value.chars().count() > MAX_TEXT_CHARS {
        return Err(ApiError::invalid_input(format!(
            "{field} must be at most {MAX_TEXT_CHARS} characters"
        )));
    }
    Ok(Some(value.to_string()))
}

/// Drop blank entries and enforce the per-collection cap.
fn validate_collection(values: Vec<String>, field: &str) -> Result<Vec<String>, ApiError> {
    if values.len() > MAX_COLLECTION_ENTRIES {
        return Err(ApiError::invalid_input(format!(
            "{field} must have at most {MAX_COLLECTION_ENTRIES} entries"
        )));
    }
    for value in &values {
        if value.chars().count() > MAX_TEXT_CHARS {
            return Err(ApiError::invalid_input(format!(
                "each {field} entry must be at most {MAX_TEXT_CHARS} characters"
            )));
        }
    }
    // Arguments may legitimately be empty strings, but a blank *filter* or env
    // key is always a UI artefact; trimming here keeps both cases sane.
    Ok(values
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect())
}

/// Resolve a submitted header map against what is already stored.
///
/// Header values are masked on the way out, so the client never holds the real
/// secret and cannot send it back. A wholesale replace would therefore turn
/// every unrelated edit into a credential wipe. Instead each entry says what to
/// do with that header:
///
/// - `Some(value)` — set it to `value` (the operator typed a new one).
/// - `None` — keep whatever is stored (the operator left the field alone).
/// - key absent from the map entirely — delete that header.
///
/// A `None` for a header that has no stored value is dropped rather than
/// written as an empty string, so a half-filled form cannot create a blank
/// `Authorization` that fails as a confusing 401 later.
fn resolve_headers(
    submitted: BTreeMap<String, Option<String>>,
    stored: &BTreeMap<String, String>,
    field: &str,
) -> Result<BTreeMap<String, String>, ApiError> {
    if submitted.len() > MAX_COLLECTION_ENTRIES {
        return Err(ApiError::invalid_input(format!(
            "{field} must have at most {MAX_COLLECTION_ENTRIES} entries"
        )));
    }
    let mut resolved = BTreeMap::new();
    for (key, value) in submitted {
        let key = key.trim().to_string();
        if key.is_empty() {
            continue;
        }
        let Some(value) = value.or_else(|| stored.get(&key).cloned()) else {
            continue;
        };
        if key.chars().count() > MAX_TEXT_CHARS || value.chars().count() > MAX_TEXT_CHARS {
            return Err(ApiError::invalid_input(format!(
                "each {field} entry must be at most {MAX_TEXT_CHARS} characters"
            )));
        }
        resolved.insert(key, value);
    }
    Ok(resolved)
}

/// Drop entries with a blank key and enforce the per-map cap.
fn validate_map(
    values: BTreeMap<String, String>,
    field: &str,
) -> Result<BTreeMap<String, String>, ApiError> {
    if values.len() > MAX_COLLECTION_ENTRIES {
        return Err(ApiError::invalid_input(format!(
            "{field} must have at most {MAX_COLLECTION_ENTRIES} entries"
        )));
    }
    let mut out = BTreeMap::new();
    for (key, value) in values {
        let key = key.trim().to_string();
        if key.is_empty() {
            continue;
        }
        if key.chars().count() > MAX_TEXT_CHARS || value.chars().count() > MAX_TEXT_CHARS {
            return Err(ApiError::invalid_input(format!(
                "each {field} entry must be at most {MAX_TEXT_CHARS} characters"
            )));
        }
        out.insert(key, value);
    }
    Ok(out)
}

fn to_json_array(values: &[String]) -> Result<String, ApiError> {
    serde_json::to_string(values).map_err(|_| ApiError::internal("failed to encode a list field"))
}

fn to_json_map(values: &BTreeMap<String, String>) -> Result<String, ApiError> {
    serde_json::to_string(values).map_err(|_| ApiError::internal("failed to encode a map field"))
}

/// Mask a secret-bearing value, keeping enough to recognize which one it is.
fn mask_secret(value: &str) -> String {
    if value.chars().count() <= 4 {
        return "****".to_string();
    }
    let suffix: String = value
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("****{suffix}")
}

fn validate_uuid(raw: &str) -> Result<String, ApiError> {
    Uuid::parse_str(raw.trim())
        .map(|id| id.to_string())
        .map_err(|_| ApiError::invalid_input("invalid MCP server id"))
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::{
        mask_secret, normalize_text, validate_collection, validate_map, validate_timeout,
        validate_transport, MAX_COLLECTION_ENTRIES, MAX_TEXT_CHARS,
    };
    use crate::mcp::config::{McpTransportKind, DEFAULT_TIMEOUT_SECONDS, MAX_TIMEOUT_SECONDS};
    use std::collections::BTreeMap;

    #[test]
    fn transport_validation_accepts_the_three_supported_kinds() {
        assert_eq!(
            validate_transport("stdio").unwrap(),
            McpTransportKind::Stdio
        );
        assert_eq!(validate_transport("sse").unwrap(), McpTransportKind::Sse);
        assert_eq!(
            validate_transport("streamable-http").unwrap(),
            McpTransportKind::StreamableHttp
        );
        assert!(validate_transport("smoke-signal").is_err());
    }

    #[test]
    fn timeout_validation_bounds_the_stored_value() {
        assert_eq!(
            validate_timeout(None).unwrap(),
            DEFAULT_TIMEOUT_SECONDS as i64
        );
        assert_eq!(validate_timeout(Some(30)).unwrap(), 30);
        assert!(validate_timeout(Some(0)).is_err());
        assert!(validate_timeout(Some(MAX_TIMEOUT_SECONDS as i64 + 1)).is_err());
    }

    #[test]
    fn blank_text_fields_become_absent() {
        assert_eq!(normalize_text(None, "command").unwrap(), None);
        assert_eq!(normalize_text(Some("  "), "command").unwrap(), None);
        assert_eq!(
            normalize_text(Some(" npx "), "command").unwrap(),
            Some("npx".to_string())
        );
    }

    #[test]
    fn oversized_text_is_rejected() {
        let huge = "x".repeat(MAX_TEXT_CHARS + 1);
        assert!(normalize_text(Some(&huge), "command").is_err());
    }

    #[test]
    fn collections_drop_blanks_and_enforce_their_cap() {
        let values = vec!["a".to_string(), "  ".to_string(), "b".to_string()];
        assert_eq!(validate_collection(values, "args").unwrap(), vec!["a", "b"]);

        let too_many = vec!["x".to_string(); MAX_COLLECTION_ENTRIES + 1];
        assert!(validate_collection(too_many, "args").is_err());
    }

    #[test]
    fn maps_drop_blank_keys_and_enforce_their_cap() {
        let mut values = BTreeMap::new();
        values.insert("  ".to_string(), "ignored".to_string());
        values.insert(" KEY ".to_string(), "value".to_string());
        let validated = validate_map(values, "env").unwrap();
        assert_eq!(validated.len(), 1);
        assert_eq!(validated.get("KEY").map(String::as_str), Some("value"));

        let too_many: BTreeMap<String, String> = (0..=MAX_COLLECTION_ENTRIES)
            .map(|index| (index.to_string(), "v".to_string()))
            .collect();
        assert!(validate_map(too_many, "env").is_err());
    }

    #[test]
    fn secrets_are_masked_down_to_a_recognizable_suffix() {
        assert_eq!(mask_secret("Bearer sk-abcdefgh"), "****efgh");
        assert_eq!(mask_secret("abc"), "****");
    }
}
