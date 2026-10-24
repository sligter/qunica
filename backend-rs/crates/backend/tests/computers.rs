use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use qunica_backend::{
    api::{self, AppState},
    computers::{ComputerExecution, SupervisorClient},
    tools::{ApprovalGrants, ToolExecutor, ToolStatus},
};
use qunica_computer_supervisor::{
    protocol::{Computer, ExecRequest, ExecResult},
    ComputerProvider, Error, Supervisor,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "computer-supervisor-test-secret-with-32-bytes";
const VENDOR_KEY: &str = "vendor-secret-must-never-reach-computer";
#[derive(Default)]
struct Provider {
    calls: Mutex<Vec<ExecRequest>>,
}
#[async_trait]
impl ComputerProvider for Provider {
    async fn create(&self, _: &Computer) -> Result<(), Error> {
        Ok(())
    }
    async fn start(&self, _: &Computer) -> Result<(), Error> {
        Ok(())
    }
    async fn stop(&self, _: &Computer) -> Result<(), Error> {
        Ok(())
    }
    async fn rebuild(&self, _: &Computer) -> Result<(), Error> {
        Ok(())
    }
    async fn destroy(&self, _: &Computer, _: bool) -> Result<(), Error> {
        Ok(())
    }
    async fn execute(&self, _: &Computer, request: ExecRequest) -> Result<ExecResult, Error> {
        self.calls.lock().await.push(request);
        Ok(ExecResult {
            exit_code: 0,
            stdout: "remote output".into(),
            stderr: String::new(),
            truncated: false,
        })
    }
}
async fn request(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        req = req.header("authorization", format!("Bearer {token}"));
    }
    let res = app
        .clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn fixture() -> (
    Router,
    AppState,
    String,
    String,
    String,
    String,
    Arc<Provider>,
    tempfile::TempDir,
    tokio::task::JoinHandle<()>,
) {
    let root = tempfile::tempdir().unwrap();
    let provider = Arc::new(Provider::default());
    let supervisor = Supervisor::open(root.path().into(), TOKEN, provider.clone())
        .unwrap()
        .router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, supervisor).await.unwrap() });
    let (_, mut state) = api::router_with_state_for_tests().await;
    state.computers = Some(Arc::new(
        SupervisorClient::new(&format!("http://{addr}"), TOKEN.into()).unwrap(),
    ));
    let app = api::router(state.clone());
    let (_, user) = request(
        &app,
        "POST",
        "/api/v2/auth/register",
        None,
        json!({"email":"computer@example.test","name":"Computer","password":"test-password"}),
    )
    .await;
    let owner = user["id"].as_str().unwrap().to_string();
    let (_, login) = request(
        &app,
        "POST",
        "/api/v2/auth/login",
        None,
        json!({"email":"computer@example.test","password":"test-password"}),
    )
    .await;
    let token = login["access_token"].as_str().unwrap().to_string();
    let group = Uuid::new_v4().to_string();
    let agent = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO groups(id,owner_id,name,status,created_at,updated_at) VALUES(?,?,'team','active','now','now')").bind(&group).bind(&owner).execute(state.db.pool()).await.unwrap();
    sqlx::query("INSERT INTO agents(id,owner_id,name,system_prompt,status,created_at,updated_at) VALUES(?,?,'agent','','active','now','now')").bind(&agent).bind(&owner).execute(state.db.pool()).await.unwrap();
    (
        app, state, token, owner, group, agent, provider, root, server,
    )
}
async fn create(app: &Router, token: &str, group: &str) -> Value {
    let (status, value) = request(
        app,
        "POST",
        "/api/v2/computers",
        Some(token),
        json!({"id":Uuid::new_v4(),"name":"Team computer","binding":{"kind":"team","id":group}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{value}");
    value
}
async fn bind(
    app: &Router,
    token: &str,
    scope: &str,
    target: &str,
    id: Value,
) -> (StatusCode, Value) {
    request(
        app,
        "PUT",
        "/api/v2/execution-environments/binding",
        Some(token),
        json!({"scope":scope,"target_id":target,"execution_environment_id":id}),
    )
    .await
}

#[tokio::test]
async fn binding_is_explicit_scoped_and_rejects_unsupported_acp_and_active_rebinding() {
    let (app, state, token, _owner, group, agent, _provider, _root, server) = fixture().await;
    let computer = create(&app, &token, &group).await;
    let id = computer["spec"]["id"].clone();
    assert_eq!(
        bind(&app, &token, "agent", &agent, id.clone()).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        bind(&app, &token, "group", &group, id.clone()).await.0,
        StatusCode::OK
    );
    sqlx::query("UPDATE agents SET runtime_kind='acp' WHERE id=?")
        .bind(&agent)
        .execute(state.db.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO group_agents(group_id,agent_id,status,joined_at,updated_at) VALUES(?,?,'active','now','now')").bind(&group).bind(&agent).execute(state.db.pool()).await.unwrap();
    assert_eq!(
        bind(&app, &token, "group", &group, id.clone()).await.0,
        StatusCode::BAD_REQUEST
    );
    sqlx::query("UPDATE agents SET runtime_kind='llm' WHERE id=?")
        .bind(&agent)
        .execute(state.db.pool())
        .await
        .unwrap();
    let owner: String = sqlx::query_scalar("SELECT owner_id FROM groups WHERE id=?")
        .bind(&group)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO execution_jobs(id,owner_id,request_id,command_name,group_id,thread_id,turn_id,stream_id,payload_json,status,worker_active,occupies_thread,created_at,updated_at) VALUES('busy',?,'busy','message.send',?,'thread','turn','stream','{}','running',1,1,'now','now')").bind(owner).bind(&group).execute(state.db.pool()).await.unwrap();
    assert_eq!(
        bind(&app, &token, "group", &group, Value::Null).await.0,
        StatusCode::CONFLICT
    );
    let (_, other) = request(
        &app,
        "POST",
        "/api/v2/auth/register",
        None,
        json!({"email":"other@example.test","name":"Other","password":"test-password"}),
    )
    .await;
    assert!(other["id"].is_string());
    let (_, login) = request(
        &app,
        "POST",
        "/api/v2/auth/login",
        None,
        json!({"email":"other@example.test","password":"test-password"}),
    )
    .await;
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("/api/v2/computers/{}", id.as_str().unwrap()),
            login["access_token"].as_str(),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    server.abort();
}

#[tokio::test]
async fn remote_tools_require_live_turn_and_generation_and_never_use_host_workspace() {
    let (app, state, token, owner, group, agent, provider, _root, server) = fixture().await;
    let computer = create(&app, &token, &group).await;
    let id = computer["spec"]["id"].clone();
    assert_eq!(
        bind(&app, &token, "group", &group, id.clone()).await.0,
        StatusCode::OK
    );
    sqlx::query("INSERT INTO threads(id,group_id,status,created_at,updated_at) VALUES('thread',?,'running','now','now')").bind(&group).execute(state.db.pool()).await.unwrap();
    sqlx::query("INSERT INTO group_turns(id,thread_id,group_id,status,scheduler_strategy,config_snapshot_json,topology_snapshot_json,created_at,updated_at) VALUES('turn','thread',?,'running','bounded','{}','{}','now','now')").bind(&group).execute(state.db.pool()).await.unwrap();
    let execution = ComputerExecution::resolve(
        state.db.pool(),
        state.computers.clone(),
        &owner,
        &group,
        &agent,
        "turn",
    )
    .await
    .unwrap()
    .unwrap();
    sqlx::query("INSERT INTO threads(id,group_id,status,created_at,updated_at) VALUES('other-thread',?,'active','now','now')").bind(&group).execute(state.db.pool()).await.unwrap();
    sqlx::query("INSERT INTO group_turns(id,thread_id,group_id,status,scheduler_strategy,config_snapshot_json,topology_snapshot_json,created_at,updated_at) VALUES('other-turn','other-thread',?,'running','bounded','{}','{}','now','now')").bind(&group).execute(state.db.pool()).await.unwrap();
    assert!(
        ComputerExecution::resolve(
            state.db.pool(),
            state.computers.clone(),
            &owner,
            &group,
            &agent,
            "other-turn"
        )
        .await
        .is_err(),
        "another group/thread cannot become a second computer driver"
    );
    let host = tempfile::tempdir().unwrap();
    let executor = ToolExecutor::new(Some(host.path().into()))
        .unwrap()
        .with_computer(execution);
    assert!(executor.workspace_root().is_none());
    let result = executor
        .execute("Bash", json!({"command":"touch must-not-exist"}))
        .await;
    assert_eq!(result.status, ToolStatus::ApprovalRequired);
    assert!(provider.calls.lock().await.is_empty());
    let result = executor
        .execute("Write", json!({"path":"artifact.txt","content":"remote"}))
        .await;
    assert_eq!(result.status, ToolStatus::Completed, "{}", result.output);
    assert!(!host.path().join("artifact.txt").exists());
    let calls = provider.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].argv[0], "python3");
    assert_eq!(calls[0].expected_generation, "1");
    drop(calls);
    let mut grants = ApprovalGrants::default();
    grants.set_bypass_all(true);
    let executor = executor.with_approvals(grants);
    assert_eq!(
        executor
            .execute("Bash", json!({"command":"echo bound"}))
            .await
            .status,
        ToolStatus::Completed
    );
    let (status, _) = request(
        &app,
        "POST",
        &format!("/api/v2/computers/{}/actions", id.as_str().unwrap()),
        Some(&token),
        json!({"expected_generation":"1","action":"stop"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        executor
            .execute("Bash", json!({"command":"echo stale"}))
            .await
            .status,
        ToolStatus::Failed
    );
    assert_eq!(
        provider.calls.lock().await.len(),
        2,
        "stale generation never executes"
    );
    sqlx::query("UPDATE group_turns SET status='cancelled' WHERE id='turn'")
        .execute(state.db.pool())
        .await
        .unwrap();
    assert_eq!(
        executor
            .execute("Write", json!({"path":"late.txt","content":"late"}))
            .await
            .status,
        ToolStatus::Failed
    );
    assert_eq!(provider.calls.lock().await.len(), 2);
    assert!(
        ComputerExecution::resolve(state.db.pool(), None, &owner, &group, &agent, "turn")
            .await
            .is_err()
    );
    server.abort();
}

#[tokio::test]
async fn remote_branch_task_maps_file_and_shell_cwd_without_a_host_workspace() {
    let (app, state, token, owner, group, agent, provider, _root, server) = fixture().await;
    let computer = create(&app, &token, &group).await;
    assert_eq!(
        bind(
            &app,
            &token,
            "group",
            &group,
            computer["spec"]["id"].clone()
        )
        .await
        .0,
        StatusCode::OK
    );
    for branch in [
        "../escape",
        "--detach",
        "foo.lock",
        "HEAD",
        "has space",
        "foo@{bar",
    ] {
        assert_eq!(
            request(
                &app,
                "POST",
                &format!("/api/v2/groups/{group}/threads"),
                Some(&token),
                json!({"title":"Invalid", "git_branch":branch})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    let (status, task) = request(
        &app,
        "POST",
        &format!("/api/v2/groups/{group}/threads"),
        Some(&token),
        json!({"title":"Remote branch", "git_branch":"feature/computer"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{task}");
    let thread = task["id"].as_str().unwrap();
    assert_eq!(task["git_branch"], "feature/computer");
    assert_eq!(
        task["worktree_path"],
        format!("computer:.worktrees/{group}/{thread}")
    );
    assert!(
        provider.calls.lock().await.is_empty(),
        "creation only accepts metadata; Runtime reserves before provisioning"
    );
    sqlx::query("INSERT INTO group_turns(id,thread_id,group_id,status,scheduler_strategy,config_snapshot_json,topology_snapshot_json,created_at,updated_at) VALUES('branch-turn',?,?,'running','bounded','{}','{}','now','now')").bind(thread).bind(&group).execute(state.db.pool()).await.unwrap();
    let execution = ComputerExecution::resolve(
        state.db.pool(),
        state.computers.clone(),
        &owner,
        &group,
        &agent,
        "branch-turn",
    )
    .await
    .unwrap()
    .unwrap();
    let expected = format!(".worktrees/{group}/{thread}");
    assert_eq!(execution.relative_workspace(), expected);
    let mut grants = ApprovalGrants::default();
    grants.set_bypass_all(true);
    let executor = ToolExecutor::without_workspace()
        .with_computer(execution)
        .with_approvals(grants);
    assert_eq!(
        executor
            .execute("Write", json!({"path":"in-task.txt","content":"task"}))
            .await
            .status,
        ToolStatus::Completed
    );
    assert_eq!(
        executor
            .execute("Bash", json!({"command":"pwd"}))
            .await
            .status,
        ToolStatus::Completed
    );
    let calls = provider.calls.lock().await;
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].cwd, "", "provision within the computer repository");
    assert_eq!(calls[1].cwd, expected);
    assert_eq!(calls[2].cwd, expected);
    drop(calls);
    server.abort();
}

async fn proxy_fixture() -> (
    AppState,
    Router,
    qunica_backend::api::computer_proxy::DispatchGrants,
    Arc<std::sync::atomic::AtomicUsize>,
    tempfile::TempDir,
    tokio::task::JoinHandle<()>,
    tokio::task::JoinHandle<()>,
) {
    let (app, state, token, owner, group, agent, _provider, root, supervisor) = fixture().await;
    let computer = create(&app, &token, &group).await;
    bind(
        &app,
        &token,
        "group",
        &group,
        computer["spec"]["id"].clone(),
    )
    .await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let upstream_calls = calls.clone();
    let upstream=Router::new().route("/v1/chat/completions",axum::routing::post(move |headers:axum::http::HeaderMap,axum::Json(body):axum::Json<Value>| {
        let calls=upstream_calls.clone();async move {
            assert_eq!(headers["authorization"],format!("Bearer {VENDOR_KEY}"));assert_eq!(body["model"],"fixture-model");
            calls.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
            let text="data: {\"choices\":[{\"delta\":{\"content\":\"hello\",\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"x\\\":1}\"}}]}}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":7}}\n\ndata: [DONE]\n\n";
            ([("content-type","text/event-stream")],text)
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let provider = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO llm_providers(id,owner_id,name,kind,base_url,api_key,default_model,created_at,updated_at) VALUES(?,?,'proxy fixture','openai-compatible',?,?,'fixture-model','now','now')").bind(&provider).bind(&owner).bind(format!("http://{address}/v1")).bind(VENDOR_KEY).execute(state.db.pool()).await.unwrap();
    sqlx::query("UPDATE agents SET provider_id=? WHERE id=?")
        .bind(&provider)
        .bind(&agent)
        .execute(state.db.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO threads(id,group_id,status,created_at,updated_at) VALUES('proxy-thread',?,'active','now','now')").bind(&group).execute(state.db.pool()).await.unwrap();
    sqlx::query("INSERT INTO group_turns(id,thread_id,group_id,status,scheduler_strategy,config_snapshot_json,topology_snapshot_json,created_at,updated_at) VALUES('proxy-turn','proxy-thread',?,'running','bounded','{}','{}','now','now')").bind(&group).execute(state.db.pool()).await.unwrap();
    sqlx::query("INSERT INTO execution_jobs(id,owner_id,request_id,command_name,group_id,thread_id,turn_id,stream_id,payload_json,status,created_at,updated_at) VALUES('proxy-job',?,'request','message.send',?,'proxy-thread','proxy-turn','stream','{}','running','now','now')").bind(&owner).bind(&group).execute(state.db.pool()).await.unwrap();
    let computer = ComputerExecution::resolve(
        state.db.pool(),
        state.computers.clone(),
        &owner,
        &group,
        &agent,
        "proxy-turn",
    )
    .await
    .unwrap()
    .unwrap();
    let grants = qunica_backend::api::computer_proxy::issue(
        state.db.pool(),
        &computer,
        &owner,
        &agent,
        "proxy-turn",
    )
    .await
    .unwrap();
    let source = computer.id.clone();
    let signer = state.computers.clone().unwrap();
    let proof = signer.egress_attestation(&source, "1");
    let proxy = qunica_backend::api::computer_proxy::router(state.clone()).layer(
        axum::middleware::from_fn(
            move |mut request: axum::extract::Request, next: axum::middleware::Next| {
                request
                    .headers_mut()
                    .insert("x-qunica-computer", source.parse().unwrap());
                request
                    .headers_mut()
                    .insert("x-qunica-generation", "1".parse().unwrap());
                request
                    .headers_mut()
                    .insert("x-qunica-egress", proof.parse().unwrap());
                async move { next.run(request).await }
            },
        ),
    );
    (state, proxy, grants, calls, root, supervisor, server)
}
async fn proxy_request(app: &Router, token: &str, body: Value) -> (StatusCode, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/internal/computer/model/chat/completions")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}
fn completion() -> Value {
    json!({"model":"fixture-model","stream":true,"messages":[{"role":"user","content":"hello"}],"max_tokens":100})
}
#[tokio::test]
async fn model_proxy_preserves_streams_accounts_once_and_never_replays() {
    let (state, proxy, grants, calls, _root, supervisor, server) = proxy_fixture().await;
    let (status, stream) = proxy_request(&proxy, &grants.model, completion()).await;
    assert_eq!(status, StatusCode::OK, "{stream}");
    assert!(stream.contains("hello"));
    assert!(stream.contains("tool_calls"));
    assert!(stream.contains("[DONE]"));
    assert!(!stream.contains(VENDOR_KEY));
    let usage: (i64, i64, i64) =
        sqlx::query_as("SELECT input_tokens,output_tokens,total_tokens FROM token_usage_records")
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(usage, (5, 7, 12));
    let created: String = sqlx::query_scalar("SELECT created_at FROM token_usage_records")
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert!(
        chrono::DateTime::parse_from_rfc3339(&created).is_ok(),
        "Proxy usage follows the same timestamp contract as the existing ledger"
    );
    assert_eq!(
        proxy_request(&proxy, &grants.model, completion()).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        proxy_request(&proxy, &grants.notes, completion()).await.0,
        StatusCode::FORBIDDEN
    );
    grants.revoke(state.db.pool()).await;
    assert_eq!(
        proxy_request(&proxy, &grants.model, completion()).await.0,
        StatusCode::FORBIDDEN
    );
    let stored: Vec<String> = sqlx::query_scalar("SELECT token_hash FROM computer_proxy_grants")
        .fetch_all(state.db.pool())
        .await
        .unwrap();
    assert!(stored.iter().all(|v| v != &grants.model && v.len() == 64));
    supervisor.abort();
    server.abort();
}
#[tokio::test]
async fn proxy_rejects_wrong_model_arbitrary_routes_expiry_cancellation_and_overspending() {
    let (state, proxy, grants, calls, _root, supervisor, server) = proxy_fixture().await;
    let mut wrong = completion();
    wrong["model"] = json!("another-model");
    assert_eq!(
        proxy_request(&proxy, &grants.model, wrong).await.0,
        StatusCode::BAD_REQUEST
    );
    let mut wrong = completion();
    wrong["base_url"] = json!("http://metadata/");
    assert_eq!(
        proxy_request(&proxy, &grants.model, wrong).await.0,
        StatusCode::BAD_REQUEST
    );
    sqlx::query("UPDATE groups SET max_total_tokens=1")
        .execute(state.db.pool())
        .await
        .unwrap();
    assert_eq!(
        proxy_request(&proxy, &grants.model, completion()).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    sqlx::query("UPDATE groups SET max_total_tokens=120000")
        .execute(state.db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE execution_jobs SET automation_snapshot_json='{\"budget\":{\"max_total_tokens\":1}}' WHERE id='proxy-job'").execute(state.db.pool()).await.unwrap();
    assert_eq!(
        proxy_request(&proxy, &grants.model, completion()).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    sqlx::query("UPDATE execution_jobs SET automation_snapshot_json=NULL,status='cancelled' WHERE id='proxy-job'").execute(state.db.pool()).await.unwrap();
    assert_eq!(
        proxy_request(&proxy, &grants.model, completion()).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE execution_jobs SET status='running' WHERE id='proxy-job'")
        .execute(state.db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE computer_proxy_grants SET expires_at=0")
        .execute(state.db.pool())
        .await
        .unwrap();
    assert_eq!(
        proxy_request(&proxy, &grants.model, completion()).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(
        proxy_request(&api::router(state), &grants.model, completion())
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    supervisor.abort();
    server.abort();
}

#[tokio::test]
async fn proxy_requires_the_originating_computer_gateway_and_current_generation() {
    let (state, _proxy, grants, calls, _root, supervisor, server) = proxy_fixture().await;
    let private = api::computer_proxy::router(state.clone());
    assert_eq!(
        proxy_request(&private, &grants.model, completion()).await.0,
        StatusCode::FORBIDDEN
    );
    let current: String = sqlx::query_scalar("SELECT id FROM computers")
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    for (computer, generation) in [(Uuid::new_v4().to_string(), "1"), (current, "0")] {
        let proof = state
            .computers
            .as_ref()
            .unwrap()
            .egress_attestation(&computer, generation);
        let response = private
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/internal/computer/model/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", grants.model))
                    .header("x-qunica-computer", computer)
                    .header("x-qunica-generation", generation)
                    .header("x-qunica-egress", proof)
                    .body(Body::from(completion().to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    supervisor.abort();
    server.abort();
}

#[tokio::test]
async fn revocation_ends_a_half_open_model_stream_without_resubmission() {
    use axum::response::IntoResponse;
    use futures_util::StreamExt;
    let (state, proxy, grants, _calls, _root, supervisor, original) = proxy_fixture().await;
    original.abort();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = calls.clone();
    let upstream=Router::new().route("/v1/chat/completions",axum::routing::post(move || {
        let calls=counter.clone();async move {
            calls.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
            let initial=futures_util::stream::once(async {Ok::<_,std::io::Error>(axum::body::Bytes::from_static(b"data: {\"choices\":[{\"delta\":{\"content\":\"waiting for more model output\"}}]}\n\n"))});
            let body=Body::from_stream(initial.chain(futures_util::stream::pending()));
            ([("content-type","text/event-stream")],body).into_response()
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    sqlx::query("UPDATE llm_providers SET base_url=?")
        .bind(format!("http://{address}/v1"))
        .execute(state.db.pool())
        .await
        .unwrap();
    let response = proxy
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/internal/computer/model/chat/completions")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", grants.model))
                .body(Body::from(completion().to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let reading = tokio::spawn(async move {
        axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap()
    });
    sqlx::query("UPDATE execution_jobs SET status='cancelled' WHERE id='proxy-job'")
        .execute(state.db.pool())
        .await
        .unwrap();
    let bytes = tokio::time::timeout(std::time::Duration::from_secs(3), reading)
        .await
        .unwrap()
        .unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("[DONE]"));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    let status: String = sqlx::query_scalar("SELECT status FROM computer_model_calls")
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(status, "unknown");
    supervisor.abort();
    server.abort();
}

#[tokio::test]
async fn notes_grants_are_separate_and_fenced_by_membership_and_dispatch() {
    let (state, proxy, grants, _calls, _root, supervisor, server) = proxy_fixture().await;
    let (owner, group, agent): (String, String, String) =
        sqlx::query_as("SELECT owner_id,binding_id,(SELECT id FROM agents LIMIT 1) FROM computers")
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    let workspace = tempfile::tempdir().unwrap();
    sqlx::query("INSERT INTO workspaces(id,owner_id,name,backend_type,local_path,status,created_at,updated_at) VALUES('notes-workspace',?,'notes','local',?,'active','now','now')").bind(&owner).bind(workspace.path().to_string_lossy().as_ref()).execute(state.db.pool()).await.unwrap();
    sqlx::query("UPDATE groups SET workspace_id='notes-workspace' WHERE id=?")
        .bind(&group)
        .execute(state.db.pool())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO group_agents(group_id,agent_id,joined_at,updated_at) VALUES(?,?,'now','now')",
    )
    .bind(&group)
    .bind(&agent)
    .execute(state.db.pool())
    .await
    .unwrap();
    let create = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"CreateGroupNote","arguments":{"title":"Container note","content":"Dispatch-scoped bridge fixture"}}});
    let (status, note) = request(
        &proxy,
        "POST",
        "/internal/computer/notes",
        Some(&grants.notes),
        create.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{note}");
    assert_eq!(note["result"]["isError"], false, "{note}");
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM group_notes WHERE group_id=? AND author_id=?")
            .bind(&group)
            .bind(&owner)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(count, 1);
    assert_eq!(
        request(
            &proxy,
            "POST",
            "/internal/computer/notes",
            Some(&grants.model),
            create.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE group_agents SET status='removed'")
        .execute(state.db.pool())
        .await
        .unwrap();
    let (_, denied) = request(
        &proxy,
        "POST",
        "/internal/computer/notes",
        Some(&grants.notes),
        create.clone(),
    )
    .await;
    assert_eq!(denied["result"]["isError"], true, "{denied}");
    sqlx::query("UPDATE execution_jobs SET status='cancelled'")
        .execute(state.db.pool())
        .await
        .unwrap();
    assert_eq!(
        request(
            &proxy,
            "POST",
            "/internal/computer/notes",
            Some(&grants.notes),
            create
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    supervisor.abort();
    server.abort();
}

#[tokio::test]
#[ignore = "requires Docker execution and egress images; uses a local protocol fixture, no paid model"]
async fn docker_opencode_acp_uses_proxy_and_executes_tools_inside_computer() {
    use futures_util::FutureExt;
    use qunica_backend::acp::{self, AcpRunRequest, PermissionPolicy};
    use qunica_computer_supervisor::{
        docker::DockerProvider,
        protocol::{Binding, ComputerSpec},
    };
    let (mut state, _proxy, _grants, _calls, _root, old_supervisor, old_upstream) =
        proxy_fixture().await;
    old_supervisor.abort();
    old_upstream.abort();
    let identity:(String,String,String,String)=sqlx::query_as("SELECT c.id,c.owner_id,c.binding_id,a.id FROM computers c JOIN agents a ON a.owner_id=c.owner_id").fetch_one(state.db.pool()).await.unwrap();
    let (id, owner, group, agent) = identity;
    let journal = tempfile::tempdir().unwrap();
    let provider = Arc::new(
        DockerProvider::connect("qunica-computer:local".into())
            .await
            .unwrap()
            .with_services(true)
            .with_attestation(TOKEN.into()),
    );
    let supervisor = Supervisor::open(journal.path().into(), TOKEN, provider)
        .unwrap()
        .router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let supervisor_server =
        tokio::spawn(async move { axum::serve(listener, supervisor).await.unwrap() });
    let client =
        Arc::new(SupervisorClient::new(&format!("http://{address}"), TOKEN.into()).unwrap());
    state.computers = Some(client.clone());
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
    let proxy_port = listener.local_addr().unwrap().port();
    let proxy = api::computer_proxy::router(state.clone());
    let proxy_server = tokio::spawn(async move { axum::serve(listener, proxy).await.unwrap() });
    let old = std::env::var("QUNICA_PROXY_BACKEND").ok();
    std::env::set_var(
        "QUNICA_PROXY_BACKEND",
        format!("http://host.docker.internal:{proxy_port}"),
    );
    let computer = client
        .request(
            reqwest::Method::POST,
            "/v1/computers",
            Some(
                &serde_json::to_value(ComputerSpec {
                    id: Uuid::parse_str(&id).unwrap(),
                    owner_id: Uuid::parse_str(&owner).unwrap(),
                    binding: Binding::Team(Uuid::parse_str(&group).unwrap()),
                    name: "ACP fixture".into(),
                })
                .unwrap(),
            ),
        )
        .await;
    if let Some(old) = old {
        std::env::set_var("QUNICA_PROXY_BACKEND", old);
    } else {
        std::env::remove_var("QUNICA_PROXY_BACKEND");
    }
    assert!(computer.is_ok(), "{computer:?}");
    let model_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = model_calls.clone();
    let upstream=Router::new().route("/v1/chat/completions",axum::routing::post(move |headers:axum::http::HeaderMap,axum::Json(body):axum::Json<Value>| {
        let count=counted.clone();async move {
            assert_eq!(headers["authorization"],format!("Bearer {VENDOR_KEY}"));
            count.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
            let has_bash=body["tools"].as_array().is_some_and(|v|v.iter().any(|t|t["function"]["name"]=="bash"));
            let has_result=body["messages"].as_array().is_some_and(|v|v.iter().any(|m|m["role"]=="tool"));
            let delta=if has_bash && !has_result {json!({"role":"assistant","tool_calls":[{"index":0,"id":"call-fixture","type":"function","function":{"name":"bash","arguments":json!({"command":"printf 'verified ACP container tool' > acp-produced.txt","description":"Write fixture artifact"}).to_string()}}]})}else{json!({"role":"assistant","content":"Fixture complete"})};
            let finish=if has_bash && !has_result {"tool_calls"}else{"stop"};
            let frames=[json!({"id":"fixture","object":"chat.completion.chunk","created":1,"model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":null}]}),json!({"id":"fixture","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":finish}],"usage":{"prompt_tokens":5,"completion_tokens":7}})];
            ([("content-type","text/event-stream")],frames.into_iter().map(|v|format!("data: {v}\n\n")).collect::<String>()+"data: [DONE]\n\n")
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let upstream_server =
        tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    sqlx::query("UPDATE llm_providers SET base_url=?")
        .bind(format!("http://{address}/v1"))
        .execute(state.db.pool())
        .await
        .unwrap();
    let outcome=std::panic::AssertUnwindSafe(async {
        client.request(reqwest::Method::POST,&format!("/v1/computers/{id}/exec"),Some(&json!({"expected_generation":"1","argv":["python3","-c","import time,socket\nfor _ in range(100):\n s=socket.socket();s.settimeout(.1)\n if s.connect_ex(('127.0.0.1',8765))==0:break\n time.sleep(.1)"]}))).await.unwrap();
        sqlx::query("UPDATE threads SET git_branch='feature/computer-fixture' WHERE id='proxy-thread'").execute(state.db.pool()).await.unwrap();
        assert!(ComputerExecution::resolve(state.db.pool(),state.computers.clone(),&owner,&group,&agent,"proxy-turn").await.is_err(),"missing computer repository must not fall back to the host or root checkout");
        let initialized=client.request(reqwest::Method::POST,&format!("/v1/computers/{id}/exec"),Some(&json!({"expected_generation":"1","argv":["/bin/sh","-c","git init -q && git -c user.name=Fixture -c user.email=fixture@example.test commit --allow-empty -qm initial"]}))).await.unwrap();
        assert_eq!(initialized["exit_code"],0,"{initialized}");
        sqlx::query("UPDATE threads SET git_branch='feature/computer-fixture' WHERE id='proxy-thread'").execute(state.db.pool()).await.unwrap();
        let computer=ComputerExecution::resolve(state.db.pool(),state.computers.clone(),&owner,&group,&agent,"proxy-turn").await.unwrap().unwrap();
        let task_path=format!(".worktrees/{group}/proxy-thread");
        assert_eq!(computer.relative_workspace(),task_path);
        let native=computer.execute(vec!["python3".into(),"-c".into(),"from pathlib import Path; Path('native-produced.txt').write_text('same task cwd')".into()],30).await.unwrap();
        assert_eq!(native["exit_code"],0,"{native}");
        assert_eq!(computer.execute(vec!["git".into(),"checkout".into(),"--detach".into()],30).await.unwrap()["exit_code"],0);
        assert!(ComputerExecution::resolve(state.db.pool(),state.computers.clone(),&owner,&group,&agent,"proxy-turn").await.is_err(),"a changed task checkout must not silently target another branch");
        assert_eq!(computer.execute(vec!["git".into(),"checkout".into(),"feature/computer-fixture".into()],30).await.unwrap()["exit_code"],0);
        let mut config=acp::normalize_acp_runtime(Some(&json!({"profile":"opencode","command":"opencode","args":["acp"],"timeout_seconds":90,"permission_policy":"auto_allow"}))).unwrap();config.permission_policy=PermissionPolicy::AutoAllow;
        let request=AcpRunRequest {notes:None,owner_id:owner.clone(),group_id:Some(group.clone()),agent_id:agent.clone(),thread_id:Some("proxy-thread".into()),config:config.clone(),cwd:"/home/agent/workspace".into(),prompt:"Write acp-produced.txt using the bash tool, then stop.".into(),prompt_images:vec![],prompt_has_image_attachments:false,incremental_prompt:None,incremental_prompt_images:vec![],incremental_prompt_has_image_attachments:false,context_hash:Some("fixture".into())};
        let mut run=acp::computer::run(state.db.pool().clone(),request,computer.clone(),"proxy-turn".into()).await.unwrap();
        let mut events=Vec::new();
        while let Some(event)=run.next_event().await {events.push(format!("{:?}",event.kind));}
        let result=run.join().await;assert!(result.is_ok(),"{result:?}, events: {events:?}, calls: {}",model_calls.load(std::sync::atomic::Ordering::SeqCst));
        let file=client.request(reqwest::Method::POST,&format!("/v1/computers/{id}/exec"),Some(&json!({"expected_generation":"1","argv":["python3","-c",format!("from pathlib import Path; root=Path({task_path:?}); assert (root/'native-produced.txt').read_text()=='same task cwd'; assert not Path('acp-produced.txt').exists(); print((root/'acp-produced.txt').read_text())")]}))).await.unwrap();
        assert_eq!(file["stdout"],"verified ACP container tool\n","{file}");
        assert!(!std::path::Path::new("acp-produced.txt").exists());

        let session:String=sqlx::query_scalar("SELECT session_id FROM computer_acp_sessions").fetch_one(state.db.pool()).await.unwrap();
        let resumed=AcpRunRequest {notes:None,owner_id:owner.clone(),group_id:Some(group.clone()),agent_id:agent.clone(),thread_id:Some("proxy-thread".into()),config:config.clone(),cwd:"/home/agent/workspace".into(),prompt:"Full context should not be repeated".into(),prompt_images:vec![],prompt_has_image_attachments:false,incremental_prompt:Some("Report that the fixture is complete.".into()),incremental_prompt_images:vec![],incremental_prompt_has_image_attachments:false,context_hash:Some("fixture".into())};
        let mut run=acp::computer::run(state.db.pool().clone(),resumed,computer.clone(),"proxy-turn".into()).await.unwrap();
        while run.next_event().await.is_some() {}
        assert!(run.join().await.is_ok());
        let restored:String=sqlx::query_scalar("SELECT session_id FROM computer_acp_sessions").fetch_one(state.db.pool()).await.unwrap();assert_eq!(session,restored);
        let cancelled=AcpRunRequest {notes:None,owner_id:owner.clone(),group_id:Some(group.clone()),agent_id:agent.clone(),thread_id:Some("proxy-thread".into()),config:config.clone(),cwd:"/home/agent/workspace".into(),prompt:"Must be cancelled".into(),prompt_images:vec![],prompt_has_image_attachments:false,incremental_prompt:None,incremental_prompt_images:vec![],incremental_prompt_has_image_attachments:false,context_hash:Some("fixture".into())};
        let mut run=acp::computer::run(state.db.pool().clone(),cancelled,computer,"proxy-turn".into()).await.unwrap();run.control().cancel();
        while run.next_event().await.is_some() {}
        assert!(matches!(run.join().await,Err(acp::AcpRunJoinError::Cancelled(_))));
        let calls:i64=sqlx::query_scalar("SELECT COUNT(*) FROM computer_model_calls WHERE input_tokens=5 AND output_tokens=7").fetch_one(state.db.pool()).await.unwrap();assert!(calls>=2);
        let billed:i64=sqlx::query_scalar("SELECT COUNT(*) FROM token_usage_records").fetch_one(state.db.pool()).await.unwrap();assert_eq!(calls,billed);
        let active:i64=sqlx::query_scalar("SELECT COUNT(*) FROM computer_proxy_grants WHERE revoked=0 AND expires_at>unixepoch() AND token_hash NOT IN (?,?)").bind(&_grants.hashes[0]).bind(&_grants.hashes[1]).fetch_one(state.db.pool()).await.unwrap();assert_eq!(active,0);
    }).catch_unwind().await;
    let _=client.request(reqwest::Method::POST,&format!("/v1/computers/{id}/actions"),Some(&json!({"expected_generation":"1","action":"destroy","confirm":id,"purge_home":true}))).await;
    supervisor_server.abort();
    proxy_server.abort();
    upstream_server.abort();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
