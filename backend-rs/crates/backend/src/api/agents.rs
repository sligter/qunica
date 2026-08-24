use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::path::{Path as FsPath, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::time::timeout;
use uuid::Uuid;

use crate::acp::{
    canonicalize_acp_runtime, dsh, normalize_acp_runtime, probe_acp_runtime_capabilities,
    shutdown_reusable_acp_sessions, AcpCapabilityError, AcpConfigValue, AcpRuntimeCapabilities,
    AcpRuntimeConfig, PermissionPolicy, DEFAULT_TIMEOUT_SECONDS,
};
use crate::api::{auth::current_user_id, error::ApiError, AppState};
use crate::llm::{
    build_provider, model_reasoning_passback, ChatDelta, ChatMessage, ChatRequest, ProviderConfig,
};
use crate::process::tokio_command_no_window;

const RUNTIME_LLM_CHAT: &str = "llm_chat";
const RUNTIME_ACP: &str = "acp";
const DEFAULT_SYSTEM_PROMPT: &str = "You are a helpful AI agent.";
const ACP_VERSION_TIMEOUT: Duration = Duration::from_secs(10);
const ACP_INSTALL_TIMEOUT: Duration = Duration::from_secs(600);
const ACP_OUTPUT_LIMIT: usize = 8 * 1024;
const MAX_PROMPT_SOURCE_CHARS: usize = 50_000;
const MAX_GENERATED_PROMPT_CHARS: usize = 20_000;
const SYSTEM_PROMPT_WRITER_PROMPT: &str = "You write production system prompts for agents running inside ag-swarmer. Return only the complete system prompt in Markdown, with no code fence or commentary.\n\
The prompt must be specific to the supplied role and usable as-is. It must tell the agent to:\n\
- establish its role, outcomes, boundaries, and a short execution workflow;\n\
- inspect relevant workspace context before acting and keep changes focused;\n\
- use only tools exposed at runtime, never invent tools, and respect workspace boundaries;\n\
- follow mounted Skills when relevant and collaborate clearly with other Agents or groups;\n\
- verify meaningful work and report changes, checks, and remaining risks concisely;\n\
- ask for clarification only when missing information blocks safe progress;\n\
- use the language of the supplied role details or existing prompt when it is clear.\n\
For an enhancement, preserve the original intent and useful constraints while removing repetition and ambiguity. Do not mention this generation request, a provider, or a model, and do not leave placeholders.";

const AGENT_COLUMNS: &str = "id, owner_id, workspace_id, \
     COALESCE((SELECT json_group_array(workspace_id) FROM agent_workspaces WHERE agent_id = agents.id), '[]') AS workspace_ids_json, \
     name, description, system_prompt, \
     runtime_kind, provider_id, model_config_json, tool_config_json, external_runtime_json, \
     skill_ids_json, status, is_system, created_at, updated_at";

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
    workspace_ids: Option<Vec<String>>,
    #[serde(default, alias = "provider_id")]
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
    #[serde(default)]
    workspace_ids: Option<Vec<String>>,
    #[serde(default, alias = "provider_id", deserialize_with = "double_option")]
    llm_provider_id: Option<Option<String>>,
    #[serde(default)]
    skill_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct GenerateSystemPromptRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
    llm_provider_id: String,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GenerateSystemPromptResponse {
    system_prompt: String,
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
    workspace_ids: Vec<String>,
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
    command: Option<String>,
    args: Vec<&'static str>,
    env: BTreeMap<String, String>,
    timeout_seconds: u64,
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

#[derive(Debug, Serialize)]
pub struct AcpRuntimeVersionListResponse {
    presets: Vec<AcpRuntimeVersionResponse>,
}

#[derive(Debug, Serialize)]
struct AcpRuntimeVersionResponse {
    id: &'static str,
    package_name: &'static str,
    /// The spec the Install button actually uses, so the custom-install hint
    /// can show it instead of guessing `<name>@latest` — which for dsh names
    /// an older, incompatible release.
    default_package_spec: String,
    installed: bool,
    local_version: Option<String>,
    latest_version: Option<String>,
    status: &'static str,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AcpRuntimeInstallRequest {
    #[serde(default)]
    package_spec: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AcpRuntimeInstallResponse {
    preset: AcpRuntimeVersionResponse,
    output: String,
}

#[derive(Debug, sqlx::FromRow)]
struct AgentRow {
    id: String,
    owner_id: String,
    workspace_id: Option<String>,
    workspace_ids_json: String,
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
    /// `1` for the built-in Assistant. Guards the mutating routes below.
    is_system: i64,
    created_at: String,
    #[allow(dead_code)]
    updated_at: String,
}

#[derive(Debug, sqlx::FromRow)]
struct PromptProviderRow {
    kind: String,
    base_url: Option<String>,
    api_key: String,
    default_model: String,
    reasoning_passback: i64,
    models_json: Option<String>,
}

impl From<AgentRow> for AgentResponse {
    fn from(row: AgentRow) -> Self {
        let skill_ids =
            serde_json::from_str::<Vec<String>>(&row.skill_ids_json).unwrap_or_default();
        let mut workspace_ids = row.workspace_id.iter().cloned().collect::<Vec<_>>();
        for id in serde_json::from_str::<Vec<String>>(&row.workspace_ids_json).unwrap_or_default() {
            if !workspace_ids.contains(&id) {
                workspace_ids.push(id);
            }
        }
        Self {
            id: row.id,
            name: row.name,
            description: row.description,
            system_prompt: row.system_prompt,
            llm_config: parse_json(row.model_config_json.as_deref()),
            tool_config: parse_json(row.tool_config_json.as_deref()),
            runtime_kind: row.runtime_kind,
            acp_runtime: canonicalized_acp_runtime_json(row.external_runtime_json.as_deref()),
            workspace_id: row.workspace_id,
            workspace_ids,
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

pub async fn generate_system_prompt(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<GenerateSystemPromptRequest>,
) -> Result<Json<GenerateSystemPromptResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let provider_id =
        validate_provider(state.db.pool(), body.llm_provider_id.trim(), &owner_id).await?;
    let provider_row: PromptProviderRow = sqlx::query_as(
        "SELECT kind, base_url, api_key, default_model, reasoning_passback, models_json \
         FROM llm_providers WHERE id = ?",
    )
    .bind(&provider_id)
    .fetch_one(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to load the prompt generation provider"))?;

    let name = body
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Agent");
    if name.chars().count() > 100 {
        return Err(ApiError::invalid_input(
            "agent name must be at most 100 characters",
        ));
    }
    let description = body
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if description.is_some_and(|value| value.chars().count() > 4_000) {
        return Err(ApiError::invalid_input(
            "agent description must be at most 4000 characters",
        ));
    }
    let existing_prompt = body
        .system_prompt
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if existing_prompt.is_some_and(|value| value.chars().count() > MAX_PROMPT_SOURCE_CHARS) {
        return Err(ApiError::invalid_input(
            "system_prompt must be at most 50000 characters for enhancement",
        ));
    }

    let model = prompt_generation_model(&provider_row, body.model.as_deref())?;
    let reasoning_passback = model_reasoning_passback(
        provider_row.models_json.as_deref(),
        &model,
        provider_row.reasoning_passback != 0,
    );
    let provider_config = ProviderConfig {
        kind: provider_row.kind,
        base_url: provider_row.base_url,
        api_key: provider_row.api_key,
        default_model: provider_row.default_model,
        reasoning_passback,
        context_window_tokens: None,
        context_output_reserve_ratio: None,
    };
    let provider = build_provider(&provider_config).map_err(|err| {
        ApiError::invalid_input(format!("system prompt generation failed: {err}"))
    })?;
    let request = ChatRequest {
        model,
        messages: system_prompt_generation_messages(name, description, existing_prompt),
        temperature: Some(0.4),
        reasoning_passback,
        include_empty_tools: false,
        tools: Vec::new(),
        reasoning_effort: None,
    };
    let mut deltas = provider.stream(request).await.map_err(|err| {
        ApiError::invalid_input(format!("system prompt generation failed: {err}"))
    })?;

    let mut raw = String::new();
    let mut output_chars = 0;
    while let Some(delta) = deltas.recv().await {
        match delta {
            ChatDelta::Token(text) => {
                output_chars += text.chars().count();
                if output_chars > MAX_GENERATED_PROMPT_CHARS {
                    return Err(ApiError::invalid_input(
                        "generated system prompt exceeds 20000 characters",
                    ));
                }
                raw.push_str(&text);
            }
            ChatDelta::Done => break,
            ChatDelta::Truncated(reason) => {
                return Err(ApiError::invalid_input(format!(
                    "system prompt generation failed: provider stream ended early: {reason}"
                )));
            }
            ChatDelta::Reasoning(_)
            | ChatDelta::ReasoningSignature(_)
            | ChatDelta::ToolCall(_)
            | ChatDelta::Usage(_) => {}
        }
    }

    Ok(Json(GenerateSystemPromptResponse {
        system_prompt: clean_generated_system_prompt(&raw)?,
    }))
}

pub async fn acp_runtime_presets() -> Json<AcpRuntimePresetListResponse> {
    Json(AcpRuntimePresetListResponse {
        presets: fallback_acp_presets(),
    })
}

pub async fn acp_runtime_versions(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AcpRuntimeVersionListResponse>, ApiError> {
    let _owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let mut presets = Vec::new();
    for preset in ACP_RUNTIME_VERSION_PRESETS {
        presets.push(runtime_version_status(preset).await);
    }
    Ok(Json(AcpRuntimeVersionListResponse { presets }))
}

pub async fn install_acp_runtime_version(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(preset_id): Path<String>,
    Json(body): Json<AcpRuntimeInstallRequest>,
) -> Result<Json<AcpRuntimeInstallResponse>, ApiError> {
    let _owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let preset = acp_runtime_version_preset(&preset_id)
        .ok_or_else(|| ApiError::not_found("ACP runtime preset not found"))?;
    let package_spec = resolve_install_package_spec(preset, body.package_spec.as_deref())?;

    let npm = npm_command().ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_GATEWAY,
            "acp_runtime_install_failed",
            "Unable to find npm. Ensure npm is installed and on PATH.",
        )
    })?;
    // Companion packages go in the same install so a multi-package runtime can
    // never end up half-present, which surfaces only as a startup failure.
    let mut args = vec!["install", "--global", "--include=optional", &package_spec];
    args.extend(preset.install_specs.iter().skip(1).copied());
    // Windows cannot replace a runtime executable while a reusable ACP session
    // still has it open. Sessions resume from their persisted IDs on next use.
    shutdown_reusable_acp_sessions().await;
    let output = run_command(&npm, &args, ACP_INSTALL_TIMEOUT)
        .await
        .map_err(|message| {
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                "acp_runtime_install_failed",
                message,
            )
        })?;

    Ok(Json(AcpRuntimeInstallResponse {
        preset: runtime_version_status(preset).await,
        output,
    }))
}

pub async fn acp_runtime_capabilities(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<AcpRuntimeCapabilities>), ApiError> {
    let _owner_id = current_user_id(&headers, &state.auth.secret_key)?;

    let mut config = match normalize_acp_runtime(Some(&body)) {
        Ok(config) => config,
        Err(error) => {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(AcpRuntimeCapabilities::warning(error.to_string())),
            ));
        }
    };
    canonicalize_acp_runtime(&mut config);
    let selected_model = config.model.take();
    config.permission_policy = PermissionPolicy::Deny;

    match probe_acp_runtime_capabilities(config, selected_model).await {
        Ok(capabilities) => Ok((StatusCode::OK, Json(capabilities))),
        Err(AcpCapabilityError::Spawn { source }) => {
            tracing::warn!(error = %source, "ACP capability probe failed to spawn runtime");
            Ok((
                StatusCode::BAD_REQUEST,
                Json(AcpRuntimeCapabilities::warning(
                    "Unable to start the configured ACP runtime.",
                )),
            ))
        }
        Err(AcpCapabilityError::Protocol { source }) => {
            tracing::warn!(error = %source, "ACP capability probe rejected by runtime");
            Ok((
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(AcpRuntimeCapabilities::warning(
                    "The configured ACP runtime rejected capability discovery.",
                )),
            ))
        }
        Err(error @ AcpCapabilityError::Timeout) => Ok((
            StatusCode::GATEWAY_TIMEOUT,
            Json(AcpRuntimeCapabilities::warning(error.to_string())),
        )),
        Err(error @ AcpCapabilityError::Environment { .. }) => Ok((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AcpRuntimeCapabilities::warning(error.to_string())),
        )),
    }
}

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateRequest>,
) -> Result<(StatusCode, Json<AgentResponse>), ApiError> {
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
) -> Result<AgentResponse, ApiError> {
    let owner_id = owner_id.to_string();
    let name = validate_name(&body.name)?;
    let system_prompt = match body.system_prompt.as_deref() {
        Some(raw) => validate_system_prompt(raw)?,
        None => DEFAULT_SYSTEM_PROMPT.to_string(),
    };
    let runtime_kind = normalize_runtime_kind(body.runtime_kind.as_deref())?;
    let workspace_id = validate_workspace(state.db.pool(), &body.workspace_id, &owner_id).await?;
    let workspace_ids = validate_workspace_ids(
        state.db.pool(),
        body.workspace_ids.as_deref(),
        &workspace_id,
        &owner_id,
    )
    .await?;
    let description = normalize_description(body.description.as_deref());
    let skill_ids_json = validate_skill_ids(body.skill_ids.as_deref())?;
    let model_config_json = json_to_db_string(body.llm_config.as_ref());
    let tool_config_json = json_to_db_string(body.tool_config.as_ref());

    // Runtime-specific binding: ACP agents store their runtime blob and never a
    // provider; LLM chat agents store an optional provider and never a runtime.
    let (provider_id, external_runtime_json) = if runtime_kind == RUNTIME_ACP {
        (
            None,
            canonicalized_acp_runtime_db_json(body.acp_runtime.as_ref())?,
        )
    } else {
        let provider = match body.llm_provider_id.as_deref() {
            Some(raw) => Some(validate_provider(state.db.pool(), raw, &owner_id).await?),
            None => None,
        };
        (provider, None)
    };

    let id = Uuid::new_v4().to_string();
    let now = now_rfc3339();
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start agent create transaction"))?;

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
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to create agent"))?;

    for extra_workspace_id in workspace_ids {
        sqlx::query(
            "INSERT INTO agent_workspaces (agent_id, workspace_id, created_at) VALUES (?, ?, ?)",
        )
        .bind(&id)
        .bind(extra_workspace_id)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal("failed to bind agent workspace"))?;
    }
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit agent create"))?;

    let row = fetch_row(state.db.pool(), &id)
        .await?
        .ok_or_else(|| ApiError::internal("agent vanished after insert"))?;
    Ok(row.into())
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<AgentResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;

    // System agents (the built-in Assistant) are reached through their own
    // route, not the library. Listing them would offer the user edit and delete
    // controls that the handlers below refuse anyway.
    let sql = format!(
        "SELECT {AGENT_COLUMNS} FROM agents \
         WHERE owner_id = ? AND status = 'active' AND is_system = 0 \
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
    Ok(Json(
        update_inner(&state, &owner_id, &agent_id, body).await?,
    ))
}

/// The body of [`update`] without the axum extractors. See [`create_inner`].
pub(crate) async fn update_inner(
    state: &AppState,
    owner_id: &str,
    agent_id: &str,
    body: UpdateRequest,
) -> Result<AgentResponse, ApiError> {
    let owner_id = owner_id.to_string();
    let agent_id = validate_uuid(agent_id, "agent id")?;

    let existing = load_active_owned_writable(state.db.pool(), &agent_id, &owner_id).await?;

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
    let workspace_ids = match (body.workspace_ids.as_deref(), workspace_id.as_deref()) {
        (Some(raw), Some(primary)) => {
            Some(validate_workspace_ids(state.db.pool(), Some(raw), primary, &owner_id).await?)
        }
        (Some(_), None) => {
            return Err(ApiError::invalid_input(
                "workspace_ids require a primary workspace_id",
            ));
        }
        (None, _) => None,
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
            Some(ref value) => canonicalized_acp_runtime_db_json(value.as_ref())?,
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
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start agent update transaction"))?;
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
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to update agent"))?;

    if let Some(workspace_ids) = workspace_ids {
        sqlx::query("DELETE FROM agent_workspaces WHERE agent_id = ?")
            .bind(&agent_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| ApiError::internal("failed to replace agent workspaces"))?;
        for extra_workspace_id in workspace_ids {
            sqlx::query(
                "INSERT INTO agent_workspaces (agent_id, workspace_id, created_at) VALUES (?, ?, ?)",
            )
            .bind(&agent_id)
            .bind(extra_workspace_id)
            .bind(&now)
            .execute(&mut *tx)
            .await
            .map_err(|_| ApiError::internal("failed to bind agent workspace"))?;
        }
    }
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit agent update"))?;

    let row = fetch_row(state.db.pool(), &agent_id)
        .await?
        .ok_or_else(|| ApiError::internal("agent vanished after update"))?;
    Ok(row.into())
}

pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let agent_id = validate_uuid(&agent_id, "agent id")?;

    // Confirms existence/ownership (and that it is not already deleted) first.
    load_active_owned_writable(state.db.pool(), &agent_id, &owner_id).await?;

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

/// Like [`load_active_owned`], but refuses the built-in Assistant.
///
/// Its name, prompt, tools and workspace define what it is safe for it to do;
/// letting the generic agent routes rewrite any of those would turn the one
/// agent with app-control tools into an arbitrary one.
async fn load_active_owned_writable(
    pool: &SqlitePool,
    agent_id: &str,
    owner_id: &str,
) -> Result<AgentRow, ApiError> {
    let row = load_active_owned(pool, agent_id, owner_id).await?;
    if row.is_system != 0 {
        return Err(ApiError::permission_denied(
            "the built-in assistant cannot be modified",
        ));
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

async fn validate_workspace_ids(
    pool: &SqlitePool,
    raw_ids: Option<&[String]>,
    primary_id: &str,
    owner_id: &str,
) -> Result<Vec<String>, ApiError> {
    let mut ids = Vec::new();
    for raw_id in raw_ids.unwrap_or_default() {
        let id = validate_workspace(pool, raw_id, owner_id).await?;
        if id != primary_id && !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
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

fn prompt_generation_model(
    provider: &PromptProviderRow,
    requested: Option<&str>,
) -> Result<String, ApiError> {
    let requested = requested
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&provider.default_model);
    if requested == provider.default_model {
        return Ok(requested.to_string());
    }
    let offered = provider
        .models_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|value| value.as_array().cloned())
        .is_some_and(|models| {
            models.iter().any(|model| {
                model.as_str() == Some(requested)
                    || model.get("id").and_then(Value::as_str) == Some(requested)
            })
        });
    if offered {
        Ok(requested.to_string())
    } else {
        Err(ApiError::invalid_input(
            "model is not offered by that provider",
        ))
    }
}

fn system_prompt_generation_messages(
    name: &str,
    description: Option<&str>,
    existing_prompt: Option<&str>,
) -> Vec<ChatMessage> {
    let task = if existing_prompt.is_some() {
        "Enhance the existing system prompt."
    } else {
        "Create a new system prompt."
    };
    let profile = json!({
        "name": name,
        "description": description,
        "existing_system_prompt": existing_prompt,
    });
    vec![
        ChatMessage::text("system", SYSTEM_PROMPT_WRITER_PROMPT),
        ChatMessage::text("user", format!("{task}\nAgent profile:\n{profile}")),
    ]
}

fn clean_generated_system_prompt(raw: &str) -> Result<String, ApiError> {
    let prompt = raw
        .lines()
        .filter(|line| !line.trim().starts_with("```"))
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err(ApiError::invalid_input(
            "provider returned an empty system prompt",
        ));
    }
    if prompt.chars().count() > MAX_GENERATED_PROMPT_CHARS {
        return Err(ApiError::invalid_input(
            "generated system prompt exceeds 20000 characters",
        ));
    }
    Ok(prompt.to_string())
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

fn canonicalized_acp_runtime_db_json(raw: Option<&Value>) -> Result<Option<String>, ApiError> {
    match raw {
        None | Some(Value::Null) => Ok(None),
        Some(raw) => {
            let mut config = normalize_acp_runtime(Some(raw))
                .map_err(|error| ApiError::invalid_input(error.to_string()))?;
            canonicalize_acp_runtime(&mut config);
            Ok(serde_json::to_string(&acp_runtime_config_value(config)).ok())
        }
    }
}

fn canonicalized_acp_runtime_json(raw: Option<&str>) -> Option<Value> {
    let raw = parse_json(raw)?;
    let mut config = normalize_acp_runtime(Some(&raw)).ok()?;
    canonicalize_acp_runtime(&mut config);
    Some(acp_runtime_config_value(config))
}

fn acp_runtime_config_value(config: AcpRuntimeConfig) -> Value {
    let config_options = config.config_options.map(|options| {
        options
            .into_iter()
            .map(|(key, value)| {
                let value = match value {
                    AcpConfigValue::Str(value) => Value::String(value),
                    AcpConfigValue::Bool(value) => Value::Bool(value),
                };
                (key, value)
            })
            .collect::<serde_json::Map<_, _>>()
    });
    json!({
        "profile": config.profile.as_str(),
        "command": config.command,
        "args": config.args,
        "env": config.env,
        "timeout_seconds": config.timeout_seconds,
        "permission_policy": config.permission_policy.as_str(),
        "model": config.model,
        "mode": config.mode,
        "thinking_effort": config.thinking_effort,
        "config_options": config_options,
    })
}

fn builtin_tools() -> Vec<BuiltinToolResponse> {
    vec![
        tool(
            "read",
            "Read",
            "Read UTF-8 files with offset and limit controls.",
            "read",
            true,
        ),
        tool(
            "write",
            "Write",
            "Create or completely overwrite UTF-8 files.",
            "write",
            true,
        ),
        tool(
            "edit",
            "Edit",
            "Apply one or more exact replacements to a UTF-8 file.",
            "write",
            true,
        ),
        tool(
            "delete_file",
            "DeleteFile",
            "Delete one regular workspace file.",
            "write",
            true,
        ),
        tool(
            "glob",
            "Glob",
            "List workspace files matching a glob pattern.",
            "read",
            true,
        ),
        tool(
            "grep",
            "Grep",
            "Search workspace file contents with a regular expression.",
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
            true,
        ),
        tool(
            "generate_video",
            "GenerateVideo",
            "Generate videos through a media provider.",
            "media",
            true,
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
        runtime_status: match id {
            "run_sub_agent" => "planned",
            _ => "available",
        },
    }
}

fn fallback_acp_presets() -> Vec<AcpRuntimePresetResponse> {
    let npx = fallback_npx_command();
    let codex_acp = command_or_name("codex-acp");
    let pi_acp = command_or_name("pi-acp");
    let opencode = command_or_name("opencode");
    let dsh_acp = command_or_name(dsh::DSH_ACP_COMMAND);

    vec![
        AcpRuntimePresetResponse {
            id: "codex",
            name: "Codex",
            description: "Codex CLI through the Agent Client Protocol Codex adapter.",
            profile: "codex",
            installed: codex_acp.installed,
            command: Some(if codex_acp.installed {
                codex_acp.command.clone()
            } else {
                npx.command.clone()
            }),
            args: if codex_acp.installed {
                Vec::new()
            } else {
                vec!["-y", "@agentclientprotocol/codex-acp"]
            },
            env: BTreeMap::new(),
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
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
                    "agent",
                    "Agent",
                    Some("Read and edit workspace files; ask for internet or external edits."),
                ),
                choice(
                    "agent-full-access",
                    "Agent Full Access",
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
                choice("max", "Max", None),
            ],
            install_hint: "Install @agentclientprotocol/codex-acp so codex-acp is on PATH, or keep the npx fallback command.",
            source: Some("fallback"),
        },
        AcpRuntimePresetResponse {
            id: "claude",
            name: "Claude Code",
            description: "Claude Agent SDK through the official Claude Agent ACP adapter.",
            profile: "claude",
            installed: npx.installed,
            command: Some(npx.command.clone()),
            args: vec!["@agentclientprotocol/claude-agent-acp"],
            env: BTreeMap::new(),
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
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
        AcpRuntimePresetResponse {
            id: "pi",
            name: "Pi Agent",
            description: "Pi Agent through the pi-acp ACP adapter.",
            profile: "pi",
            installed: pi_acp.installed,
            command: Some(if pi_acp.installed {
                pi_acp.command.clone()
            } else {
                npx.command.clone()
            }),
            args: if pi_acp.installed {
                Vec::new()
            } else {
                vec!["-y", "pi-acp"]
            },
            env: BTreeMap::new(),
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
            permission_policy: "deny",
            default_model: None,
            default_mode: None,
            default_thinking_effort: None,
            model_options: Vec::new(),
            mode_options: Vec::new(),
            thinking_effort_options: Vec::new(),
            install_hint: "Install pi-acp so it is on PATH, or keep the npx fallback command.",
            source: Some("fallback"),
        },
        AcpRuntimePresetResponse {
            id: "opencode",
            name: "OpenCode",
            description: "OpenCode ACP server through the opencode CLI.",
            profile: "opencode",
            installed: opencode.installed,
            command: Some(opencode.command.clone()),
            args: vec!["acp"],
            env: BTreeMap::new(),
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
            permission_policy: "deny",
            default_model: None,
            default_mode: None,
            default_thinking_effort: None,
            model_options: Vec::new(),
            mode_options: Vec::new(),
            thinking_effort_options: Vec::new(),
            install_hint: "Install opencode so it is on PATH; the ACP command is opencode acp.",
            source: Some("fallback"),
        },
        AcpRuntimePresetResponse {
            id: "dsh",
            name: "DeepSeek Harness",
            description: "deepseek-harness ACP server. It streams no tool or plan \
                          updates and sessions cannot be resumed; token usage is \
                          estimated by ag-swarmer rather than reported.",
            profile: "dsh",
            installed: dsh_acp.installed,
            command: Some(if dsh_acp.installed {
                dsh_acp.command.clone()
            } else {
                npx.command.clone()
            }),
            args: if dsh_acp.installed {
                Vec::new()
            } else {
                vec!["-y", dsh::DSH_ACP_INSTALL_SPEC]
            },
            env: BTreeMap::new(),
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
            permission_policy: "deny",
            default_model: Some(dsh::DSH_DEFAULT_MODEL),
            default_mode: Some("workspace-write"),
            default_thinking_effort: None,
            model_options: vec![
                choice("deepseek-v4-pro", "DeepSeek V4 Pro", None),
                choice("deepseek-v4-flash", "DeepSeek V4 Flash", None),
            ],
            mode_options: vec![
                choice(
                    "text-only",
                    "Text Only",
                    Some("No shell or filesystem tools; the agent only talks."),
                ),
                choice(
                    "read-only",
                    "Read Only",
                    Some("Shell and filesystem tools with every write denied."),
                ),
                choice(
                    "workspace-write",
                    "Workspace Write",
                    Some(
                        "Writes allowed in the workspace and the platform temp directories, \
                         denied everywhere else. Enforcement is partial on Windows and older \
                         Linux kernels: files granting Everyone write, and hard links, stay \
                         reachable.",
                    ),
                ),
                choice(
                    "danger-full-access",
                    "Danger: Full Access",
                    Some("No confinement, and the runtime stops asking before it writes."),
                ),
            ],
            thinking_effort_options: vec![
                choice("off", "Off", Some("Disable thinking.")),
                choice("high", "High", Some("High reasoning effort (the dsh default).")),
                choice("max", "Max", Some("Maximum reasoning effort.")),
            ],
            install_hint: "Install the dsh preview packages so dsh-acp-demo is on PATH. \
                           Set DEEPSEEK_API_KEY in the runtime environment. Requires \
                           Node 22.19+ or 24+.",
            source: Some("fallback"),
        },
    ]
}

#[derive(Debug, Clone, Copy)]
struct AcpRuntimeVersionPreset {
    id: &'static str,
    package_name: &'static str,
    /// The npm dist-tag carrying the version this runtime should track.
    ///
    /// Almost always `latest`. deepseek-harness is the exception: its `latest`
    /// tag still points at an older release with a different dependency graph,
    /// so checking it would tell users to "update" by downgrading.
    dist_tag: &'static str,
    /// Every package a working install needs, primary first. Empty means the
    /// runtime is a single package installed at `dist_tag`.
    install_specs: &'static [&'static str],
}

const ACP_RUNTIME_VERSION_PRESETS: [AcpRuntimeVersionPreset; 5] = [
    AcpRuntimeVersionPreset {
        id: "codex",
        package_name: "@agentclientprotocol/codex-acp",
        dist_tag: "latest",
        install_specs: &[],
    },
    AcpRuntimeVersionPreset {
        id: "claude",
        package_name: "@agentclientprotocol/claude-agent-acp",
        dist_tag: "latest",
        install_specs: &[],
    },
    AcpRuntimeVersionPreset {
        id: "pi",
        package_name: "pi-acp",
        dist_tag: "latest",
        install_specs: &[],
    },
    AcpRuntimeVersionPreset {
        id: "opencode",
        package_name: "opencode-ai",
        dist_tag: "latest",
        install_specs: &[],
    },
    AcpRuntimeVersionPreset {
        id: "dsh",
        package_name: dsh::DSH_ACP_PACKAGE,
        dist_tag: dsh::DSH_DIST_TAG,
        install_specs: dsh::DSH_INSTALL_SPECS,
    },
];

fn acp_runtime_version_preset(id: &str) -> Option<AcpRuntimeVersionPreset> {
    ACP_RUNTIME_VERSION_PRESETS
        .into_iter()
        .find(|preset| preset.id == id)
}

/// The package names a preset's install covers, primary first.
fn preset_package_names(preset: AcpRuntimeVersionPreset) -> Vec<&'static str> {
    if preset.install_specs.is_empty() {
        return vec![preset.package_name];
    }
    preset
        .install_specs
        .iter()
        .map(|spec| package_name_of_spec(spec))
        .collect()
}

/// Split the version off an npm spec. Handles scoped names, whose leading `@`
/// is part of the name rather than a version separator.
fn package_name_of_spec(spec: &'static str) -> &'static str {
    match spec.rsplit_once('@') {
        Some((name, _)) if !name.is_empty() => name,
        _ => spec,
    }
}

async fn runtime_version_status(preset: AcpRuntimeVersionPreset) -> AcpRuntimeVersionResponse {
    let local_version = local_npm_package_version(preset.package_name).await;
    let latest_version = npm_latest_package_version(preset.package_name, preset.dist_tag).await;

    // A multi-package runtime is only usable once every package is present:
    // dsh boots into a config that names its plugins by package, and a missing
    // one fails the plugin tree at startup rather than at first use. The whole
    // global list is read once — a `npm list` per companion would put ten
    // subprocesses on the path of a single version card.
    let mut missing_companion = None;
    if local_version.is_some() {
        let companions = preset_package_names(preset);
        if companions.len() > 1 {
            let installed_globals = local_npm_global_packages().await;
            missing_companion = companions
                .into_iter()
                .skip(1)
                .find(|name| !installed_globals.contains(*name));
        }
    }

    let installed = local_version.is_some() && missing_companion.is_none();
    let status = match (&local_version, missing_companion, &latest_version) {
        (None, _, _) | (_, Some(_), _) => "not_installed",
        (Some(local), None, Some(latest)) if local == latest => "current",
        (Some(_), None, Some(_)) => "update_available",
        (Some(_), None, None) => "local_only",
    };
    let message = if let Some(name) = missing_companion {
        Some(format!("Incomplete install: {name} is missing."))
    } else if !installed {
        Some("Not installed locally.".to_string())
    } else if latest_version.is_none() {
        Some("Unable to check the npm registry.".to_string())
    } else {
        None
    };

    AcpRuntimeVersionResponse {
        id: preset.id,
        package_name: preset.package_name,
        default_package_spec: default_install_spec(preset),
        installed,
        local_version,
        latest_version,
        status,
        message,
    }
}

async fn local_npm_package_version(package_name: &str) -> Option<String> {
    let npm = npm_command()?;
    let output = run_command(
        &npm,
        &["list", "--global", package_name, "--depth=0", "--json"],
        ACP_VERSION_TIMEOUT,
    )
    .await
    .ok()?;
    let value: Value = serde_json::from_str(&output).ok()?;
    value
        .get("dependencies")?
        .get(package_name)?
        .get("version")?
        .as_str()
        .map(str::to_string)
}

/// The names of every globally installed npm package, read in one call.
///
/// An unreadable list yields an empty set, which reports every companion as
/// missing. That is the safe direction: the install button is idempotent, so a
/// false "incomplete" costs a re-run, while a false "complete" hides a runtime
/// that cannot boot.
async fn local_npm_global_packages() -> std::collections::BTreeSet<String> {
    let mut names = std::collections::BTreeSet::new();
    let Some(npm) = npm_command() else {
        return names;
    };
    let Ok(output) = run_command(
        &npm,
        &["list", "--global", "--depth=0", "--json"],
        ACP_VERSION_TIMEOUT,
    )
    .await
    else {
        return names;
    };
    let Ok(value) = serde_json::from_str::<Value>(&output) else {
        return names;
    };
    if let Some(dependencies) = value.get("dependencies").and_then(Value::as_object) {
        names.extend(dependencies.keys().cloned());
    }
    names
}

async fn npm_latest_package_version(package_name: &str, dist_tag: &str) -> Option<String> {
    let encoded = package_name.replace('/', "%2F");
    let url = format!("https://registry.npmjs.org/{encoded}");
    let client = reqwest::Client::builder()
        .timeout(ACP_VERSION_TIMEOUT)
        .build()
        .ok()?;
    let value: Value = client
        .get(url)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    value
        .get("dist-tags")?
        .get(dist_tag)?
        .as_str()
        .map(str::to_string)
}

/// The spec the plain Install button installs the primary package at.
///
/// Multi-package runtimes provide their primary spec; everything else tracks
/// its dist-tag. Never the bare package name: npm would resolve that through
/// `latest`, which for dsh is an older, incompatible release.
fn default_install_spec(preset: AcpRuntimeVersionPreset) -> String {
    preset
        .install_specs
        .first()
        .map(|spec| (*spec).to_string())
        .unwrap_or_else(|| format!("{}@{}", preset.package_name, preset.dist_tag))
}

fn resolve_install_package_spec(
    preset: AcpRuntimeVersionPreset,
    requested: Option<&str>,
) -> Result<String, ApiError> {
    let default_spec = default_install_spec(preset);
    let package_spec = requested.unwrap_or(&default_spec).trim();
    if package_spec.is_empty()
        || package_spec.len() > 200
        || package_spec.chars().any(char::is_whitespace)
        || !is_package_spec_for(package_spec, preset.package_name)
    {
        return Err(ApiError::invalid_input(
            "custom installation must be a version or dist-tag for the selected ACP package",
        ));
    }
    Ok(package_spec.to_string())
}

fn is_package_spec_for(package_spec: &str, package_name: &str) -> bool {
    package_spec == package_name
        || package_spec
            .strip_prefix(package_name)
            .is_some_and(|suffix| suffix.starts_with('@') && suffix.len() > 1)
}

async fn run_command(
    command: &str,
    args: &[&str],
    command_timeout: Duration,
) -> Result<String, String> {
    let (command, args) = windows_batch_command(command, args);
    let mut standard = StdCommand::new(command);
    standard.args(args);
    let mut child = tokio_command_no_window(standard);
    child.kill_on_drop(true);
    let child = child.output();
    let output = timeout(command_timeout, child)
        .await
        .map_err(|_| "The command timed out.".to_string())?
        .map_err(|_| "Unable to start npm. Ensure npm is installed and on PATH.".to_string())?;
    let mut combined = output.stdout;
    combined.extend(output.stderr);
    let text = String::from_utf8_lossy(&combined);
    let text = truncate_command_output(&text);
    if output.status.success() {
        Ok(text)
    } else {
        Err(if text.is_empty() {
            "npm could not complete the operation.".to_string()
        } else {
            text
        })
    }
}

fn npm_command() -> Option<String> {
    find_command_on_path("npm").map(|path| path.to_string_lossy().into_owned())
}

fn windows_batch_command<'a>(command: &'a str, args: &'a [&'a str]) -> (PathBuf, Vec<&'a str>) {
    #[cfg(windows)]
    {
        if matches!(
            FsPath::new(command)
                .extension()
                .and_then(|extension| extension.to_str()),
            Some(extension) if extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
        ) {
            let comspec = env::var_os("COMSPEC")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"));
            let mut launch_args = vec!["/d", "/s", "/c", "call", command];
            launch_args.extend_from_slice(args);
            return (comspec, launch_args);
        }
    }

    (PathBuf::from(command), args.to_vec())
}

fn truncate_command_output(value: &str) -> String {
    if value.len() <= ACP_OUTPUT_LIMIT {
        return value.trim().to_string();
    }
    let start = value.len() - ACP_OUTPUT_LIMIT;
    format!("...{}", value[start..].trim())
}

#[derive(Debug, Clone)]
struct ResolvedCommand {
    command: String,
    installed: bool,
}

fn fallback_npx_command() -> ResolvedCommand {
    match find_command_on_path("npx") {
        Some(path) => ResolvedCommand {
            command: path.to_string_lossy().into_owned(),
            installed: true,
        },
        None => ResolvedCommand {
            command: "npx".to_string(),
            installed: false,
        },
    }
}

fn command_or_name(command: &str) -> ResolvedCommand {
    match find_command_on_path(command) {
        Some(path) => ResolvedCommand {
            command: path.to_string_lossy().into_owned(),
            installed: true,
        },
        None => ResolvedCommand {
            command: command.to_string(),
            installed: false,
        },
    }
}

fn find_command_on_path(command: &str) -> Option<PathBuf> {
    find_command_on_path_with_env(command, env::var_os("PATH"), env::var_os("PATHEXT"))
}

fn find_command_on_path_with_env(
    command: &str,
    path_env: Option<OsString>,
    pathext_env: Option<OsString>,
) -> Option<PathBuf> {
    let path_env = path_env?;
    let candidates = command_candidates(command, pathext_env);

    env::split_paths(&path_env)
        .flat_map(|dir| candidates.iter().map(move |candidate| dir.join(candidate)))
        .find(|path| is_executable_file(path))
        .map(absolute_command_path)
}

fn absolute_command_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(&path))
            .unwrap_or(path)
    }
}

fn command_candidates(command: &str, pathext_env: Option<OsString>) -> Vec<String> {
    let command_path = FsPath::new(command);
    if command_path.extension().is_some() {
        return vec![command.to_string()];
    }

    let mut candidates: Vec<String> = Vec::new();

    #[cfg(windows)]
    {
        let pathext = pathext_env.unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
        let mut extensions = vec![".cmd".to_string()];
        extensions.extend(
            pathext
                .to_string_lossy()
                .split(';')
                .filter_map(|extension| {
                    if extension.is_empty() {
                        return None;
                    }
                    Some(if extension.starts_with('.') {
                        extension.to_string()
                    } else {
                        format!(".{extension}")
                    })
                }),
        );

        for extension in extensions {
            if candidates
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(&format!("{command}{extension}")))
            {
                continue;
            }
            candidates.push(format!("{command}{extension}"));
        }
    }

    #[cfg(not(windows))]
    {
        let _ = pathext_env;
        candidates.push(command.to_string());
    }

    candidates
}

fn is_executable_file(path: &FsPath) -> bool {
    path.is_file()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn generated_system_prompt_is_grounded_in_ag_swarmer_and_enhances_the_draft() {
        let messages = system_prompt_generation_messages(
            "Reviewer",
            Some("Reviews code"),
            Some("Be precise."),
        );

        assert!(messages[0].content.contains("inside ag-swarmer"));
        assert!(messages[0].content.contains("mounted Skills"));
        assert!(messages[1]
            .content
            .starts_with("Enhance the existing system prompt."));
        assert!(messages[1].content.contains("Be precise."));
    }

    #[test]
    fn generated_system_prompt_removes_markdown_fences_only() {
        assert_eq!(
            clean_generated_system_prompt("```markdown\n# Role\nWork carefully.\n```").unwrap(),
            "# Role\nWork carefully."
        );
    }

    #[test]
    fn custom_install_package_specs_are_scoped_to_the_selected_preset() {
        let codex = acp_runtime_version_preset("codex").unwrap();

        assert_eq!(
            resolve_install_package_spec(codex, Some("@agentclientprotocol/codex-acp@next"))
                .unwrap(),
            "@agentclientprotocol/codex-acp@next"
        );
        assert!(resolve_install_package_spec(codex, Some("pi-acp@latest")).is_err());
        assert!(
            resolve_install_package_spec(codex, Some("@agentclientprotocol/codex-acp @next"))
                .is_err()
        );
    }

    #[test]
    #[cfg(windows)]
    fn find_command_on_path_prefers_cmd_for_npx_on_windows() {
        let dir = tempfile::tempdir().unwrap();
        let npx_cmd = dir.path().join("npx.cmd");
        File::create(&npx_cmd).unwrap();
        File::create(dir.path().join("npx.exe")).unwrap();
        let path_env = env::join_paths([dir.path()]).unwrap();

        let resolved =
            find_command_on_path_with_env("npx", Some(path_env), Some(OsString::from(".EXE;.CMD")))
                .unwrap();

        assert_eq!(resolved, npx_cmd);
    }

    #[test]
    #[cfg(windows)]
    fn find_command_on_path_uses_pathext_on_windows() {
        let dir = tempfile::tempdir().unwrap();
        let npx_bat = dir.path().join("npx.bat");
        File::create(&npx_bat).unwrap();
        let path_env = env::join_paths([dir.path()]).unwrap();

        let resolved =
            find_command_on_path_with_env("npx", Some(path_env), Some(OsString::from(".EXE;.BAT")))
                .unwrap();

        assert!(resolved
            .to_string_lossy()
            .eq_ignore_ascii_case(&npx_bat.to_string_lossy()));
    }

    #[test]
    #[cfg(windows)]
    fn find_command_on_path_returns_absolute_path_from_relative_path_env_on_windows() {
        let cwd = env::current_dir().unwrap();
        let dir = tempfile::Builder::new()
            .prefix("ag-swarmer-npx-")
            .tempdir_in(&cwd)
            .unwrap();
        let relative_dir = dir.path().strip_prefix(&cwd).unwrap();
        let npx_cmd = dir.path().join("npx.cmd");
        File::create(&npx_cmd).unwrap();
        let path_env = env::join_paths([relative_dir]).unwrap();

        let resolved =
            find_command_on_path_with_env("npx", Some(path_env), Some(OsString::from(".CMD")))
                .unwrap();

        assert!(resolved.is_absolute());
        assert!(resolved
            .to_string_lossy()
            .eq_ignore_ascii_case(&npx_cmd.to_string_lossy()));
    }

    #[test]
    #[cfg(not(windows))]
    fn find_command_on_path_uses_bare_command_off_windows() {
        let dir = tempfile::tempdir().unwrap();
        let npx = dir.path().join("npx");
        File::create(&npx).unwrap();
        let path_env = env::join_paths([dir.path()]).unwrap();

        let resolved = find_command_on_path_with_env("npx", Some(path_env), None).unwrap();

        assert_eq!(resolved, npx);
    }

    #[test]
    #[cfg(not(windows))]
    fn find_command_on_path_returns_absolute_path_from_relative_path_env_off_windows() {
        let cwd = env::current_dir().unwrap();
        let dir = tempfile::Builder::new()
            .prefix("ag-swarmer-npx-")
            .tempdir_in(&cwd)
            .unwrap();
        let relative_dir = dir.path().strip_prefix(&cwd).unwrap();
        let npx = dir.path().join("npx");
        File::create(&npx).unwrap();
        let path_env = env::join_paths([relative_dir]).unwrap();

        let resolved = find_command_on_path_with_env("npx", Some(path_env), None).unwrap();

        assert!(resolved.is_absolute());
        assert_eq!(resolved, npx);
    }
}
