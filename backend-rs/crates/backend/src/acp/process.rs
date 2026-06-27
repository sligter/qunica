//! ACP child-process audit persistence.
//!
//! Every ACP turn is recorded in `external_agent_runs`: a `running` row is
//! inserted when the run starts and updated to its terminal status when it
//! finishes. Task 9a provides only this audit foundation and a bounded output
//! [`Tail`]; Task 9b will spawn the actual child process, drive the ACP stdio
//! protocol, and call these helpers to persist the outcome.

use serde_json::json;
use sqlx::SqlitePool;
use thiserror::Error;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

/// Maximum number of characters retained in a captured stdout/stderr tail.
pub const MAX_TAIL_CHARS: usize = 12_000;

/// A failure while persisting an ACP audit row.
#[derive(Debug, Error)]
pub enum AcpAuditError {
    /// The underlying database operation failed.
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    /// Serializing `argv` to JSON failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// A bounded, char-limited capture of streamed process output.
///
/// Appending always keeps only the most recent [`MAX_TAIL_CHARS`] characters
/// (or a custom limit), mirroring the Python `_Tail` helper used to retain the
/// tail end of a child process's stdout/stderr.
#[derive(Debug, Clone)]
pub struct Tail {
    limit: usize,
    value: String,
}

impl Tail {
    /// A tail bounded to [`MAX_TAIL_CHARS`] characters.
    pub fn new() -> Self {
        Self::with_limit(MAX_TAIL_CHARS)
    }

    /// A tail bounded to `limit` characters.
    pub fn with_limit(limit: usize) -> Self {
        Self {
            limit,
            value: String::new(),
        }
    }

    /// Append `text`, dropping leading characters so at most `limit` remain.
    pub fn append(&mut self, text: &str) {
        self.value.push_str(text);
        let count = self.value.chars().count();
        if count > self.limit {
            self.value = self.value.chars().skip(count - self.limit).collect();
        }
    }

    /// The current retained tail.
    pub fn snapshot(&self) -> &str {
        &self.value
    }

    /// Consume the tail, returning its retained contents.
    pub fn into_string(self) -> String {
        self.value
    }
}

impl Default for Tail {
    fn default() -> Self {
        Self::new()
    }
}

/// Identifiers and metadata for a new ACP run, captured before the child starts.
#[derive(Debug, Clone)]
pub struct AcpRunContext {
    /// Owning user id.
    pub owner_id: String,
    /// Group id, if the run is part of a group turn.
    pub group_id: Option<String>,
    /// The agent being run.
    pub agent_id: String,
    /// Thread id, if the run is bound to a thread.
    pub thread_id: Option<String>,
    /// Resolved working directory the child runs in.
    pub cwd: String,
    /// The full argv (`command` followed by `args`).
    pub argv: Vec<String>,
}

/// A live handle to one `external_agent_runs` row.
///
/// [`AcpRunAudit::start`] inserts the `running` row; one of [`complete`],
/// [`fail`], or [`cancel`] later stamps the terminal status, output tails, and
/// `ended_at`.
///
/// [`complete`]: AcpRunAudit::complete
/// [`fail`]: AcpRunAudit::fail
/// [`cancel`]: AcpRunAudit::cancel
#[derive(Clone)]
pub struct AcpRunAudit {
    pool: SqlitePool,
    id: String,
}

impl AcpRunAudit {
    /// Insert a `running` audit row for the given context and return its handle.
    pub async fn start(pool: &SqlitePool, ctx: &AcpRunContext) -> Result<Self, AcpAuditError> {
        let id = Uuid::new_v4().to_string();
        let argv_json = serde_json::to_string(&json!(ctx.argv))?;
        let started_at = now_rfc3339();

        sqlx::query(
            "INSERT INTO external_agent_runs \
             (id, owner_id, group_id, agent_id, thread_id, adapter, cwd, status, argv_json, \
              started_at) \
             VALUES (?, ?, ?, ?, ?, 'acp', ?, 'running', ?, ?)",
        )
        .bind(&id)
        .bind(&ctx.owner_id)
        .bind(&ctx.group_id)
        .bind(&ctx.agent_id)
        .bind(&ctx.thread_id)
        .bind(&ctx.cwd)
        .bind(&argv_json)
        .bind(&started_at)
        .execute(pool)
        .await?;

        Ok(Self {
            pool: pool.clone(),
            id,
        })
    }

    /// The id of the persisted run row.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Mark the run `completed` with an exit code and output tails.
    pub async fn complete(
        &self,
        exit_code: Option<i64>,
        stdout_tail: Option<&str>,
        stderr_tail: Option<&str>,
    ) -> Result<(), AcpAuditError> {
        self.finish("completed", exit_code, stdout_tail, stderr_tail, None)
            .await
    }

    /// Mark the run `failed` with an exit code, output tails, and an error.
    pub async fn fail(
        &self,
        exit_code: Option<i64>,
        stdout_tail: Option<&str>,
        stderr_tail: Option<&str>,
        error_message: &str,
    ) -> Result<(), AcpAuditError> {
        self.finish(
            "failed",
            exit_code,
            stdout_tail,
            stderr_tail,
            Some(error_message),
        )
        .await
    }

    /// Mark the run `cancelled` with output tails and an error message. A
    /// cancelled run has no meaningful exit code.
    pub async fn cancel(
        &self,
        stdout_tail: Option<&str>,
        stderr_tail: Option<&str>,
        error_message: &str,
    ) -> Result<(), AcpAuditError> {
        self.finish(
            "cancelled",
            None,
            stdout_tail,
            stderr_tail,
            Some(error_message),
        )
        .await
    }

    async fn finish(
        &self,
        status: &str,
        exit_code: Option<i64>,
        stdout_tail: Option<&str>,
        stderr_tail: Option<&str>,
        error_message: Option<&str>,
    ) -> Result<(), AcpAuditError> {
        let ended_at = now_rfc3339();
        sqlx::query(
            "UPDATE external_agent_runs \
             SET status = ?, exit_code = ?, stdout_tail = ?, stderr_tail = ?, error_message = ?, \
                 ended_at = ? \
             WHERE id = ?",
        )
        .bind(status)
        .bind(exit_code)
        .bind(bound_tail(stdout_tail))
        .bind(bound_tail(stderr_tail))
        .bind(error_message)
        .bind(&ended_at)
        .bind(&self.id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

/// Bound an optional output tail to the most recent [`MAX_TAIL_CHARS`] chars.
fn bound_tail(value: Option<&str>) -> Option<String> {
    value.map(|text| {
        let count = text.chars().count();
        if count <= MAX_TAIL_CHARS {
            text.to_string()
        } else {
            text.chars().skip(count - MAX_TAIL_CHARS).collect()
        }
    })
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}
