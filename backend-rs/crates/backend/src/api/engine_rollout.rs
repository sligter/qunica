use crate::api::{auth::current_user_id, error::ApiError, AppState};
use axum::{
    extract::{Query, State},
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct Capabilities {
    rig_available: bool,
    default_engine: &'static str,
    rig_paused: bool,
    app_version: &'static str,
    rig_version: &'static str,
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Capabilities>, ApiError> {
    let owner = current_user_id(&headers, &state).await?;
    let paused: bool = sqlx::query_scalar(
        "SELECT COALESCE((SELECT rig_paused FROM engine_rollouts WHERE owner_id=?),0)",
    )
    .bind(owner)
    .fetch_one(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to load engine rollout"))?;
    Ok(Json(Capabilities {
        rig_available: cfg!(feature = "rig-engine"),
        default_engine: "native",
        rig_paused: paused,
        app_version: env!("CARGO_PKG_VERSION"),
        rig_version: crate::runtime::engine_observation::RIG_VERSION,
    }))
}

#[derive(Deserialize)]
pub struct PauseRequest {
    paused: bool,
}
pub async fn pause(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PauseRequest>,
) -> Result<Json<Capabilities>, ApiError> {
    let owner = current_user_id(&headers, &state).await?;
    let _guard = state.write_lock.lock().await;
    sqlx::query("INSERT INTO engine_rollouts(owner_id,rig_paused,updated_at) VALUES(?,?,?) ON CONFLICT(owner_id) DO UPDATE SET rig_paused=excluded.rig_paused,updated_at=excluded.updated_at")
        .bind(&owner).bind(body.paused).bind(super::commands::now()).execute(state.db.pool()).await.map_err(|_|ApiError::internal("failed to save engine rollout"))?;
    drop(_guard);
    get(State(state), headers).await
}

#[derive(Default, Deserialize)]
pub struct MetricsQuery {
    days: Option<u16>,
    agent_id: Option<String>,
    provider_id: Option<String>,
    model: Option<String>,
}
#[derive(Serialize, sqlx::FromRow)]
pub struct EngineMetrics {
    engine: String,
    app_version: String,
    engine_version: String,
    provider_id: Option<String>,
    provider_kind: String,
    model: String,
    executions: i64,
    partial_executions: i64,
    completed: i64,
    failed: i64,
    cancelled: i64,
    interrupted: i64,
    pending: i64,
    blocked: i64,
    outcome_unknown: i64,
    active_duration_ms: i64,
    tool_calls: i64,
    settled_tool_calls: i64,
    tool_errors: i64,
    unsettled_tools: i64,
    total_tokens: i64,
    usage_calls: i64,
    executions_without_usage: i64,
    #[sqlx(skip)]
    failure_rate: Option<f64>,
    #[sqlx(skip)]
    tool_error_rate: Option<f64>,
}
#[derive(Serialize)]
pub struct MetricsResponse {
    days: u16,
    rows: Vec<EngineMetrics>,
    unattributed_calls: i64,
}

pub async fn metrics(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<MetricsQuery>,
) -> Result<Json<MetricsResponse>, ApiError> {
    let owner = current_user_id(&headers, &state).await?;
    let days = query.days.unwrap_or(7);
    if !(1..=90).contains(&days) {
        return Err(ApiError::invalid_input("days must be between 1 and 90"));
    }
    let since = (time::OffsetDateTime::now_utc() - time::Duration::days(days.into()))
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|_| ApiError::internal("invalid metrics window"))?;
    // Aggregate each ledger before joining: resumes update one execution, and
    // repeated usage gauges upsert one call rather than multiplying its cost.
    let mut rows=sqlx::query_as::<_,EngineMetrics>(r#"
        SELECT e.engine,e.app_version,e.engine_version,e.provider_id,e.provider_kind,e.model,
            COUNT(*) AS executions,
            SUM(e.coverage_complete=0) AS partial_executions,
            SUM(e.status='completed' AND e.coverage_complete=1) AS completed,
            SUM(e.status='failed' AND e.coverage_complete=1) AS failed,
            SUM(e.status='cancelled') AS cancelled,
            SUM(e.status='interrupted') AS interrupted,
            SUM(e.status IN ('running','waiting_for_user')) AS pending,
            SUM(e.status='blocked') AS blocked,
            SUM(e.status='outcome_unknown') AS outcome_unknown,
            SUM(CASE WHEN e.coverage_complete=1 THEN e.active_duration_ms ELSE 0 END) AS active_duration_ms,
            SUM(CASE WHEN e.coverage_complete=1 THEN e.tool_calls ELSE 0 END) AS tool_calls,
            SUM(CASE WHEN e.coverage_complete=1 THEN e.settled_tool_calls ELSE 0 END) AS settled_tool_calls,
            SUM(CASE WHEN e.coverage_complete=1 THEN e.tool_errors ELSE 0 END) AS tool_errors,
            SUM(e.unsettled_tools) AS unsettled_tools,
            SUM(CASE WHEN e.coverage_complete=1 THEN COALESCE(t.total_tokens,0) ELSE 0 END) AS total_tokens,
            SUM(CASE WHEN e.coverage_complete=1 THEN COALESCE(t.usage_calls,0) ELSE 0 END) AS usage_calls,
            SUM(e.coverage_complete=1 AND e.status='completed' AND COALESCE(t.usage_calls,0)=0) AS executions_without_usage
        FROM engine_executions e
        LEFT JOIN (
            SELECT execution_id,SUM(total_tokens) AS total_tokens,COUNT(*) AS usage_calls
            FROM token_usage_records WHERE owner_id=? AND execution_id IS NOT NULL GROUP BY execution_id
        ) t ON t.execution_id=e.id
        WHERE e.owner_id=? AND e.started_at>=?
            AND (? IS NULL OR e.agent_id=?) AND (? IS NULL OR e.provider_id=?) AND (? IS NULL OR e.model=?)
        GROUP BY e.engine,e.app_version,e.engine_version,e.provider_id,e.provider_kind,e.model
        ORDER BY e.engine,e.app_version,e.provider_kind,e.model
    "#)
        .bind(&owner).bind(&owner).bind(&since).bind(&query.agent_id).bind(&query.agent_id).bind(&query.provider_id).bind(&query.provider_id).bind(&query.model).bind(&query.model)
        .fetch_all(state.db.pool()).await.map_err(|_|ApiError::internal("failed to load engine metrics"))?;
    for row in &mut rows {
        let terminal = row.completed + row.failed;
        row.failure_rate = (terminal > 0).then(|| row.failed as f64 / terminal as f64);
        row.tool_error_rate = (row.settled_tool_calls > 0)
            .then(|| row.tool_errors as f64 / row.settled_tool_calls as f64);
    }
    let unattributed_calls=sqlx::query_scalar("SELECT COUNT(*) FROM token_usage_records WHERE owner_id=? AND created_at>=? AND engine IS NULL")
        .bind(owner).bind(since).fetch_one(state.db.pool()).await.map_err(|_|ApiError::internal("failed to load ledger coverage"))?;
    Ok(Json(MetricsResponse {
        days,
        rows,
        unattributed_calls,
    }))
}
