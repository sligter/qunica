//! Owner-scoped canary snapshots. No prompts, arguments or credentials are stored.
use serde_json::Value;
use sqlx::SqlitePool;
use std::time::Instant;

pub(crate) const RIG_VERSION: &str = "0.45.0";

pub(crate) struct Observation {
    pub id: String,
    pub paused: bool,
    started: Instant,
}

pub(crate) struct Start<'a> {
    pub id: &'a str,
    pub owner: &'a str,
    pub group: &'a str,
    pub thread: &'a str,
    pub agent: &'a str,
    pub provider: Option<&'a str>,
    pub engine: &'a str,
    pub resumed: bool,
}

impl Observation {
    pub async fn begin(
        pool: &SqlitePool,
        lock: &tokio::sync::Mutex<()>,
        start: Start<'_>,
    ) -> anyhow::Result<Self> {
        let _guard = lock.lock().await;
        let mut tx = crate::db::begin_write(pool).await?;
        let paused: bool = sqlx::query_scalar(
            "SELECT COALESCE((SELECT rig_paused FROM engine_rollouts WHERE owner_id=?),0)",
        )
        .bind(start.owner)
        .fetch_one(&mut *tx)
        .await?;
        // Note: pause only gates intake. Existing accepted calls/approvals retain
        // their engine; routing them to native would discard signed history.
        let paused = paused && start.engine == "rig" && !start.resumed;
        let now = crate::api::commands::now();
        let version = if start.engine == "rig" {
            RIG_VERSION.to_string()
        } else {
            // Legacy host checkpoints retain native attribution. A changed
            // transport must make their original observations partial.
            format!("legacy-host/rig-{RIG_VERSION}")
        };
        let changed = sqlx::query("INSERT INTO engine_executions(id,owner_id,group_id,thread_id,agent_id,provider_id,engine,app_version,engine_version,status,coverage_complete,started_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET status=excluded.status,invocations=engine_executions.invocations+1,coverage_complete=CASE WHEN engine_executions.app_version<>excluded.app_version OR engine_executions.engine_version<>excluded.engine_version OR engine_executions.provider_id IS NOT excluded.provider_id THEN 0 ELSE engine_executions.coverage_complete END,updated_at=excluded.updated_at WHERE engine_executions.owner_id=excluded.owner_id AND engine_executions.agent_id=excluded.agent_id AND engine_executions.thread_id=excluded.thread_id AND engine_executions.engine=excluded.engine")
            .bind(start.id).bind(start.owner).bind(start.group).bind(start.thread).bind(start.agent).bind(start.provider)
            .bind(start.engine).bind(env!("CARGO_PKG_VERSION")).bind(version).bind(if paused { "blocked" } else { "running" }).bind(!start.resumed).bind(&now).bind(&now)
            .execute(&mut *tx).await?.rows_affected();
        anyhow::ensure!(changed == 1, "engine observation identity changed");
        tx.commit().await?;
        Ok(Self {
            id: start.id.into(),
            paused,
            started: Instant::now(),
        })
    }

    pub async fn model(
        pool: &SqlitePool,
        id: &str,
        provider_kind: &str,
        model: &str,
    ) -> anyhow::Result<()> {
        sqlx::query("UPDATE engine_executions SET coverage_complete=CASE WHEN provider_kind<>'' AND (provider_kind<>? OR model<>?) THEN 0 ELSE coverage_complete END,provider_kind=CASE WHEN provider_kind='' THEN ? ELSE provider_kind END,model=CASE WHEN provider_kind='' THEN ? ELSE model END WHERE id=?")
            .bind(provider_kind)
            .bind(model)
            .bind(provider_kind)
            .bind(model)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn finish(
        &self,
        pool: &SqlitePool,
        status: &str,
        snapshot: Option<Value>,
    ) -> anyhow::Result<()> {
        let snapshot = match snapshot {
            Some(snapshot) => snapshot,
            None => {
                let raw: Option<String> = sqlx::query_scalar::<_, Option<String>>(
                    "SELECT content_json FROM messages WHERE id=?",
                )
                .bind(&self.id)
                .fetch_optional(pool)
                .await?
                .flatten();
                raw.as_deref()
                    .map(serde_json::from_str)
                    .transpose()?
                    .unwrap_or(Value::Null)
            }
        };
        let calls = snapshot["tool_calls"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut settled = 0i64;
        let mut errors = 0i64;
        let mut unsettled = 0i64;
        for call in calls {
            match call["status"].as_str().unwrap_or_default() {
                "pending" | "started" | "outcome_unknown" => unsettled += 1,
                "approval_required" | "input_required" | "" => {}
                status => {
                    settled += 1;
                    if status == "failed" {
                        errors += 1;
                    }
                }
            }
        }
        let status = if snapshot["phase"] == "outcome_unknown" {
            "outcome_unknown"
        } else {
            status
        };
        sqlx::query("UPDATE engine_executions SET status=?,active_duration_ms=active_duration_ms+?,tool_calls=?,settled_tool_calls=?,tool_errors=?,unsettled_tools=?,updated_at=? WHERE id=?")
            .bind(status).bind(self.started.elapsed().as_millis().min(i64::MAX as u128) as i64)
            .bind(calls.len() as i64).bind(settled).bind(errors).bind(unsettled)
            .bind(crate::api::commands::now()).bind(&self.id).execute(pool).await?;
        Ok(())
    }
}
