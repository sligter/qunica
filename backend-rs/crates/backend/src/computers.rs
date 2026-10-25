//! Narrow Supervisor client and per-invocation authorization. Never runs Docker
//! or falls back to a local process. See docs/guide/computers.md.
pub(crate) mod artifacts;
mod publication;
use std::{sync::Arc, time::Duration};

use hmac::{Hmac, Mac};
use reqwest::{Method, Url};
use serde_json::{json, Value};
use sha2::Sha256;
use sqlx::SqlitePool;

use crate::api::error::ApiError;

#[derive(Clone)]
pub struct SupervisorClient {
    client: reqwest::Client,
    endpoint: Url,
    token: String,
}

impl SupervisorClient {
    fn proof(&self, id: &str, generation: &str) -> Hmac<Sha256> {
        let mut proof = Hmac::<Sha256>::new_from_slice(self.token.as_bytes())
            .expect("HMAC accepts any key length");
        proof.update(format!("qunica.egress.v1\0{id}\0{generation}").as_bytes());
        proof
    }
    pub fn egress_attestation(&self, id: &str, generation: &str) -> String {
        format!("{:x}", self.proof(id, generation).finalize().into_bytes())
    }
    pub fn verify_egress(&self, id: &str, generation: &str, provided: &str) -> bool {
        if provided.len() != 64 || !provided.is_ascii() {
            return false;
        }
        let bytes: Result<Vec<u8>, _> = (0..64)
            .step_by(2)
            .map(|start| u8::from_str_radix(&provided[start..start + 2], 16))
            .collect();
        bytes.is_ok_and(|bytes| self.proof(id, generation).verify_slice(&bytes).is_ok())
    }
    pub fn new(endpoint: &str, token: String) -> anyhow::Result<Self> {
        let endpoint = Url::parse(endpoint)?;
        anyhow::ensure!(
            matches!(endpoint.scheme(), "http" | "https")
                && endpoint.username().is_empty()
                && endpoint.password().is_none()
                && endpoint.path() == "/"
                && endpoint.query().is_none()
                && endpoint.fragment().is_none(),
            "invalid supervisor endpoint"
        );
        anyhow::ensure!(
            token.len() >= 32 && !token.contains(char::is_whitespace),
            "invalid supervisor credential"
        );
        Ok(Self {
            endpoint,
            token,
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(315))
                .build()?,
        })
    }

    pub fn from_env() -> anyhow::Result<Option<Arc<Self>>> {
        let Ok(endpoint) = std::env::var("QUNICA_SUPERVISOR_URL") else {
            return Ok(None);
        };
        let file = std::env::var("QUNICA_SUPERVISOR_TOKEN_FILE")?;
        let token = std::fs::read_to_string(file)?;
        Ok(Some(Arc::new(Self::new(
            &endpoint,
            token.trim().to_string(),
        )?)))
    }

    pub async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, ApiError> {
        let url = self
            .endpoint
            .join(path)
            .map_err(|_| ApiError::internal("invalid supervisor path"))?;
        let mut request = self.client.request(method, url).bearer_auth(&self.token);
        if let Some(body) = body {
            request = request.json(body);
        }
        // Note: mutations are never retried. A lost result may represent an
        // external side effect; the Supervisor keeps that action fenced.
        let mut response = request.send().await.map_err(|_| ApiError::new(axum::http::StatusCode::BAD_GATEWAY,"computer_unavailable","Computer connection failed; execution may be uncertain. Inspect the computer before continuing."))?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| ApiError::internal("computer response interrupted"))?
        {
            if bytes.len() + chunk.len() > 512 * 1024 {
                return Err(ApiError::internal("computer response exceeded limit"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| ApiError::internal("invalid computer response"))?;
        if !status.is_success() {
            let code = match value["error"]["code"].as_str().unwrap_or("") {
                "computer_busy" => "computer_busy",
                "computer_not_found" => "computer_not_found",
                "generation_mismatch" => "generation_mismatch",
                "execution_uncertain" => "execution_uncertain",
                "explicit_stop_required" => "explicit_stop_required",
                "computer_not_stopped" => "computer_not_stopped",
                "computer_destroyed" => "computer_destroyed",
                "invalid_input" => "invalid_input",
                "computer_control_held" => "computer_control_held",
                "control_lease_expired" => "control_lease_expired",
                "capability_unavailable" => "capability_unavailable",
                _ => "computer_unavailable",
            };
            // Only the code is forwarded, never a provider body or credentials.
            return Err(ApiError::new(status, code, code));
        }
        Ok(value)
    }
}

pub async fn selected_environment(
    pool: &SqlitePool,
    owner: &str,
    group: &str,
    agent: &str,
) -> anyhow::Result<Option<String>> {
    Ok(sqlx::query_scalar::<_, Option<String>>("SELECT COALESCE(a.execution_environment_id,g.execution_environment_id) FROM agents a JOIN groups g ON g.id=? WHERE a.id=? AND a.owner_id=? AND g.owner_id=?")
        .bind(group).bind(agent).bind(owner).bind(owner).fetch_optional(pool).await?.flatten())
}

#[derive(Clone)]
pub struct ComputerExecution {
    pub id: String,
    pub generation: String,
    client: Arc<SupervisorClient>,
    pool: SqlitePool,
    owner: String,
    group: String,
    agent: String,
    turn: String,
    cwd: String,
    pub capabilities: Value,
}

impl std::fmt::Debug for ComputerExecution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComputerExecution")
            .field("id", &self.id)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl ComputerExecution {
    pub fn workspace_path(&self) -> String {
        if self.cwd.is_empty() {
            "/home/agent/workspace".into()
        } else {
            format!("/home/agent/workspace/{}", self.cwd)
        }
    }

    pub fn relative_workspace(&self) -> &str {
        &self.cwd
    }
    pub fn turn_id(&self) -> &str {
        &self.turn
    }
    pub async fn resolve(
        pool: &SqlitePool,
        client: Option<Arc<SupervisorClient>>,
        owner: &str,
        group: &str,
        agent: &str,
        turn: &str,
    ) -> anyhow::Result<Option<Self>> {
        let Some(id) = selected_environment(pool, owner, group, agent).await? else {
            return Ok(None);
        };
        let client = client.ok_or_else(|| anyhow::anyhow!("Computer Supervisor is not configured; host execution is disabled for this binding"))?;
        let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM computers WHERE id=? AND owner_id=? AND ((binding_kind='team' AND binding_id=?) OR (binding_kind='private' AND binding_id=?)))").bind(&id).bind(owner).bind(group).bind(agent).fetch_one(pool).await?;
        anyhow::ensure!(
            valid,
            "Computer binding is unavailable or belongs to another scope"
        );
        let value = client
            .request(Method::GET, &format!("/v1/computers/{id}"), None)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e.message_text()))?;
        anyhow::ensure!(
            value["status"] == "running",
            "Computer is not running; explicitly inspect and start it before continuing"
        );
        anyhow::ensure!(
            value["spec"]["id"] == id && value["spec"]["owner_id"] == owner,
            "Computer identity does not match its binding"
        );
        let generation = value["generation"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Computer generation missing"))?
            .to_string();
        let capabilities = client
            .request(Method::GET, "/v1/health", None)
            .await
            .map_err(|_| anyhow::anyhow!("Computer capabilities unavailable"))?;
        let binding_kind = value["spec"]["binding"]["kind"].as_str().unwrap_or("");
        let binding_id = value["spec"]["binding"]["id"].as_str().unwrap_or("");
        anyhow::ensure!(
            (binding_kind == "team" && binding_id == group)
                || (binding_kind == "private" && binding_id == agent),
            "Supervisor scope does not match the execution binding"
        );
        // Note: independent task threads can run in parallel, and a private
        // Agent can appear in different groups. The computer needs its own lease.
        let mut tx = crate::db::begin_write(pool).await?;
        let occupied:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM computer_run_bindings b JOIN group_turns t ON t.id=b.turn_id WHERE b.computer_id=? AND b.turn_id!=? AND (t.status IN ('pending','running','waiting_for_user') OR EXISTS(SELECT 1 FROM computer_operations o WHERE o.turn_id=b.turn_id AND o.computer_id=b.computer_id AND o.status IN ('running','uncertain'))))")
            .bind(&id).bind(turn).fetch_one(&mut *tx).await?;
        anyhow::ensure!(
            !occupied,
            "Computer is reserved by another active or unconfirmed turn"
        );
        sqlx::query("INSERT INTO computer_run_bindings(turn_id,agent_id,computer_id,generation) VALUES(?,?,?,?) ON CONFLICT DO NOTHING")
            .bind(turn).bind(agent).bind(&id).bind(&generation).execute(&mut *tx).await?;
        tx.commit().await?;
        let mut execution = Self {
            id,
            generation,
            client,
            pool: pool.clone(),
            owner: owner.into(),
            group: group.into(),
            agent: agent.into(),
            turn: turn.into(),
            cwd: String::new(),
            capabilities,
        };
        let task: Option<(String, Option<String>)> = sqlx::query_as("SELECT th.id,th.git_branch FROM threads th JOIN group_turns t ON t.thread_id=th.id WHERE t.id=? AND t.group_id=?")
            .bind(turn).bind(group).fetch_optional(pool).await?;
        if let Some((thread, Some(branch))) = task {
            // Note: legacy host paths are never mounted or interpreted on a
            // computer. Resolve the task's branch in its own repository instead.
            let result = execution
                .execute(
                    vec![
                        "python3".into(),
                        "-c".into(),
                        include_str!("tools/computer_worktree.py").into(),
                        group.into(),
                        thread.clone(),
                        branch,
                    ],
                    120,
                )
                .await
                .map_err(|_| anyhow::anyhow!("Computer task worktree preparation failed"))?;
            anyhow::ensure!(result["exit_code"] == 0, "Computer task worktree is unavailable; inspect the repository and branch on the computer");
            execution.cwd = format!(".worktrees/{group}/{thread}");
        }
        Ok(Some(execution))
    }

    pub async fn execute(
        &self,
        argv: Vec<String>,
        timeout_seconds: u64,
    ) -> Result<Value, ApiError> {
        // Note: the worker outlives a cancelled runtime future, just as the
        // Supervisor action outlives its HTTP waiter. Its durable intent keeps
        // group occupancy until the result is definite or explicitly verified.
        let execution = self.clone();
        let body = json!({"expected_generation":self.generation,"argv":argv,"cwd":self.cwd,"timeout_seconds":timeout_seconds.clamp(1,300)});
        tokio::spawn(async move { execution.request_owned("exec", body).await })
            .await
            .map_err(|_| ApiError::internal("computer execution result is uncertain"))?
    }

    pub async fn call(&self, request: Value) -> Result<Value, ApiError> {
        let execution = self.clone();
        let body = json!({"expected_generation":self.generation,"request":request});
        let value = tokio::spawn(async move { execution.request_owned("call", body).await })
            .await
            .map_err(|_| ApiError::internal("computer result uncertain"))??;
        if value["truncated"] == true {
            return Err(ApiError::conflict("Computer capability failed; inspect the computer or refresh its page/screen reference"));
        }
        let mut result: Value = serde_json::from_str(value["stdout"].as_str().unwrap_or(""))
            .map_err(|_| ApiError::internal("invalid computer capability response"))?;
        if result["code"] == "action_repeat_limit" {
            return Err(ApiError::new(
                axum::http::StatusCode::CONFLICT,
                "computer_action_repeat",
                "Repeated browser action has no effect; inspect the computer before continuing",
            ));
        }
        if value["exit_code"] != 0 {
            return Err(ApiError::conflict("Computer capability failed; inspect the computer or refresh its page/screen reference"));
        }
        let mut registered = Vec::new();
        if let Some(items) = result["artifacts"].as_array() {
            for item in items {
                if let (Some(path), Some(size)) = (item["path"].as_str(), item["size"].as_i64()) {
                    let artifact_id: String = sqlx::query_scalar("INSERT INTO computer_artifacts(id,owner_id,computer_id,turn_id,generation,path,size) VALUES(?,?,?,?,?,?,?) ON CONFLICT(turn_id,computer_id,path) DO UPDATE SET size=excluded.size,generation=excluded.generation RETURNING id")
                        .bind(uuid::Uuid::new_v4().to_string()).bind(&self.owner).bind(&self.id).bind(&self.turn).bind(&self.generation).bind(path).bind(size).fetch_one(&self.pool).await.map_err(|_|ApiError::internal("artifact record unavailable"))?;
                    registered.push(json!({"id":artifact_id,"path":path,"size":size}));
                }
            }
        }
        if !registered.is_empty() {
            result["registered_artifacts"] = json!(registered);
        }
        Ok(result)
    }

    pub(crate) async fn stop_driver(&self) -> Result<(), ApiError> {
        let mut attempts = 0;
        let result =
            loop {
                let result = self
            .client
            .request(
                Method::POST,
                &format!("/v1/computers/{}/call", self.id),
                Some(&json!({"expected_generation":self.generation,"request":{"op":"quiesce"}})),
            )
            .await;
                match result {
                    Err(error) if error.code() == "computer_busy" && attempts < 20 => {
                        attempts += 1;
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                    result => break result?,
                }
            };
        if result["exit_code"] != 0 {
            return Err(ApiError::conflict("Computer driver stop is uncertain"));
        }
        Ok(())
    }

    async fn request_owned(&self, action: &str, body: Value) -> Result<Value, ApiError> {
        let mut tx = crate::db::begin_write(&self.pool)
            .await
            .map_err(|_| ApiError::internal("computer authorization unavailable"))?;
        let active: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM group_turns t JOIN groups g ON g.id=t.group_id WHERE t.id=? AND t.group_id=? AND g.owner_id=? AND g.status='active' AND t.status='running')")
            .bind(&self.turn).bind(&self.group).bind(&self.owner).fetch_one(&mut *tx).await.map_err(|_|ApiError::internal("computer authorization unavailable"))?;
        let selected=sqlx::query_scalar::<_,Option<String>>("SELECT COALESCE(a.execution_environment_id,g.execution_environment_id) FROM agents a JOIN groups g ON g.id=? WHERE a.id=? AND a.owner_id=? AND g.owner_id=?").bind(&self.group).bind(&self.agent).bind(&self.owner).bind(&self.owner).fetch_optional(&mut *tx).await.map_err(|_|ApiError::internal("computer authorization unavailable"))?.flatten();
        if !active || selected.as_deref() != Some(&self.id) {
            return Err(ApiError::conflict(
                "Computer execution authorization is no longer active",
            ));
        }
        let operation = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO computer_operations(id,turn_id,computer_id,status) VALUES(?,?,?,'running')").bind(&operation).bind(&self.turn).bind(&self.id).execute(&mut *tx).await.map_err(|_|ApiError::internal("computer operation storage unavailable"))?;
        tx.commit()
            .await
            .map_err(|_| ApiError::internal("computer operation storage unavailable"))?;
        let result = self
            .client
            .request(
                Method::POST,
                &format!("/v1/computers/{}/{action}", self.id),
                Some(&body),
            )
            .await;
        let uncertain = result.as_ref().err().is_some_and(|error| {
            !matches!(
                error.code(),
                "computer_busy"
                    | "computer_not_found"
                    | "generation_mismatch"
                    | "explicit_stop_required"
                    | "computer_not_stopped"
                    | "computer_destroyed"
                    | "invalid_input"
                    | "computer_control_held"
                    | "control_lease_expired"
                    | "capability_unavailable"
            )
        });
        sqlx::query("UPDATE computer_operations SET status=? WHERE id=? AND status='running'")
            .bind(if uncertain { "uncertain" } else { "completed" })
            .bind(&operation)
            .execute(&self.pool)
            .await
            .map_err(|_| ApiError::internal("computer execution result is uncertain"))?;
        result
    }
}
