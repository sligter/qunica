use std::{sync::Arc, time::Duration};

use serde_json::Value;
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};
use thiserror::Error;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::Mutex;

use qunica_domain::events::StreamEventKind;

use crate::db::begin_write;
use crate::runtime::sequence::persist_message_with_event_in_tx;

use super::{
    model::{
        ActionKind, DispatchSnapshot, FinishDispatch, NewDispatch, NewTurn, SchedulerModelError,
        SelectionReason, TurnReason, TurnSnapshot, TurnTrace,
    },
    state::{
        validate_dispatch_transition, validate_turn_transition, DispatchStatus,
        SchedulerStateError, TurnStatus,
    },
};

const SQLITE_WRITE_ATTEMPTS: usize = 3;
const SQLITE_WRITE_RETRY_DELAY: Duration = Duration::from_millis(25);

#[derive(Clone)]
pub struct SchedulerStore {
    pool: SqlitePool,
    write_lock: Arc<Mutex<()>>,
}

impl SchedulerStore {
    pub fn new(pool: SqlitePool, write_lock: Arc<Mutex<()>>) -> Self {
        Self { pool, write_lock }
    }

    /// Supersede the thread's active turn (if any) and create the replacement in
    /// one transaction.
    ///
    /// Doing this as two calls would let two concurrent sends both observe "no
    /// active turn" and race on the partial unique index, failing the later send
    /// instead of letting it take over.
    pub async fn supersede_and_create_turn(
        &self,
        input: NewTurn,
    ) -> Result<(Option<TurnSnapshot>, TurnSnapshot), SchedulerStoreError> {
        let config_snapshot_json = serde_json::to_string(&input.config_snapshot)?;
        let topology_snapshot_json = serde_json::to_string(&input.topology_snapshot)?;
        let now = now_rfc3339();
        let _guard = self.write_lock.lock().await;
        let mut tx = begin_write(&self.pool).await?;

        let superseded = supersede_active_turn_in_tx(&mut tx, &input.thread_id, &now).await?;
        let created = insert_turn_in_tx(
            &mut tx,
            &input,
            &config_snapshot_json,
            &topology_snapshot_json,
            &now,
        )
        .await?;
        tx.commit().await?;
        Ok((superseded, created))
    }

    pub async fn create_turn(&self, input: NewTurn) -> Result<TurnSnapshot, SchedulerStoreError> {
        let config_snapshot_json = serde_json::to_string(&input.config_snapshot)?;
        let topology_snapshot_json = serde_json::to_string(&input.topology_snapshot)?;
        let now = now_rfc3339();
        let _guard = self.write_lock.lock().await;
        let mut tx = begin_write(&self.pool).await?;
        let snapshot = insert_turn_in_tx(
            &mut tx,
            &input,
            &config_snapshot_json,
            &topology_snapshot_json,
            &now,
        )
        .await?;
        tx.commit().await?;
        Ok(snapshot)
    }

    pub async fn transition_turn(
        &self,
        turn_id: &str,
        expected: TurnStatus,
        next: TurnStatus,
        reason: Option<&str>,
    ) -> Result<TurnSnapshot, SchedulerStoreError> {
        let reason = reason.map(TurnReason::try_from).transpose()?;
        let now = now_rfc3339();
        let _guard = self.write_lock.lock().await;
        let mut tx = begin_write(&self.pool).await?;
        let snapshot =
            transition_turn_in_tx(&mut tx, turn_id, expected, next, reason, &now).await?;
        tx.commit().await?;
        Ok(snapshot)
    }

    /// Idempotently mark one active turn as user-cancelled. The persistent
    /// state changes before the caller signals its in-memory cancellation
    /// token, preventing further dispatch output from committing.
    pub async fn cancel_turn(&self, turn_id: &str) -> Result<TurnSnapshot, SchedulerStoreError> {
        let now = now_rfc3339();
        let _guard = self.write_lock.lock().await;
        let mut tx = begin_write(&self.pool).await?;
        let current = fetch_turn_in_tx(&mut tx, turn_id).await?;
        let snapshot = if is_terminal_turn(current.status) {
            current
        } else {
            transition_turn_in_tx(
                &mut tx,
                turn_id,
                current.status,
                TurnStatus::Cancelled,
                Some(TurnReason::UserCancelled),
                &now,
            )
            .await?
        };
        tx.commit().await?;
        Ok(snapshot)
    }

    /// Supersede the one durable active turn for a thread, if one exists.
    ///
    /// This is intentionally separate from registry signalling: callers must
    /// persist the superseded state first, then signal the matching token.
    pub async fn supersede_active_turn_for_thread(
        &self,
        thread_id: &str,
    ) -> Result<Option<TurnSnapshot>, SchedulerStoreError> {
        let now = now_rfc3339();
        let _guard = self.write_lock.lock().await;
        let mut tx = begin_write(&self.pool).await?;
        let snapshot = supersede_active_turn_in_tx(&mut tx, thread_id, &now).await?;
        tx.commit().await?;
        Ok(snapshot)
    }

    pub async fn queue_dispatch(
        &self,
        input: NewDispatch,
    ) -> Result<DispatchSnapshot, SchedulerStoreError> {
        if input.hop < 0 {
            return Err(SchedulerStoreError::InvalidInput(
                "dispatch hop must be non-negative".to_owned(),
            ));
        }
        let now = now_rfc3339();
        let _guard = self.write_lock.lock().await;
        let mut tx = begin_write(&self.pool).await?;

        let result = sqlx::query(
            "INSERT INTO agent_dispatches \
             (id, turn_id, parent_dispatch_id, source_agent_id, target_agent_id, \
              selection_reason, action_kind, hop, status, input_message_id, created_at, updated_at) \
             SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ? \
             FROM agents WHERE id = ? AND status = 'active'",
        )
        .bind(&input.id)
        .bind(&input.turn_id)
        .bind(&input.parent_dispatch_id)
        .bind(&input.source_agent_id)
        .bind(&input.target_agent_id)
        .bind(input.selection_reason.as_str())
        .bind(input.action_kind.as_str())
        .bind(input.hop)
        .bind(DispatchStatus::Queued.as_str())
        .bind(&input.input_message_id)
        .bind(&now)
        .bind(&now)
        .bind(&input.target_agent_id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(SchedulerStoreError::InvalidInput(
                "dispatch target agent is inactive".to_owned(),
            ));
        }

        let snapshot = fetch_dispatch_in_tx(&mut tx, &input.id).await?;
        tx.commit().await?;
        Ok(snapshot)
    }

    pub async fn start_dispatch(
        &self,
        dispatch_id: &str,
    ) -> Result<DispatchSnapshot, SchedulerStoreError> {
        validate_dispatch_transition(DispatchStatus::Queued, DispatchStatus::Running)?;
        let now = now_rfc3339();
        let _guard = self.write_lock.lock().await;
        let mut tx = begin_write(&self.pool).await?;

        let result = sqlx::query(
            "UPDATE agent_dispatches \
             SET status = ?, started_at = COALESCE(started_at, ?), updated_at = ? \
             WHERE id = ? AND status = ?",
        )
        .bind(DispatchStatus::Running.as_str())
        .bind(&now)
        .bind(&now)
        .bind(dispatch_id)
        .bind(DispatchStatus::Queued.as_str())
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() == 0 {
            return Err(
                dispatch_transition_conflict(&mut tx, dispatch_id, DispatchStatus::Queued).await?,
            );
        }

        let snapshot = fetch_dispatch_in_tx(&mut tx, dispatch_id).await?;
        tx.commit().await?;
        Ok(snapshot)
    }

    pub async fn finish_dispatch(
        &self,
        input: FinishDispatch,
    ) -> Result<DispatchSnapshot, SchedulerStoreError> {
        validate_dispatch_transition(DispatchStatus::Running, input.next)?;
        if input.total_tokens < 0 {
            return Err(SchedulerStoreError::InvalidInput(
                "dispatch total_tokens must be non-negative".to_owned(),
            ));
        }
        if let Some(output) = input.output.as_ref() {
            validate_dispatch_output(input.next, output)?;
        }
        let artifact_json = input
            .artifact
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;

        for attempt in 1..=SQLITE_WRITE_ATTEMPTS {
            match self
                .finish_dispatch_once(&input, artifact_json.as_deref())
                .await
            {
                Err(error)
                    if attempt < SQLITE_WRITE_ATTEMPTS && is_transient_sqlite_lock(&error) =>
                {
                    tracing::warn!(
                        dispatch_id = %input.dispatch_id,
                        attempt,
                        error = %error,
                        "retrying scheduler dispatch persistence after SQLite lock"
                    );
                    tokio::time::sleep(SQLITE_WRITE_RETRY_DELAY).await;
                }
                Err(error) => {
                    if matches!(
                        error,
                        SchedulerStoreError::Database(_) | SchedulerStoreError::Persistence(_)
                    ) {
                        tracing::error!(
                            dispatch_id = %input.dispatch_id,
                            error = %error,
                            "failed to persist finished scheduler dispatch"
                        );
                    }
                    return Err(error);
                }
                Ok(snapshot) => return Ok(snapshot),
            }
        }
        unreachable!("scheduler dispatch retry loop always returns")
    }

    async fn finish_dispatch_once(
        &self,
        input: &FinishDispatch,
        artifact_json: Option<&str>,
    ) -> Result<DispatchSnapshot, SchedulerStoreError> {
        let now = now_rfc3339();
        let _guard = self.write_lock.lock().await;
        let mut tx = begin_write(&self.pool).await?;

        let result = sqlx::query(
            "UPDATE agent_dispatches \
             SET status = ?, artifact_json = ?, total_tokens = ?, failure_code = ?, \
                 completed_at = ?, updated_at = ? \
             WHERE id = ? AND status = ?",
        )
        .bind(input.next.as_str())
        .bind(artifact_json)
        .bind(input.total_tokens)
        .bind(&input.failure_code)
        .bind(&now)
        .bind(&now)
        .bind(&input.dispatch_id)
        .bind(DispatchStatus::Running.as_str())
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() == 0 {
            return Err(dispatch_transition_conflict(
                &mut tx,
                &input.dispatch_id,
                DispatchStatus::Running,
            )
            .await?);
        }

        if let Some(output) = input.output.as_ref() {
            let (turn_id, thread_id, group_id, turn_status): (String, String, String, String) =
                sqlx::query_as(
                    "SELECT d.turn_id, t.thread_id, t.group_id, t.status \
                 FROM agent_dispatches d \
                 JOIN group_turns t ON t.id = d.turn_id \
                 WHERE d.id = ?",
                )
                .bind(&input.dispatch_id)
                .fetch_one(&mut *tx)
                .await?;
            let turn_status = TurnStatus::try_from(turn_status.as_str())?;
            if turn_status != TurnStatus::Running {
                return Err(SchedulerStoreError::InactiveTurn {
                    turn_id,
                    status: turn_status,
                });
            }
            if output.thread_id != thread_id || output.group_id != group_id {
                return Err(SchedulerStoreError::InvalidInput(
                    "dispatch output thread/group does not match its turn".to_owned(),
                ));
            }

            persist_message_with_event_in_tx(
                &mut tx,
                &output.thread_id,
                &output.group_id,
                &output.message,
                &output.event,
            )
            .await?;

            sqlx::query(
                "UPDATE messages \
                 SET turn_id = ?, dispatch_id = ?, \
                     reply_to_message_id = ( \
                         SELECT COALESCE(dispatch.input_message_id, parent.output_message_id) \
                         FROM agent_dispatches dispatch \
                         LEFT JOIN agent_dispatches parent ON parent.id = dispatch.parent_dispatch_id \
                         WHERE dispatch.id = ? \
                     ) \
                 WHERE id = ?",
            )
                .bind(&turn_id)
                .bind(&input.dispatch_id)
                .bind(&input.dispatch_id)
                .bind(&output.message.id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE agent_dispatches SET output_message_id = ? WHERE id = ?")
                .bind(&output.message.id)
                .bind(&input.dispatch_id)
                .execute(&mut *tx)
                .await?;
        }

        let snapshot = fetch_dispatch_in_tx(&mut tx, &input.dispatch_id).await?;
        tx.commit().await?;
        Ok(snapshot)
    }

    pub async fn cancel_queued_dispatches(
        &self,
        turn_id: &str,
    ) -> Result<u64, SchedulerStoreError> {
        validate_dispatch_transition(DispatchStatus::Queued, DispatchStatus::Cancelled)?;
        let now = now_rfc3339();
        let _guard = self.write_lock.lock().await;
        let mut tx = begin_write(&self.pool).await?;
        let result = sqlx::query(
            "UPDATE agent_dispatches \
             SET status = ?, completed_at = ?, updated_at = ? \
             WHERE turn_id = ? AND status = ?",
        )
        .bind(DispatchStatus::Cancelled.as_str())
        .bind(&now)
        .bind(&now)
        .bind(turn_id)
        .bind(DispatchStatus::Queued.as_str())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(result.rows_affected())
    }

    pub async fn recover_incomplete_turns(&self) -> Result<(), SchedulerStoreError> {
        validate_dispatch_transition(DispatchStatus::Queued, DispatchStatus::Cancelled)?;
        validate_dispatch_transition(DispatchStatus::Running, DispatchStatus::Interrupted)?;
        validate_turn_transition(TurnStatus::Pending, TurnStatus::Failed)?;
        validate_turn_transition(TurnStatus::Running, TurnStatus::Failed)?;
        validate_turn_transition(TurnStatus::WaitingForUser, TurnStatus::Failed)?;

        let now = now_rfc3339();
        let _guard = self.write_lock.lock().await;
        let mut tx = begin_write(&self.pool).await?;

        sqlx::query(
            "UPDATE agent_dispatches \
             SET status = CASE \
                     WHEN status = 'running' THEN 'interrupted' \
                     ELSE 'cancelled' \
                 END, \
                 completed_at = COALESCE(completed_at, ?), updated_at = ? \
             WHERE status IN ('queued', 'running')",
        )
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "UPDATE group_turns \
             SET status = ?, termination_reason = ?, \
                 completed_at = COALESCE(completed_at, ?), updated_at = ? \
             WHERE status IN ('pending', 'running', 'waiting_for_user')",
        )
        .bind(TurnStatus::Failed.as_str())
        .bind(TurnReason::ServerRestart.as_str())
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        // Persistence failures end a turn, not its reusable task thread. Also
        // repair legacy rows that coupled the two states.
        sqlx::query(
            "UPDATE threads \
             SET status = CASE \
                     WHEN EXISTS ( \
                         SELECT 1 FROM messages \
                         WHERE messages.thread_id = threads.id \
                           AND messages.status = 'interrupted' \
                     ) THEN 'paused' \
                     ELSE 'active' \
                 END, \
                 updated_at = ? \
             WHERE status IN ('running', 'failed')",
        )
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn update_turn_budget(
        &self,
        turn_id: &str,
        agent_steps: i64,
        moderator_calls: i64,
        consecutive_failures: i64,
        total_failures: i64,
        total_tokens: i64,
    ) -> Result<TurnSnapshot, SchedulerStoreError> {
        if [
            agent_steps,
            moderator_calls,
            consecutive_failures,
            total_failures,
            total_tokens,
        ]
        .iter()
        .any(|value| *value < 0)
        {
            return Err(SchedulerStoreError::InvalidInput(
                "turn budget counters must be non-negative".to_owned(),
            ));
        }
        let now = now_rfc3339();
        let _guard = self.write_lock.lock().await;
        let mut tx = begin_write(&self.pool).await?;
        let result = sqlx::query(
            "UPDATE group_turns \
             SET agent_steps = ?, moderator_calls = ?, consecutive_failures = ?, \
                 total_failures = ?, total_tokens = ?, updated_at = ? \
             WHERE id = ? AND status = ?",
        )
        .bind(agent_steps)
        .bind(moderator_calls)
        .bind(consecutive_failures)
        .bind(total_failures)
        .bind(total_tokens)
        .bind(&now)
        .bind(turn_id)
        .bind(TurnStatus::Running.as_str())
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(turn_transition_conflict(&mut tx, turn_id, TurnStatus::Running).await?);
        }
        let snapshot = fetch_turn_in_tx(&mut tx, turn_id).await?;
        tx.commit().await?;
        Ok(snapshot)
    }

    pub async fn load_turn_trace(&self, turn_id: &str) -> Result<TurnTrace, SchedulerStoreError> {
        // Read-only: a deferred transaction is right here, and taking the writer
        // would serialize trace reads behind every scheduler write.
        let mut tx = self.pool.begin().await?;
        let turn = fetch_turn_in_tx(&mut tx, turn_id).await?;
        let rows = sqlx::query_as::<_, DispatchRow>(
            "SELECT id, turn_id, parent_dispatch_id, source_agent_id, target_agent_id, \
                    selection_reason, action_kind, hop, status, input_message_id, \
                    output_message_id, artifact_json, total_tokens, failure_code, \
                    created_at, started_at, completed_at, updated_at \
             FROM agent_dispatches WHERE turn_id = ? ORDER BY created_at, rowid",
        )
        .bind(turn_id)
        .fetch_all(&mut *tx)
        .await?;
        let dispatches = rows
            .into_iter()
            .map(DispatchSnapshot::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        tx.commit().await?;
        Ok(TurnTrace { turn, dispatches })
    }
}

#[derive(FromRow)]
struct TurnRow {
    id: String,
    thread_id: String,
    group_id: String,
    trigger_message_id: Option<String>,
    status: String,
    scheduler_strategy: String,
    config_snapshot_json: String,
    topology_snapshot_json: String,
    agent_steps: i64,
    moderator_calls: i64,
    consecutive_failures: i64,
    total_failures: i64,
    total_tokens: i64,
    termination_reason: Option<String>,
    created_at: String,
    started_at: Option<String>,
    completed_at: Option<String>,
    updated_at: String,
}

#[derive(FromRow)]
struct DispatchRow {
    id: String,
    turn_id: String,
    parent_dispatch_id: Option<String>,
    source_agent_id: Option<String>,
    target_agent_id: String,
    selection_reason: String,
    action_kind: String,
    hop: i64,
    status: String,
    input_message_id: Option<String>,
    output_message_id: Option<String>,
    artifact_json: Option<String>,
    total_tokens: i64,
    failure_code: Option<String>,
    created_at: String,
    started_at: Option<String>,
    completed_at: Option<String>,
    updated_at: String,
}

impl TryFrom<TurnRow> for TurnSnapshot {
    type Error = SchedulerStoreError;

    fn try_from(row: TurnRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            thread_id: row.thread_id,
            group_id: row.group_id,
            trigger_message_id: row.trigger_message_id,
            status: TurnStatus::try_from(row.status.as_str())?,
            scheduler_strategy: row.scheduler_strategy,
            config_snapshot: serde_json::from_str(&row.config_snapshot_json)?,
            topology_snapshot: serde_json::from_str(&row.topology_snapshot_json)?,
            agent_steps: row.agent_steps,
            moderator_calls: row.moderator_calls,
            consecutive_failures: row.consecutive_failures,
            total_failures: row.total_failures,
            total_tokens: row.total_tokens,
            termination_reason: row
                .termination_reason
                .as_deref()
                .map(TurnReason::try_from)
                .transpose()?,
            created_at: row.created_at,
            started_at: row.started_at,
            completed_at: row.completed_at,
            updated_at: row.updated_at,
        })
    }
}

impl TryFrom<DispatchRow> for DispatchSnapshot {
    type Error = SchedulerStoreError;

    fn try_from(row: DispatchRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            turn_id: row.turn_id,
            parent_dispatch_id: row.parent_dispatch_id,
            source_agent_id: row.source_agent_id,
            target_agent_id: row.target_agent_id,
            selection_reason: SelectionReason::try_from(row.selection_reason.as_str())?,
            action_kind: ActionKind::try_from(row.action_kind.as_str())?,
            hop: row.hop,
            status: DispatchStatus::try_from(row.status.as_str())?,
            input_message_id: row.input_message_id,
            output_message_id: row.output_message_id,
            artifact: row
                .artifact_json
                .map(|value| serde_json::from_str::<Value>(&value))
                .transpose()?,
            total_tokens: row.total_tokens,
            failure_code: row.failure_code,
            created_at: row.created_at,
            started_at: row.started_at,
            completed_at: row.completed_at,
            updated_at: row.updated_at,
        })
    }
}

#[derive(Debug, Error)]
pub enum SchedulerStoreError {
    #[error(transparent)]
    State(#[from] SchedulerStateError),
    #[error(transparent)]
    Model(#[from] SchedulerModelError),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("scheduler persistence failed")]
    Persistence(#[from] anyhow::Error),
    #[error("{entity} not found: {id}")]
    NotFound { entity: &'static str, id: String },
    #[error("active turn already exists for thread {thread_id}")]
    ActiveTurnExists { thread_id: String },
    #[error("turn {turn_id} is not running (status: {status:?})")]
    InactiveTurn { turn_id: String, status: TurnStatus },
    #[error(
        "{entity} transition compare-and-set failed for {id}: expected {expected}, actual {actual:?}"
    )]
    TransitionConflict {
        entity: &'static str,
        id: String,
        expected: String,
        actual: Option<String>,
    },
    #[error("invalid scheduler input: {0}")]
    InvalidInput(String),
}

fn is_transient_sqlite_lock(error: &SchedulerStoreError) -> bool {
    let error = match error {
        SchedulerStoreError::Database(error) => Some(error),
        SchedulerStoreError::Persistence(error) => error.downcast_ref::<sqlx::Error>(),
        _ => None,
    };
    error
        .and_then(sqlx::Error::as_database_error)
        .and_then(|error| error.code())
        .and_then(|code| code.parse::<i32>().ok())
        .is_some_and(|code| matches!(code & 0xff, 5 | 6))
}

/// Insert one `pending` turn and link its trigger message.
async fn insert_turn_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: &NewTurn,
    config_snapshot_json: &str,
    topology_snapshot_json: &str,
    now: &str,
) -> Result<TurnSnapshot, SchedulerStoreError> {
    let result = sqlx::query(
        "INSERT INTO group_turns \
         (id, thread_id, group_id, trigger_message_id, status, scheduler_strategy, \
          config_snapshot_json, topology_snapshot_json, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&input.id)
    .bind(&input.thread_id)
    .bind(&input.group_id)
    .bind(&input.trigger_message_id)
    .bind(TurnStatus::Pending.as_str())
    .bind(&input.scheduler_strategy)
    .bind(config_snapshot_json)
    .bind(topology_snapshot_json)
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await;

    if let Err(error) = result {
        if is_active_turn_unique_violation(&error) {
            return Err(SchedulerStoreError::ActiveTurnExists {
                thread_id: input.thread_id.clone(),
            });
        }
        return Err(error.into());
    }

    if let Some(trigger_message_id) = input.trigger_message_id.as_deref() {
        let linked = sqlx::query(
            "UPDATE messages \
             SET turn_id = ? \
             WHERE id = ? AND thread_id = ? AND group_id = ?",
        )
        .bind(&input.id)
        .bind(trigger_message_id)
        .bind(&input.thread_id)
        .bind(&input.group_id)
        .execute(&mut **tx)
        .await?;
        if linked.rows_affected() != 1 {
            return Err(SchedulerStoreError::InvalidInput(
                "turn trigger message does not belong to its thread and group".to_owned(),
            ));
        }
    }

    fetch_turn_in_tx(tx, &input.id).await
}

/// Mark the thread's one durable active turn as superseded, if it has one.
async fn supersede_active_turn_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
    now: &str,
) -> Result<Option<TurnSnapshot>, SchedulerStoreError> {
    let turn_id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM group_turns \
         WHERE thread_id = ? AND status IN ('pending', 'running', 'waiting_for_user') \
         ORDER BY created_at, rowid LIMIT 1",
    )
    .bind(thread_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(turn_id) = turn_id else {
        return Ok(None);
    };
    let current = fetch_turn_in_tx(tx, &turn_id).await?;
    let snapshot = transition_turn_in_tx(
        tx,
        &turn_id,
        current.status,
        TurnStatus::Superseded,
        Some(TurnReason::Superseded),
        now,
    )
    .await?;
    Ok(Some(snapshot))
}

async fn fetch_turn_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    turn_id: &str,
) -> Result<TurnSnapshot, SchedulerStoreError> {
    let row = sqlx::query_as::<_, TurnRow>(TURN_SELECT)
        .bind(turn_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| SchedulerStoreError::NotFound {
            entity: "turn",
            id: turn_id.to_owned(),
        })?;
    row.try_into()
}

async fn fetch_dispatch_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    dispatch_id: &str,
) -> Result<DispatchSnapshot, SchedulerStoreError> {
    let row = sqlx::query_as::<_, DispatchRow>(DISPATCH_SELECT)
        .bind(dispatch_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| SchedulerStoreError::NotFound {
            entity: "dispatch",
            id: dispatch_id.to_owned(),
        })?;
    row.try_into()
}

async fn transition_turn_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    turn_id: &str,
    expected: TurnStatus,
    next: TurnStatus,
    reason: Option<TurnReason>,
    now: &str,
) -> Result<TurnSnapshot, SchedulerStoreError> {
    validate_turn_transition(expected, next)?;
    let completed_at = is_terminal_turn(next).then_some(now);
    let started_at = (next == TurnStatus::Running).then_some(now);
    let result = sqlx::query(
        "UPDATE group_turns \
         SET status = ?, termination_reason = ?, \
             started_at = CASE WHEN ? IS NULL THEN started_at ELSE COALESCE(started_at, ?) END, \
             completed_at = CASE WHEN ? IS NULL THEN completed_at ELSE ? END, \
             updated_at = ? \
         WHERE id = ? AND status = ?",
    )
    .bind(next.as_str())
    .bind(reason.map(TurnReason::as_str))
    .bind(started_at)
    .bind(started_at)
    .bind(completed_at)
    .bind(completed_at)
    .bind(now)
    .bind(turn_id)
    .bind(expected.as_str())
    .execute(&mut **tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err(turn_transition_conflict(tx, turn_id, expected).await?);
    }

    if matches!(next, TurnStatus::Cancelled | TurnStatus::Superseded) {
        sqlx::query(
            "UPDATE agent_dispatches \
             SET status = CASE \
                     WHEN status = 'queued' THEN 'cancelled' \
                     ELSE 'interrupted' \
                 END, \
                 completed_at = COALESCE(completed_at, ?), updated_at = ? \
             WHERE turn_id = ? AND status IN ('queued', 'running')",
        )
        .bind(now)
        .bind(now)
        .bind(turn_id)
        .execute(&mut **tx)
        .await?;
    }

    fetch_turn_in_tx(tx, turn_id).await
}

async fn turn_transition_conflict(
    tx: &mut Transaction<'_, Sqlite>,
    turn_id: &str,
    expected: TurnStatus,
) -> Result<SchedulerStoreError, SchedulerStoreError> {
    let actual: Option<String> = sqlx::query_scalar("SELECT status FROM group_turns WHERE id = ?")
        .bind(turn_id)
        .fetch_optional(&mut **tx)
        .await?;
    if actual.is_none() {
        return Ok(SchedulerStoreError::NotFound {
            entity: "turn",
            id: turn_id.to_owned(),
        });
    }
    Ok(SchedulerStoreError::TransitionConflict {
        entity: "turn",
        id: turn_id.to_owned(),
        expected: expected.as_str().to_owned(),
        actual,
    })
}

async fn dispatch_transition_conflict(
    tx: &mut Transaction<'_, Sqlite>,
    dispatch_id: &str,
    expected: DispatchStatus,
) -> Result<SchedulerStoreError, SchedulerStoreError> {
    let actual: Option<String> =
        sqlx::query_scalar("SELECT status FROM agent_dispatches WHERE id = ?")
            .bind(dispatch_id)
            .fetch_optional(&mut **tx)
            .await?;
    if actual.is_none() {
        return Ok(SchedulerStoreError::NotFound {
            entity: "dispatch",
            id: dispatch_id.to_owned(),
        });
    }
    Ok(SchedulerStoreError::TransitionConflict {
        entity: "dispatch",
        id: dispatch_id.to_owned(),
        expected: expected.as_str().to_owned(),
        actual,
    })
}

fn is_active_turn_unique_violation(error: &sqlx::Error) -> bool {
    error.as_database_error().is_some_and(|database_error| {
        database_error.is_unique_violation()
            && database_error.message().contains("group_turns.thread_id")
    })
}

fn validate_dispatch_output(
    next: DispatchStatus,
    output: &super::model::DispatchOutput,
) -> Result<(), SchedulerStoreError> {
    if !matches!(
        next,
        DispatchStatus::Completed | DispatchStatus::WaitingForUser
    ) {
        return Err(SchedulerStoreError::InvalidInput(format!(
            "dispatch status {} cannot produce visible output",
            next.as_str()
        )));
    }
    if output.event.kind != StreamEventKind::AgentMessage {
        return Err(SchedulerStoreError::InvalidInput(
            "visible dispatch output requires an agent_message event".to_owned(),
        ));
    }
    if output
        .event
        .payload
        .get("message_id")
        .and_then(Value::as_str)
        != Some(output.message.id.as_str())
    {
        return Err(SchedulerStoreError::InvalidInput(
            "dispatch output event message_id does not match the message".to_owned(),
        ));
    }
    Ok(())
}

fn is_terminal_turn(status: TurnStatus) -> bool {
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

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

const TURN_SELECT: &str =
    "SELECT id, thread_id, group_id, trigger_message_id, status, scheduler_strategy, \
            config_snapshot_json, topology_snapshot_json, agent_steps, moderator_calls, \
            consecutive_failures, total_failures, total_tokens, termination_reason, \
            created_at, started_at, completed_at, updated_at \
     FROM group_turns WHERE id = ?";

const DISPATCH_SELECT: &str =
    "SELECT id, turn_id, parent_dispatch_id, source_agent_id, target_agent_id, \
            selection_reason, action_kind, hop, status, input_message_id, output_message_id, \
            artifact_json, total_tokens, failure_code, created_at, started_at, completed_at, \
            updated_at \
     FROM agent_dispatches WHERE id = ?";
