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
//! - Its file and shell tools reach one scratch workspace of its own under the
//!   system temp directory and nothing else, and it has no MCP tools. The
//!   user's own directories stay out of reach, and configuration still only
//!   changes by *proposing* something the user approves.

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

/// Directory holding the Assistant's scratch workspace, created under the
/// system temp directory: `/tmp/qunica-assistant` on Unix, and the platform
/// equivalent (`%TEMP%\qunica-assistant`) elsewhere.
const ASSISTANT_WORKSPACE_DIR: &str = "qunica-assistant";

/// Name of the workspace row bound to the Assistant. It shows up in the user's
/// workspace list like any other, so it says what it is.
const ASSISTANT_WORKSPACE_NAME: &str = "Assistant Workspace";

/// The Assistant's system prompt.
///
/// It states the approval boundary in the prompt as well as enforcing it in
/// code. The code is what makes it true; saying it here stops the model from
/// promising the user things it will then fail to do.
const ASSISTANT_SYSTEM_PROMPT: &str = "\
You are the built-in assistant for Qunica, a multi-agent collaboration \
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
- List and inspect reusable group templates and shared group notes with \
  AppList/AppGet. Propose saving a group as a template with target_kind \
  'group_template', or creating/updating a shared note with 'group_note'.
- For changes you are not allowed to stage — provider API keys, MCP servers \
  that launch local processes, CLI runtime installs, and resource deletion \
  (group membership removal is supported) — use AppPrefill to hand the user \
  a prefilled form to complete themselves.
- Work with files and run commands in your own scratch workspace using Read, \
  Write and Bash. It is a temporary directory that belongs to you, not the \
  user's project: use it for drafts, notes, generated files and one-off \
  commands, and do not keep anything there the user would miss.
- Look things up online with WebSearch, and read one specific page with Fetch.

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
- Your file and shell tools reach your own scratch workspace only. You cannot \
  browse the user's other directories, and AppGet can read app-managed group \
  notes only. For work on their own files, suggest a regular agent with that \
  workspace bound to it.
- Be concise. The user is reading you in a small floating panel.";

/// Tools mounted on the Assistant.
///
/// Stored in the same `tool_config_json` shape `enabled_tool_names` reads. The
/// file and shell tools are safe to offer because [`ensure_workspace`] is the
/// only thing that ever binds a workspace to this agent, and it binds a scratch
/// directory — so `Bash` and `Write` have somewhere to work without reaching
/// anything the user cares about. `edit`, `glob` and `grep` stay off: read and
/// write already cover what a scratch directory is for, and the shell covers
/// the rest.
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
            "todo_write": { "enabled": true },
            "read": { "enabled": true },
            "write": { "enabled": true },
            "bash": { "enabled": true },
            "web_search": { "enabled": true },
            "fetch": { "enabled": true }
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
    let found = match load(state.db.pool(), owner_id).await? {
        Some(found) => {
            sync_definition(state.db.pool(), owner_id, &found.agent_id).await?;
            found
        }
        None => {
            create(state, owner_id).await?;
            load(state.db.pool(), owner_id)
                .await?
                .ok_or_else(|| ApiError::internal("assistant vanished after insert"))?
        }
    };
    ensure_workspace(state, owner_id, &found.agent_id).await?;
    Ok(found)
}

/// Insert the Assistant's agent row and the direct chat it is reached through.
async fn create(state: &AppState, owner_id: &str) -> Result<(), ApiError> {
    // Serialize against a concurrent first request from the same account: two
    // dock mounts racing would otherwise each insert an Assistant, and the
    // loser's chat would be orphaned.
    let _guard = state.write_lock.lock().await;
    if load(state.db.pool(), owner_id).await?.is_some() {
        return Ok(());
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

    // `SelfOnly` is what keeps the scratch workspace the *only* root in reach:
    // the chat itself is given no conversation workspace, and this mode stops
    // the agent following one if a later feature ever binds it one.
    crate::api::direct_chats::insert_direct_chat(
        state.db.pool(),
        owner_id,
        &agent_id,
        None,
        ASSISTANT_NAME,
        WorkspaceMode::SelfOnly,
    )
    .await?;

    Ok(())
}

/// Bind the Assistant to a scratch workspace of its own, creating both the
/// directory and the workspace row on first use.
///
/// The Assistant has file and shell tools, so it needs somewhere to put things.
/// A temp directory is what keeps those tools somewhere harmless: it is not a
/// place the user keeps work, and losing it costs them nothing.
///
/// Called on every fetch rather than only at creation, so accounts that predate
/// the workspace pick one up, and so a directory a temp sweep removed between
/// sessions is restored before the next turn tries to use it.
async fn ensure_workspace(
    state: &AppState,
    owner_id: &str,
    agent_id: &str,
) -> Result<(), ApiError> {
    if let Some(bound) = bound_workspace_path(state.db.pool(), owner_id, agent_id).await? {
        if let Err(error) = std::fs::create_dir_all(&bound) {
            tracing::warn!(%error, path = %bound, "failed to restore the assistant workspace");
        }
        return Ok(());
    }

    // Same race as [`create`]: two dock mounts arriving together would each
    // insert a workspace row, and the loser's would be left unreferenced.
    let _guard = state.write_lock.lock().await;
    if bound_workspace_path(state.db.pool(), owner_id, agent_id)
        .await?
        .is_some()
    {
        return Ok(());
    }
    let Some(path) = prepare_workspace_dir() else {
        return Ok(());
    };

    // Reuse the row an earlier bind left behind. Deleting the workspace clears
    // the agent's binding but keeps its own row, so re-opening the dock would
    // otherwise stack up workspaces over the same directory.
    let existing = sqlx::query_scalar::<_, String>(
        "SELECT id FROM workspaces \
         WHERE owner_id = ? AND status = 'active' AND backend_type = 'local' AND local_path = ? \
         ORDER BY created_at ASC, id ASC LIMIT 1",
    )
    .bind(owner_id)
    .bind(&path)
    .fetch_optional(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("database error"))?;
    let workspace_id = match existing {
        Some(id) => id,
        None => insert_workspace(state.db.pool(), owner_id, &path).await?,
    };

    sqlx::query("UPDATE agents SET workspace_id = ?, updated_at = ? WHERE id = ? AND owner_id = ?")
        .bind(&workspace_id)
        .bind(now_rfc3339())
        .bind(agent_id)
        .bind(owner_id)
        .execute(state.db.pool())
        .await
        .map_err(|_| ApiError::internal("failed to bind the assistant workspace"))?;
    Ok(())
}

/// The local path of the Assistant's workspace, or `None` when it has no usable
/// one — never bound, deleted since, or bound to a backend with no local path.
async fn bound_workspace_path(
    pool: &SqlitePool,
    owner_id: &str,
    agent_id: &str,
) -> Result<Option<String>, ApiError> {
    sqlx::query_scalar::<_, Option<String>>(
        "SELECT w.local_path FROM agents a \
         JOIN workspaces w ON w.id = a.workspace_id \
         WHERE a.id = ? AND a.owner_id = ? AND w.status = 'active' AND w.backend_type = 'local'",
    )
    .bind(agent_id)
    .bind(owner_id)
    .fetch_optional(pool)
    .await
    .map(Option::flatten)
    .map_err(|_| ApiError::internal("database error"))
}

/// Create the scratch directory and return its canonical path.
///
/// A temp directory that cannot be created costs the Assistant its file tools —
/// they report `WORKSPACE_REQUIRED` with nothing bound — rather than costing the
/// user the whole dock, which is why this warns instead of returning an error.
fn prepare_workspace_dir() -> Option<String> {
    let path = std::env::temp_dir().join(ASSISTANT_WORKSPACE_DIR);
    if let Err(error) = std::fs::create_dir_all(&path) {
        tracing::warn!(%error, path = %path.display(), "failed to create the assistant workspace");
        return None;
    }
    match std::fs::canonicalize(&path) {
        Ok(canonical) => Some(canonical.to_string_lossy().into_owned()),
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "failed to resolve the assistant workspace");
            None
        }
    }
}

async fn insert_workspace(
    pool: &SqlitePool,
    owner_id: &str,
    local_path: &str,
) -> Result<String, ApiError> {
    let id = Uuid::new_v4().to_string();
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO workspaces \
         (id, owner_id, name, backend_type, local_path, config_json, status, created_at, updated_at) \
         VALUES (?, ?, ?, 'local', ?, NULL, 'active', ?, ?)",
    )
    .bind(&id)
    .bind(owner_id)
    .bind(ASSISTANT_WORKSPACE_NAME)
    .bind(local_path)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|_| ApiError::internal("failed to create the assistant workspace"))?;
    Ok(id)
}

/// Keep existing system agents aligned with the build that is running.
///
/// The Assistant predates normal migrations for prompt/tool changes because it
/// is created lazily. Refreshing the two fixed fields here makes an application
/// upgrade effective for existing accounts without touching their provider or
/// model choices.
async fn sync_definition(
    pool: &SqlitePool,
    owner_id: &str,
    agent_id: &str,
) -> Result<(), ApiError> {
    let tools = assistant_tool_config().to_string();
    sqlx::query(
        "UPDATE agents SET system_prompt = ?, tool_config_json = ?, updated_at = ? \
         WHERE id = ? AND owner_id = ? \
           AND (system_prompt != ? OR COALESCE(tool_config_json, '') != ?)",
    )
    .bind(ASSISTANT_SYSTEM_PROMPT)
    .bind(&tools)
    .bind(now_rfc3339())
    .bind(agent_id)
    .bind(owner_id)
    .bind(ASSISTANT_SYSTEM_PROMPT)
    .bind(&tools)
    .execute(pool)
    .await
    .map_err(|_| ApiError::internal("failed to update the assistant definition"))?;
    Ok(())
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
