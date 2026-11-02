//! Optional idle stopping uses existing resource/operation records and lifecycle.
use super::SupervisorClient;
use anyhow::Context;
use reqwest::Method;
use serde_json::json;
use sqlx::SqlitePool;
use std::{sync::Arc, time::Duration};
use tokio::{sync::Mutex, time::Instant};

/// A live desktop/transfer prevents automatic stop, including between polls.
pub(crate) struct ActiveUse {
    _lease: Arc<()>,
    id: String,
    last_recorded: Mutex<Option<Instant>>,
}

impl ActiveUse {
    pub(crate) async fn record(
        &self,
        pool: &SqlitePool,
        generation: &str,
    ) -> Result<(), sqlx::Error> {
        let mut last = self.last_recorded.lock().await;
        // Note: each exchange/chunk counts as activity, but high frame rates
        // and 1000 MiB transfers must not create a SQLite write per chunk.
        if last.is_none_or(|at| at.elapsed() >= Duration::from_secs(30)) {
            record_activity(pool, &self.id, generation).await?;
            *last = Some(Instant::now());
        }
        Ok(())
    }
}

pub(super) fn parse_seconds(raw: Option<&str>) -> anyhow::Result<u64> {
    let raw = raw.unwrap_or("0");
    anyhow::ensure!(
        !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit()),
        "QUNICA_COMPUTER_IDLE_SECONDS must be unsigned whole seconds (0 disables it)"
    );
    let seconds = raw
        .parse::<u64>()
        .context("QUNICA_COMPUTER_IDLE_SECONDS is too large")?;
    anyhow::ensure!(
        seconds <= i64::MAX as u64 / 1000,
        "QUNICA_COMPUTER_IDLE_SECONDS is too large"
    );
    Ok(seconds)
}

pub(crate) async fn record_activity(
    pool: &SqlitePool,
    id: &str,
    generation: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE computers SET last_activity_at_ms=?,idle_observed_generation=? WHERE id=?")
        .bind(chrono::Utc::now().timestamp_millis())
        .bind(generation)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

fn eligible(
    seconds: u64,
    now: i64,
    last: Option<i64>,
    occupied: bool,
    control_free: bool,
    same_generation: bool,
) -> bool {
    seconds > 0
        && !occupied
        && control_free
        && same_generation
        && last.is_some_and(|last| {
            now.checked_sub(last)
                .is_some_and(|elapsed| elapsed > (seconds * 1000) as i64)
        })
}

impl SupervisorClient {
    pub(crate) async fn active_use(&self, id: &str) -> ActiveUse {
        // Note: register under the lifecycle lock so a sweep cannot pass its
        // occupancy check concurrently. The lease, not the lifecycle lock,
        // spans the session: explicit stop stays usable and sweeps can skip a
        // live stream immediately instead of starving other computers.
        let _guard = self.lifecycle_lock(id).await;
        let mut uses = self.active_uses.lock().unwrap_or_else(|e| e.into_inner());
        uses.retain(|_, lease| lease.strong_count() > 0);
        let lease = uses
            .get(id)
            .and_then(std::sync::Weak::upgrade)
            .unwrap_or_else(|| {
                let lease = Arc::new(());
                uses.insert(id.to_owned(), Arc::downgrade(&lease));
                lease
            });
        ActiveUse {
            _lease: lease,
            id: id.to_owned(),
            last_recorded: Mutex::new(None),
        }
    }

    pub fn idle_enabled(&self) -> bool {
        self.idle_seconds > 0
    }

    /// One conservative sweep. Missing state fails closed for that computer.
    pub async fn stop_idle(&self, pool: &SqlitePool) -> anyhow::Result<()> {
        if !self.idle_enabled() {
            return Ok(());
        }
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM computers")
            .fetch_all(pool)
            .await?;
        for id in ids {
            if let Err(error) = self.stop_idle_computer(pool, &id).await {
                tracing::warn!(computer_id=%id, %error, "computer idle check skipped");
            }
        }
        Ok(())
    }

    async fn stop_idle_computer(&self, pool: &SqlitePool, id: &str) -> anyhow::Result<()> {
        // Note: resolve, operation admission and manual lifecycle/control share
        // this lock. New work waits until stop finishes, then binds the new
        // generation after wake; no SQLite writer is held during Docker I/O.
        let _guard = self.lifecycle_lock(id).await;
        if self
            .active_uses
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .is_some_and(|lease| lease.strong_count() > 0)
        {
            return Ok(());
        }
        let value = self
            .request(Method::GET, &format!("/v1/computers/{id}"), None)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e.code()))?;
        if value["status"] != "running" {
            return Ok(());
        }
        let generation = value["generation"]
            .as_str()
            .context("computer generation missing")?;
        let (owner, kind, target, observed): (String, String, String, Option<String>) =
            sqlx::query_as("SELECT owner_id,binding_kind,binding_id,idle_observed_generation FROM computers WHERE id=?")
                .bind(id).fetch_one(pool).await?;
        anyhow::ensure!(
            value["spec"]["id"] == id
                && value["spec"]["owner_id"] == owner
                && value["spec"]["binding"]["kind"] == kind
                && value["spec"]["binding"]["id"] == target,
            "computer identity or scope mismatch"
        );
        if observed.as_deref() != Some(generation) {
            // Fresh/legacy records and externally changed generations receive a
            // full grace period. A cached generation is never runtime authority.
            record_activity(pool, id, generation).await?;
            return Ok(());
        }
        let control = self
            .request(
                Method::POST,
                &format!("/v1/computers/{id}/control"),
                Some(&json!({"expected_generation":generation,"action":"status"})),
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e.code()))?;
        let control_free = control["generation"] == generation
            && matches!(
                control["state"].as_str(),
                Some("agent_controlled" | "released")
            );
        if !control_free {
            record_activity(pool, id, generation).await?;
            return Ok(());
        }
        // Include selected scopes before resolve has created a run binding.
        // Private computers may be selected by an Agent in several groups. Read
        // occupancy and activity in one SQLite snapshot: completion must not
        // clear an intent while leaving us with its pre-completion idle time.
        let (occupied, last): (bool, Option<i64>) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM computer_operations WHERE computer_id=?1 AND status IN ('running','uncertain')) OR EXISTS(SELECT 1 FROM group_turns t JOIN groups g ON g.id=t.group_id WHERE t.status NOT IN ('completed','silence','budget_exhausted','failure_budget_exhausted','cancelled','superseded','failed') AND (g.execution_environment_id=?1 OR EXISTS(SELECT 1 FROM computer_run_bindings b WHERE b.turn_id=t.id AND b.computer_id=?1) OR EXISTS(SELECT 1 FROM group_agents ga JOIN agents a ON a.id=ga.agent_id WHERE ga.group_id=t.group_id AND a.execution_environment_id=?1))), CASE WHEN last_activity_at_ms IS NULL THEN NULL ELSE MAX(last_activity_at_ms,COALESCE((SELECT MAX(o.last_activity_at_ms) FROM computer_operations o WHERE o.computer_id=computers.id),0)) END FROM computers WHERE id=?1")
            .bind(id).fetch_one(pool).await?;
        if !eligible(
            self.idle_seconds,
            chrono::Utc::now().timestamp_millis(),
            last,
            occupied,
            control_free,
            observed.as_deref() == Some(generation),
        ) {
            return Ok(());
        }
        // Note: Supervisor rechecks generation and control under its own lock.
        // A stale candidate or takeover is abandoned, never retried as a stop.
        match self
            .request(
                Method::POST,
                &format!("/v1/computers/{id}/actions"),
                Some(
                    &json!({"expected_generation":generation,"action":"stop","only_if_idle":true}),
                ),
            )
            .await
        {
            Ok(_) => {
                sqlx::query("UPDATE computers SET revision=revision+1 WHERE id=?")
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
            Err(error)
                if matches!(
                    error.code(),
                    "generation_mismatch" | "computer_busy" | "computer_control_held"
                ) => {}
            Err(error) => anyhow::bail!(
                "automatic stop failed ({}); inspect the computer",
                error.code()
            ),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_threshold_is_strict_and_fails_closed() {
        assert!(!eligible(0, 20_000, Some(0), false, true, true));
        assert!(!eligible(10, 10_000, Some(0), false, true, true));
        assert!(eligible(10, 10_001, Some(0), false, true, true));
        assert!(!eligible(10, 10_001, None, false, true, true));
        assert!(!eligible(10, -1, Some(0), false, true, true));
        assert!(!eligible(10, 10_001, Some(0), true, true, true));
        assert!(!eligible(10, 10_001, Some(0), false, false, true));
        assert!(!eligible(10, 10_001, Some(0), false, true, false));
    }

    #[test]
    fn idle_config_rejects_malformed_and_overflow_values() {
        assert_eq!(parse_seconds(None).unwrap(), 0);
        assert_eq!(parse_seconds(Some("600")).unwrap(), 600);
        for raw in [
            "",
            "-1",
            "+1",
            "1.5",
            " 1",
            "false",
            "18446744073709551615",
            "18446744073709551616",
        ] {
            assert!(parse_seconds(Some(raw)).is_err(), "{raw}");
        }
    }
}
