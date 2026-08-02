//! The built-in Assistant: a system agent that helps the user configure and
//! operate the app.
//!
//! It is deliberately an ordinary `llm_chat` agent row reached through an
//! ordinary direct chat, so SSE streaming, resume, interruption and turn traces
//! all apply to it with no special casing. Two things make it different, and
//! both are safety properties rather than features:
//!
//! - `agents.is_system = 1` hides it from the agent library and makes the
//!   generic update/delete routes refuse it.
//! - It has no bound workspace, and its tool set contains no file, shell or MCP
//!   tool. Its only capabilities are reading configuration and *proposing*
//!   changes the user must approve.

use axum::{extract::State, http::HeaderMap, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::api::{auth::current_user_id, error::ApiError, AppState};
use crate::runtime::workspace_scope::WorkspaceMode;

/// Name shown on the Assistant's messages.
const ASSISTANT_NAME: &str = "AG Assistant";

/// The Assistant's system prompt.
///
/// It states the approval boundary in the prompt as well as enforcing it in
/// code. The code is what makes it true; saying it here stops the model from
/// promising the user things it will then fail to do.
const ASSISTANT_SYSTEM_PROMPT: &str = "\
You are the built-in assistant for AG Swarmer, a multi-agent collaboration \
workbench. You help the user set the app up, configure its features, explain \
how they work, and carry out tasks in the app on their behalf.

Your capabilities:
- Inspect the user's configuration with AppList, AppGet and AppState.
- Answer questions about the app using AppDocs. Prefer it over your own \
  recollection: it describes this build specifically.
- Propose configuration changes with AppPropose. A proposal is staged, not \
  applied; the user sees a card and normally approves it before anything \
  changes. If auto-approval mode is enabled, the same approval endpoint applies \
  eligible staged changes automatically.
- Create a group or private chat and optionally send its first message, or \
  send a message in an existing conversation, with AppPropose target_kind \
  'group' or 'chat'. These actions also use the approval flow.
- Inspect a group's current Agent and user members with AppGet, then propose \
  one add/remove membership operation at a time with target_kind 'group'.
- For changes you are not allowed to stage — provider API keys, MCP servers \
  that launch local processes, CLI runtime installs, and resource deletion \
  (group membership removal is supported) — use AppPrefill to hand the user \
  a prefilled form to complete themselves.

Rules:
- Prefer doing the work over asking. If something is missing but you can \
  propose it, propose it — a workspace, for instance, is created for the user \
  with backend_type 'local' and auto_create true, with no path. Only ask when \
  the answer is genuinely theirs, such as which of several existing things to \
  use.
- When you do ask, offer concrete choices rather than an open question, so the \
  user can answer with one click.
- Never claim you have changed something. Say you have proposed it, and that \
  it takes effect once they approve.
- Check the current state with AppList or AppGet before proposing a change, so \
  you do not propose something that already exists.
- You have no access to the filesystem, a shell, or the user's workspaces. If a \
  request needs those, say so and suggest the user create a regular agent with \
  a workspace bound to it.
- Be concise. The user is reading you in a small floating panel.";

/// Tools mounted on the Assistant.
///
/// Stored in the same `tool_config_json` shape `enabled_tool_names` reads. The
/// absence of `read`/`write`/`edit`/`glob`/`grep`/`bash` here is the point: with
/// no workspace bound they would report `WORKSPACE_REQUIRED` anyway, but not
/// offering them keeps the model from trying and keeps the prompt honest.
fn assistant_tool_config() -> serde_json::Value {
    json!({
        "tools": {
            "app_list": { "enabled": true },
            "app_get": { "enabled": true },
            "app_state": { "enabled": true },
            "app_docs": { "enabled": true },
            "app_propose": { "enabled": true },
            "app_prefill": { "enabled": true },
            "ask_user": { "enabled": true },
            "todo_write": { "enabled": true }
        }
    })
}

#[derive(Debug, Serialize)]
pub struct AssistantResponse {
    agent_id: String,
    chat_id: String,
    provider_id: Option<String>,
    /// Model to use, or `None` to follow the provider's default. Kept out of
    /// the provider row so changing providers does not silently pin the
    /// Assistant to a model the new one may not offer.
    model: Option<String>,
    /// Whether the Assistant can actually hold a conversation yet. The dock
    /// shows its setup panel while this is false, because an LLM agent cannot
    /// talk the user through configuring the provider it needs in order to
    /// talk.
    provider_configured: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRequest {
    /// Provider to call. Omitted clears the binding.
    #[serde(default)]
    llm_provider_id: Option<String>,
    /// Model to use. Omitted follows the provider's default.
    #[serde(default)]
    model: Option<String>,
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AssistantResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    Ok(Json(ensure(&state, &owner_id).await?))
}

pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<UpdateRequest>,
) -> Result<Json<AssistantResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let existing = ensure(&state, &owner_id).await?;

    let provider_id = match body.llm_provider_id.as_deref() {
        Some(raw) => Some(validate_provider(state.db.pool(), raw, &owner_id).await?),
        None => None,
    };
    let model = match body
        .model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
    {
        Some(model) => {
            let Some(provider_id) = provider_id.as_deref() else {
                return Err(ApiError::invalid_input(
                    "a provider is required before choosing a model",
                ));
            };
            Some(validate_model(state.db.pool(), provider_id, model).await?)
        }
        None => None,
    };
    // `model_config_json` is the same shape a normal agent uses, so the runtime
    // reads it through the existing `model_from_config` path.
    let model_config_json = model
        .as_deref()
        .map(|model| json!({ "model": model }).to_string());

    sqlx::query(
        "UPDATE agents SET provider_id = ?, model_config_json = ?, updated_at = ?          WHERE id = ? AND owner_id = ?",
    )
    .bind(&provider_id)
    .bind(&model_config_json)
    .bind(now_rfc3339())
    .bind(&existing.agent_id)
    .bind(&owner_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to update the assistant"))?;

    Ok(Json(AssistantResponse {
        provider_configured: provider_id.is_some(),
        provider_id,
        model,
        ..existing
    }))
}

/// Confirm a model is one the bound provider actually offers.
///
/// Without this the Assistant can be pinned to a model that only fails at send
/// time, inside a stream, with no obvious cause.
async fn validate_model(
    pool: &SqlitePool,
    provider_id: &str,
    model: &str,
) -> Result<String, ApiError> {
    let row: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT default_model, models_json FROM llm_providers WHERE id = ?")
            .bind(provider_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| ApiError::internal("database error"))?;
    let Some((default_model, models_json)) = row else {
        return Err(ApiError::invalid_input(
            "llm_provider_id does not reference a provider",
        ));
    };
    if default_model == model {
        return Ok(model.to_string());
    }
    let listed = models_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|value| {
            value.as_array().map(|models| {
                models
                    .iter()
                    .any(|entry| entry.get("id").and_then(Value::as_str) == Some(model))
            })
        })
        .unwrap_or(false);
    if listed {
        Ok(model.to_string())
    } else {
        Err(ApiError::invalid_input(
            "model is not offered by that provider",
        ))
    }
}

/// Return this owner's Assistant, creating it on first use.
///
/// Creation is lazy rather than part of registration so the row never exists
/// for accounts that never open the dock, and so accounts that predate this
/// feature pick one up without a data migration.
pub(crate) async fn ensure(
    state: &AppState,
    owner_id: &str,
) -> Result<AssistantResponse, ApiError> {
    if let Some(found) = load(state.db.pool(), owner_id).await? {
        return Ok(found);
    }

    // Serialize against a concurrent first request from the same account: two
    // dock mounts racing would otherwise each insert an Assistant, and the
    // loser's chat would be orphaned.
    let _guard = state.write_lock.lock().await;
    if let Some(found) = load(state.db.pool(), owner_id).await? {
        return Ok(found);
    }

    let agent_id = Uuid::new_v4().to_string();
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO agents \
         (id, owner_id, workspace_id, name, description, system_prompt, runtime_kind, \
          provider_id, model_config_json, tool_config_json, external_runtime_json, \
          skill_ids_json, status, is_system, created_at, updated_at) \
         VALUES (?, ?, NULL, ?, NULL, ?, 'llm_chat', NULL, NULL, ?, NULL, '[]', 'active', 1, ?, ?)",
    )
    .bind(&agent_id)
    .bind(owner_id)
    .bind(ASSISTANT_NAME)
    .bind(ASSISTANT_SYSTEM_PROMPT)
    .bind(assistant_tool_config().to_string())
    .bind(&now)
    .bind(&now)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to create the assistant"))?;

    // `SelfOnly` against a null `workspace_id` makes `resolve_workspaces`
    // return no primary root at all, so the workspace-scoped tools stay
    // unreachable even if one is ever enabled on this row by mistake.
    crate::api::direct_chats::insert_direct_chat(
        state.db.pool(),
        owner_id,
        &agent_id,
        None,
        ASSISTANT_NAME,
        WorkspaceMode::SelfOnly,
    )
    .await?;

    load(state.db.pool(), owner_id)
        .await?
        .ok_or_else(|| ApiError::internal("assistant vanished after insert"))
}

#[derive(sqlx::FromRow)]
struct AssistantRow {
    agent_id: String,
    chat_id: String,
    provider_id: Option<String>,
    model_config_json: Option<String>,
}

async fn load(pool: &SqlitePool, owner_id: &str) -> Result<Option<AssistantResponse>, ApiError> {
    let row = sqlx::query_as::<_, AssistantRow>(
        "SELECT a.id AS agent_id, g.id AS chat_id, a.provider_id AS provider_id,                 a.model_config_json AS model_config_json \
         FROM agents a \
         JOIN groups g ON g.direct_agent_id = a.id \
           AND g.conversation_kind = 'direct' AND g.status = 'active' \
         WHERE a.owner_id = ? AND a.is_system = 1 AND a.status = 'active' \
         ORDER BY a.created_at ASC, a.id ASC LIMIT 1",
    )
    .bind(owner_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;

    Ok(row.map(|row| AssistantResponse {
        agent_id: row.agent_id,
        chat_id: row.chat_id,
        provider_configured: row.provider_id.is_some(),
        provider_id: row.provider_id,
        model: row
            .model_config_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .and_then(|value| {
                value
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            }),
    }))
}

/// Resolve an active provider the caller owns.
///
/// Mirrors `agents::validate_provider`; kept local so the Assistant's binding
/// does not depend on a private helper in another module.
async fn validate_provider(
    pool: &SqlitePool,
    raw_id: &str,
    owner_id: &str,
) -> Result<String, ApiError> {
    let id = Uuid::parse_str(raw_id.trim())
        .map(|id| id.to_string())
        .map_err(|_| ApiError::invalid_input("invalid llm_provider_id"))?;
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

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}
