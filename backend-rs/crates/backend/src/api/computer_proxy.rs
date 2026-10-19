//! Dispatch-scoped proxy. Vendor credentials are resolved here and never leave
//! this process; streams are never replayed after an ambiguous disconnect.
use super::{error::ApiError, AppState};
use crate::computers::{ComputerExecution, SupervisorClient};
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqlitePool};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

fn hash(value: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(value.as_ref()))
}
fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
fn storage(_: sqlx::Error) -> ApiError {
    ApiError::internal("Proxy authorization unavailable")
}
fn denied() -> ApiError {
    ApiError::permission_denied("Computer proxy authorization expired or unavailable")
}

struct CallGuard {
    pool: SqlitePool,
    id: String,
}

// Vendors occasionally echo authentication failures inside a successful SSE
// response. Hold a suffix so even a key split across network chunks is masked.
struct Redactor {
    pending: Vec<u8>,
    secrets: Vec<Vec<u8>>,
    keep: usize,
}
impl Redactor {
    fn new(secrets: Vec<String>) -> Self {
        let secrets: Vec<Vec<u8>> = secrets
            .into_iter()
            .filter(|s| !s.is_empty())
            .map(String::into_bytes)
            .collect();
        let keep = secrets
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or(1)
            .saturating_sub(1);
        Self {
            pending: Vec::new(),
            secrets,
            keep,
        }
    }
    fn feed(&mut self, data: &[u8], eof: bool) -> Vec<u8> {
        self.pending.extend_from_slice(data);
        let limit = if eof {
            self.pending.len()
        } else {
            self.pending.len().saturating_sub(self.keep)
        };
        let mut position = 0;
        let mut output = Vec::new();
        while position < limit {
            if let Some(secret) = self
                .secrets
                .iter()
                .filter(|secret| self.pending[position..].starts_with(secret))
                .max_by_key(|secret| secret.len())
            {
                output.extend_from_slice(b"[redacted]");
                position += secret.len();
            } else {
                output.push(self.pending[position]);
                position += 1;
            }
        }
        self.pending.drain(..position);
        output
    }
}
impl Drop for CallGuard {
    fn drop(&mut self) {
        let pool = self.pool.clone();
        let id = self.id.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {let _=sqlx::query("UPDATE computer_model_calls SET status='unknown' WHERE id=? AND status='running'").bind(id).execute(&pool).await;});
        }
    }
}
pub async fn recover(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let mut tx = crate::db::begin_write(pool).await?;
    sqlx::query("UPDATE computer_proxy_grants SET revoked=1 WHERE revoked=0")
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE computer_model_calls SET status='unknown' WHERE status='running'")
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

pub fn router(state: AppState) -> axum::Router {
    axum::Router::new()
        .route(
            "/internal/computer/model/chat/completions",
            axum::routing::post(model),
        )
        .route("/internal/computer/notes", axum::routing::post(notes))
        .layer(axum::extract::DefaultBodyLimit::max(8 * 1024 * 1024))
        .with_state(state)
}

#[derive(Clone, FromRow)]
pub struct Grant {
    pub token_hash: String,
    pub scope: String,
    pub owner_id: String,
    pub computer_id: String,
    pub generation: String,
    pub agent_id: String,
    pub turn_id: String,
    pub job_id: String,
    pub provider_id: Option<String>,
    pub model: String,
    pub expires_at: i64,
    pub revoked: bool,
}
pub struct DispatchGrants {
    pub model: String,
    pub notes: String,
    pub model_name: String,
    pub hashes: Vec<String>,
}
impl DispatchGrants {
    pub async fn revoke(&self, pool: &SqlitePool) {
        for token in &self.hashes {
            let _ = sqlx::query("UPDATE computer_proxy_grants SET revoked=1 WHERE token_hash=?")
                .bind(token)
                .execute(pool)
                .await;
        }
    }
}
pub async fn issue(
    pool: &SqlitePool,
    computer: &ComputerExecution,
    owner: &str,
    agent: &str,
    turn: &str,
) -> anyhow::Result<DispatchGrants> {
    issue_for_model(pool, computer, owner, agent, turn, None).await
}
pub async fn issue_for_model(
    pool: &SqlitePool,
    computer: &ComputerExecution,
    owner: &str,
    agent: &str,
    turn: &str,
    requested_model: Option<&str>,
) -> anyhow::Result<DispatchGrants> {
    let (job,provider,mut model):(String,String,String)=sqlx::query_as("SELECT j.id,p.id,COALESCE(json_extract(a.model_config_json,'$.model'),p.default_model) FROM execution_jobs j JOIN agents a ON a.id=? AND a.owner_id=j.owner_id JOIN llm_providers p ON p.id=a.provider_id AND p.owner_id=j.owner_id AND p.status='active' WHERE j.turn_id=? AND j.owner_id=? AND j.status='running' AND p.kind IN ('openai','openai_compatible','openai-compatible','deepseek','vllm','openrouter')")
        .bind(agent).bind(turn).bind(owner).fetch_optional(pool).await?.ok_or_else(||anyhow::anyhow!("Computer OpenCode requires an owned OpenAI-compatible API provider and an active job"))?;
    // This selection comes from the owned Agent config, never from a model
    // request in the container. The issued grant authorizes exactly one model.
    if let Some(requested) = requested_model.filter(|v| !v.trim().is_empty()) {
        model = requested
            .strip_prefix("qunica/")
            .unwrap_or(requested)
            .to_string();
        anyhow::ensure!(
            model.len() <= 256 && !model.contains(char::is_control),
            "Invalid computer model selection"
        );
    }
    let mut tokens = Vec::new();
    let mut hashes = Vec::new();
    for scope in ["model", "notes"] {
        let token = Uuid::new_v4().to_string() + &Uuid::new_v4().to_string();
        let digest = hash(&token);
        sqlx::query("INSERT INTO computer_proxy_grants(token_hash,scope,owner_id,computer_id,generation,agent_id,turn_id,job_id,provider_id,model,expires_at) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
            .bind(&digest).bind(scope).bind(owner).bind(&computer.id).bind(&computer.generation).bind(agent).bind(turn).bind(&job).bind(&provider).bind(&model).bind(now()+3600).execute(pool).await?;
        tokens.push(token);
        hashes.push(digest);
    }
    Ok(DispatchGrants {
        model: tokens[0].clone(),
        notes: tokens[1].clone(),
        model_name: model,
        hashes,
    })
}
pub async fn authorize(
    pool: &SqlitePool,
    client: Option<&Arc<SupervisorClient>>,
    headers: &HeaderMap,
    scope: &str,
) -> Result<Grant, ApiError> {
    if headers.contains_key("origin") {
        return Err(denied());
    }
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|v| v.len() <= 128)
        .ok_or_else(denied)?;
    let grant:Grant=sqlx::query_as("SELECT * FROM computer_proxy_grants WHERE token_hash=? AND scope=? AND revoked=0 AND expires_at>?").bind(hash(token)).bind(scope).bind(now()).fetch_optional(pool).await.map_err(storage)?.ok_or_else(denied)?;
    let source = headers
        .get("x-qunica-computer")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let generation = headers
        .get("x-qunica-generation")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let proof = headers
        .get("x-qunica-egress")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if source != grant.computer_id
        || generation != grant.generation
        || !client.is_some_and(|c| c.verify_egress(source, generation, proof))
    {
        return Err(denied());
    }
    validate(pool, client, &grant).await?;
    Ok(grant)
}
async fn validate(
    pool: &SqlitePool,
    client: Option<&Arc<SupervisorClient>>,
    grant: &Grant,
) -> Result<(), ApiError> {
    let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM computer_proxy_grants x JOIN execution_jobs j ON j.id=x.job_id JOIN group_turns t ON t.id=x.turn_id JOIN agents a ON a.id=x.agent_id JOIN groups g ON g.id=t.group_id JOIN computers c ON c.id=x.computer_id WHERE x.token_hash=? AND x.scope=? AND x.revoked=0 AND x.expires_at>? AND j.id=? AND j.owner_id=x.owner_id AND j.status='running' AND t.status='running' AND a.owner_id=x.owner_id AND a.status='active' AND g.owner_id=x.owner_id AND g.status='active' AND c.owner_id=x.owner_id AND COALESCE(a.execution_environment_id,g.execution_environment_id)=c.id)")
        .bind(&grant.token_hash).bind(&grant.scope).bind(now()).bind(&grant.job_id).fetch_one(pool).await.map_err(storage)?;
    if !active || grant.revoked || grant.expires_at <= now() {
        return Err(denied());
    }
    let client = client.ok_or_else(denied)?;
    let computer = client
        .request(
            reqwest::Method::GET,
            &format!("/v1/computers/{}", grant.computer_id),
            None,
        )
        .await?;
    if computer["generation"] != grant.generation
        || !matches!(computer["status"].as_str(), Some("running" | "executing"))
    {
        return Err(denied());
    }
    let control = client
        .request(
            reqwest::Method::POST,
            &format!("/v1/computers/{}/control", grant.computer_id),
            Some(&json!({"expected_generation":grant.generation,"action":"status"})),
        )
        .await?;
    if control["state"] != "agent_controlled" {
        return Err(denied());
    }
    Ok(())
}
#[derive(FromRow)]
struct Provider {
    base_url: Option<String>,
    api_key: String,
    headers_json: String,
    user_agent: Option<String>,
}

pub async fn model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut body): Json<Value>,
) -> Result<Response, ApiError> {
    let grant = authorize(state.db.pool(), state.computers.as_ref(), &headers, "model").await?;
    if body["model"] != grant.model || !body["messages"].is_array() || body["stream"] != true {
        return Err(ApiError::invalid_input(
            "Only authorized streaming chat completions are supported",
        ));
    }
    let allowed = [
        "model",
        "messages",
        "stream",
        "stream_options",
        "tools",
        "tool_choice",
        "parallel_tool_calls",
        "temperature",
        "top_p",
        "max_tokens",
        "max_completion_tokens",
        "stop",
        "presence_penalty",
        "frequency_penalty",
        "seed",
        "response_format",
        "reasoning_effort",
    ];
    if body
        .as_object()
        .is_none_or(|o| o.keys().any(|k| !allowed.contains(&k.as_str())))
    {
        return Err(ApiError::invalid_input("Unsupported proxy field"));
    }
    let max = body
        .get("max_completion_tokens")
        .or_else(|| body.get("max_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(4096);
    if !(1..=8192).contains(&max) {
        return Err(ApiError::invalid_input(
            "Output token budget must be 1–8192",
        ));
    }
    if body.get("max_completion_tokens").is_some() {
        body["max_completion_tokens"] = json!(max);
    } else {
        body["max_tokens"] = json!(max);
    }
    body["stream_options"] = json!({"include_usage":true});
    let bytes = body.to_string();
    // Reserve conservatively before contacting the vendor. Unknown usage keeps
    // its reservation, including after a restart or a new dispatch credential.
    let reserve = bytes.len() as i64 + max;
    let id = Uuid::new_v4().to_string();
    let mut tx = crate::db::begin_write(state.db.pool())
        .await
        .map_err(storage)?;
    let (count,tokens,running):(i64,i64,i64)=sqlx::query_as("SELECT COUNT(*),COALESCE(SUM(COALESCE(input_tokens+output_tokens,reserved_tokens)),0),COALESCE(SUM(status='running'),0) FROM computer_model_calls WHERE turn_id=?").bind(&grant.turn_id).fetch_one(&mut *tx).await.map_err(storage)?;
    let budget:i64=sqlx::query_scalar("SELECT MIN(131072,g.max_total_tokens,COALESCE(json_extract(t.config_snapshot_json,'$.max_total_tokens'),g.max_total_tokens),COALESCE(json_extract(j.automation_snapshot_json,'$.budget.max_total_tokens'),131072)) FROM execution_jobs j JOIN group_turns t ON t.id=j.turn_id JOIN groups g ON g.id=j.group_id WHERE j.id=?").bind(&grant.job_id).fetch_one(&mut *tx).await.map_err(storage)?;
    if count >= 64 || tokens + reserve > budget || running >= 1 {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "proxy_budget_exhausted",
            "Model call, concurrency or token budget exhausted",
        ));
    }
    let inserted=sqlx::query("INSERT INTO computer_model_calls(id,token_hash,turn_id,request_hash,status,reserved_tokens) VALUES(?,?,?,?,'running',?) ON CONFLICT(token_hash,request_hash) DO NOTHING").bind(&id).bind(&grant.token_hash).bind(&grant.turn_id).bind(hash(&bytes)).bind(reserve).execute(&mut *tx).await.map_err(storage)?.rows_affected();
    if inserted != 1 {
        return Err(ApiError::conflict(
            "This model request was already submitted; inference is not replayed",
        ));
    }
    tx.commit().await.map_err(storage)?;
    let guard = CallGuard {
        pool: state.db.pool().clone(),
        id: id.clone(),
    };
    let provider:Provider=sqlx::query_as("SELECT base_url,api_key,headers_json,user_agent FROM llm_providers WHERE id=? AND owner_id=? AND status='active'").bind(&grant.provider_id).bind(&grant.owner_id).fetch_optional(state.db.pool()).await.map_err(storage)?.ok_or_else(denied)?;
    let upstream = provider
        .base_url
        .unwrap_or_else(|| "https://api.openai.com/v1".into());
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|_| ApiError::internal("Proxy transport unavailable"))?;
    let mut req = client
        .post(format!(
            "{}/chat/completions",
            upstream.trim_end_matches('/')
        ))
        .bearer_auth(&provider.api_key)
        .header("content-type", "application/json")
        .body(bytes);
    let mut sensitive = vec![provider.api_key.clone()];
    if let Ok(extra) =
        serde_json::from_str::<std::collections::BTreeMap<String, String>>(&provider.headers_json)
    {
        for (k, v) in extra {
            if !matches!(
                k.to_ascii_lowercase().as_str(),
                "accept" | "content-type" | "user-agent" | "accept-encoding"
            ) {
                sensitive.push(v.clone());
            }
            if !matches!(
                k.to_ascii_lowercase().as_str(),
                "host" | "content-length" | "transfer-encoding" | "connection"
            ) {
                req = req.header(k, v);
            }
        }
    }
    if sensitive.iter().any(|v| v.len() > 16384) {
        return Err(ApiError::invalid_input(
            "Provider authentication value exceeds the proxy limit",
        ));
    }
    if let Some(agent) = provider.user_agent {
        req = req.header("user-agent", agent);
    }
    // No upstream error body is returned: vendors can echo an API key in it.
    let sending = req.send();
    tokio::pin!(sending);
    let mut auth_check = tokio::time::interval(Duration::from_millis(500));
    let response = loop {
        tokio::select! {
            response=&mut sending=>break response,
            _=auth_check.tick()=>if validate(state.db.pool(),state.computers.as_ref(),&grant).await.is_err() {return Err(denied());},
        }
    };
    let mut response = match response {
        Ok(res) if res.status().is_success() => res,
        _ => {
            finish(&state, &id, None, "unknown").await;
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "model_proxy_failed",
                "Model provider request failed; it was not retried",
            ));
        }
    };
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(8);
    let call = id.clone();
    tokio::spawn(async move {
        let _guard = guard;
        let mut timer = tokio::time::interval(Duration::from_millis(500));
        let mut pending = Vec::new();
        let mut usage = None;
        let mut done = false;
        let mut eof = false;
        let mut redactor = Redactor::new(sensitive);
        loop {
            tokio::select! {
                biased;
                _=tx.closed()=>break,
                _=timer.tick()=>if validate(state.db.pool(),state.computers.as_ref(),&grant).await.is_err() { break; },
                chunk=response.chunk()=>match chunk {
                    Ok(Some(chunk))=>{
                        pending.extend_from_slice(&chunk);
                        while let Some(end)=pending.iter().position(|b|*b==b'\n') {
                            let line:Vec<u8>=pending.drain(..=end).collect();
                            if let Some(data)=std::str::from_utf8(&line).ok().and_then(|s|s.strip_prefix("data:")) {
                                let data=data.trim();
                                if data=="[DONE]" { done=true; }
                                if let Ok(value)=serde_json::from_str::<Value>(data) { if let (Some(input),Some(output))=(value["usage"]["prompt_tokens"].as_i64(),value["usage"]["completion_tokens"].as_i64()) { usage=Some((input.max(0),output.max(0))); } }
                            }
                        }
                        if pending.len()>1024*1024 { break; }
                        // A bounded send must remain revocable even when the
                        // caller stops reading. Never retain a DB transaction.
                        let safe=redactor.feed(&chunk,false);
                        if !safe.is_empty() && tokio::time::timeout(Duration::from_secs(1),tx.send(Ok(Bytes::from(safe)))).await.is_err() { break; }
                    },
                    Ok(None)=>{eof=true;break;},
                    Err(_)=>break,
                }
            }
        }
        drop(response);
        if eof {
            let remaining = redactor.feed(&[], true);
            if !remaining.is_empty() {
                let _ = tokio::time::timeout(
                    Duration::from_secs(1),
                    tx.send(Ok(Bytes::from(remaining))),
                )
                .await;
            }
        }
        finish(
            &state,
            &call,
            usage,
            if done { "completed" } else { "unknown" },
        )
        .await;
    });
    let stream = futures_util::stream::unfold(rx, |mut rx| async {
        rx.recv().await.map(|item| (item, rx))
    });
    let mut response = Body::from_stream(stream).into_response();
    response
        .headers_mut()
        .insert("content-type", "text/event-stream".parse().unwrap());
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert("x-model-call-id", id.parse().unwrap());
    Ok(response)
}

async fn finish(state: &AppState, id: &str, usage: Option<(i64, i64)>, status: &str) {
    let outcome:Result<(),sqlx::Error>=async {
    let mut tx=crate::db::begin_write(state.db.pool()).await?;
    sqlx::query(
        "UPDATE computer_model_calls SET status=?,input_tokens=?,output_tokens=? WHERE id=?",
    )
    .bind(status)
    .bind(usage.map(|v| v.0))
    .bind(usage.map(|v| v.1))
    .bind(id)
    .execute(&mut *tx)
    .await?;
    if let Some((input, output)) = usage {
        sqlx::query("INSERT INTO token_usage_records(id,owner_id,group_id,group_name,conversation_kind,thread_id,agent_id,agent_name,provider_id,provider_name,model,input_tokens,output_tokens,total_tokens,created_at,updated_at) SELECT c.id,x.owner_id,g.id,g.name,g.conversation_kind,t.thread_id,a.id,a.name,p.id,p.name,x.model,?,?,?,strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now') FROM computer_model_calls c JOIN computer_proxy_grants x ON x.token_hash=c.token_hash JOIN group_turns t ON t.id=x.turn_id JOIN groups g ON g.id=t.group_id JOIN agents a ON a.id=x.agent_id JOIN llm_providers p ON p.id=x.provider_id WHERE c.id=? ON CONFLICT(id) DO NOTHING").bind(input).bind(output).bind(input.saturating_add(output)).bind(id).execute(&mut *tx).await?;
    }
    tx.commit().await
    }.await;
    if let Err(error) = outcome {
        tracing::warn!(call_id=id,%error,"Model proxy usage settlement failed");
    }
}
pub async fn notes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let grant = authorize(state.db.pool(), state.computers.as_ref(), &headers, "notes").await?;
    let (group, thread): (String, String) =
        sqlx::query_as("SELECT group_id,thread_id FROM group_turns WHERE id=?")
            .bind(&grant.turn_id)
            .fetch_one(state.db.pool())
            .await
            .map_err(storage)?;
    Ok(crate::acp::notes_mcp::remote_call(
        crate::acp::notes_mcp::NotesContext {
            pool: state.db.pool().clone(),
            write_lock: state.write_lock.clone(),
            owner_id: grant.owner_id,
            group_id: group,
            agent_id: grant.agent_id,
            thread_id: thread,
            decision: None,
        },
        body,
    )
    .await)
}

#[cfg(test)]
mod redaction_tests {
    use super::Redactor;
    #[test]
    fn credentials_split_across_chunks_are_masked_without_changing_other_bytes() {
        let mut redact = Redactor::new(vec!["long-vendor-secret".into()]);
        let mut output = redact.feed(b"data: long-vendor-", false);
        output.extend(redact.feed(b"secret\n\ndata: [DONE]\n\n", false));
        output.extend(redact.feed(&[], true));
        assert_eq!(output, b"data: [redacted]\n\ndata: [DONE]\n\n");
    }
}
