use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

use super::{
    schedule::{MissedPolicy, Schedule, Trigger},
    signature,
};
use crate::api::{
    auth,
    commands::{self, Command, MessageAcceptance, PreparedMessage},
    error::ApiError,
    AppState,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Budget {
    pub max_agent_steps: u32,
    pub max_total_tokens: u64,
    pub turn_timeout_seconds: u64,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            max_agent_steps: 8,
            max_total_tokens: 64_000,
            turn_timeout_seconds: 300,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub group_id: String,
    pub name: String,
    pub prompt: String,
    pub trigger: Trigger,
    pub timezone: String,
    #[serde(default)]
    pub missed_policy: MissedPolicy,
    #[serde(default)]
    pub budget: Budget,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub expected_version: Option<i64>,
}
fn yes() -> bool {
    true
}

#[derive(Clone, sqlx::FromRow)]
struct Automation {
    id: String,
    owner_id: String,
    group_id: String,
    name: String,
    prompt: String,
    trigger_json: String,
    timezone: String,
    missed_policy: String,
    budget_json: String,
    enabled: bool,
    version: i64,
    next_run_at: Option<i64>,
    hook_id: String,
    hook_secret: Option<String>,
    created_at: String,
    updated_at: String,
}
const SELECT:&str="SELECT id,owner_id,group_id,name,prompt,trigger_json,timezone,missed_policy,budget_json,enabled,version,next_run_at,hook_id,hook_secret,created_at,updated_at FROM automations";
fn db_error(error: sqlx::Error) -> ApiError {
    tracing::error!(%error,"automation database operation failed");
    ApiError::internal("automation database operation failed")
}
fn queue_full() -> ApiError {
    ApiError::new(
        StatusCode::TOO_MANY_REQUESTS,
        "automation_queue_full",
        "Queue or delivery rate limit reached; retry later",
    )
    .with_details(json!({"retry_after_seconds":60}))
}
fn decode<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T, ApiError> {
    serde_json::from_str(raw)
        .map_err(|_| ApiError::internal("invalid saved automation configuration"))
}
fn next_at(config: &Config, now: DateTime<Utc>) -> Result<Option<i64>, ApiError> {
    if config.name.trim().is_empty()
        || config.name.len() > 120
        || config.prompt.trim().is_empty()
        || config.prompt.len() > 16 * 1024
    {
        return Err(ApiError::invalid_input(
            "name (1–120 bytes) and prompt (1–16384 bytes) are required",
        ));
    }
    if !(1..=100).contains(&config.budget.max_agent_steps)
        || !(1..=1_000_000).contains(&config.budget.max_total_tokens)
        || !(1..=3600).contains(&config.budget.turn_timeout_seconds)
    {
        return Err(ApiError::invalid_input("budget exceeds supported limits"));
    }
    let next = Schedule::parse(&config.trigger, &config.timezone)?
        .next_after(now)?
        .map(|at| at.timestamp());
    if config.enabled && matches!(config.trigger, Trigger::Once { .. }) && next.is_none() {
        return Err(ApiError::invalid_input(
            "once.at must be in the future when enabling a task",
        ));
    }
    Ok(if config.enabled { next } else { None })
}
async fn ensure_group(
    tx: &mut Transaction<'_, Sqlite>,
    owner: &str,
    group: &str,
) -> Result<(), ApiError> {
    let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM groups WHERE id=? AND owner_id=? AND status='active' AND conversation_kind='group')")
        .bind(group).bind(owner).fetch_one(&mut **tx).await.map_err(db_error)?;
    if !valid {
        return Err(ApiError::not_found("active group not found"));
    }
    Ok(())
}
type LatestOccurrence = (String, Option<String>, Option<String>, Option<String>);
type HistoryRow = (
    String,
    i64,
    String,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
);
async fn owned(state: &AppState, owner: &str, id: &str) -> Result<Automation, ApiError> {
    sqlx::query_as::<_, Automation>(&format!("{SELECT} WHERE id=? AND owner_id=?"))
        .bind(id)
        .bind(owner)
        .fetch_optional(state.db.pool())
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("automation not found"))
}
async fn view(state: &AppState, a: &Automation, now: DateTime<Utc>) -> Result<Value, ApiError> {
    let trigger: Trigger = decode(&a.trigger_json)?;
    let preview = Schedule::parse(&trigger, &a.timezone)?.preview(now)?;
    let latest:Option<LatestOccurrence>=sqlx::query_as("SELECT o.id,o.job_id,j.status,j.error FROM automation_occurrences o LEFT JOIN execution_jobs j ON j.id=o.job_id WHERE o.automation_id=? ORDER BY o.created_at DESC,o.rowid DESC LIMIT 1")
        .bind(&a.id).fetch_optional(state.db.pool()).await.map_err(db_error)?;
    let permissions:Vec<(String,String,Option<String>)>=sqlx::query_as("SELECT a.id,a.name,a.tool_config_json FROM agents a JOIN group_agents m ON m.agent_id=a.id WHERE m.group_id=? AND m.status='active' AND a.owner_id=? AND a.status='active' ORDER BY a.id")
        .bind(&a.group_id).bind(&a.owner_id).fetch_all(state.db.pool()).await.map_err(db_error)?;
    Ok(
        json!({"id":a.id,"group_id":a.group_id,"name":a.name,"prompt":a.prompt,"trigger":trigger,"timezone":a.timezone,
        "missed_policy":a.missed_policy,"budget":decode::<Value>(&a.budget_json)?,"enabled":a.enabled,"version":a.version,
        "next_run_at":a.next_run_at,"preview":preview,"hook_id":a.hook_id,"hook_configured":a.hook_secret.is_some(),
        "latest":latest.map(|(id,job,status,error)|json!({"occurrence_id":id,"job_id":job,"status":status,"error":error})),
        "permissions":permissions.into_iter().map(|(id,name,raw)| {let v=raw.and_then(|r|serde_json::from_str::<Value>(&r).ok());json!({"agent_id":id,"name":name,"bypass_approvals":v.as_ref().and_then(|v|v.get("bypass_approvals")).and_then(Value::as_bool).unwrap_or(false)})}).collect::<Vec<_>>(),
        "created_at":a.created_at,"updated_at":a.updated_at}),
    )
}
#[derive(Deserialize)]
pub struct ListQuery {
    group_id: Option<String>,
}
pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let owner = auth::current_user_id(&headers, &state).await?;
    let rows=sqlx::query_as::<_,Automation>(&format!("{SELECT} WHERE owner_id=? AND (? IS NULL OR group_id=?) ORDER BY created_at DESC LIMIT 200"))
        .bind(&owner).bind(&query.group_id).bind(&query.group_id).fetch_all(state.db.pool()).await.map_err(db_error)?;
    let mut items = Vec::new();
    for a in rows {
        items.push(view(&state, &a, Utc::now()).await?);
    }
    Ok(Json(json!({"items":items})))
}
pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let owner = auth::current_user_id(&headers, &state).await?;
    Ok(Json(
        view(&state, &owned(&state, &owner, &id).await?, Utc::now()).await?,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewInput {
    trigger: Trigger,
    timezone: String,
}
/// Preview is read-only and does not create an occurrence or job. The editor
/// calls it before save so an invalid cron/timezone is visible in the form.
pub async fn preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<PreviewInput>,
) -> Result<Json<Value>, ApiError> {
    auth::current_user_id(&headers, &state).await?;
    let preview = Schedule::parse(&input.trigger, &input.timezone)?.preview(Utc::now())?;
    Ok(Json(json!({ "preview": preview })))
}
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(config): Json<Config>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let owner = auth::current_user_id(&headers, &state).await?;
    let next = next_at(&config, Utc::now())?;
    let id = Uuid::new_v4().to_string();
    let now = commands::now();
    let mut tx = state.db.begin_write().await.map_err(db_error)?;
    ensure_group(&mut tx, &owner, &config.group_id).await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM automations WHERE owner_id=?")
        .bind(&owner)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
    if count >= 200 {
        return Err(queue_full());
    }
    sqlx::query("INSERT INTO automations(id,owner_id,group_id,name,prompt,trigger_json,timezone,missed_policy,budget_json,enabled,next_run_at,hook_id,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(&id).bind(&owner).bind(&config.group_id).bind(config.name.trim()).bind(config.prompt.trim())
        .bind(serde_json::to_string(&config.trigger).unwrap()).bind(&config.timezone).bind(serde_json::to_value(config.missed_policy).unwrap().as_str().unwrap())
        .bind(serde_json::to_string(&config.budget).unwrap()).bind(config.enabled).bind(next).bind(Uuid::new_v4().to_string()).bind(&now).bind(&now)
        .execute(&mut *tx).await.map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok((
        StatusCode::CREATED,
        Json(view(&state, &owned(&state, &owner, &id).await?, Utc::now()).await?),
    ))
}
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(config): Json<Config>,
) -> Result<Json<Value>, ApiError> {
    let owner = auth::current_user_id(&headers, &state).await?;
    let next = next_at(&config, Utc::now())?;
    let mut tx = state.db.begin_write().await.map_err(db_error)?;
    ensure_group(&mut tx, &owner, &config.group_id).await?;
    let changed=sqlx::query("UPDATE automations SET group_id=?,name=?,prompt=?,trigger_json=?,timezone=?,missed_policy=?,budget_json=?,enabled=?,version=version+1,next_run_at=?,updated_at=? WHERE id=? AND owner_id=? AND version=?")
        .bind(&config.group_id).bind(config.name.trim()).bind(config.prompt.trim()).bind(serde_json::to_string(&config.trigger).unwrap())
        .bind(&config.timezone).bind(serde_json::to_value(config.missed_policy).unwrap().as_str().unwrap()).bind(serde_json::to_string(&config.budget).unwrap())
        .bind(config.enabled).bind(next).bind(commands::now()).bind(&id).bind(&owner).bind(config.expected_version)
        .execute(&mut *tx).await.map_err(db_error)?.rows_affected();
    if changed != 1 {
        return Err(ApiError::conflict(
            "automation version changed or task no longer exists",
        ));
    }
    // Config edits replace all unstarted occurrences; running and waiting jobs
    // retain the configuration snapshot they were accepted with.
    cancel_queued(&mut tx, &id).await?;
    tx.commit().await.map_err(db_error)?;
    commands::launch(state.clone());
    Ok(Json(
        view(&state, &owned(&state, &owner, &id).await?, Utc::now()).await?,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Toggle {
    enabled: bool,
    expected_version: i64,
}
pub async fn toggle(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Toggle>,
) -> Result<Json<Value>, ApiError> {
    let owner = auth::current_user_id(&headers, &state).await?;
    let a = owned(&state, &owner, &id).await?;
    let config = Config {
        group_id: a.group_id,
        name: a.name,
        prompt: a.prompt,
        trigger: decode(&a.trigger_json)?,
        timezone: a.timezone,
        missed_policy: decode(&format!("\"{}\"", a.missed_policy))?,
        budget: decode(&a.budget_json)?,
        enabled: body.enabled,
        expected_version: Some(body.expected_version),
    };
    update(State(state), headers, Path(id), Json(config)).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretAction {
    action: String,
    expected_version: i64,
}
pub async fn secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SecretAction>,
) -> Result<Json<Value>, ApiError> {
    let owner = auth::current_user_id(&headers, &state).await?;
    let secret = match body.action.as_str() {
        "rotate" => Some(format!(
            "{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        )),
        "revoke" => None,
        _ => return Err(ApiError::invalid_input("action must be rotate or revoke")),
    };
    let mut tx = state.db.begin_write().await.map_err(db_error)?;
    let hook:Option<String>=sqlx::query_scalar("UPDATE automations SET hook_secret=?,version=version+1,updated_at=? WHERE id=? AND owner_id=? AND version=? AND json_extract(trigger_json,'$.kind') IN ('webhook','github') RETURNING hook_id")
        .bind(&secret).bind(commands::now()).bind(&id).bind(owner).bind(body.expected_version).fetch_optional(&mut *tx).await.map_err(db_error)?;
    let hook = hook.ok_or_else(|| ApiError::conflict("webhook task or version changed"))?;
    tx.commit().await.map_err(db_error)?;
    // This is the only response containing the secret. Never include it in
    // ordinary views, account events, receipt data or prompt text.
    Ok(Json(
        json!({"hook_id":hook,"secret":secret,"version":body.expected_version+1}),
    ))
}
pub async fn history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let owner = auth::current_user_id(&headers, &state).await?;
    owned(&state, &owner, &id).await?;
    let rows:Vec<HistoryRow>=sqlx::query_as("SELECT o.id,o.automation_version,o.source,o.scheduled_at,o.job_id,j.thread_id,j.status,j.error,o.created_at FROM automation_occurrences o LEFT JOIN execution_jobs j ON j.id=o.job_id WHERE o.automation_id=? ORDER BY o.created_at DESC,o.rowid DESC LIMIT 100")
        .bind(id).fetch_all(state.db.pool()).await.map_err(db_error)?;
    Ok(Json(
        json!({"items":rows.into_iter().map(|(id,version,source,scheduled,job,thread,status,error,created)|json!({"id":id,"version":version,"source":source,"scheduled_at":scheduled,"job_id":job,"thread_id":thread,"status":status.unwrap_or_else(||"skipped".into()),"error":error,"created_at":created})).collect::<Vec<_>>()}),
    ))
}
async fn cancel_queued(tx: &mut Transaction<'_, Sqlite>, automation: &str) -> Result<(), ApiError> {
    let ids:Vec<String>=sqlx::query_scalar("SELECT id FROM execution_jobs WHERE automation_id=? AND status='queued' AND worker_active=0 AND occupies_thread=0")
        .bind(automation).fetch_all(&mut **tx).await.map_err(db_error)?;
    for id in ids {
        commands::cancel_queued_job_in_tx(tx, &id)
            .await
            .map_err(db_error)?;
    }
    Ok(())
}

struct Occurrence<'a> {
    source: &'a str,
    key: String,
    hash: String,
    scheduled: Option<i64>,
    payload: Option<Value>,
    now: DateTime<Utc>,
    /// Human-readable provenance used in the untrusted-data label.
    label: Option<String>,
    /// GitHub's unsigned delivery header cannot be the only dedup identity;
    /// matching normalized payloads absorbs replays with a changed header.
    dedup_payload: bool,
    /// Digests a pre-pin binary may have stored for this payload, matched
    /// only against `digest_version=1` rows; `None` when too many floats.
    legacy_hashes: Option<Vec<String>>,
}
async fn enqueue(
    tx: &mut Transaction<'_, Sqlite>,
    a: &Automation,
    event: Occurrence<'_>,
) -> Result<Value, ApiError> {
    let prior:Option<(String,String,i64)>=sqlx::query_as("SELECT payload_hash,response_json,digest_version FROM automation_occurrences WHERE automation_id=? AND source=? AND event_key=?")
        .bind(&a.id).bind(event.source).bind(&event.key).fetch_optional(&mut **tx).await.map_err(db_error)?;
    if let Some((hash, response, version)) = prior {
        let legacy_match = version == 1
            && event
                .legacy_hashes
                .as_ref()
                .is_some_and(|hashes| hashes.contains(&hash));
        if hash != event.hash && !legacy_match {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "idempotency_conflict",
                "event_id was already accepted with different content",
            ));
        }
        return decode(&response);
    }
    if event.dedup_payload {
        // Note: rows hashed before the exact float parser was pinned are
        // matched through every reading that parser could have made; past the
        // enumeration limit a replay cannot be ruled out, so it is refused.
        let legacy = event
            .legacy_hashes
            .as_ref()
            .map(|hashes| serde_json::to_string(hashes).unwrap_or_default());
        let duplicate: Option<(String, String)> = sqlx::query_as(
            "SELECT payload_hash,response_json FROM automation_occurrences \
             WHERE automation_id=? AND source=? AND (payload_hash=? OR (digest_version=1 \
             AND payload_hash<>'' AND (?4 IS NULL OR payload_hash IN (SELECT value FROM json_each(?4))))) \
             ORDER BY (payload_hash=?3) DESC,created_at,rowid LIMIT 1",
        )
        .bind(&a.id)
        .bind(event.source)
        .bind(&event.hash)
        .bind(&legacy)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_error)?;
        if let Some((hash, response)) = duplicate {
            if hash != event.hash && legacy.is_none() {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "legacy_replay_unverifiable",
                    "payload has too many decimal numbers to rule out a replay of a pre-upgrade delivery",
                ));
            }
            return decode(&response);
        }
    }
    if !a.enabled {
        return Err(ApiError::conflict("automation is paused"));
    }
    ensure_group(tx, &a.owner_id, &a.group_id).await?;
    let queued: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM execution_jobs WHERE owner_id=? AND status='queued'",
    )
    .bind(&a.owner_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_error)?;
    let rate:i64=sqlx::query_scalar("SELECT COUNT(*) FROM automation_occurrences o JOIN automations a ON a.id=o.automation_id WHERE a.owner_id=? AND o.source='webhook' AND o.created_at>=?")
        .bind(&a.owner_id).bind((event.now-chrono::Duration::minutes(1)).to_rfc3339_opts(chrono::SecondsFormat::Secs,true)).fetch_one(&mut **tx).await.map_err(db_error)?;
    if queued >= 128 || (event.source == "webhook" && rate >= 120) {
        return Err(queue_full());
    }
    if event.source == "timer" {
        cancel_queued(tx, &a.id).await?;
    }
    let occurrence = Uuid::new_v4().to_string();
    let thread = Uuid::new_v4().to_string();
    let request = Uuid::new_v4().to_string();
    let now = event
        .now
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    // Payload is ordinary user data, labelled and encoded. It never becomes
    // a system instruction and no adapter executes arbitrary filter scripts.
    let content = if let Some(payload) = event.payload {
        format!(
            "{}\n\n[Untrusted {} data; automation {}, event {}]\n{}",
            a.prompt,
            event.label.as_deref().unwrap_or("webhook"),
            a.id,
            event.key,
            payload
        )
    } else {
        a.prompt.clone()
    };
    let prepared = PreparedMessage {
        content,
        attachments: vec![],
        model_override: None,
        effort_override: None,
    };
    let command = Command {
        request_id: request,
        name: "message.send".into(),
        target: json!({"conversation_id":a.group_id,"thread_id":thread}),
        payload: json!({"content":prepared.content}),
    };
    let digest = commands::payload_hash(&command);
    sqlx::query("INSERT INTO threads(id,group_id,agent_id,title,status,next_seq,created_at,updated_at) VALUES(?,?,NULL,?,'active',1,?,?)")
        .bind(&thread).bind(&a.group_id).bind(&a.name).bind(&now).bind(&now).execute(&mut **tx).await.map_err(db_error)?;
    let mut response = commands::insert_message_in_tx(
        tx,
        MessageAcceptance {
            owner: &a.owner_id,
            command: &command,
            digest: &digest,
            group: &a.group_id,
            thread: &thread,
            prepared: &prepared,
            now: &now,
            reuse_job: None,
        },
    )
    .await?;
    let snapshot = json!({"automation_id":a.id,"version":a.version,"source":event.source,"event_id":event.key,"scheduled_at":event.scheduled,"budget":decode::<Budget>(&a.budget_json)?});
    sqlx::query("UPDATE execution_jobs SET automation_id=?,occurrence_id=?,automation_snapshot_json=? WHERE id=?")
        .bind(&a.id).bind(&occurrence).bind(snapshot.to_string()).bind(response["job_id"].as_str()).execute(&mut **tx).await.map_err(db_error)?;
    response["occurrence_id"] = json!(occurrence);
    sqlx::query("INSERT INTO automation_occurrences(id,automation_id,automation_version,source,event_key,payload_hash,scheduled_at,job_id,response_json,created_at,digest_version) VALUES(?,?,?,?,?,?,?,?,?,?,2)")
        .bind(&occurrence).bind(&a.id).bind(a.version).bind(event.source).bind(event.key).bind(event.hash).bind(event.scheduled).bind(response["job_id"].as_str()).bind(response.to_string()).bind(&now)
        .execute(&mut **tx).await.map_err(db_error)?;
    Ok(response)
}
pub async fn webhook(
    State(state): State<AppState>,
    Path(hook): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    // Authenticate and enqueue under the same write transaction so rotation or
    // pause cannot race between a successful signature check and acceptance.
    let mut tx = state.db.begin_write().await.map_err(db_error)?;
    let a = sqlx::query_as::<_, Automation>(&format!("{SELECT} WHERE hook_id=?"))
        .bind(hook)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::unauthorized("Webhook authentication failed"))?;
    let secret = a
        .hook_secret
        .as_deref()
        .ok_or_else(|| ApiError::unauthorized("Webhook authentication failed"))?;
    let trigger = decode::<Trigger>(&a.trigger_json)?;
    let now = Utc::now();
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
    };
    let occurrence = match trigger {
        Trigger::Webhook => {
            let event = signature::verify(
                secret.as_bytes(),
                header("x-qunica-timestamp"),
                header("x-qunica-signature"),
                &body,
                now.timestamp(),
            )?;
            Occurrence {
                source: "webhook",
                key: event.event_id,
                hash: event.payload_hash,
                scheduled: None,
                payload: Some(event.payload),
                now,
                label: None,
                dedup_payload: false,
                legacy_hashes: event.legacy_hashes,
            }
        }
        Trigger::Github { events } => {
            let event = signature::verify_github(
                secret.as_bytes(),
                header("x-github-event"),
                header("x-github-delivery"),
                header("x-hub-signature-256"),
                &body,
            )?;
            let subscribed = if events.is_empty() {
                // A hook creation sends `ping`; it is a connectivity check, not
                // a task trigger unless the owner explicitly subscribes to it.
                event.event != "ping"
            } else {
                events
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(&event.event))
            };
            if !subscribed {
                return Ok((
                    StatusCode::ACCEPTED,
                    Json(json!({"status":"ignored","reason":"event_not_subscribed"})),
                ));
            }
            Occurrence {
                source: "webhook",
                key: format!("github:{}", event.delivery_id),
                hash: event.payload_hash,
                scheduled: None,
                payload: Some(event.payload),
                now,
                label: Some(format!("GitHub {}", event.event)),
                dedup_payload: true,
                legacy_hashes: event.legacy_hashes,
            }
        }
        _ => return Err(ApiError::unauthorized("Webhook authentication failed")),
    };
    let response = enqueue(&mut tx, &a, occurrence).await?;
    tx.commit().await.map_err(db_error)?;
    commands::launch(state);
    Ok((StatusCode::ACCEPTED, Json(response)))
}

async fn disable_automation(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    version: i64,
) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE automations SET enabled=0,next_run_at=NULL,updated_at=? WHERE id=? AND version=?",
    )
    .bind(commands::now())
    .bind(id)
    .bind(version)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?;
    Ok(())
}

/// Called by the server even when there are no clients. Injected UTC time also
/// drives deterministic restart/missed-run tests against the real database.
pub async fn tick(
    state: &AppState,
    now: DateTime<Utc>,
    recovering: bool,
) -> Result<usize, ApiError> {
    let mut tx = state.db.begin_write().await.map_err(db_error)?;
    let due = sqlx::query_as::<_, Automation>(&format!(
        "{SELECT} WHERE enabled=1 AND next_run_at<=? ORDER BY next_run_at,id LIMIT 200"
    ))
    .bind(now.timestamp())
    .fetch_all(&mut *tx)
    .await
    .map_err(db_error)?;
    let mut accepted = 0;
    for a in due {
        // One malformed row must not stop every other due task. Disabling the
        // task keeps an unreadable trigger from failing the whole tick forever.
        let preparsed = (|| -> Result<(Schedule, MissedPolicy, i64), ApiError> {
            let trigger = decode::<Trigger>(&a.trigger_json)?;
            let schedule = Schedule::parse(&trigger, &a.timezone)?;
            let policy = decode::<MissedPolicy>(&format!("\"{}\"", a.missed_policy))?;
            let scheduled = a
                .next_run_at
                .ok_or_else(|| ApiError::internal("due automation has no next run"))?;
            Ok((schedule, policy, scheduled))
        })();
        let (schedule, policy, scheduled) = match preparsed {
            Ok(values) => values,
            Err(error) => {
                disable_automation(&mut tx, &a.id, a.version).await?;
                tracing::warn!(
                    automation_id = a.id,
                    code = error.code(),
                    "automation disabled because its trigger is invalid"
                );
                continue;
            }
        };
        let plan = match schedule.due(scheduled, now, policy, recovering) {
            Ok(plan) => plan,
            Err(error) => {
                disable_automation(&mut tx, &a.id, a.version).await?;
                tracing::warn!(
                    automation_id = a.id,
                    code = error.code(),
                    "automation disabled because its schedule cannot be computed"
                );
                continue;
            }
        };
        if let Some(run_at) = plan.run_at {
            let key = format!("{}:{run_at}", a.version);
            // A savepoint keeps a failed occurrence from leaving half a thread
            // behind while allowing other due schedules to make progress.
            sqlx::query("SAVEPOINT automation_occurrence")
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
            let result = enqueue(
                &mut tx,
                &a,
                Occurrence {
                    source: "timer",
                    key: key.clone(),
                    hash: key,
                    scheduled: Some(run_at),
                    payload: None,
                    now,
                    label: None,
                    dedup_payload: false,
                    legacy_hashes: Some(Vec::new()),
                },
            )
            .await;
            match result {
                Ok(_) => {
                    accepted += 1;
                    sqlx::query("RELEASE automation_occurrence")
                        .execute(&mut *tx)
                        .await
                        .map_err(db_error)?;
                }
                Err(error) => {
                    sqlx::query("ROLLBACK TO automation_occurrence")
                        .execute(&mut *tx)
                        .await
                        .map_err(db_error)?;
                    sqlx::query("RELEASE automation_occurrence")
                        .execute(&mut *tx)
                        .await
                        .map_err(db_error)?;
                    if error.code() == "automation_queue_full" {
                        continue;
                    }
                    // Invalidated group permissions disable the trigger rather
                    // than repeatedly creating doomed work every five seconds.
                    disable_automation(&mut tx, &a.id, a.version).await?;
                    tracing::warn!(
                        automation_id = a.id,
                        code = error.code(),
                        "automation disabled because its group is unavailable"
                    );
                    continue;
                }
            }
        }
        if let Some(until) = plan.skipped_until {
            let key = format!("{}:{scheduled}:{until}", a.version);
            sqlx::query("INSERT OR IGNORE INTO automation_occurrences(id,automation_id,automation_version,source,event_key,payload_hash,scheduled_at,response_json,created_at,digest_version) VALUES(?,?,?,'skipped',?,'',?,'{}',?,2)")
                .bind(Uuid::new_v4().to_string()).bind(&a.id).bind(a.version).bind(key).bind(until).bind(commands::now()).execute(&mut *tx).await.map_err(db_error)?;
        }
        sqlx::query("UPDATE automations SET next_run_at=?,updated_at=? WHERE id=? AND version=?")
            .bind(plan.next_at)
            .bind(commands::now())
            .bind(&a.id)
            .bind(a.version)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
    }
    tx.commit().await.map_err(db_error)?;
    if accepted > 0 {
        commands::launch(state.clone());
    }
    Ok(accepted)
}
