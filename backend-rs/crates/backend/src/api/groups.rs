use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};
use uuid::Uuid;

use crate::api::{auth::current_user_id, error::ApiError, AppState};

const GROUP_COLUMNS: &str = "id, owner_id, workspace_id, name, description, announcement, \
     free_speech, proactive_mode, proactive_max_rounds, proactive_reply_multiplier, \
     allow_agent_free_mention, agent_free_mention_max_dispatches, communication_mode, \
     muted_agent_ids_json, admin_agent_ids_json, muted_member_ids_json, status, \
     created_at, updated_at";

const GROUP_AGENT_COLUMNS: &str = "group_agents.group_id, group_agents.agent_id, \
     group_agents.display_name, agents.name AS agent_name, group_agents.role, \
     group_agents.topology_role, group_agents.speaking_order, group_agents.response_mode, \
     group_agents.context_scope_json, group_agents.status, group_agents.joined_at";

const GROUP_MEMBER_COLUMNS: &str = "group_members.group_id, group_members.user_id, \
     users.name AS user_name, group_members.role, group_members.status, \
     group_members.joined_at";

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    announcement: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    free_speech: Option<bool>,
    #[serde(default)]
    proactive_mode: Option<bool>,
    #[serde(default)]
    proactive_max_rounds: Option<i64>,
    #[serde(default)]
    proactive_reply_multiplier: Option<i64>,
    #[serde(default)]
    allow_agent_free_mention: Option<bool>,
    #[serde(default)]
    agent_free_mention_max_dispatches: Option<i64>,
    #[serde(default)]
    communication_mode: Option<String>,
    #[serde(default)]
    initial_agents: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRequest {
    #[serde(default)]
    name: Option<String>,
    // Double `Option` distinguishes an omitted field (outer `None`) from an
    // explicit JSON `null` (inner `None`) for nullable fields.
    #[serde(default, deserialize_with = "double_option")]
    description: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    announcement: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    workspace_id: Option<Option<String>>,
    #[serde(default)]
    free_speech: Option<bool>,
    #[serde(default)]
    proactive_mode: Option<bool>,
    #[serde(default)]
    proactive_max_rounds: Option<i64>,
    #[serde(default)]
    proactive_reply_multiplier: Option<i64>,
    #[serde(default)]
    allow_agent_free_mention: Option<bool>,
    #[serde(default)]
    agent_free_mention_max_dispatches: Option<i64>,
    #[serde(default)]
    communication_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GroupAgentAddRequest {
    agent_id: String,
    #[serde(default)]
    share_group_workspace: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct GroupMemberAddRequest {
    user_id: String,
}

#[derive(Debug, Deserialize)]
pub struct MemberCandidatesQuery {
    #[serde(default)]
    q: String,
}

#[derive(Debug, Deserialize)]
pub struct GroupMemberMuteRequest {
    muted: bool,
}

#[derive(Debug, Deserialize)]
pub struct GroupAgentMuteRequest {
    muted: bool,
}

#[derive(Debug, Deserialize)]
pub struct GroupAgentTopologyRequest {
    #[serde(default)]
    topology_role: Option<String>,
    #[serde(default)]
    speaking_order: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct GroupAgentWorkspaceSharingRequest {
    share_group_workspace: bool,
}

#[derive(Debug, Serialize)]
pub struct GroupResponse {
    id: String,
    workspace_id: Option<String>,
    name: String,
    description: Option<String>,
    announcement: Option<String>,
    free_speech: bool,
    proactive_mode: bool,
    proactive_max_rounds: i64,
    proactive_reply_multiplier: i64,
    allow_agent_free_mention: bool,
    agent_free_mention_max_dispatches: i64,
    communication_mode: String,
    muted_agent_ids: Option<Vec<String>>,
    admin_agent_ids: Option<Vec<String>>,
    muted_member_ids: Option<Vec<String>>,
    status: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct GroupAgentResponse {
    id: String,
    group_id: String,
    agent_id: String,
    display_name: String,
    role: Option<String>,
    topology_role: Option<String>,
    speaking_order: Option<i64>,
    response_mode: String,
    share_group_workspace: bool,
    context_usage: Option<Value>,
    status: String,
    joined_at: String,
}

#[derive(Debug, Serialize)]
pub struct GroupMemberResponse {
    id: String,
    group_id: String,
    user_id: String,
    display_name: String,
    role: String,
    status: String,
    is_muted: bool,
    joined_at: String,
}

#[derive(Debug, Serialize)]
pub struct UserReadResponse {
    id: String,
    email: String,
    name: String,
    avatar_url: Option<String>,
    created_at: String,
}

#[derive(Debug, sqlx::FromRow)]
struct GroupRow {
    id: String,
    owner_id: String,
    workspace_id: Option<String>,
    name: String,
    description: Option<String>,
    announcement: Option<String>,
    // SQLite stores booleans as integers; these are exposed as booleans below.
    free_speech: i64,
    proactive_mode: i64,
    proactive_max_rounds: i64,
    proactive_reply_multiplier: i64,
    allow_agent_free_mention: i64,
    agent_free_mention_max_dispatches: i64,
    communication_mode: String,
    muted_agent_ids_json: Option<String>,
    admin_agent_ids_json: Option<String>,
    muted_member_ids_json: Option<String>,
    status: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, sqlx::FromRow)]
struct ActiveGroupAgentRow {
    agent_id: String,
    topology_role: Option<String>,
    speaking_order: Option<i64>,
}

#[derive(Debug, sqlx::FromRow)]
struct GroupAgentRow {
    group_id: String,
    agent_id: String,
    display_name: Option<String>,
    agent_name: String,
    role: Option<String>,
    topology_role: Option<String>,
    speaking_order: Option<i64>,
    response_mode: String,
    context_scope_json: Option<String>,
    status: String,
    joined_at: String,
}

#[derive(Debug, sqlx::FromRow)]
struct GroupMemberRow {
    group_id: String,
    user_id: String,
    user_name: String,
    role: String,
    status: String,
    joined_at: String,
}

#[derive(Debug, sqlx::FromRow)]
struct UserRow {
    id: String,
    email: String,
    name: String,
    avatar_url: Option<String>,
    created_at: String,
}

impl From<GroupRow> for GroupResponse {
    fn from(row: GroupRow) -> Self {
        Self {
            id: row.id,
            workspace_id: row.workspace_id,
            name: row.name,
            description: row.description,
            announcement: row.announcement,
            free_speech: row.free_speech != 0,
            proactive_mode: row.proactive_mode != 0,
            proactive_max_rounds: row.proactive_max_rounds,
            proactive_reply_multiplier: row.proactive_reply_multiplier,
            allow_agent_free_mention: row.allow_agent_free_mention != 0,
            agent_free_mention_max_dispatches: row.agent_free_mention_max_dispatches,
            communication_mode: row.communication_mode,
            muted_agent_ids: parse_json_list(row.muted_agent_ids_json.as_deref()),
            admin_agent_ids: parse_json_list(row.admin_agent_ids_json.as_deref()),
            muted_member_ids: parse_json_list(row.muted_member_ids_json.as_deref()),
            status: row.status,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

impl From<GroupAgentRow> for GroupAgentResponse {
    fn from(row: GroupAgentRow) -> Self {
        let id = format!("{}:{}", row.group_id, row.agent_id);
        let share_group_workspace = group_workspace_shared(row.context_scope_json.as_deref());
        Self {
            id,
            group_id: row.group_id,
            agent_id: row.agent_id,
            display_name: row.display_name.unwrap_or(row.agent_name),
            role: row.role,
            topology_role: row.topology_role,
            speaking_order: row.speaking_order,
            response_mode: row.response_mode,
            share_group_workspace,
            context_usage: None,
            status: row.status,
            joined_at: row.joined_at,
        }
    }
}

impl GroupMemberRow {
    fn into_response(self, muted_member_ids: &[String]) -> GroupMemberResponse {
        let id = format!("{}:{}", self.group_id, self.user_id);
        let is_muted = muted_member_ids
            .iter()
            .any(|value| value == self.user_id.as_str());
        GroupMemberResponse {
            id,
            group_id: self.group_id,
            user_id: self.user_id,
            display_name: self.user_name,
            role: self.role,
            status: self.status,
            is_muted,
            joined_at: self.joined_at,
        }
    }
}

impl From<UserRow> for UserReadResponse {
    fn from(row: UserRow) -> Self {
        Self {
            id: row.id,
            email: row.email,
            name: row.name,
            avatar_url: row.avatar_url,
            created_at: row.created_at,
        }
    }
}

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateRequest>,
) -> Result<(StatusCode, Json<GroupResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;

    let name = validate_name(&body.name)?;
    let description = normalize_description(body.description.as_deref());
    let announcement = normalize_description(body.announcement.as_deref());
    let free_speech = body.free_speech.unwrap_or(false);
    let proactive_mode = body.proactive_mode.unwrap_or(false);
    let proactive_max_rounds = validate_proactive_max_rounds(body.proactive_max_rounds)?;
    let multiplier = validate_multiplier(body.proactive_reply_multiplier)?;
    let allow_agent_free_mention = body.allow_agent_free_mention.unwrap_or(true);
    let agent_free_mention_max_dispatches =
        validate_agent_free_mention_max_dispatches(body.agent_free_mention_max_dispatches)?;
    let communication_mode = validate_communication_mode(body.communication_mode.as_deref())?;
    let initial_agents =
        validate_initial_agents(state.db.pool(), body.initial_agents.as_deref(), &owner_id).await?;

    let id = Uuid::new_v4().to_string();
    let now = now_rfc3339();
    let workspace_id = match body.workspace_id.as_deref() {
        Some(raw) => validate_workspace(state.db.pool(), raw, &owner_id).await?,
        None => create_group_workspace(state.db.pool(), &owner_id, &id, &name, &now).await?,
    };

    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start group create transaction"))?;

    sqlx::query(
        "INSERT INTO groups \
         (id, owner_id, workspace_id, name, description, announcement, free_speech, \
          proactive_mode, proactive_max_rounds, proactive_reply_multiplier, \
          allow_agent_free_mention, agent_free_mention_max_dispatches, communication_mode, \
          status, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'active', ?, ?)",
    )
    .bind(&id)
    .bind(&owner_id)
    .bind(&workspace_id)
    .bind(&name)
    .bind(&description)
    .bind(&announcement)
    .bind(free_speech as i64)
    .bind(proactive_mode as i64)
    .bind(proactive_max_rounds)
    .bind(multiplier)
    .bind(allow_agent_free_mention as i64)
    .bind(agent_free_mention_max_dispatches)
    .bind(&communication_mode)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to create group"))?;

    sqlx::query(
        "INSERT INTO group_members (group_id, user_id, role, status, joined_at) \
         VALUES (?, ?, 'owner', 'active', ?)",
    )
    .bind(&id)
    .bind(&owner_id)
    .bind(&now)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to create owner membership"))?;

    for (position, agent_id) in initial_agents.iter().enumerate() {
        let (topology_role, speaking_order) = initial_agent_topology(&communication_mode, position);
        let joined_at = now_plus_rfc3339(position as i64);
        sqlx::query(
            "INSERT INTO group_agents \
             (group_id, agent_id, topology_role, speaking_order, response_mode, \
              context_scope_json, status, joined_at, updated_at) \
             VALUES (?, ?, ?, ?, 'mentioned_only', ?, 'active', ?, ?)",
        )
        .bind(&id)
        .bind(agent_id)
        .bind(topology_role)
        .bind(speaking_order)
        .bind(r#"{"share_group_workspace":true}"#)
        .bind(&joined_at)
        .bind(&joined_at)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal("failed to initialize group agent"))?;
    }

    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group create"))?;

    let row = fetch_row(state.db.pool(), &id)
        .await?
        .ok_or_else(|| ApiError::internal("group vanished after insert"))?;
    Ok((StatusCode::CREATED, Json(row.into())))
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<GroupResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;

    let sql = format!(
        "SELECT {GROUP_COLUMNS} FROM groups \
         WHERE owner_id = ? AND status = 'active' \
         ORDER BY created_at DESC, id DESC"
    );
    let rows = sqlx::query_as::<_, GroupRow>(&sql)
        .bind(&owner_id)
        .fetch_all(state.db.pool())
        .await
        .map_err(|_| ApiError::internal("database error"))?;

    Ok(Json(rows.into_iter().map(GroupResponse::from).collect()))
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<GroupResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let row = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    Ok(Json(row.into()))
}

pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<UpdateRequest>,
) -> Result<Json<GroupResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let existing = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;

    let name = match body.name {
        Some(ref raw) => validate_name(raw)?,
        None => existing.name.clone(),
    };
    let description = match body.description {
        Some(ref value) => normalize_description(value.as_deref()),
        None => existing.description.clone(),
    };
    let announcement = match body.announcement {
        Some(ref value) => normalize_description(value.as_deref()),
        None => existing.announcement.clone(),
    };
    // `Some(Some(id))` rebinds to an owned active workspace; `Some(None)` clears
    // the binding; `None` leaves the existing binding untouched.
    let workspace_id = match body.workspace_id {
        Some(ref value) => match value.as_deref() {
            Some(raw) => Some(validate_workspace(state.db.pool(), raw, &owner_id).await?),
            None => None,
        },
        None => existing.workspace_id.clone(),
    };
    let free_speech = body
        .free_speech
        .map(|b| b as i64)
        .unwrap_or(existing.free_speech);
    let proactive_mode = body
        .proactive_mode
        .map(|b| b as i64)
        .unwrap_or(existing.proactive_mode);
    let proactive_max_rounds = match body.proactive_max_rounds {
        Some(value) => validate_proactive_max_rounds(Some(value))?,
        None => existing.proactive_max_rounds,
    };
    let multiplier = match body.proactive_reply_multiplier {
        Some(value) => validate_multiplier(Some(value))?,
        None => existing.proactive_reply_multiplier,
    };
    let allow_agent_free_mention = body
        .allow_agent_free_mention
        .map(|b| b as i64)
        .unwrap_or(existing.allow_agent_free_mention);
    let agent_free_mention_max_dispatches = match body.agent_free_mention_max_dispatches {
        Some(value) => validate_agent_free_mention_max_dispatches(Some(value))?,
        None => existing.agent_free_mention_max_dispatches,
    };
    let communication_mode = match body.communication_mode.as_deref() {
        Some(raw) => validate_communication_mode(Some(raw))?,
        None => existing.communication_mode.clone(),
    };

    let now = now_rfc3339();
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start group update transaction"))?;
    sqlx::query(
        "UPDATE groups SET \
         name = ?, description = ?, announcement = ?, workspace_id = ?, free_speech = ?, \
         proactive_mode = ?, proactive_max_rounds = ?, proactive_reply_multiplier = ?, \
         allow_agent_free_mention = ?, agent_free_mention_max_dispatches = ?, \
         communication_mode = ?, updated_at = ? \
         WHERE id = ? AND owner_id = ?",
    )
    .bind(&name)
    .bind(&description)
    .bind(&announcement)
    .bind(&workspace_id)
    .bind(free_speech)
    .bind(proactive_mode)
    .bind(proactive_max_rounds)
    .bind(multiplier)
    .bind(allow_agent_free_mention)
    .bind(agent_free_mention_max_dispatches)
    .bind(&communication_mode)
    .bind(&now)
    .bind(&group_id)
    .bind(&owner_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to update group"))?;

    if communication_mode != existing.communication_mode {
        normalize_group_agent_topology(&mut tx, &group_id, &communication_mode, &now).await?;
    }

    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group update"))?;

    let row = fetch_row(state.db.pool(), &group_id)
        .await?
        .ok_or_else(|| ApiError::internal("group vanished after update"))?;
    Ok(Json(row.into()))
}

pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    // Confirms existence/ownership (and that it is not already deleted) first.
    load_active_owned(state.db.pool(), &group_id, &owner_id).await?;

    let now = now_rfc3339();
    sqlx::query(
        "UPDATE groups SET status = 'deleted', updated_at = ? WHERE id = ? AND owner_id = ?",
    )
    .bind(&now)
    .bind(&group_id)
    .bind(&owner_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to delete group"))?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_group_members(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<Vec<GroupMemberResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let muted_member_ids =
        parse_json_list(group.muted_member_ids_json.as_deref()).unwrap_or_default();
    let rows = fetch_group_member_rows(state.db.pool(), &group_id).await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| row.into_response(&muted_member_ids))
            .collect(),
    ))
}

pub async fn search_group_member_candidates(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<MemberCandidatesQuery>,
) -> Result<Json<Vec<UserReadResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let rows = search_user_rows(state.db.pool(), &query.q).await?;
    Ok(Json(rows.into_iter().map(UserReadResponse::from).collect()))
}

pub async fn add_group_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<GroupMemberAddRequest>,
) -> Result<(StatusCode, Json<GroupMemberResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let user_id = validate_uuid(&body.user_id, "user_id")?;
    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    fetch_user_row(state.db.pool(), &user_id)
        .await?
        .ok_or_else(|| ApiError::not_found("user not found"))?;

    let now = now_rfc3339();
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start group member transaction"))?;

    let existing = sqlx::query_as::<_, (String,)>(
        "SELECT status FROM group_members WHERE group_id = ? AND user_id = ?",
    )
    .bind(&group_id)
    .bind(&user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to load group member"))?;

    if matches!(existing.as_ref().map(|row| row.0.as_str()), Some("active")) {
        return Err(ApiError::conflict("user already in group"));
    }

    if existing.is_some() {
        sqlx::query(
            "UPDATE group_members SET role = 'member', status = 'active' \
             WHERE group_id = ? AND user_id = ?",
        )
        .bind(&group_id)
        .bind(&user_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal("failed to reactivate group member"))?;
    } else {
        let result = sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role, status, joined_at) \
             VALUES (?, ?, 'member', 'active', ?)",
        )
        .bind(&group_id)
        .bind(&user_id)
        .bind(&now)
        .execute(&mut *tx)
        .await;

        if let Err(err) = result {
            if is_unique_violation(&err) {
                return Err(ApiError::conflict("user already in group"));
            }
            return Err(ApiError::internal("failed to add group member"));
        }
    }

    touch_group(&mut tx, &group_id, &now).await?;
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group member transaction"))?;

    let row = fetch_group_member_row(state.db.pool(), &group_id, &user_id)
        .await?
        .ok_or_else(|| ApiError::internal("group member vanished after add"))?;
    let muted_member_ids =
        parse_json_list(group.muted_member_ids_json.as_deref()).unwrap_or_default();
    Ok((
        StatusCode::CREATED,
        Json(row.into_response(&muted_member_ids)),
    ))
}

pub async fn remove_group_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, user_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let user_id = validate_uuid(&user_id, "user id")?;
    load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let member = load_active_group_member(state.db.pool(), &group_id, &user_id).await?;
    if member.role == "owner" {
        return Err(ApiError::permission_denied("group owner cannot be removed"));
    }

    let now = now_rfc3339();
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start group member transaction"))?;

    sqlx::query(
        "UPDATE group_members SET status = 'removed' \
         WHERE group_id = ? AND user_id = ? AND status = 'active'",
    )
    .bind(&group_id)
    .bind(&user_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to remove group member"))?;

    set_group_member_muted_json(&mut tx, &group_id, &user_id, false, &now).await?;
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group member removal"))?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_group_member_muted(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, user_id)): Path<(String, String)>,
    Json(body): Json<GroupMemberMuteRequest>,
) -> Result<Json<GroupMemberResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let user_id = validate_uuid(&user_id, "user id")?;
    load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let member = load_active_group_member(state.db.pool(), &group_id, &user_id).await?;
    if member.role == "owner" {
        return Err(ApiError::permission_denied("group owner cannot be muted"));
    }

    let now = now_rfc3339();
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start group member mute transaction"))?;
    set_group_member_muted_json(&mut tx, &group_id, &user_id, body.muted, &now).await?;
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group member mute update"))?;

    let group = fetch_row(state.db.pool(), &group_id)
        .await?
        .ok_or_else(|| ApiError::internal("group vanished after member mute update"))?;
    let row = fetch_group_member_row(state.db.pool(), &group_id, &user_id)
        .await?
        .ok_or_else(|| ApiError::internal("group member vanished after mute update"))?;
    let muted_member_ids =
        parse_json_list(group.muted_member_ids_json.as_deref()).unwrap_or_default();
    Ok(Json(row.into_response(&muted_member_ids)))
}

pub async fn list_group_agents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<Vec<GroupAgentResponse>>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;

    load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let rows = fetch_group_agent_rows(state.db.pool(), &group_id).await?;
    Ok(Json(
        rows.into_iter().map(GroupAgentResponse::from).collect(),
    ))
}

pub async fn add_group_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Json(body): Json<GroupAgentAddRequest>,
) -> Result<(StatusCode, Json<GroupAgentResponse>), ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let agent_id = validate_uuid(&body.agent_id, "agent_id")?;
    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    validate_owned_active_agent(state.db.pool(), &agent_id, &owner_id).await?;

    let now = now_rfc3339();
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start group agent transaction"))?;

    let existing = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT status, context_scope_json FROM group_agents WHERE group_id = ? AND agent_id = ?",
    )
    .bind(&group_id)
    .bind(&agent_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to load group agent"))?;

    if matches!(existing.as_ref().map(|row| row.0.as_str()), Some("active")) {
        return Err(ApiError::conflict("agent already in group"));
    }

    let (topology_role, speaking_order) =
        new_agent_topology(&mut tx, &group_id, &group.communication_mode).await?;
    let existing_context_scope = existing.as_ref().and_then(|row| row.1.as_deref());
    let context_scope_json = context_scope_with_group_workspace(
        existing_context_scope,
        body.share_group_workspace.unwrap_or(false),
    )?;

    if existing.is_some() {
        sqlx::query(
            "UPDATE group_agents SET \
             topology_role = ?, speaking_order = ?, response_mode = 'mentioned_only', \
             context_scope_json = ?, status = 'active', updated_at = ? \
             WHERE group_id = ? AND agent_id = ?",
        )
        .bind(&topology_role)
        .bind(speaking_order)
        .bind(&context_scope_json)
        .bind(&now)
        .bind(&group_id)
        .bind(&agent_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal("failed to reactivate group agent"))?;
    } else {
        let result = sqlx::query(
            "INSERT INTO group_agents \
             (group_id, agent_id, topology_role, speaking_order, response_mode, \
              context_scope_json, status, joined_at, updated_at) \
             VALUES (?, ?, ?, ?, 'mentioned_only', ?, 'active', ?, ?)",
        )
        .bind(&group_id)
        .bind(&agent_id)
        .bind(&topology_role)
        .bind(speaking_order)
        .bind(&context_scope_json)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await;

        if let Err(err) = result {
            if is_unique_violation(&err) {
                return Err(ApiError::conflict("agent already in group"));
            }
            return Err(ApiError::internal("failed to add group agent"));
        }
    }

    touch_group(&mut tx, &group_id, &now).await?;
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group agent transaction"))?;

    let row = fetch_group_agent_row(state.db.pool(), &group_id, &agent_id)
        .await?
        .ok_or_else(|| ApiError::internal("group agent vanished after add"))?;
    Ok((StatusCode::CREATED, Json(row.into())))
}

pub async fn remove_group_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, agent_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let agent_id = validate_uuid(&agent_id, "agent id")?;
    load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    load_active_group_agent(state.db.pool(), &group_id, &agent_id).await?;

    let now = now_rfc3339();
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start group agent transaction"))?;

    sqlx::query(
        "UPDATE group_agents SET status = 'removed', updated_at = ? \
         WHERE group_id = ? AND agent_id = ? AND status = 'active'",
    )
    .bind(&now)
    .bind(&group_id)
    .bind(&agent_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to remove group agent"))?;

    remove_agent_from_group_lists(&mut tx, &group_id, &agent_id, &now).await?;
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group agent removal"))?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_group_agent_muted(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, agent_id)): Path<(String, String)>,
    Json(body): Json<GroupAgentMuteRequest>,
) -> Result<Json<GroupAgentResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let agent_id = validate_uuid(&agent_id, "agent id")?;
    load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    load_active_group_agent(state.db.pool(), &group_id, &agent_id).await?;

    let now = now_rfc3339();
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start group mute transaction"))?;
    set_group_agent_muted_json(&mut tx, &group_id, &agent_id, body.muted, &now).await?;
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit group mute update"))?;

    let row = fetch_group_agent_row(state.db.pool(), &group_id, &agent_id)
        .await?
        .ok_or_else(|| ApiError::internal("group agent vanished after mute update"))?;
    Ok(Json(row.into()))
}

pub async fn set_group_agent_workspace_sharing(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, agent_id)): Path<(String, String)>,
    Json(body): Json<GroupAgentWorkspaceSharingRequest>,
) -> Result<Json<GroupAgentResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let agent_id = validate_uuid(&agent_id, "agent id")?;
    load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    let existing = load_active_group_agent(state.db.pool(), &group_id, &agent_id).await?;

    let context_scope_json = context_scope_with_group_workspace(
        existing.context_scope_json.as_deref(),
        body.share_group_workspace,
    )?;
    let now = now_rfc3339();
    sqlx::query(
        "UPDATE group_agents SET context_scope_json = ?, updated_at = ? \
         WHERE group_id = ? AND agent_id = ? AND status = 'active'",
    )
    .bind(&context_scope_json)
    .bind(&now)
    .bind(&group_id)
    .bind(&agent_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to update group agent workspace sharing"))?;

    let row = fetch_group_agent_row(state.db.pool(), &group_id, &agent_id)
        .await?
        .ok_or_else(|| ApiError::internal("group agent vanished after workspace update"))?;
    Ok(Json(row.into()))
}

pub async fn set_group_agent_topology(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, agent_id)): Path<(String, String)>,
    Json(body): Json<GroupAgentTopologyRequest>,
) -> Result<Json<GroupAgentResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let agent_id = validate_uuid(&agent_id, "agent id")?;
    let group = load_active_owned(state.db.pool(), &group_id, &owner_id).await?;
    load_active_group_agent(state.db.pool(), &group_id, &agent_id).await?;

    let (topology_role, speaking_order) =
        validate_agent_topology_patch(&group.communication_mode, &body)?;
    let now = now_rfc3339();
    let mut tx = state
        .db
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::internal("failed to start topology transaction"))?;

    if topology_role.as_deref() == Some("hub") {
        sqlx::query(
            "UPDATE group_agents SET topology_role = NULL, updated_at = ? \
             WHERE group_id = ? AND agent_id <> ? AND status = 'active'",
        )
        .bind(&now)
        .bind(&group_id)
        .bind(&agent_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal("failed to clear existing topology hub"))?;
    }

    sqlx::query(
        "UPDATE group_agents SET topology_role = ?, speaking_order = ?, updated_at = ? \
         WHERE group_id = ? AND agent_id = ? AND status = 'active'",
    )
    .bind(&topology_role)
    .bind(speaking_order)
    .bind(&now)
    .bind(&group_id)
    .bind(&agent_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to update group agent topology"))?;

    touch_group(&mut tx, &group_id, &now).await?;
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("failed to commit topology update"))?;

    let row = fetch_group_agent_row(state.db.pool(), &group_id, &agent_id)
        .await?
        .ok_or_else(|| ApiError::internal("group agent vanished after topology update"))?;
    Ok(Json(row.into()))
}

/// Fetch an active group by id and enforce caller ownership.
///
/// Returns `404 not_found` when no row exists or it has been soft-deleted, and
/// `403 permission_denied` when an active row belongs to another user.
async fn load_active_owned(
    pool: &SqlitePool,
    group_id: &str,
    owner_id: &str,
) -> Result<GroupRow, ApiError> {
    let row = fetch_row(pool, group_id)
        .await?
        .ok_or_else(|| ApiError::not_found("group not found"))?;
    if row.status == "deleted" {
        return Err(ApiError::not_found("group not found"));
    }
    if row.owner_id != owner_id {
        return Err(ApiError::permission_denied("group belongs to another user"));
    }
    Ok(row)
}

async fn fetch_row(pool: &SqlitePool, group_id: &str) -> Result<Option<GroupRow>, ApiError> {
    let sql = format!("SELECT {GROUP_COLUMNS} FROM groups WHERE id = ?");
    sqlx::query_as::<_, GroupRow>(&sql)
        .bind(group_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}

async fn fetch_group_agent_rows(
    pool: &SqlitePool,
    group_id: &str,
) -> Result<Vec<GroupAgentRow>, ApiError> {
    let sql = format!(
        "SELECT {GROUP_AGENT_COLUMNS} \
         FROM group_agents \
         JOIN agents ON agents.id = group_agents.agent_id \
         WHERE group_agents.group_id = ? \
           AND group_agents.status = 'active' \
           AND agents.status = 'active' \
         ORDER BY group_agents.joined_at ASC, group_agents.agent_id ASC"
    );
    sqlx::query_as::<_, GroupAgentRow>(&sql)
        .bind(group_id)
        .fetch_all(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}

async fn fetch_group_agent_row(
    pool: &SqlitePool,
    group_id: &str,
    agent_id: &str,
) -> Result<Option<GroupAgentRow>, ApiError> {
    let sql = format!(
        "SELECT {GROUP_AGENT_COLUMNS} \
         FROM group_agents \
         JOIN agents ON agents.id = group_agents.agent_id \
         WHERE group_agents.group_id = ? \
           AND group_agents.agent_id = ? \
           AND group_agents.status = 'active' \
           AND agents.status = 'active'"
    );
    sqlx::query_as::<_, GroupAgentRow>(&sql)
        .bind(group_id)
        .bind(agent_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}

async fn fetch_group_member_rows(
    pool: &SqlitePool,
    group_id: &str,
) -> Result<Vec<GroupMemberRow>, ApiError> {
    let sql = format!(
        "SELECT {GROUP_MEMBER_COLUMNS} \
         FROM group_members \
         JOIN users ON users.id = group_members.user_id \
         WHERE group_members.group_id = ? \
           AND group_members.status = 'active' \
         ORDER BY group_members.joined_at ASC, group_members.user_id ASC"
    );
    sqlx::query_as::<_, GroupMemberRow>(&sql)
        .bind(group_id)
        .fetch_all(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}

async fn fetch_group_member_row(
    pool: &SqlitePool,
    group_id: &str,
    user_id: &str,
) -> Result<Option<GroupMemberRow>, ApiError> {
    let sql = format!(
        "SELECT {GROUP_MEMBER_COLUMNS} \
         FROM group_members \
         JOIN users ON users.id = group_members.user_id \
         WHERE group_members.group_id = ? \
           AND group_members.user_id = ? \
           AND group_members.status = 'active'"
    );
    sqlx::query_as::<_, GroupMemberRow>(&sql)
        .bind(group_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}

async fn load_active_group_member(
    pool: &SqlitePool,
    group_id: &str,
    user_id: &str,
) -> Result<GroupMemberRow, ApiError> {
    fetch_group_member_row(pool, group_id, user_id)
        .await?
        .ok_or_else(|| ApiError::not_found("group member not found"))
}

async fn load_active_group_agent(
    pool: &SqlitePool,
    group_id: &str,
    agent_id: &str,
) -> Result<GroupAgentRow, ApiError> {
    fetch_group_agent_row(pool, group_id, agent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("group agent not found"))
}

async fn fetch_user_row(pool: &SqlitePool, user_id: &str) -> Result<Option<UserRow>, ApiError> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, email, name, avatar_url, created_at FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))
}

async fn search_user_rows(pool: &SqlitePool, query: &str) -> Result<Vec<UserRow>, ApiError> {
    let trimmed = query.trim().to_lowercase();
    if trimmed.is_empty() {
        return sqlx::query_as::<_, UserRow>(
            "SELECT id, email, name, avatar_url, created_at \
             FROM users \
             ORDER BY created_at DESC, id DESC \
             LIMIT 20",
        )
        .fetch_all(pool)
        .await
        .map_err(|_| ApiError::internal("database error"));
    }

    let pattern = format!("%{trimmed}%");
    sqlx::query_as::<_, UserRow>(
        "SELECT id, email, name, avatar_url, created_at \
         FROM users \
         WHERE LOWER(name) LIKE ? OR LOWER(email) LIKE ? \
         ORDER BY created_at DESC, id DESC \
         LIMIT 20",
    )
    .bind(&pattern)
    .bind(&pattern)
    .fetch_all(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))
}

async fn validate_owned_active_agent(
    pool: &SqlitePool,
    agent_id: &str,
    owner_id: &str,
) -> Result<(), ApiError> {
    let row =
        sqlx::query_as::<_, (String, String)>("SELECT owner_id, status FROM agents WHERE id = ?")
            .bind(agent_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| ApiError::internal("database error"))?;

    match row {
        None => Err(ApiError::not_found("agent not found")),
        Some((_, status)) if status != "active" => Err(ApiError::not_found("agent not found")),
        Some((owner, _)) if owner != owner_id => {
            Err(ApiError::permission_denied("agent belongs to another user"))
        }
        Some(_) => Ok(()),
    }
}

async fn new_agent_topology(
    tx: &mut Transaction<'_, Sqlite>,
    group_id: &str,
    mode: &str,
) -> Result<(Option<String>, Option<i64>), ApiError> {
    match mode {
        "mesh" => Ok((None, None)),
        "star" => {
            let active_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) \
                 FROM group_agents \
                 JOIN agents ON agents.id = group_agents.agent_id \
                 WHERE group_agents.group_id = ? \
                   AND group_agents.status = 'active' \
                   AND agents.status = 'active'",
            )
            .bind(group_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|_| ApiError::internal("failed to load star topology state"))?;
            let role = if active_count == 0 {
                Some("hub".to_string())
            } else {
                None
            };
            Ok((role, None))
        }
        "hierarchical" => Ok((Some("worker".to_string()), None)),
        "ring" => {
            let max_order: Option<i64> = sqlx::query_scalar(
                "SELECT MAX(group_agents.speaking_order) \
                 FROM group_agents \
                 JOIN agents ON agents.id = group_agents.agent_id \
                 WHERE group_agents.group_id = ? \
                   AND group_agents.status = 'active' \
                   AND agents.status = 'active' \
                   AND group_agents.speaking_order > 0",
            )
            .bind(group_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|_| ApiError::internal("failed to load ring topology state"))?;
            Ok((None, Some(max_order.unwrap_or_default() + 1)))
        }
        _ => Err(ApiError::internal("unsupported communication mode")),
    }
}

fn validate_agent_topology_patch(
    mode: &str,
    body: &GroupAgentTopologyRequest,
) -> Result<(Option<String>, Option<i64>), ApiError> {
    let role = body
        .topology_role
        .as_deref()
        .map(str::trim)
        .map(str::to_string);

    match mode {
        "mesh" => {
            if role.is_some() || body.speaking_order.is_some() {
                return Err(ApiError::invalid_input(
                    "mesh mode does not use agent topology settings",
                ));
            }
            Ok((None, None))
        }
        "star" => {
            if body.speaking_order.is_some() || !matches!(role.as_deref(), None | Some("hub")) {
                return Err(ApiError::invalid_input(
                    "star mode only accepts hub topology role",
                ));
            }
            Ok((role, None))
        }
        "hierarchical" => {
            if body.speaking_order.is_some()
                || !matches!(role.as_deref(), None | Some("leader") | Some("worker"))
            {
                return Err(ApiError::invalid_input(
                    "hierarchical mode accepts leader or worker topology role",
                ));
            }
            Ok((role, None))
        }
        "ring" => {
            if role.is_some() {
                return Err(ApiError::invalid_input(
                    "ring mode only accepts speaking order",
                ));
            }
            if matches!(body.speaking_order, Some(order) if order < 1) {
                return Err(ApiError::invalid_input(
                    "ring speaking_order must be null or >= 1",
                ));
            }
            Ok((None, body.speaking_order))
        }
        _ => Err(ApiError::internal("unsupported communication mode")),
    }
}

async fn touch_group(
    tx: &mut Transaction<'_, Sqlite>,
    group_id: &str,
    now: &str,
) -> Result<(), ApiError> {
    sqlx::query("UPDATE groups SET updated_at = ? WHERE id = ?")
        .bind(now)
        .bind(group_id)
        .execute(&mut **tx)
        .await
        .map_err(|_| ApiError::internal("failed to update group timestamp"))?;
    Ok(())
}

async fn set_group_agent_muted_json(
    tx: &mut Transaction<'_, Sqlite>,
    group_id: &str,
    agent_id: &str,
    muted: bool,
    now: &str,
) -> Result<(), ApiError> {
    let (raw_muted,): (Option<String>,) =
        sqlx::query_as("SELECT muted_agent_ids_json FROM groups WHERE id = ?")
            .bind(group_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|_| ApiError::internal("failed to load group mute list"))?;
    let muted_agent_ids_json = if muted {
        add_to_json_list(raw_muted.as_deref(), agent_id)?
    } else {
        remove_from_json_list(raw_muted.as_deref(), agent_id)?
    };
    sqlx::query("UPDATE groups SET muted_agent_ids_json = ?, updated_at = ? WHERE id = ?")
        .bind(&muted_agent_ids_json)
        .bind(now)
        .bind(group_id)
        .execute(&mut **tx)
        .await
        .map_err(|_| ApiError::internal("failed to update group mute list"))?;
    Ok(())
}

async fn set_group_member_muted_json(
    tx: &mut Transaction<'_, Sqlite>,
    group_id: &str,
    user_id: &str,
    muted: bool,
    now: &str,
) -> Result<(), ApiError> {
    let (raw_muted,): (Option<String>,) =
        sqlx::query_as("SELECT muted_member_ids_json FROM groups WHERE id = ?")
            .bind(group_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|_| ApiError::internal("failed to load group member mute list"))?;
    let muted_member_ids_json = if muted {
        add_to_json_list(raw_muted.as_deref(), user_id)?
    } else {
        remove_from_json_list(raw_muted.as_deref(), user_id)?
    };
    sqlx::query("UPDATE groups SET muted_member_ids_json = ?, updated_at = ? WHERE id = ?")
        .bind(&muted_member_ids_json)
        .bind(now)
        .bind(group_id)
        .execute(&mut **tx)
        .await
        .map_err(|_| ApiError::internal("failed to update group member mute list"))?;
    Ok(())
}

async fn remove_agent_from_group_lists(
    tx: &mut Transaction<'_, Sqlite>,
    group_id: &str,
    agent_id: &str,
    now: &str,
) -> Result<(), ApiError> {
    let (raw_muted, raw_admin): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT muted_agent_ids_json, admin_agent_ids_json FROM groups WHERE id = ?",
    )
    .bind(group_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| ApiError::internal("failed to load group agent lists"))?;
    let muted_agent_ids_json = remove_from_json_list(raw_muted.as_deref(), agent_id)?;
    let admin_agent_ids_json = remove_from_json_list(raw_admin.as_deref(), agent_id)?;
    sqlx::query(
        "UPDATE groups SET muted_agent_ids_json = ?, admin_agent_ids_json = ?, updated_at = ? \
         WHERE id = ?",
    )
    .bind(&muted_agent_ids_json)
    .bind(&admin_agent_ids_json)
    .bind(now)
    .bind(group_id)
    .execute(&mut **tx)
    .await
    .map_err(|_| ApiError::internal("failed to clear group agent lists"))?;
    Ok(())
}

fn add_to_json_list(raw: Option<&str>, item: &str) -> Result<String, ApiError> {
    let mut values = parse_json_list(raw).unwrap_or_default();
    if !values.iter().any(|value| value == item) {
        values.push(item.to_string());
    }
    json_list_to_db(values)
}

fn remove_from_json_list(raw: Option<&str>, item: &str) -> Result<String, ApiError> {
    let mut values = parse_json_list(raw).unwrap_or_default();
    values.retain(|value| value != item);
    json_list_to_db(values)
}

fn json_list_to_db(values: Vec<String>) -> Result<String, ApiError> {
    serde_json::to_string(&values)
        .map_err(|_| ApiError::internal("failed to serialize group id list"))
}

fn group_workspace_shared(raw: Option<&str>) -> bool {
    raw.and_then(|value| serde_json::from_str::<Value>(value).ok())
        .and_then(|value| value.get("share_group_workspace").and_then(Value::as_bool))
        == Some(true)
}

fn context_scope_with_group_workspace(
    raw: Option<&str>,
    share: bool,
) -> Result<Option<String>, ApiError> {
    let mut object = raw
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .and_then(|value| match value {
            Value::Object(object) => Some(object),
            _ => None,
        })
        .unwrap_or_else(Map::new);

    if share {
        object.insert("share_group_workspace".to_string(), Value::Bool(true));
    } else {
        object.remove("share_group_workspace");
    }

    if object.is_empty() {
        Ok(None)
    } else {
        serde_json::to_string(&Value::Object(object))
            .map(Some)
            .map_err(|_| ApiError::internal("failed to serialize context scope"))
    }
}

async fn normalize_group_agent_topology(
    tx: &mut Transaction<'_, Sqlite>,
    group_id: &str,
    mode: &str,
    now: &str,
) -> Result<(), ApiError> {
    let rows = sqlx::query_as::<_, ActiveGroupAgentRow>(
        "SELECT group_agents.agent_id, group_agents.topology_role, group_agents.speaking_order \
         FROM group_agents \
         JOIN agents ON agents.id = group_agents.agent_id \
         WHERE group_agents.group_id = ? \
           AND group_agents.status = 'active' \
           AND agents.status = 'active' \
         ORDER BY group_agents.joined_at ASC, group_agents.agent_id ASC",
    )
    .bind(group_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| ApiError::internal("failed to load group agents for topology update"))?;

    let updates = match mode {
        "mesh" => rows
            .iter()
            .map(|row| (row.agent_id.clone(), None, None))
            .collect(),
        "star" => star_topology_updates(&rows),
        "hierarchical" => hierarchical_topology_updates(&rows),
        "ring" => ring_topology_updates(&rows),
        _ => return Err(ApiError::internal("unsupported communication mode")),
    };

    for (agent_id, topology_role, speaking_order) in updates {
        sqlx::query(
            "UPDATE group_agents \
             SET topology_role = ?, speaking_order = ?, updated_at = ? \
             WHERE group_id = ? AND agent_id = ? AND status = 'active'",
        )
        .bind(topology_role)
        .bind(speaking_order)
        .bind(now)
        .bind(group_id)
        .bind(agent_id)
        .execute(&mut **tx)
        .await
        .map_err(|_| ApiError::internal("failed to update group agent topology"))?;
    }

    Ok(())
}

fn star_topology_updates(
    rows: &[ActiveGroupAgentRow],
) -> Vec<(String, Option<String>, Option<i64>)> {
    let hub_agent_id = rows
        .iter()
        .find(|row| row.topology_role.as_deref() == Some("hub"))
        .or_else(|| rows.first())
        .map(|row| row.agent_id.as_str());

    rows.iter()
        .map(|row| {
            let role = if hub_agent_id == Some(row.agent_id.as_str()) {
                Some("hub".to_string())
            } else {
                None
            };
            (row.agent_id.clone(), role, None)
        })
        .collect()
}

fn hierarchical_topology_updates(
    rows: &[ActiveGroupAgentRow],
) -> Vec<(String, Option<String>, Option<i64>)> {
    rows.iter()
        .map(|row| {
            let role = match row.topology_role.as_deref() {
                Some("leader") => "leader",
                Some("worker") => "worker",
                _ => "worker",
            };
            (row.agent_id.clone(), Some(role.to_string()), None)
        })
        .collect()
}

fn ring_topology_updates(
    rows: &[ActiveGroupAgentRow],
) -> Vec<(String, Option<String>, Option<i64>)> {
    let mut order_counts = BTreeMap::new();
    for row in rows {
        if let Some(order) = row.speaking_order.filter(|order| *order > 0) {
            *order_counts.entry(order).or_insert(0usize) += 1;
        }
    }

    let mut used_orders = BTreeSet::new();
    let mut updates = Vec::with_capacity(rows.len());
    for row in rows {
        let order = row.speaking_order.filter(|order| {
            *order > 0 && order_counts.get(order).copied().unwrap_or_default() == 1
        });
        if let Some(order) = order {
            used_orders.insert(order);
        }
        updates.push((row.agent_id.clone(), None, order));
    }

    let mut next_order = used_orders.iter().next_back().copied().unwrap_or_default() + 1;
    for (_, _, speaking_order) in &mut updates {
        if speaking_order.is_some() {
            continue;
        }
        while used_orders.contains(&next_order) {
            next_order += 1;
        }
        *speaking_order = Some(next_order);
        used_orders.insert(next_order);
        next_order += 1;
    }

    updates
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

async fn create_group_workspace(
    pool: &SqlitePool,
    owner_id: &str,
    group_id: &str,
    group_name: &str,
    now: &str,
) -> Result<String, ApiError> {
    let root = require_group_workspace_root(pool, owner_id).await?;
    let storage_dir = PathBuf::from(root).join(group_id);
    std::fs::create_dir_all(&storage_dir)
        .map_err(|_| ApiError::internal("failed to create group workspace directory"))?;
    let local_path = std::fs::canonicalize(&storage_dir)
        .map_err(|_| ApiError::internal("failed to resolve group workspace directory"))?
        .to_string_lossy()
        .into_owned();

    let workspace_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO workspaces \
         (id, owner_id, name, backend_type, local_path, config_json, status, created_at, updated_at) \
         VALUES (?, ?, ?, 'local', ?, NULL, 'active', ?, ?)",
    )
    .bind(&workspace_id)
    .bind(owner_id)
    .bind(format!("group:{group_name}"))
    .bind(&local_path)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|_| ApiError::internal("failed to create group workspace"))?;

    Ok(workspace_id)
}

async fn require_group_workspace_root(
    pool: &SqlitePool,
    owner_id: &str,
) -> Result<String, ApiError> {
    let row = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT group_workspace_root FROM system_settings WHERE owner_id = ?",
    )
    .bind(owner_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;

    row.and_then(|(root,)| root)
        .map(|root| root.trim().to_string())
        .filter(|root| !root.is_empty())
        .ok_or_else(|| ApiError::invalid_input("group_workspace_root is required"))
}

async fn validate_initial_agents(
    pool: &SqlitePool,
    raw_ids: Option<&[String]>,
    owner_id: &str,
) -> Result<Vec<String>, ApiError> {
    let Some(raw_ids) = raw_ids else {
        return Ok(Vec::new());
    };

    let mut seen = BTreeSet::new();
    let mut ids = Vec::with_capacity(raw_ids.len());
    for raw_id in raw_ids {
        let id = validate_uuid(raw_id, "initial_agents")?;
        if !seen.insert(id.clone()) {
            return Err(ApiError::invalid_input(
                "initial_agents must not contain duplicates",
            ));
        }
        let row = sqlx::query_as::<_, (String, String)>(
            "SELECT owner_id, status FROM agents WHERE id = ?",
        )
        .bind(&id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))?;

        match row {
            None => return Err(ApiError::not_found("agent not found")),
            Some((_, status)) if status != "active" => {
                return Err(ApiError::not_found("agent not found"));
            }
            Some((owner, _)) if owner != owner_id => {
                return Err(ApiError::permission_denied("agent belongs to another user"));
            }
            Some(_) => ids.push(id),
        }
    }
    Ok(ids)
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

fn validate_proactive_max_rounds(raw: Option<i64>) -> Result<i64, ApiError> {
    let value = raw.unwrap_or(1);
    if !(1..=5).contains(&value) {
        return Err(ApiError::invalid_input(
            "proactive_max_rounds must be between 1 and 5",
        ));
    }
    Ok(value)
}

fn validate_multiplier(raw: Option<i64>) -> Result<i64, ApiError> {
    let value = raw.unwrap_or(1);
    if value < 1 {
        return Err(ApiError::invalid_input(
            "proactive_reply_multiplier must be >= 1",
        ));
    }
    Ok(value)
}

fn validate_agent_free_mention_max_dispatches(raw: Option<i64>) -> Result<i64, ApiError> {
    let value = raw.unwrap_or(8);
    if value < 0 {
        return Err(ApiError::invalid_input(
            "agent_free_mention_max_dispatches must be >= 0",
        ));
    }
    Ok(value)
}

fn validate_communication_mode(raw: Option<&str>) -> Result<String, ApiError> {
    let mode = raw
        .map(str::trim)
        .filter(|mode| !mode.is_empty())
        .unwrap_or("mesh");
    match mode {
        "mesh" | "star" | "hierarchical" | "ring" => Ok(mode.to_string()),
        _ => Err(ApiError::invalid_input(
            "communication_mode must be one of mesh, star, hierarchical, or ring",
        )),
    }
}

fn normalize_description(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| d.to_string())
}

fn initial_agent_topology(mode: &str, position: usize) -> (Option<&'static str>, Option<i64>) {
    match mode {
        "star" if position == 0 => (Some("hub"), None),
        "hierarchical" => (Some("worker"), None),
        "ring" => (None, Some(position as i64 + 1)),
        _ => (None, None),
    }
}

fn parse_json_list(raw: Option<&str>) -> Option<Vec<String>> {
    raw.and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
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

fn is_unique_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.is_unique_violation())
}

fn now_plus_rfc3339(microseconds: i64) -> String {
    (OffsetDateTime::now_utc() + Duration::microseconds(microseconds))
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
