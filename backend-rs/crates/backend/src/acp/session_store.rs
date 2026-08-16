//! Durable ACP session ids.
//!
//! The reusable-session map in [`super`] dies with the process. What can
//! outlive it is the `sessionId` the agent handed out: a runtime that
//! advertises `loadSession` can be asked to reopen that session on the next
//! launch, restoring what the host transcript cannot carry — the agent's tool
//! results, the files it read, its own reasoning.
//!
//! A stored id is only offered back when the session would have been reused in
//! memory anyway: same owner, workspace, runtime config and host context. Any
//! difference means a fresh session, exactly as it does in process.
//!
//! Every failure here is logged and swallowed. Losing a session id costs the
//! agent its native context, which the full transcript prompt still covers;
//! failing the turn over it would cost the user their answer.

use sqlx::SqlitePool;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

/// The persisted ACP session for one (thread, agent) pair.
#[derive(Debug, Clone)]
pub(crate) struct AcpSessionStore {
    pool: SqlitePool,
    thread_id: String,
    agent_id: String,
    owner_id: String,
    cwd: String,
    config_hash: String,
    context_hash: Option<String>,
}

impl AcpSessionStore {
    /// Bind the store to the session signature of the turn being run.
    pub(crate) fn new(
        pool: SqlitePool,
        thread_id: String,
        agent_id: String,
        owner_id: String,
        cwd: String,
        config_hash: String,
        context_hash: Option<String>,
    ) -> Self {
        Self {
            pool,
            thread_id,
            agent_id,
            owner_id,
            cwd,
            config_hash,
            context_hash,
        }
    }

    /// The session id worth trying to reopen, or `None` when nothing was
    /// stored or the stored session belongs to a different signature.
    ///
    /// `IS` rather than `=` on `context_hash`: an untracked context is stored
    /// as NULL, and NULL never equals NULL.
    pub(crate) async fn resumable(&self) -> Option<String> {
        let stored: Result<Option<(String,)>, sqlx::Error> = sqlx::query_as(
            "SELECT session_id FROM acp_sessions \
             WHERE thread_id = ? AND agent_id = ? AND owner_id = ? \
               AND cwd = ? AND config_hash = ? AND context_hash IS ?",
        )
        .bind(&self.thread_id)
        .bind(&self.agent_id)
        .bind(&self.owner_id)
        .bind(&self.cwd)
        .bind(&self.config_hash)
        .bind(self.context_hash.as_deref())
        .fetch_optional(&self.pool)
        .await;

        match stored {
            Ok(row) => row
                .map(|(session_id,)| session_id)
                .filter(|id| !id.is_empty()),
            Err(err) => {
                tracing::warn!(error = %err, "failed to read the stored ACP session");
                None
            }
        }
    }

    /// Record the session the agent just opened, replacing any older one for
    /// this (thread, agent).
    ///
    /// Called as soon as the id exists rather than at the end of the turn: a
    /// crash mid-turn is exactly the case resuming is for.
    pub(crate) async fn remember(&self, session_id: &str) {
        if session_id.is_empty() {
            return;
        }
        let now = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default();
        let stored = sqlx::query(
            "INSERT INTO acp_sessions \
                 (thread_id, agent_id, session_id, owner_id, cwd, config_hash, context_hash, \
                  created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (thread_id, agent_id) DO UPDATE SET \
                 session_id = excluded.session_id, \
                 owner_id = excluded.owner_id, \
                 cwd = excluded.cwd, \
                 config_hash = excluded.config_hash, \
                 context_hash = excluded.context_hash, \
                 updated_at = excluded.updated_at",
        )
        .bind(&self.thread_id)
        .bind(&self.agent_id)
        .bind(session_id)
        .bind(&self.owner_id)
        .bind(&self.cwd)
        .bind(&self.config_hash)
        .bind(self.context_hash.as_deref())
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await;
        if let Err(err) = stored {
            tracing::warn!(error = %err, "failed to store the ACP session id");
        }
    }

    /// Drop the stored session, so the next turn starts a fresh one.
    pub(crate) async fn forget(&self) {
        let deleted = sqlx::query("DELETE FROM acp_sessions WHERE thread_id = ? AND agent_id = ?")
            .bind(&self.thread_id)
            .bind(&self.agent_id)
            .execute(&self.pool)
            .await;
        if let Err(err) = deleted {
            tracing::warn!(error = %err, "failed to drop the stored ACP session id");
        }
    }
}
