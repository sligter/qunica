use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::BTreeMap;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::api::{auth::current_user_id, error::ApiError, AppState};

const RUNTIME_LLM_CHAT: &str = "llm_chat";
const RUNTIME_ACP: &str = "acp";
const DEFAULT_SYSTEM_PROMPT: &str = "You are a helpful AI agent.";

const AGENT_COLUMNS: &str = "id, owner_id, workspace_id, name, description, system_prompt, \
     runtime_kind, provider_id, model_config_json, tool_config_json, external_runtime_json, \
     skill_ids_json, status, created_at, updated_at";

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default)]
    llm_config: Option<Value>,
    #[serde(default)]
    tool_config: Option<Value>,
    #[serde(default)]
    runtime_kind: Option<String>,
    #[serde(default)]
    acp_runtime: Option<Value>,
    workspace_id: String,
    #[serde(default)]
    llm_provider_id: Option<String>,
    #[serde(default)]
    skill_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRequest {
    #[serde(default)]
    name: Option<String>,
    // Double `Option` distinguishes an omitted field (outer `None`) from an
    // explicit JSON `null` (inner `None`) for nullable/json fields.
    #[serde(default, deserialize_with = "double_option")]
    description: Option<Option<String>>,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    llm_config: Option<Option<Value>>,
    #[serde(default, deserialize_with = "double_option")]
    tool_config: Option<Option<Value>>,
    #[serde(default)]
    runtime_kind: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    acp_runtime: Option<Option<Value>>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    llm_provider_id: Option<Option<String>>,
    #[serde(default)]
    skill_ids: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct AgentResponse {
    id: String,
    name: String,
    description: Option<String>,
    system_prompt: String,
    llm_config: Option<Value>,
    tool_config: Option<Value>,
    runtime_kind: String,
    acp_runtime: Option<Value>,
    workspace_id: Option<String>,
    llm_provider_id: Option<String>,
    skill_ids: Vec<String>,
    status: String,
    created_at: String,
}

#[derive(Debug, Serialize)]
pub struct ToolCatalogResponse {
    tools: Vec<BuiltinToolResponse>,
}

#[derive(Debug, Serialize)]
struct BuiltinToolResponse {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    policy: &'static str,
    requires_workspace: bool,
    requires_sandbox: bool,
    runtime_status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct AcpRuntimePresetListResponse {
    presets: Vec<AcpRuntimePresetResponse>,
}

#[derive(Debug, Serialize)]
struct AcpRuntimePresetResponse {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    profile: &'static str,
    installed: bool,
    command: Option<&'static str>,
    args: Vec<&'static str>,
    env: BTreeMap<String, String>,
    timeout_seconds: i64,
    permission_policy: &'static str,
    default_model: Option<&'static str>,
    default_mode: Option<&'static str>,
    default_thinking_effort: Option<&'static str>,
    model_options: Vec<AcpRuntimeChoiceResponse>,
    mode_options: Vec<AcpRuntimeChoiceResponse>,
    thinking_effort_options: Vec<AcpRuntimeChoiceResponse>,
    install_hint: &'static str,
    source: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct AcpRuntimeChoiceResponse {
    value: &'static str,
    label: &'static str,
    description: Option<&'static str>,
}

#[derive(Debug, sqlx::FromRow)]
struct AgentRow {
    id: String,
    owner_id: String,
    workspace_id: Option<String>,
    name: String,
    description: Option<String>,
    system_prompt: String,
    runtime_kind: String,
    provider_id: Option<String>,
    model_config_json: Option<String>,
    tool_config_json: Option<String>,
    external_runtime_json: Option<String>,
    skill_ids_json: String,
    status: String,
    created_at: String,
    #[allow(dead_code)]
    updated_at: String,
}

impl From<AgentRow> for AgentResponse {
    fn from(row: AgentRow) -> Self {
        let skill_ids =
            serde_json::from_str::<Vec<String>>(&row.skill_ids_json).unwrap_or_default();
        Self {
            id: row.id,
            name: row.name,
            description: row.description,
            system_prompt: row.system_prompt,
            llm_config: parse_json(row.model_config_json.as_deref()),
            tool_config: parse_json(row.tool_config_json.as_deref()),
            runtime_kind: row.runtime_kind,
            acp_runtime: parse_json(row.external_runtime_json.as_deref()),
            workspace_id: row.workspace_id,
            llm_provider_id: row.provider_id,
            skill_ids,
            status: row.status,
            created_at: row.created_at,
        }
    }
}

pub async fn tool_catalog() -> Json<ToolCatalogResponse> {
    Json(ToolCatalogResponse {
        tools: builtin_tools(),
    })
}

pub async fn acp_runtime_presets() -> Json<AcpRuntimePresetListResponse> {
    Json(AcpRuntimePresetListResponse {
        presets: fallback_acp_presets(),
    })
}

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateRequest>,
) -> Result<(StatusCode, Json<AgentResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;

    let name = validate_name(&body.name)?;
    let system_prompt = match body.system_prompt.as_deref() {
        Some(raw) => validate_system_prompt(raw)?,
        None => DEFAULT_SYSTEM_PROMPT.to_string(),
    };
    let runtime_kind = normalize_runtime_kind(body.runtime_kind.as_deref())?;
    let workspace_id = validate_workspace(state.db.pool(), &body.workspace_id, &owner_id).await?;
    let description = normalize_description(body.description.as_deref());
    let skill_ids_json = validate_skill_ids(body.skill_ids.as_deref())?;
    let model_config_json = json_to_db_string(body.llm_config.as_ref());
    let tool_config_json = json_to_db_string(body.tool_config.as_ref());

    // Runtime-specific binding: ACP agents store their runtime blob and never a
    // provider; LLM chat agents store an optional provider and never a runtime.
    let (provider_id, external_runtime_json) = if runtime_kind == RUNTIME_ACP {
        (None, json_to_db_string(body.acp_runtime.as_ref()))
    } else {
        let provider = match body.llm_provider_id.as_deref() {
            Some(raw) => Some(validate_provider(state.db.pool(), raw, &owner_id).await?),
            None => None,
        };
        (provider, None)
    };

    let id = Uuid::new_v4().to_string();
    let now = now_rfc3339();

    sqlx::query(
        "INSERT INTO agents \
         (id, owner_id, workspace_id, name, description, system_prompt, runtime_kind, \
          provider_id, model_config_json, tool_config_json, external_runtime_json, \
          skill_ids_json, status, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'active', ?, ?)",
    )
    .bind(&id)
    .bind(&owner_id)
    .bind(&workspace_id)
    .bind(&name)
    .bind(&description)
    .bind(&system_prompt)
    .bind(&runtime_kind)
    .bind(&provider_id)
    .bind(&model_config_json)
    .bind(&tool_config_json)
    .bind(&external_runtime_json)
    .bind(&skill_ids_json)
    .bind(&now)
    .bind(&now)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to create agent"))?;

    let row = fetch_row(state.db.pool(), &id)
        .await?
        .ok_or_else(|| ApiError::internal("agent vanished after insert"))?;
    Ok((StatusCode::CREATED, Json(row.into())))
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<AgentResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;

    let sql = format!(
        "SELECT {AGENT_COLUMNS} FROM agents \
         WHERE owner_id = ? AND status = 'active' \
         ORDER BY created_at DESC, id DESC"
    );
    let rows = sqlx::query_as::<_, AgentRow>(&sql)
        .bind(&owner_id)
        .fetch_all(state.db.pool())
        .await
        .map_err(|_| ApiError::internal("database error"))?;

    Ok(Json(rows.into_iter().map(AgentResponse::from).collect()))
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<Json<AgentResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let agent_id = validate_uuid(&agent_id, "agent id")?;

    let row = load_active_owned(state.db.pool(), &agent_id, &owner_id).await?;
    Ok(Json(row.into()))
}

pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
    Json(body): Json<UpdateRequest>,
) -> Result<Json<AgentResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let agent_id = validate_uuid(&agent_id, "agent id")?;

    let existing = load_active_owned(state.db.pool(), &agent_id, &owner_id).await?;

    let name = match body.name {
        Some(ref raw) => validate_name(raw)?,
        None => existing.name.clone(),
    };
    let description = match body.description {
        Some(ref value) => normalize_description(value.as_deref()),
        None => existing.description.clone(),
    };
    let system_prompt = match body.system_prompt.as_deref() {
        Some(raw) => validate_system_prompt(raw)?,
        None => existing.system_prompt.clone(),
    };
    let runtime_kind = match body.runtime_kind.as_deref() {
        Some(raw) => normalize_runtime_kind(Some(raw))?,
        None => existing.runtime_kind.clone(),
    };
    let workspace_id = match body.workspace_id.as_deref() {
        Some(raw) => Some(validate_workspace(state.db.pool(), raw, &owner_id).await?),
        None => existing.workspace_id.clone(),
    };
    let model_config_json = match body.llm_config {
        Some(ref value) => json_to_db_string(value.as_ref()),
        None => existing.model_config_json.clone(),
    };
    let tool_config_json = match body.tool_config {
        Some(ref value) => json_to_db_string(value.as_ref()),
        None => existing.tool_config_json.clone(),
    };
    let skill_ids_json = match body.skill_ids {
        Some(ref list) => validate_skill_ids(Some(list))?,
        None => existing.skill_ids_json.clone(),
    };

    // The effective runtime kind decides which of provider/runtime survives.
    let (provider_id, external_runtime_json) = if runtime_kind == RUNTIME_ACP {
        let runtime = match body.acp_runtime {
            Some(ref value) => json_to_db_string(value.as_ref()),
            None => existing.external_runtime_json.clone(),
        };
        (None, runtime)
    } else {
        let provider = match body.llm_provider_id {
            Some(ref value) => match value.as_deref() {
                Some(raw) => Some(validate_provider(state.db.pool(), raw, &owner_id).await?),
                None => None,
            },
            None => match existing.provider_id.as_deref() {
                Some(raw) => Some(validate_provider(state.db.pool(), raw, &owner_id).await?),
                None => None,
            },
        };
        (provider, None)
    };

    let now = now_rfc3339();
    sqlx::query(
        "UPDATE agents SET \
         name = ?, description = ?, system_prompt = ?, runtime_kind = ?, workspace_id = ?, \
         provider_id = ?, model_config_json = ?, tool_config_json = ?, external_runtime_json = ?, \
         skill_ids_json = ?, updated_at = ? \
         WHERE id = ? AND owner_id = ?",
    )
    .bind(&name)
    .bind(&description)
    .bind(&system_prompt)
    .bind(&runtime_kind)
    .bind(&workspace_id)
    .bind(&provider_id)
    .bind(&model_config_json)
    .bind(&tool_config_json)
    .bind(&external_runtime_json)
    .bind(&skill_ids_json)
    .bind(&now)
    .bind(&agent_id)
    .bind(&owner_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to update agent"))?;

    let row = fetch_row(state.db.pool(), &agent_id)
        .await?
        .ok_or_else(|| ApiError::internal("agent vanished after update"))?;
    Ok(Json(row.into()))
}

pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let agent_id = validate_uuid(&agent_id, "agent id")?;

    // Confirms existence/ownership (and that it is not already deleted) first.
    load_active_owned(state.db.pool(), &agent_id, &owner_id).await?;

    let now = now_rfc3339();
    sqlx::query(
        "UPDATE agents SET status = 'deleted', workspace_id = NULL, updated_at = ? \
         WHERE id = ? AND owner_id = ?",
    )
    .bind(&now)
    .bind(&agent_id)
    .bind(&owner_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to delete agent"))?;

    Ok(StatusCode::NO_CONTENT)
}

/// Fetch an active agent by id and enforce caller ownership.
///
/// Returns `404 not_found` when no row exists or it has been soft-deleted, and
/// `403 permission_denied` when an active row belongs to another user.
async fn load_active_owned(
    pool: &SqlitePool,
    agent_id: &str,
    owner_id: &str,
) -> Result<AgentRow, ApiError> {
    let row = fetch_row(pool, agent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("agent not found"))?;
    if row.status == "deleted" {
        return Err(ApiError::not_found("agent not found"));
    }
    if row.owner_id != owner_id {
        return Err(ApiError::permission_denied("agent belongs to another user"));
    }
    Ok(row)
}

async fn fetch_row(pool: &SqlitePool, agent_id: &str) -> Result<Option<AgentRow>, ApiError> {
    let sql = format!("SELECT {AGENT_COLUMNS} FROM agents WHERE id = ?");
    sqlx::query_as::<_, AgentRow>(&sql)
        .bind(agent_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}

/// Resolve a workspace reference to its canonical id, requiring it to be an
/// active workspace owned by the caller.
async fn validate_workspace(
    pool: &SqlitePool,
    raw_id: &str,
    owner_id: &str,
) -> Result<String, ApiError> {
    let id = validate_uuid(raw_id, "workspace_id")?;
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT owner_id, status FROM workspaces WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;

    match row {
        None => Err(ApiError::invalid_input(
            "workspace_id does not reference a workspace",
        )),
        Some((owner, _)) if owner != owner_id => Err(ApiError::permission_denied(
            "workspace belongs to another user",
        )),
        Some((_, status)) if status != "active" => {
            Err(ApiError::invalid_input("workspace is not active"))
        }
        Some(_) => Ok(id),
    }
}

/// Resolve a provider reference to its canonical id, requiring it to be an
/// active provider owned by the caller.
async fn validate_provider(
    pool: &SqlitePool,
    raw_id: &str,
    owner_id: &str,
) -> Result<String, ApiError> {
    let id = validate_uuid(raw_id, "llm_provider_id")?;
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT owner_id, status FROM llm_providers WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;

    match row {
        None => Err(ApiError::invalid_input(
            "llm_provider_id does not reference a provider",
        )),
        Some((_, status)) if status != "active" => {
            Err(ApiError::invalid_input("llm_provider_id is not active"))
        }
        Some((owner, _)) if owner != owner_id => Err(ApiError::permission_denied(
            "provider belongs to another user",
        )),
        Some(_) => Ok(id),
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

fn validate_system_prompt(raw: &str) -> Result<String, ApiError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ApiError::invalid_input("system_prompt must not be empty"));
    }
    Ok(trimmed.to_string())
}

fn normalize_runtime_kind(raw: Option<&str>) -> Result<String, ApiError> {
    match raw.map(str::trim) {
        None | Some("") => Ok(RUNTIME_LLM_CHAT.to_string()),
        Some(RUNTIME_LLM_CHAT) => Ok(RUNTIME_LLM_CHAT.to_string()),
        Some(RUNTIME_ACP) => Ok(RUNTIME_ACP.to_string()),
        Some(_) => Err(ApiError::invalid_input(
            "runtime_kind must be 'llm_chat' or 'acp'",
        )),
    }
}

fn normalize_description(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| d.to_string())
}

/// Validate that every supplied skill id is a UUID and serialize them to the
/// JSON array stored in `skill_ids_json`. Absent input yields an empty array.
fn validate_skill_ids(skills: Option<&[String]>) -> Result<String, ApiError> {
    let ids = match skills {
        Some(list) => list
            .iter()
            .map(|s| validate_uuid(s, "skill_ids"))
            .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };
    Ok(serde_json::to_string(&ids).unwrap_or_else(|_| "[]".to_string()))
}

fn json_to_db_string(value: Option<&Value>) -> Option<String> {
    match value {
        Some(v) if !v.is_null() => serde_json::to_string(v).ok(),
        _ => None,
    }
}

fn parse_json(raw: Option<&str>) -> Option<Value> {
    raw.and_then(|r| serde_json::from_str::<Value>(r).ok())
}

fn builtin_tools() -> Vec<BuiltinToolResponse> {
    vec![
        tool(
            "read",
            "Read",
            "Read files from the bound workspace.",
            "read",
            true,
        ),
        tool(
            "write",
            "Write",
            "Create or replace files in the bound workspace.",
            "write",
            true,
        ),
        tool(
            "edit",
            "Edit",
            "Patch existing files in the bound workspace.",
            "write",
            true,
        ),
        tool(
            "glob",
            "Glob",
            "Find files in the bound workspace by pattern.",
            "read",
            true,
        ),
        tool(
            "grep",
            "Grep",
            "Search file contents in the bound workspace.",
            "read",
            true,
        ),
        tool(
            "bash",
            "Bash",
            "Run guarded shell commands in the bound workspace.",
            "execute",
            true,
        ),
        tool(
            "ask_user",
            "AskUser",
            "Ask the user for clarification or approval.",
            "planning",
            false,
        ),
        tool(
            "web_search",
            "WebSearch",
            "Search the web for current information.",
            "network",
            false,
        ),
        tool(
            "fetch",
            "Fetch",
            "Fetch and inspect a specific URL.",
            "network",
            false,
        ),
        tool(
            "run_sub_agent",
            "RunSubAgent",
            "Delegate read-only exploration to a sub-agent.",
            "orchestration",
            false,
        ),
        tool(
            "generate_image",
            "GenerateImage",
            "Generate images through a media provider.",
            "media",
            false,
        ),
        tool(
            "generate_video",
            "GenerateVideo",
            "Generate videos through a media provider.",
            "media",
            false,
        ),
        tool(
            "skill_manager",
            "SkillManager",
            "Inspect and activate mounted skills.",
            "orchestration",
            false,
        ),
        tool(
            "todo_write",
            "TodoWrite",
            "Track multi-step agent tasks.",
            "planning",
            false,
        ),
        tool(
            "exit_plan_mode",
            "ExitPlanMode",
            "Request user approval after planning.",
            "planning",
            false,
        ),
    ]
}

fn tool(
    id: &'static str,
    name: &'static str,
    description: &'static str,
    policy: &'static str,
    requires_workspace: bool,
) -> BuiltinToolResponse {
    BuiltinToolResponse {
        id,
        name,
        description,
        policy,
        requires_workspace,
        requires_sandbox: false,
        runtime_status: "available",
    }
}

fn fallback_acp_presets() -> Vec<AcpRuntimePresetResponse> {
    vec![
        AcpRuntimePresetResponse {
            id: "codex",
            name: "Codex",
            description: "Codex CLI through the Zed Codex ACP adapter.",
            profile: "codex",
            installed: false,
            command: Some("npx"),
            args: vec!["@zed-industries/codex-acp"],
            env: BTreeMap::new(),
            timeout_seconds: 3600,
            permission_policy: "deny",
            default_model: None,
            default_mode: Some("read-only"),
            default_thinking_effort: Some("medium"),
            model_options: Vec::new(),
            mode_options: vec![
                choice(
                    "read-only",
                    "Read Only",
                    Some("Read files in the current workspace; ask before edits or internet."),
                ),
                choice(
                    "auto",
                    "Default",
                    Some("Read and edit workspace files; ask for internet or external edits."),
                ),
                choice(
                    "full-access",
                    "Full Access",
                    Some("Edit outside the workspace and access the internet without asking."),
                ),
            ],
            thinking_effort_options: vec![
                choice("", "Default", None),
                choice("minimal", "Minimal", None),
                choice("low", "Low", None),
                choice("medium", "Medium", None),
                choice("high", "High", None),
                choice("xhigh", "XHigh", None),
            ],
            install_hint: "Install @zed-industries/codex-acp so codex-acp is on PATH, or keep the npx fallback command.",
            source: Some("fallback"),
        },
        AcpRuntimePresetResponse {
            id: "claude",
            name: "Claude Code",
            description: "Claude Agent SDK through the official Claude Agent ACP adapter.",
            profile: "claude",
            installed: false,
            command: Some("npx"),
            args: vec!["@agentclientprotocol/claude-agent-acp"],
            env: BTreeMap::new(),
            timeout_seconds: 3600,
            permission_policy: "deny",
            default_model: None,
            default_mode: Some("default"),
            default_thinking_effort: Some("high"),
            model_options: Vec::new(),
            mode_options: vec![
                choice("default", "Default", None),
                choice(
                    "auto",
                    "Auto",
                    Some("Use a model classifier to approve or deny permission prompts."),
                ),
                choice("acceptEdits", "Accept Edits", None),
                choice("plan", "Plan", None),
                choice("dontAsk", "Don't Ask", None),
                choice("bypassPermissions", "Bypass Permissions", None),
            ],
            thinking_effort_options: vec![
                choice("low", "Low", None),
                choice("medium", "Medium", None),
                choice("high", "High", None),
                choice("max", "Max", None),
            ],
            install_hint: "Install @agentclientprotocol/claude-agent-acp so claude-agent-acp is on PATH, or keep the npx fallback command.",
            source: Some("fallback"),
        },
    ]
}

fn choice(
    value: &'static str,
    label: &'static str,
    description: Option<&'static str>,
) -> AcpRuntimeChoiceResponse {
    AcpRuntimeChoiceResponse {
        value,
        label,
        description,
    }
}

fn validate_uuid(raw: &str, field: &str) -> Result<String, ApiError> {
    Uuid::parse_str(raw.trim())
        .map(|id| id.to_string())
        .map_err(|_| ApiError::invalid_input(format!("invalid {field}")))
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
