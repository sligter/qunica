//! Scheduler turn trace endpoints.
//!
//! Trace data is scoped to the owning active group. Dispatch artifacts are
//! reduced to an explicit public allowlist before the response is serialized.

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde::Serialize;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::{
    api::{auth::current_user_id, error::ApiError, AppState},
    runtime::group_scheduler::{
        DispatchSnapshot, SchedulerStore, TurnSnapshot, TurnStatus, TurnTrace,
    },
};

#[derive(Debug, Serialize)]
pub struct GroupTurnTraceResponse {
    turn: TurnSnapshot,
    budget: TurnBudgetResponse,
    dispatches: Vec<DispatchSnapshot>,
    estimated_cost: Option<EstimatedCostResponse>,
    cost_estimation_status: CostEstimationStatus,
}

#[derive(Debug, Serialize)]
struct TurnBudgetResponse {
    agent_steps: i64,
    moderator_calls: i64,
    consecutive_failures: i64,
    total_failures: i64,
    total_tokens: i64,
}

#[derive(Debug, Serialize)]
struct EstimatedCostResponse {
    amount: String,
    currency: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum CostEstimationStatus {
    Unavailable,
}

/// Return an owner-scoped, public scheduler trace in durable dispatch creation
/// order. Provider pricing is not configured yet, so cost remains null.
pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, turn_id)): Path<(String, String)>,
) -> Result<Json<GroupTurnTraceResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let turn_id = validate_uuid(&turn_id, "turn id")?;
    ensure_active_owned_group(state.db.pool(), &group_id, &owner_id).await?;

    let store = SchedulerStore::new(state.db.pool().clone(), state.write_lock.clone());
    let trace = store
        .load_turn_trace(&turn_id)
        .await
        .map_err(|error| match error {
            crate::runtime::group_scheduler::SchedulerStoreError::NotFound { .. } => {
                ApiError::not_found("turn not found")
            }
            _ => ApiError::internal("failed to load turn trace"),
        })?;
    if trace.turn.group_id != group_id {
        return Err(ApiError::not_found("turn not found"));
    }

    Ok(Json(trace_response(trace)))
}

/// Idempotently cancel an owner-scoped scheduler turn. Persistent state is
/// finalized before its matching in-memory turn token is signalled.
pub async fn cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, turn_id)): Path<(String, String)>,
) -> Result<Json<GroupTurnTraceResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let group_id = validate_uuid(&group_id, "group id")?;
    let turn_id = validate_uuid(&turn_id, "turn id")?;
    ensure_active_owned_group(state.db.pool(), &group_id, &owner_id).await?;

    let store = SchedulerStore::new(state.db.pool().clone(), state.write_lock.clone());
    let existing = store
        .load_turn_trace(&turn_id)
        .await
        .map_err(map_trace_error)?;
    if existing.turn.group_id != group_id {
        return Err(ApiError::not_found("turn not found"));
    }
    let was_active = matches!(
        existing.turn.status,
        TurnStatus::Pending | TurnStatus::Running | TurnStatus::WaitingForUser
    );
    let turn = store.cancel_turn(&turn_id).await.map_err(map_trace_error)?;
    if was_active {
        state.active_turns.cancel(&turn.thread_id, &turn.id).await;
    }
    let trace = store
        .load_turn_trace(&turn_id)
        .await
        .map_err(map_trace_error)?;
    Ok(Json(trace_response(trace)))
}

fn trace_response(trace: TurnTrace) -> GroupTurnTraceResponse {
    let budget = TurnBudgetResponse {
        agent_steps: trace.turn.agent_steps,
        moderator_calls: trace.turn.moderator_calls,
        consecutive_failures: trace.turn.consecutive_failures,
        total_failures: trace.turn.total_failures,
        total_tokens: trace.turn.total_tokens,
    };
    let dispatches = trace
        .dispatches
        .into_iter()
        .map(|mut dispatch| {
            dispatch.artifact = public_artifact(dispatch.artifact.as_ref());
            dispatch
        })
        .collect();

    GroupTurnTraceResponse {
        turn: trace.turn,
        budget,
        dispatches,
        estimated_cost: None,
        cost_estimation_status: CostEstimationStatus::Unavailable,
    }
}

fn map_trace_error(error: crate::runtime::group_scheduler::SchedulerStoreError) -> ApiError {
    match error {
        crate::runtime::group_scheduler::SchedulerStoreError::NotFound { .. } => {
            ApiError::not_found("turn not found")
        }
        _ => ApiError::internal("failed to load turn trace"),
    }
}

async fn ensure_active_owned_group(
    pool: &sqlx::SqlitePool,
    group_id: &str,
    owner_id: &str,
) -> Result<(), ApiError> {
    let group: Option<(String, String)> =
        sqlx::query_as("SELECT owner_id, status FROM groups WHERE id = ?")
            .bind(group_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| ApiError::internal("database error"))?;
    let Some((actual_owner_id, status)) = group else {
        return Err(ApiError::not_found("group not found"));
    };
    if status != "active" {
        return Err(ApiError::not_found("group not found"));
    }
    if actual_owner_id != owner_id {
        return Err(ApiError::permission_denied("group belongs to another user"));
    }
    Ok(())
}

fn public_artifact(artifact: Option<&Value>) -> Option<Value> {
    let Value::Object(artifact) = artifact? else {
        return None;
    };

    // The scheduler may persist private tool data or a helper's final content
    // in an artifact. Only these routing fields are meaningful in a public
    // trace and none carry model reasoning or private tool input/output.
    let mut public = Map::new();
    for key in [
        "mode",
        "target_agent_id",
        "child_dispatch_id",
        "outcome",
        "failure_code",
    ] {
        if let Some(value) = artifact.get(key) {
            public.insert(key.to_owned(), value.clone());
        }
    }
    (!public.is_empty()).then_some(Value::Object(public))
}

fn validate_uuid(raw: &str, field: &str) -> Result<String, ApiError> {
    Uuid::parse_str(raw.trim())
        .map(|id| id.to_string())
        .map_err(|_| ApiError::invalid_input(format!("invalid {field}")))
}

#[cfg(test)]
mod tests {
    use super::public_artifact;
    use serde_json::json;

    #[test]
    fn trace_artifact_allowlist_omits_private_content() {
        assert_eq!(
            public_artifact(Some(&json!({
                "mode": "handoff",
                "target_agent_id": "agent-1",
                "final_content": "private response",
                "reasoning": ["hidden"],
                "tool_io": { "secret": "no" },
            }))),
            Some(json!({
                "mode": "handoff",
                "target_agent_id": "agent-1",
            }))
        );
    }
}
