//! Scheduler finalization, preserving the existing commit and delivery order.
//!
//! Note: waiting-for-user is resumable and stays with the coordinator. Normal
//! completion uses its already-saved budget; cancellation reads the durable
//! snapshot. Do not unify their event mappings or overwrite an existing final
//! status on error. See specs/scheduler-terminal-contract.md.

use super::{turn_budget_payload, Cancelled, StepErr, StreamCtx, TurnOutcome};
use crate::runtime::group_scheduler::{SchedulerStore, TurnBudget, TurnReason, TurnStatus};
use qunica_domain::events::StreamEventKind;
use serde_json::json;

/// The caller has already settled usage and committed the final budget.
pub(super) async fn finish_scheduled_turn(
    ctx: &mut StreamCtx,
    store: &SchedulerStore,
    turn_id: &str,
    status: TurnStatus,
    reason: TurnReason,
    budget: &TurnBudget,
    had_visible: bool,
) -> Result<TurnOutcome, Cancelled> {
    let (status, reason, outcome) = match status {
        TurnStatus::Silence if had_visible => (TurnStatus::Completed, None, TurnOutcome::Completed),
        // Nothing was said because a dispatch failed, not because the
        // agents chose to stay quiet. Filing that as "completed
        // silently" puts a green check on a provider fault.
        TurnStatus::Silence if budget.total_failures() > 0 => {
            (TurnStatus::Failed, None, TurnOutcome::Error)
        }
        TurnStatus::Completed => (TurnStatus::Completed, Some(reason), TurnOutcome::Completed),
        _ => (status, Some(reason), TurnOutcome::Silence),
    };
    if let Err(_error) = store
        .transition_turn(
            turn_id,
            TurnStatus::Running,
            status,
            reason.map(TurnReason::as_str),
        )
        .await
    {
        tracing::error!(turn_id, "failed to persist scheduled turn completion");
        return fail_scheduled_persistence(ctx, store, turn_id).await;
    }
    if matches!(outcome, TurnOutcome::Silence) {
        match ctx
            .emit_durable_event(
                StreamEventKind::Silence,
                json!({
                    "turn_id": turn_id,
                }),
            )
            .await
        {
            Ok(()) => {}
            Err(StepErr::Cancelled) => return Ok(TurnOutcome::Cancelled),
            Err(StepErr::Db(_) | StepErr::SchedulerPersistence) => {
                return fail_scheduled_persistence(ctx, store, turn_id).await;
            }
        }
    }
    if let Err(error) = emit_turn_terminal(ctx, turn_id, status, reason, budget).await {
        return match error {
            StepErr::Cancelled => Ok(TurnOutcome::Cancelled),
            StepErr::Db(_) | StepErr::SchedulerPersistence => {
                fail_scheduled_persistence(ctx, store, turn_id).await
            }
        };
    }
    Ok(outcome)
}

pub(super) async fn emit_turn_terminal(
    ctx: &mut StreamCtx,
    turn_id: &str,
    status: TurnStatus,
    reason: Option<TurnReason>,
    budget: &TurnBudget,
) -> Result<(), StepErr> {
    let kind = match status {
        TurnStatus::BudgetExhausted | TurnStatus::FailureBudgetExhausted => {
            StreamEventKind::TurnBudgetExhausted
        }
        TurnStatus::Cancelled => StreamEventKind::TurnCancelled,
        TurnStatus::Superseded => StreamEventKind::TurnSuperseded,
        // A queued turn has not started, so it cannot have produced a terminal
        // event; reaching this path would mean the scheduler skipped a claim.
        TurnStatus::Queued | TurnStatus::Pending | TurnStatus::Running => {
            return Err(StepErr::SchedulerPersistence);
        }
        TurnStatus::WaitingForUser
        | TurnStatus::Completed
        | TurnStatus::Silence
        | TurnStatus::Failed => StreamEventKind::TurnCompleted,
    };
    ctx.emit_scheduler_terminal(
        kind,
        json!({
            "turn_id": turn_id,
            "status": status.as_str(),
            "reason": reason.map(TurnReason::as_str),
            "budget": turn_budget_payload(budget),
        }),
        turn_id,
    )
    .await
}

pub(super) async fn cancel_scheduled_turn(
    ctx: &mut StreamCtx,
    store: &SchedulerStore,
    turn_id: &str,
) -> Result<TurnOutcome, Cancelled> {
    ctx.scheduled_dispatch = None;
    let turn = match store.cancel_turn(turn_id).await {
        Ok(turn) => turn,
        Err(_error) => {
            tracing::error!(turn_id, "failed to persist cancelled scheduler turn status");
            return fail_scheduled_persistence(ctx, store, turn_id).await;
        }
    };
    let kind = match turn.status {
        TurnStatus::Cancelled => StreamEventKind::TurnCancelled,
        TurnStatus::Superseded => StreamEventKind::TurnSuperseded,
        _ if is_terminal_scheduler_turn(turn.status) => StreamEventKind::TurnCompleted,
        _ => {
            tracing::error!(
                turn_id,
                status = turn.status.as_str(),
                "scheduler cancellation left a non-terminal turn"
            );
            return fail_scheduled_persistence(ctx, store, turn_id).await;
        }
    };
    if ctx
        .emit_scheduler_terminal(
            kind,
            json!({
                "turn_id": turn.id,
                "status": turn.status.as_str(),
                "reason": turn.termination_reason.map(TurnReason::as_str),
                "budget": {
                    "agent_steps": turn.agent_steps,
                    "moderator_calls": turn.moderator_calls,
                    "consecutive_failures": turn.consecutive_failures,
                    "total_failures": turn.total_failures,
                    "total_tokens": turn.total_tokens,
                },
            }),
            turn_id,
        )
        .await
        .is_err()
    {
        return fail_scheduled_persistence(ctx, store, turn_id).await;
    }
    Ok(TurnOutcome::Cancelled)
}

fn is_terminal_scheduler_turn(status: TurnStatus) -> bool {
    matches!(
        status,
        TurnStatus::Completed
            | TurnStatus::Silence
            | TurnStatus::BudgetExhausted
            | TurnStatus::FailureBudgetExhausted
            | TurnStatus::Cancelled
            | TurnStatus::Superseded
            | TurnStatus::Failed
    )
}

pub(super) async fn fail_scheduled_persistence(
    ctx: &mut StreamCtx,
    store: &SchedulerStore,
    turn_id: &str,
) -> Result<TurnOutcome, Cancelled> {
    tracing::error!(turn_id, "scheduler persistence operation failed");
    match store.load_turn_trace(turn_id).await {
        Ok(trace) if !is_terminal_scheduler_turn(trace.turn.status) => {
            if store
                .transition_turn(
                    turn_id,
                    trace.turn.status,
                    TurnStatus::Failed,
                    Some(TurnReason::PersistenceFailed.as_str()),
                )
                .await
                .is_err()
            {
                tracing::error!(
                    turn_id,
                    "failed to terminalize scheduler turn after persistence error"
                );
            }
        }
        Ok(_) => {}
        Err(_) => {
            tracing::error!(
                turn_id,
                "failed to load scheduler turn after persistence error"
            );
        }
    }
    ctx.fail("scheduler persistence failed").await
}
