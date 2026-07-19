use ag_swarmer_backend::api::{router_with_state_for_tests, AppState};
use axum::{
    body::Body,
    http::{header, HeaderMap, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Command,
};
use tower::ServiceExt;
use uuid::Uuid;

async fn app() -> Router {
    ag_swarmer_backend::api::router_for_tests().await
}

async fn app_with_state() -> (Router, AppState) {
    router_with_state_for_tests().await
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

async fn send_bytes(app: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, headers, bytes.to_vec())
}

fn post_json(uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn authed_json(method: &str, uri: &str, token: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn authed(method: &str, uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

fn authed_multipart_file(
    uri: &str,
    token: &str,
    field_name: &str,
    filename: &str,
    content_type: Option<&str>,
    bytes: &[u8],
) -> Request<Body> {
    let boundary = "ag-swarmer-test-boundary";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"{field_name}\"; filename=\"{filename}\"\r\n"
        )
        .as_bytes(),
    );
    if let Some(content_type) = content_type {
        body.extend_from_slice(format!("Content-Type: {content_type}\r\n").as_bytes());
    }
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    Request::builder()
        .method("POST")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap()
}

/// Register and log in a user, returning a bearer token.
async fn register_and_login(app: &Router, email: &str) -> String {
    register_and_login_named(app, email, "Tester").await
}

/// Register and log in a user with a specific display name, returning a bearer token.
async fn register_and_login_named(app: &Router, email: &str, name: &str) -> String {
    let (status, _) = send(
        app,
        post_json(
            "/api/v2/auth/register",
            json!({"email": email, "password": "supersecret", "name": name}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, token) = send(
        app,
        post_json(
            "/api/v2/auth/login",
            json!({"email": email, "password": "supersecret"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    token["access_token"].as_str().unwrap().to_string()
}

/// Create a cloud-sandbox workspace (no local path required) and return its id.
async fn create_workspace(app: &Router, token: &str) -> String {
    let (status, workspace) = send(
        app,
        authed_json(
            "POST",
            "/api/v2/workspaces",
            token,
            json!({"name": "WS", "backend_type": "cloud_sandbox"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    workspace["id"].as_str().unwrap().to_string()
}

async fn create_local_workspace(
    app: &Router,
    token: &str,
    name: &str,
) -> (tempfile::TempDir, String) {
    let root = tempfile::tempdir().unwrap();
    let (status, workspace) = send(
        app,
        authed_json(
            "POST",
            "/api/v2/workspaces",
            token,
            json!({
                "name": name,
                "backend_type": "local",
                "local_path": root.path().to_string_lossy()
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    (root, workspace["id"].as_str().unwrap().to_string())
}

async fn create_agent(app: &Router, token: &str, workspace_id: &str, name: &str) -> String {
    let (status, agent) = send(
        app,
        authed_json(
            "POST",
            "/api/v2/agents",
            token,
            json!({"name": name, "workspace_id": workspace_id}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    agent["id"].as_str().unwrap().to_string()
}

async fn create_llm_provider(app: &Router, token: &str, base_url: &str) -> String {
    let (status, provider) = send(
        app,
        authed_json(
            "POST",
            "/api/v2/llm-providers",
            token,
            json!({
                "name": "Fake",
                "kind": "openai-compatible",
                "base_url": base_url,
                "api_key": "test-key",
                "default_model": "test-model"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    provider["id"].as_str().unwrap().to_string()
}

async fn create_llm_agent(
    app: &Router,
    token: &str,
    workspace_id: &str,
    provider_id: &str,
    name: &str,
) -> String {
    let (status, agent) = send(
        app,
        authed_json(
            "POST",
            "/api/v2/agents",
            token,
            json!({
                "name": name,
                "workspace_id": workspace_id,
                "runtime_kind": "llm_chat",
                "llm_provider_id": provider_id
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    agent["id"].as_str().unwrap().to_string()
}

async fn create_group_with_initial_agents(
    app: &Router,
    token: &str,
    workspace_id: &str,
    mode: &str,
    initial_agents: &[&str],
) -> Value {
    let (status, group) = send(
        app,
        authed_json(
            "POST",
            "/api/v2/groups",
            token,
            json!({
                "name": format!("{mode} group"),
                "workspace_id": workspace_id,
                "communication_mode": mode,
                "initial_agents": initial_agents
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    group
}

async fn patch_group_mode(app: &Router, token: &str, group_id: &str, mode: &str) -> Value {
    let (status, group) = send(
        app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}"),
            token,
            json!({"communication_mode": mode}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    group
}

async fn owner_id(state: &AppState, email: &str) -> String {
    sqlx::query_scalar::<_, String>("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(state.db.pool())
        .await
        .unwrap()
}

async fn create_group_note(
    app: &Router,
    token: &str,
    group_id: &str,
    title: &str,
    content: &str,
) -> Value {
    let (status, note) = send(
        app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/notes"),
            token,
            json!({"title": title, "content": content}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    note
}

fn group_note_file(root: &Path, note_id: &str) -> PathBuf {
    root.join("Notes").join(format!("{note_id}.md"))
}

fn group_upload_file(root: &Path, filename: &str) -> PathBuf {
    root.join("uploads").join(filename)
}

fn workspace_file_url(group_id: &str, path: &str) -> String {
    format!("/api/v2/groups/{group_id}/workspace-files?path={path}")
}

fn workspace_git_url(group_id: &str, action: &str) -> String {
    format!("/api/v2/groups/{group_id}/workspace-git/{action}")
}

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap_or_else(|err| panic!("failed to run git {args:?}: {err}"));
    assert!(
        output.status.success(),
        "git {args:?} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_git_repo(root: &Path) {
    run_git(root, &["init"]);
    run_git(root, &["config", "user.email", "tests@example.com"]);
    run_git(root, &["config", "user.name", "Tests"]);
    std::fs::write(root.join("tracked.txt"), b"initial").unwrap();
    run_git(root, &["add", "tracked.txt"]);
    run_git(root, &["commit", "-m", "initial"]);
}

async fn fake_provider(body: String) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new().fallback(move || {
        let body = body.clone();
        async move { ([(header::CONTENT_TYPE, "text/event-stream")], body) }
    });
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

fn git_status_file<'a>(status: &'a Value, path: &str) -> &'a Value {
    status["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == path)
        .unwrap_or_else(|| panic!("missing git status row for {path}: {status}"))
}

fn workspace_file_route_requests(group_id: &str, token: &str) -> Vec<Request<Body>> {
    vec![
        authed(
            "GET",
            &format!("/api/v2/groups/{group_id}/workspace-files/root"),
            token,
        ),
        authed("GET", &workspace_file_url(group_id, ""), token),
        authed(
            "GET",
            &format!("/api/v2/groups/{group_id}/workspace-files/preview?path=missing.txt"),
            token,
        ),
        authed_multipart_file(
            &format!("/api/v2/groups/{group_id}/workspace-files/upload"),
            token,
            "file",
            "blocked.txt",
            Some("text/plain"),
            b"blocked",
        ),
        authed(
            "GET",
            &format!("/api/v2/groups/{group_id}/workspace-files/download?path=missing.txt"),
            token,
        ),
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/workspace-files/rename?path=missing.txt"),
            token,
            json!({"new_path": "renamed.txt"}),
        ),
        authed(
            "DELETE",
            &format!("/api/v2/groups/{group_id}/workspace-files?path=missing.txt"),
            token,
        ),
    ]
}

async fn assert_workspace_file_route_errors(
    app: &Router,
    token: &str,
    group_id: &str,
    expected_status: StatusCode,
    expected_code: &str,
) {
    for request in workspace_file_route_requests(group_id, token) {
        let (status, body) = send(app, request).await;
        assert_eq!(status, expected_status, "body: {body:?}");
        assert_eq!(body["error"]["code"], expected_code);
    }
}

#[cfg(unix)]
fn create_dir_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_dir_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

fn remove_symlink(link: &Path) -> std::io::Result<()> {
    std::fs::remove_file(link).or_else(|_| std::fs::remove_dir(link))
}

#[cfg(unix)]
fn create_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[tokio::test]
async fn group_create_requires_active_owned_workspace() {
    let app = app().await;
    let token_a = register_and_login(&app, "ownera@example.com").await;
    let workspace_a = create_workspace(&app, &token_a).await;

    let (status, group) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/groups",
            &token_a,
            json!({"name": "Team", "workspace_id": workspace_a}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(group["workspace_id"], workspace_a);
    assert_eq!(group["status"], "active");
    // Defaults are exposed as booleans/numbers.
    assert_eq!(group["free_speech"], false);
    assert_eq!(group["proactive_mode"], false);
    assert_eq!(group["proactive_reply_multiplier"], 1);
    assert_eq!(group["allow_agent_free_mention"], true);

    // A workspace owned by another user cannot be referenced.
    let token_b = register_and_login(&app, "ownerb@example.com").await;
    let workspace_b = create_workspace(&app, &token_b).await;

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/groups",
            &token_a,
            json!({"name": "Trespasser", "workspace_id": workspace_b}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");
}

#[tokio::test]
async fn group_create_and_read_return_expanded_fields_and_owner_membership() {
    let (app, state) = app_with_state().await;
    let email = "expanded@example.com";
    let token = register_and_login(&app, email).await;
    let workspace = create_workspace(&app, &token).await;

    let (status, group) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/groups",
            &token,
            json!({
                "name": "Expanded Team",
                "workspace_id": workspace,
                "description": "Operators",
                "announcement": "Stand by",
                "free_speech": true,
                "proactive_mode": true,
                "proactive_max_rounds": 4,
                "proactive_reply_multiplier": 2,
                "allow_agent_free_mention": false,
                "agent_free_mention_max_dispatches": 12,
                "communication_mode": "star"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(group["workspace_id"], workspace);
    assert_eq!(group["name"], "Expanded Team");
    assert_eq!(group["description"], "Operators");
    assert_eq!(group["announcement"], "Stand by");
    assert_eq!(group["free_speech"], true);
    assert_eq!(group["proactive_mode"], true);
    assert_eq!(group["proactive_max_rounds"], 4);
    assert_eq!(group["proactive_reply_multiplier"], 2);
    assert_eq!(group["allow_agent_free_mention"], false);
    assert_eq!(group["agent_free_mention_max_dispatches"], 12);
    assert_eq!(group["communication_mode"], "star");
    assert_eq!(group["muted_agent_ids"], Value::Null);
    assert_eq!(group["admin_agent_ids"], Value::Null);
    assert_eq!(group["muted_member_ids"], Value::Null);
    assert_eq!(group["status"], "active");
    assert!(group["created_at"].as_str().is_some());

    let group_id = group["id"].as_str().unwrap().to_string();
    let (status, fetched) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["announcement"], "Stand by");
    assert_eq!(fetched["communication_mode"], "star");
    assert_eq!(fetched["agent_free_mention_max_dispatches"], 12);

    let owner = owner_id(&state, email).await;
    let membership = sqlx::query_as::<_, (String, String)>(
        "SELECT role, status FROM group_members WHERE group_id = ? AND user_id = ?",
    )
    .bind(&group_id)
    .bind(&owner)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(membership, ("owner".to_string(), "active".to_string()));
}

#[tokio::test]
async fn group_members_add_list_and_candidates_are_owner_scoped() {
    let (app, state) = app_with_state().await;
    let owner_email = "group-members-owner@example.com";
    let token = register_and_login_named(&app, owner_email, "Owner One").await;
    let workspace = create_workspace(&app, &token).await;
    let _member_token =
        register_and_login_named(&app, "ada-candidate@example.com", "Ada Lovelace").await;
    let _other_token =
        register_and_login_named(&app, "grace-hopper@example.com", "Grace Hopper").await;
    let member_id = owner_id(&state, "ada-candidate@example.com").await;
    let owner_user_id = owner_id(&state, owner_email).await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    let (status, initial_members) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}/members"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let rows = initial_members.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], format!("{group_id}:{owner_user_id}"));
    assert_eq!(rows[0]["group_id"], group_id);
    assert_eq!(rows[0]["user_id"], owner_user_id);
    assert_eq!(rows[0]["display_name"], "Owner One");
    assert_eq!(rows[0]["role"], "owner");
    assert_eq!(rows[0]["status"], "active");
    assert_eq!(rows[0]["is_muted"], false);
    assert!(rows[0]["joined_at"].as_str().is_some());

    let (status, candidates_by_name) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/groups/{group_id}/member-candidates?q=Ada"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(candidates_by_name
        .as_array()
        .unwrap()
        .iter()
        .any(|user| user["id"] == member_id
            && user["email"] == "ada-candidate@example.com"
            && user["name"] == "Ada Lovelace"
            && user["avatar_url"] == Value::Null
            && user["created_at"].as_str().is_some()));

    let (status, candidates_by_email) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/groups/{group_id}/member-candidates?q=hopper"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(candidates_by_email
        .as_array()
        .unwrap()
        .iter()
        .any(|user| user["email"] == "grace-hopper@example.com"));

    let (status, body) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/groups/{group_id}/member-candidates?q=Ada"),
            _other_token.as_str(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");

    let (status, added) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/members"),
            &token,
            json!({"user_id": member_id}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(added["id"], format!("{group_id}:{member_id}"));
    assert_eq!(added["group_id"], group_id);
    assert_eq!(added["user_id"], member_id);
    assert_eq!(added["display_name"], "Ada Lovelace");
    assert_eq!(added["role"], "member");
    assert_eq!(added["status"], "active");
    assert_eq!(added["is_muted"], false);
    assert!(added["joined_at"].as_str().is_some());

    let (status, list) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}/members"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let rows = list.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .any(|row| row["user_id"] == owner_user_id && row["role"] == "owner"));
    assert!(rows
        .iter()
        .any(|row| row["user_id"] == member_id && row["role"] == "member"));

    let (status, candidates_after_add) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/groups/{group_id}/member-candidates?q=Ada"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(candidates_after_add
        .as_array()
        .unwrap()
        .iter()
        .any(|user| user["id"] == member_id));
}

#[tokio::test]
async fn group_members_duplicate_conflict_and_readd_removed_member() {
    let (app, state) = app_with_state().await;
    let token = register_and_login(&app, "group-members-readd@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let _member_token = register_and_login(&app, "group-members-readd-target@example.com").await;
    let member_id = owner_id(&state, "group-members-readd-target@example.com").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    let members_url = format!("/api/v2/groups/{group_id}/members");
    let member_url = format!("{members_url}/{member_id}");

    let (status, _) = send(
        &app,
        authed_json("POST", &members_url, &token, json!({"user_id": member_id})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = send(
        &app,
        authed_json("POST", &members_url, &token, json!({"user_id": member_id})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "conflict");
    assert_eq!(body["error"]["message"], "user already in group");

    let (status, body) = send(&app, authed("DELETE", &member_url, &token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);

    let (status, readded) = send(
        &app,
        authed_json("POST", &members_url, &token, json!({"user_id": member_id})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(readded["user_id"], member_id);
    assert_eq!(readded["role"], "member");
    assert_eq!(readded["status"], "active");

    let (status, list) = send(&app, authed("GET", &members_url, &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn group_members_concurrent_duplicate_add_returns_created_and_conflict() {
    let (app, state) = app_with_state().await;
    let token = register_and_login(&app, "group-members-concurrent@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let _member_token =
        register_and_login(&app, "group-members-concurrent-target@example.com").await;
    let member_id = owner_id(&state, "group-members-concurrent-target@example.com").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    let members_url = format!("/api/v2/groups/{group_id}/members");

    let req_a = authed_json(
        "POST",
        &members_url,
        &token,
        json!({"user_id": member_id.clone()}),
    );
    let req_b = authed_json(
        "POST",
        &members_url,
        &token,
        json!({"user_id": member_id.clone()}),
    );

    let (first, second) = tokio::join!(send(&app, req_a), send(&app, req_b));
    let responses = vec![first, second];

    assert!(
        responses
            .iter()
            .all(|(status, _)| *status != StatusCode::INTERNAL_SERVER_ERROR),
        "responses: {responses:?}"
    );
    assert_eq!(
        responses
            .iter()
            .filter(|(status, _)| *status == StatusCode::CREATED)
            .count(),
        1,
        "responses: {responses:?}"
    );
    assert_eq!(
        responses
            .iter()
            .filter(|(status, _)| *status == StatusCode::CONFLICT)
            .count(),
        1,
        "responses: {responses:?}"
    );

    for (status, body) in responses {
        match status {
            StatusCode::CREATED => {
                assert_eq!(body["group_id"], group_id);
                assert_eq!(body["user_id"], member_id);
                assert_eq!(body["status"], "active");
            }
            StatusCode::CONFLICT => {
                assert_eq!(body["error"]["code"], "conflict");
                assert_eq!(body["error"]["message"], "user already in group");
            }
            other => panic!("unexpected status {other}: {body:?}"),
        }
    }
}

#[tokio::test]
async fn group_members_delete_hides_member_and_clears_muted_id() {
    let (app, state) = app_with_state().await;
    let token = register_and_login(&app, "group-members-delete@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let _member_token = register_and_login(&app, "group-members-delete-target@example.com").await;
    let member_id = owner_id(&state, "group-members-delete-target@example.com").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    let (status, _) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/members"),
            &token,
            json!({"user_id": member_id}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, muted) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/members/{member_id}/mute"),
            &token,
            json!({"muted": true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(muted["is_muted"], true);

    let (status, _) = send(
        &app,
        authed(
            "DELETE",
            &format!("/api/v2/groups/{group_id}/members/{member_id}"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, list) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}/members"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!list
        .as_array()
        .unwrap()
        .iter()
        .any(|member| member["user_id"] == member_id));

    let (status, group) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_json_array_missing(&group["muted_member_ids"], &member_id);
}

#[tokio::test]
async fn group_members_mute_updates_group_read_and_unmute_removes_it() {
    let (app, state) = app_with_state().await;
    let token = register_and_login(&app, "group-members-mute@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let _member_token = register_and_login(&app, "group-members-mute-target@example.com").await;
    let member_id = owner_id(&state, "group-members-mute-target@example.com").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    let (status, _) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/members"),
            &token,
            json!({"user_id": member_id}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, muted) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/members/{member_id}/mute"),
            &token,
            json!({"muted": true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(muted["user_id"], member_id);
    assert_eq!(muted["is_muted"], true);

    let (status, members) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}/members"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(members
        .as_array()
        .unwrap()
        .iter()
        .any(|member| member["user_id"] == member_id && member["is_muted"] == true));

    let (status, group) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_json_array_contains(&group["muted_member_ids"], &member_id);

    let (status, unmuted) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/members/{member_id}/mute"),
            &token,
            json!({"muted": false}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unmuted["is_muted"], false);

    let (status, group) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_json_array_missing(&group["muted_member_ids"], &member_id);
}

#[tokio::test]
async fn group_members_owner_protection_and_cross_owner_mutation_rejection() {
    let (app, state) = app_with_state().await;
    let owner_email = "group-members-owner-protect@example.com";
    let token = register_and_login(&app, owner_email).await;
    let workspace = create_workspace(&app, &token).await;
    let member_token =
        register_and_login(&app, "group-members-owner-protect-member@example.com").await;
    let owner_user_id = owner_id(&state, owner_email).await;
    let member_id = owner_id(&state, "group-members-owner-protect-member@example.com").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    let (status, _) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/members"),
            &token,
            json!({"user_id": member_id}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = send(
        &app,
        authed(
            "DELETE",
            &format!("/api/v2/groups/{group_id}/members/{owner_user_id}"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");
    assert_eq!(body["error"]["message"], "group owner cannot be removed");

    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/members/{owner_user_id}/mute"),
            &token,
            json!({"muted": true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");
    assert_eq!(body["error"]["message"], "group owner cannot be muted");

    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/members/{member_id}/mute"),
            &member_token,
            json!({"muted": true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");
}

#[tokio::test]
async fn group_create_without_workspace_id_creates_local_workspace_from_settings_root() {
    let (app, state) = app_with_state().await;
    let token = register_and_login(&app, "auto-workspace@example.com").await;
    let root = tempfile::tempdir().unwrap();
    let raw_root = root.path().to_str().unwrap().to_string();

    let (status, _) = send(
        &app,
        authed_json(
            "PATCH",
            "/api/v2/settings/system",
            &token,
            json!({"group_workspace_root": raw_root}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, group) = send(
        &app,
        authed_json("POST", "/api/v2/groups", &token, json!({"name": "Auto WS"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let group_id = group["id"].as_str().unwrap().to_string();
    let workspace_id = group["workspace_id"].as_str().unwrap().to_string();

    let expected_path = std::fs::canonicalize(root.path().join(&group_id))
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(std::path::Path::new(&expected_path).is_dir());

    let workspace = sqlx::query_as::<_, (String, String, Option<String>)>(
        "SELECT name, backend_type, local_path FROM workspaces WHERE id = ?",
    )
    .bind(&workspace_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(workspace.0, "group:Auto WS");
    assert_eq!(workspace.1, "local");
    assert_eq!(workspace.2.as_deref(), Some(expected_path.as_str()));
}

#[tokio::test]
async fn group_notes_create_writes_markdown_file() {
    let app = app().await;
    let token = register_and_login(&app, "group-notes-create@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Notes WS").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    let (status, note) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/notes"),
            &token,
            json!({"title": "  Plan  ", "content": "first draft"}),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    let note_id = note["id"].as_str().unwrap();
    assert_eq!(note["group_id"], group_id);
    assert_eq!(note["title"], "Plan");
    assert_eq!(note["content"], "first draft");
    assert!(note["created_at"].as_str().is_some());
    assert!(note["updated_at"].as_str().is_some());
    assert_eq!(
        std::fs::read_to_string(group_note_file(root.path(), note_id)).unwrap(),
        "first draft"
    );
}

#[tokio::test]
async fn group_notes_list_orders_active_notes_and_reads_file_content() {
    let (app, state) = app_with_state().await;
    let token = register_and_login(&app, "group-notes-list@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Notes WS").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    let older = create_group_note(&app, &token, group_id, "Older", "db older").await;
    let newer = create_group_note(&app, &token, group_id, "Newer", "db newer").await;
    let deleted = create_group_note(&app, &token, group_id, "Deleted", "db deleted").await;
    let older_id = older["id"].as_str().unwrap();
    let newer_id = newer["id"].as_str().unwrap();
    let deleted_id = deleted["id"].as_str().unwrap();

    std::fs::write(group_note_file(root.path(), older_id), "file older").unwrap();
    std::fs::remove_file(group_note_file(root.path(), newer_id)).unwrap();
    sqlx::query("UPDATE group_notes SET updated_at = ? WHERE id = ?")
        .bind("2026-01-01T00:00:00Z")
        .bind(older_id)
        .execute(state.db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE group_notes SET updated_at = ? WHERE id = ?")
        .bind("2026-01-01T00:00:01Z")
        .bind(newer_id)
        .execute(state.db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE group_notes SET status = 'deleted' WHERE id = ?")
        .bind(deleted_id)
        .execute(state.db.pool())
        .await
        .unwrap();

    let (status, list) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}/notes"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let rows = list.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["id"], newer_id);
    assert_eq!(rows[0]["content"], "db newer");
    assert_eq!(rows[1]["id"], older_id);
    assert_eq!(rows[1]["content"], "file older");
    assert!(!rows.iter().any(|row| row["id"] == deleted_id));
}

#[tokio::test]
async fn group_notes_patch_title_only_preserves_file_content() {
    let app = app().await;
    let token = register_and_login(&app, "group-notes-title-only@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Notes WS").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    let note = create_group_note(&app, &token, group_id, "Before", "db content").await;
    let note_id = note["id"].as_str().unwrap();
    let path = group_note_file(root.path(), note_id);
    std::fs::write(&path, "file content").unwrap();

    let (status, patched) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/notes/{note_id}"),
            &token,
            json!({"title": "  After  "}),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(patched["title"], "After");
    assert_eq!(patched["content"], "file content");
    assert_eq!(std::fs::read_to_string(path).unwrap(), "file content");
}

#[tokio::test]
async fn group_notes_patch_content_rewrites_markdown_file() {
    let app = app().await;
    let token = register_and_login(&app, "group-notes-content@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Notes WS").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    let note = create_group_note(&app, &token, group_id, "Note", "before").await;
    let note_id = note["id"].as_str().unwrap();

    let (status, patched) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/notes/{note_id}"),
            &token,
            json!({"content": "after"}),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(patched["content"], "after");
    assert_eq!(
        std::fs::read_to_string(group_note_file(root.path(), note_id)).unwrap(),
        "after"
    );
}

#[tokio::test]
async fn group_notes_delete_hides_note_and_removes_markdown_file() {
    let app = app().await;
    let token = register_and_login(&app, "group-notes-delete@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Notes WS").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    let note = create_group_note(&app, &token, group_id, "Delete", "gone").await;
    let note_id = note["id"].as_str().unwrap();
    let path = group_note_file(root.path(), note_id);
    assert!(path.is_file());

    let (status, body) = send(
        &app,
        authed(
            "DELETE",
            &format!("/api/v2/groups/{group_id}/notes/{note_id}"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);
    assert!(!path.exists());

    let (status, list) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}/notes"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(list.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn group_notes_reject_invalid_titles() {
    let app = app().await;
    let token = register_and_login(&app, "group-notes-invalid-title@example.com").await;
    let (_root, workspace) = create_local_workspace(&app, &token, "Notes WS").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    let too_long = "x".repeat(201);

    for title in ["", "   ", too_long.as_str()] {
        let (status, body) = send(
            &app,
            authed_json(
                "POST",
                &format!("/api/v2/groups/{group_id}/notes"),
                &token,
                json!({"title": title, "content": ""}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_input");
    }

    let note = create_group_note(&app, &token, group_id, "Valid", "").await;
    let note_id = note["id"].as_str().unwrap();
    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/notes/{note_id}"),
            &token,
            json!({"title": "   "}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
}

#[tokio::test]
async fn group_notes_cloud_or_unbound_workspace_returns_client_error() {
    let app = app().await;
    let token = register_and_login(&app, "group-notes-workspace-errors@example.com").await;
    let cloud_workspace = create_workspace(&app, &token).await;
    let cloud_group =
        create_group_with_initial_agents(&app, &token, &cloud_workspace, "mesh", &[]).await;
    let cloud_group_id = cloud_group["id"].as_str().unwrap();

    let (status, body) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/groups/{cloud_group_id}/notes"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");

    let (_root, local_workspace) = create_local_workspace(&app, &token, "Notes WS").await;
    let local_group =
        create_group_with_initial_agents(&app, &token, &local_workspace, "mesh", &[]).await;
    let local_group_id = local_group["id"].as_str().unwrap();
    let (status, _) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{local_group_id}"),
            &token,
            json!({"workspace_id": Value::Null}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{local_group_id}/notes"),
            &token,
            json!({"title": "Blocked", "content": ""}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
}

#[tokio::test]
async fn group_notes_cross_owner_access_is_rejected() {
    let app = app().await;
    let token_a = register_and_login(&app, "group-notes-cross-a@example.com").await;
    let (_root, workspace) = create_local_workspace(&app, &token_a, "Notes WS").await;
    let group = create_group_with_initial_agents(&app, &token_a, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    let note = create_group_note(&app, &token_a, group_id, "Private", "secret").await;
    let note_id = note["id"].as_str().unwrap();
    let token_b = register_and_login(&app, "group-notes-cross-b@example.com").await;

    for request in [
        authed("GET", &format!("/api/v2/groups/{group_id}/notes"), &token_b),
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/notes"),
            &token_b,
            json!({"title": "Nope", "content": ""}),
        ),
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/notes/{note_id}"),
            &token_b,
            json!({"title": "Nope"}),
        ),
        authed(
            "DELETE",
            &format!("/api/v2/groups/{group_id}/notes/{note_id}"),
            &token_b,
        ),
    ] {
        let (status, body) = send(&app, request).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "permission_denied");
    }
}

#[tokio::test]
async fn group_notes_rejects_notes_symlink_escape() {
    let app = app().await;
    let token = register_and_login(&app, "group-notes-symlink@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Notes WS").await;
    let outside = tempfile::tempdir().unwrap();
    if create_dir_symlink(outside.path(), &root.path().join("Notes")).is_err() {
        return;
    }
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/notes"),
            &token,
            json!({"title": "Escape", "content": ""}),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
}

#[tokio::test]
async fn group_notes_rejects_note_file_symlink_before_patch_read_and_delete() {
    let app = app().await;
    let token = register_and_login(&app, "group-notes-file-symlink@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Notes WS").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    let note = create_group_note(&app, &token, group_id, "Note", "before").await;
    let note_id = note["id"].as_str().unwrap();
    let note_path = group_note_file(root.path(), note_id);
    let outside = tempfile::tempdir().unwrap();
    let outside_target = outside.path().join("missing-note-target.md");
    assert!(!outside_target.exists());

    std::fs::remove_file(&note_path).unwrap();
    if create_file_symlink(&outside_target, &note_path).is_err() {
        return;
    }
    assert!(std::fs::symlink_metadata(&note_path)
        .unwrap()
        .file_type()
        .is_symlink());

    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/notes/{note_id}"),
            &token,
            json!({"content": "after"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
    assert!(!outside_target.exists());

    let (status, body) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}/notes"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
    assert!(!outside_target.exists());

    let (status, body) = send(
        &app,
        authed(
            "DELETE",
            &format!("/api/v2/groups/{group_id}/notes/{note_id}"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
    assert!(!outside_target.exists());
    assert!(std::fs::symlink_metadata(&note_path)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[tokio::test]
async fn group_files_upload_writes_file_and_records_metadata() {
    let (app, state) = app_with_state().await;
    let token = register_and_login(&app, "group-files-upload@example.com").await;
    let uploader_id = owner_id(&state, "group-files-upload@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Files WS").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    let (status, file) = send(
        &app,
        authed_multipart_file(
            &format!("/api/v2/groups/{group_id}/files"),
            &token,
            "file",
            "plan.txt",
            Some("text/plain"),
            b"hello files",
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    let file_id = file["id"].as_str().unwrap();
    assert_eq!(file["group_id"], group_id);
    assert_eq!(file["filename"], "plan.txt");
    assert_eq!(file["file_size"], 11);
    assert_eq!(file["mime_type"], "text/plain");
    assert!(file["created_at"].as_str().is_some());

    let upload_path = group_upload_file(root.path(), "plan.txt");
    assert_eq!(std::fs::read(&upload_path).unwrap(), b"hello files");

    let row = sqlx::query_as::<_, (String, String, i64, Option<String>, String)>(
        "SELECT uploader_id, file_path, file_size, mime_type, status \
         FROM group_files WHERE id = ?",
    )
    .bind(file_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(row.0, uploader_id);
    assert_eq!(
        PathBuf::from(row.1).canonicalize().unwrap(),
        upload_path.canonicalize().unwrap()
    );
    assert_eq!(row.2, 11);
    assert_eq!(row.3.as_deref(), Some("text/plain"));
    assert_eq!(row.4, "active");
}

#[tokio::test]
async fn group_files_list_orders_active_files_newest_first_without_disk_requirement() {
    let (app, state) = app_with_state().await;
    let token = register_and_login(&app, "group-files-list@example.com").await;
    let uploader_id = owner_id(&state, "group-files-list@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Files WS").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    let older_id = Uuid::new_v4().to_string();
    let newer_id = Uuid::new_v4().to_string();
    let deleted_id = Uuid::new_v4().to_string();

    for (id, filename, created_at, status) in [
        (
            older_id.as_str(),
            "older.txt",
            "2026-01-01T00:00:00Z",
            "active",
        ),
        (
            newer_id.as_str(),
            "newer.txt",
            "2026-01-01T00:00:01Z",
            "active",
        ),
        (
            deleted_id.as_str(),
            "deleted.txt",
            "2026-01-01T00:00:02Z",
            "deleted",
        ),
    ] {
        sqlx::query(
            "INSERT INTO group_files \
             (id, group_id, uploader_id, filename, file_path, file_size, mime_type, status, created_at) \
             VALUES (?, ?, ?, ?, ?, 7, 'text/plain', ?, ?)",
        )
        .bind(id)
        .bind(group_id)
        .bind(&uploader_id)
        .bind(filename)
        .bind(root.path().join("missing").join(filename).to_string_lossy().to_string())
        .bind(status)
        .bind(created_at)
        .execute(state.db.pool())
        .await
        .unwrap();
    }

    let (status, list) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}/files"), &token),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let rows = list.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["id"], newer_id);
    assert_eq!(rows[0]["filename"], "newer.txt");
    assert_eq!(rows[1]["id"], older_id);
    assert!(!rows.iter().any(|row| row["id"] == deleted_id));
}

#[tokio::test]
async fn group_files_delete_hides_row_and_keeps_physical_file() {
    let app = app().await;
    let token = register_and_login(&app, "group-files-delete@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Files WS").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    let (status, file) = send(
        &app,
        authed_multipart_file(
            &format!("/api/v2/groups/{group_id}/files"),
            &token,
            "file",
            "keep.txt",
            Some("text/plain"),
            b"keep me",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let file_id = file["id"].as_str().unwrap();
    let path = group_upload_file(root.path(), "keep.txt");
    assert!(path.is_file());

    let (status, body) = send(
        &app,
        authed(
            "DELETE",
            &format!("/api/v2/groups/{group_id}/files/{file_id}"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);
    assert_eq!(std::fs::read(&path).unwrap(), b"keep me");

    let (status, list) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}/files"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(list.as_array().unwrap().is_empty());

    let (status, body) = send(
        &app,
        authed(
            "DELETE",
            &format!("/api/v2/groups/{group_id}/files/{}", Uuid::new_v4()),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
}

#[tokio::test]
async fn group_files_reject_missing_file_field_and_unsafe_filenames() {
    let (app, state) = app_with_state().await;
    let token = register_and_login(&app, "group-files-invalid@example.com").await;
    let (_root, workspace) = create_local_workspace(&app, &token, "Files WS").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    let (status, body) = send(
        &app,
        authed_multipart_file(
            &format!("/api/v2/groups/{group_id}/files"),
            &token,
            "other",
            "ignored.txt",
            Some("text/plain"),
            b"ignored",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");

    for filename in [
        "",
        "   ",
        ".",
        "..",
        "../escape.txt",
        "dir/name.txt",
        "dir\\name.txt",
        "C:evil.txt",
        "C:\\evil.txt",
        "\\\\server\\share.txt",
        "//server/share.txt",
        "bad:name.txt",
    ] {
        let (status, body) = send(
            &app,
            authed_multipart_file(
                &format!("/api/v2/groups/{group_id}/files"),
                &token,
                "file",
                filename,
                Some("text/plain"),
                b"blocked",
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "filename {filename:?}");
        assert_eq!(body["error"]["code"], "invalid_input");
    }

    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM group_files WHERE group_id = ? AND status = 'active'",
    )
    .bind(group_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn group_files_duplicate_upload_rejected_without_overwrite_or_second_row() {
    let (app, state) = app_with_state().await;
    let token = register_and_login(&app, "group-files-duplicate@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Files WS").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    let path = group_upload_file(root.path(), "dup.txt");

    let (status, _) = send(
        &app,
        authed_multipart_file(
            &format!("/api/v2/groups/{group_id}/files"),
            &token,
            "file",
            "dup.txt",
            Some("text/plain"),
            b"first",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = send(
        &app,
        authed_multipart_file(
            &format!("/api/v2/groups/{group_id}/files"),
            &token,
            "file",
            "dup.txt",
            Some("text/plain"),
            b"second",
        ),
    )
    .await;
    assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(matches!(
        status,
        StatusCode::BAD_REQUEST | StatusCode::CONFLICT
    ));
    assert!(matches!(
        body["error"]["code"].as_str(),
        Some("invalid_input" | "conflict")
    ));
    assert_eq!(std::fs::read(&path).unwrap(), b"first");

    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM group_files \
         WHERE group_id = ? AND filename = 'dup.txt' AND status = 'active'",
    )
    .bind(group_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn group_files_cloud_or_unbound_workspace_returns_client_error() {
    let app = app().await;
    let token = register_and_login(&app, "group-files-workspace-errors@example.com").await;
    let cloud_workspace = create_workspace(&app, &token).await;
    let cloud_group =
        create_group_with_initial_agents(&app, &token, &cloud_workspace, "mesh", &[]).await;
    let cloud_group_id = cloud_group["id"].as_str().unwrap();

    let (status, body) = send(
        &app,
        authed_multipart_file(
            &format!("/api/v2/groups/{cloud_group_id}/files"),
            &token,
            "file",
            "blocked.txt",
            Some("text/plain"),
            b"blocked",
        ),
    )
    .await;
    assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(status.is_client_error());
    assert_eq!(body["error"]["code"], "invalid_input");

    let (_root, local_workspace) = create_local_workspace(&app, &token, "Files WS").await;
    let local_group =
        create_group_with_initial_agents(&app, &token, &local_workspace, "mesh", &[]).await;
    let local_group_id = local_group["id"].as_str().unwrap();
    let (status, _) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{local_group_id}"),
            &token,
            json!({"workspace_id": Value::Null}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(
        &app,
        authed_multipart_file(
            &format!("/api/v2/groups/{local_group_id}/files"),
            &token,
            "file",
            "blocked.txt",
            Some("text/plain"),
            b"blocked",
        ),
    )
    .await;
    assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(status.is_client_error());
    assert_eq!(body["error"]["code"], "invalid_input");
}

#[tokio::test]
async fn group_files_cross_owner_access_is_rejected() {
    let app = app().await;
    let token_a = register_and_login(&app, "group-files-cross-a@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token_a, "Files WS").await;
    let group = create_group_with_initial_agents(&app, &token_a, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    let (status, file) = send(
        &app,
        authed_multipart_file(
            &format!("/api/v2/groups/{group_id}/files"),
            &token_a,
            "file",
            "private.txt",
            Some("text/plain"),
            b"private",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let file_id = file["id"].as_str().unwrap();
    let token_b = register_and_login(&app, "group-files-cross-b@example.com").await;

    for request in [
        authed("GET", &format!("/api/v2/groups/{group_id}/files"), &token_b),
        authed_multipart_file(
            &format!("/api/v2/groups/{group_id}/files"),
            &token_b,
            "file",
            "nope.txt",
            Some("text/plain"),
            b"nope",
        ),
        authed(
            "DELETE",
            &format!("/api/v2/groups/{group_id}/files/{file_id}"),
            &token_b,
        ),
    ] {
        let (status, body) = send(&app, request).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "permission_denied");
    }
    assert!(!group_upload_file(root.path(), "nope.txt").exists());
}

#[tokio::test]
async fn group_files_rejects_uploads_symlink_escape() {
    let (app, state) = app_with_state().await;
    let token = register_and_login(&app, "group-files-uploads-symlink@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Files WS").await;
    let outside = tempfile::tempdir().unwrap();
    if create_dir_symlink(outside.path(), &root.path().join("uploads")).is_err() {
        return;
    }
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    let (status, body) = send(
        &app,
        authed_multipart_file(
            &format!("/api/v2/groups/{group_id}/files"),
            &token,
            "file",
            "escape.txt",
            Some("text/plain"),
            b"escape",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
    assert!(!outside.path().join("escape.txt").exists());

    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM group_files WHERE group_id = ? AND status = 'active'",
    )
    .bind(group_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn group_files_rejects_final_upload_path_symlink_escape() {
    let (app, state) = app_with_state().await;
    let token = register_and_login(&app, "group-files-file-symlink@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Files WS").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    let uploads = root.path().join("uploads");
    std::fs::create_dir_all(&uploads).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_target = outside.path().join("missing-target.txt");
    let upload_path = uploads.join("escape.txt");
    assert!(!outside_target.exists());
    if create_file_symlink(&outside_target, &upload_path).is_err() {
        return;
    }
    assert!(std::fs::symlink_metadata(&upload_path)
        .unwrap()
        .file_type()
        .is_symlink());

    let (status, body) = send(
        &app,
        authed_multipart_file(
            &format!("/api/v2/groups/{group_id}/files"),
            &token,
            "file",
            "escape.txt",
            Some("text/plain"),
            b"escape",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
    assert!(!outside_target.exists());
    assert!(std::fs::symlink_metadata(&upload_path)
        .unwrap()
        .file_type()
        .is_symlink());

    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM group_files WHERE group_id = ? AND status = 'active'",
    )
    .bind(group_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn workspace_files_root_and_list_returns_canonical_children() {
    let app = app().await;
    let token = register_and_login(&app, "workspace-files-list@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Workspace Files").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    std::fs::create_dir(root.path().join("Zoo")).unwrap();
    std::fs::create_dir(root.path().join("alpha")).unwrap();
    std::fs::write(root.path().join("aardvark.txt"), b"first").unwrap();
    std::fs::write(root.path().join("Beta.txt"), b"second").unwrap();
    std::fs::write(root.path().join(".hidden"), b"hidden").unwrap();
    std::fs::write(root.path().join("alpha").join("nested.txt"), b"nested").unwrap();

    let (status, body) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/groups/{group_id}/workspace-files/root"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["root"],
        root.path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string()
    );
    assert_eq!(body["separator"], std::path::MAIN_SEPARATOR.to_string());

    let (status, list) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/groups/{group_id}/workspace-files"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let rows = list.as_array().unwrap();
    let names: Vec<&str> = rows
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["alpha", "Zoo", "aardvark.txt", "Beta.txt"]);
    assert!(!rows.iter().any(|row| row["name"] == ".hidden"));
    assert!(!rows.iter().any(|row| row["path"] == "alpha/nested.txt"));
    assert_eq!(rows[0]["path"], "alpha");
    assert_eq!(rows[0]["is_dir"], true);
    assert_eq!(rows[0]["size"], Value::Null);
    assert!(rows[0]["modified_at"].as_str().is_some());
    assert_eq!(
        PathBuf::from(rows[0]["abs_path"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        root.path().join("alpha").canonicalize().unwrap()
    );
    assert_eq!(rows[2]["path"], "aardvark.txt");
    assert_eq!(rows[2]["is_dir"], false);
    assert_eq!(rows[2]["size"], 5);

    let (status, body) = send(
        &app,
        authed("GET", &workspace_file_url(group_id, "aardvark.txt"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
}

#[tokio::test]
async fn workspace_files_preview_handles_text_truncation_and_binary() {
    let app = app().await;
    let token = register_and_login(&app, "workspace-files-preview@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Workspace Files").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    std::fs::write(root.path().join("note.md"), "hello preview").unwrap();
    std::fs::write(root.path().join("large.txt"), "a".repeat(70 * 1024)).unwrap();
    std::fs::write(root.path().join("binary.bin"), [0, 1, 2, 3]).unwrap();
    let mut late_nul = vec![b'a'; 4097];
    late_nul.push(0);
    late_nul.extend_from_slice(b"after-nul");
    std::fs::write(root.path().join("late-nul.txt"), late_nul).unwrap();
    let mut late_invalid_utf8 = vec![b'a'; 4097];
    late_invalid_utf8.push(0xFF);
    late_invalid_utf8.extend_from_slice(b"after-invalid");
    std::fs::write(root.path().join("late-invalid.bin"), late_invalid_utf8).unwrap();

    let (status, preview) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/groups/{group_id}/workspace-files/preview?path=note.md"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["path"], "note.md");
    assert_eq!(preview["name"], "note.md");
    assert_eq!(preview["is_text"], true);
    assert_eq!(preview["content"], "hello preview");
    assert_eq!(preview["truncated"], false);
    assert_eq!(preview["message"], Value::Null);
    assert_eq!(preview["size"], 13);

    let (status, preview) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/groups/{group_id}/workspace-files/preview?path=large.txt"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["is_text"], true);
    assert_eq!(preview["truncated"], true);
    assert_eq!(preview["content"].as_str().unwrap().chars().count(), 20_000);

    let (status, preview) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/groups/{group_id}/workspace-files/preview?path=binary.bin"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["is_text"], false);
    assert_eq!(preview["content"], Value::Null);
    assert_eq!(
        preview["message"],
        "Preview is not available for binary or unsupported files."
    );
    assert_eq!(preview["size"], 4);

    let (status, preview) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/groups/{group_id}/workspace-files/preview?path=late-nul.txt"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["is_text"], false);
    assert_eq!(preview["content"], Value::Null);
    assert_eq!(
        preview["message"],
        "Preview is not available for binary or unsupported files."
    );

    let (status, preview) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/groups/{group_id}/workspace-files/preview?path=late-invalid.bin"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["is_text"], false);
    assert_eq!(preview["content"], Value::Null);
    assert_eq!(
        preview["message"],
        "Preview is not available for binary or unsupported files."
    );
}

#[tokio::test]
async fn workspace_files_upload_writes_uploads_and_rejects_bad_inputs() {
    let app = app().await;
    let token = register_and_login(&app, "workspace-files-upload@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Workspace Files").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    let (status, uploaded) = send(
        &app,
        authed_multipart_file(
            &format!("/api/v2/groups/{group_id}/workspace-files/upload"),
            &token,
            "file",
            "plan.txt",
            Some("text/plain"),
            b"uploaded",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(uploaded["path"], "uploads/plan.txt");
    assert_eq!(uploaded["name"], "plan.txt");
    assert_eq!(uploaded["is_dir"], false);
    assert_eq!(uploaded["size"], 8);
    assert_eq!(
        std::fs::read(group_upload_file(root.path(), "plan.txt")).unwrap(),
        b"uploaded"
    );

    let (status, body) = send(
        &app,
        authed_multipart_file(
            &format!("/api/v2/groups/{group_id}/workspace-files/upload"),
            &token,
            "file",
            "plan.txt",
            Some("text/plain"),
            b"duplicate",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "conflict");
    assert_eq!(
        std::fs::read(group_upload_file(root.path(), "plan.txt")).unwrap(),
        b"uploaded"
    );

    let (status, body) = send(
        &app,
        authed_multipart_file(
            &format!("/api/v2/groups/{group_id}/workspace-files/upload"),
            &token,
            "other",
            "ignored.txt",
            Some("text/plain"),
            b"ignored",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");

    for filename in [
        "",
        "   ",
        "../escape.txt",
        "dir/name.txt",
        "dir\\name.txt",
        "C:evil.txt",
        "C:\\evil.txt",
        "\\\\server\\share.txt",
        "//server/share.txt",
    ] {
        let (status, body) = send(
            &app,
            authed_multipart_file(
                &format!("/api/v2/groups/{group_id}/workspace-files/upload"),
                &token,
                "file",
                filename,
                Some("text/plain"),
                b"blocked",
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "filename {filename:?}");
        assert_eq!(body["error"]["code"], "invalid_input");
    }

    let oversized = vec![b'x'; 25 * 1024 * 1024 + 1];
    let (status, body) = send(
        &app,
        authed_multipart_file(
            &format!("/api/v2/groups/{group_id}/workspace-files/upload"),
            &token,
            "file",
            "too-large.txt",
            Some("text/plain"),
            &oversized,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
    assert!(!group_upload_file(root.path(), "too-large.txt").exists());
}

#[tokio::test]
async fn attachment_message_persists_workspace_image_metadata() {
    let (app, state) = app_with_state().await;
    let token = register_and_login(&app, "attachment-message@example.com").await;
    let (_root, workspace) = create_local_workspace(&app, &token, "Attachment Messages").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    let (status, _) = send(
        &app,
        authed_multipart_file(
            &format!("/api/v2/groups/{group_id}/workspace-files/upload"),
            &token,
            "file",
            "diagram.png",
            Some("image/png"),
            b"PNG!",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/messages"),
            &token,
            json!({"content":"", "attachments":[{"path":"uploads/diagram.png"}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let attachments = &body["user_message"]["attachments"];
    assert_eq!(attachments[0]["path"], "uploads/diagram.png");
    assert_eq!(attachments[0]["name"], "diagram.png");
    assert_eq!(attachments[0]["mime_type"], "image/png");
    assert_eq!(attachments[0]["size"], 4);
    assert_eq!(attachments[0]["kind"], "image");

    let event_payload: String = sqlx::query_scalar(
        "SELECT payload_json FROM stream_events WHERE kind = 'user_message' ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    let event_payload: Value = serde_json::from_str(&event_payload).unwrap();
    assert_eq!(event_payload["attachments"], *attachments);

    let (status, messages) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/groups/{group_id}/messages"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(messages[0]["attachments"], *attachments);
}

#[tokio::test]
async fn attachment_message_rejects_unsafe_or_duplicate_paths_without_insertion() {
    let (app, state) = app_with_state().await;
    let token = register_and_login(&app, "attachment-message-reject@example.com").await;
    let (_root, workspace) = create_local_workspace(&app, &token, "Attachment Messages").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    let (status, _) = send(
        &app,
        authed_multipart_file(
            &format!("/api/v2/groups/{group_id}/workspace-files/upload"),
            &token,
            "file",
            "diagram.png",
            Some("image/png"),
            b"PNG!",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    for attachments in [
        json!([{"path":"../secret.txt"}]),
        json!([{"path":"uploads/diagram.png"}, {"path":"uploads/diagram.png"}]),
    ] {
        let (status, body) = send(
            &app,
            authed_json(
                "POST",
                &format!("/api/v2/groups/{group_id}/messages/stream"),
                &token,
                json!({"content":"", "attachments": attachments}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_input");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE group_id = ?")
            .bind(group_id)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}

#[tokio::test]
async fn workspace_files_download_returns_bytes_and_download_headers() {
    let app = app().await;
    let token = register_and_login(&app, "workspace-files-download@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Workspace Files").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    std::fs::write(root.path().join("archive.weird"), b"download me").unwrap();

    let (status, headers, bytes) = send_bytes(
        &app,
        authed(
            "GET",
            &format!("/api/v2/groups/{group_id}/workspace-files/download?path=archive.weird"),
            &token,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, b"download me");
    assert_eq!(
        headers.get("content-type").unwrap().to_str().unwrap(),
        "application/octet-stream"
    );
    assert!(headers
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("filename=\"archive.weird\""));

    std::fs::write(root.path().join("报告.txt"), b"unicode name").unwrap();
    let (status, headers, bytes) = send_bytes(
        &app,
        authed(
            "GET",
            &format!(
                "/api/v2/groups/{group_id}/workspace-files/download?path=%E6%8A%A5%E5%91%8A.txt"
            ),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, b"unicode name");
    assert!(headers
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("filename=\"__.txt\""));
}

#[tokio::test]
async fn workspace_files_rename_moves_files_and_rejects_invalid_destinations() {
    let app = app().await;
    let token = register_and_login(&app, "workspace-files-rename@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Workspace Files").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    std::fs::write(root.path().join("source.txt"), b"source").unwrap();
    let (status, renamed) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/workspace-files/rename?path=source.txt"),
            &token,
            json!({"new_path": " renamed.txt "}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["path"], "renamed.txt");
    assert_eq!(
        std::fs::read(root.path().join("renamed.txt")).unwrap(),
        b"source"
    );
    assert!(!root.path().join("source.txt").exists());

    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/workspace-files/rename?path="),
            &token,
            json!({"new_path": "root-renamed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");

    std::fs::write(root.path().join("collision-source.txt"), b"source").unwrap();
    std::fs::write(root.path().join("existing.txt"), b"existing").unwrap();
    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/workspace-files/rename?path=collision-source.txt"),
            &token,
            json!({"new_path": "existing.txt"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "conflict");
    assert_eq!(
        std::fs::read(root.path().join("collision-source.txt")).unwrap(),
        b"source"
    );

    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/workspace-files/rename?path=collision-source.txt"),
            &token,
            json!({"new_path": "   "}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");

    let outside = tempfile::tempdir().unwrap();
    if create_dir_symlink(outside.path(), &root.path().join("link")).is_ok() {
        let (status, body) = send(
            &app,
            authed_json(
                "PATCH",
                &format!(
                    "/api/v2/groups/{group_id}/workspace-files/rename?path=collision-source.txt"
                ),
                &token,
                json!({"new_path": "link/out.txt"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_input");
        assert!(!outside.path().join("out.txt").exists());
        assert!(root.path().join("collision-source.txt").exists());
    }
}

#[tokio::test]
async fn workspace_files_delete_removes_files_and_empty_directories_only() {
    let app = app().await;
    let token = register_and_login(&app, "workspace-files-delete@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Workspace Files").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    std::fs::write(root.path().join("delete.txt"), b"delete").unwrap();
    std::fs::create_dir(root.path().join("empty-dir")).unwrap();
    std::fs::create_dir(root.path().join("non-empty")).unwrap();
    std::fs::write(root.path().join("non-empty").join("child.txt"), b"child").unwrap();

    let (status, body) = send(
        &app,
        authed(
            "DELETE",
            &workspace_file_url(group_id, "delete.txt"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);
    assert!(!root.path().join("delete.txt").exists());

    let (status, body) = send(
        &app,
        authed("DELETE", &workspace_file_url(group_id, "empty-dir"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);
    assert!(!root.path().join("empty-dir").exists());

    let (status, body) = send(
        &app,
        authed("DELETE", &workspace_file_url(group_id, ""), &token),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
    assert!(root.path().exists());

    let (status, body) = send(
        &app,
        authed("DELETE", &workspace_file_url(group_id, "non-empty"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
    assert!(root.path().join("non-empty").exists());
}

#[tokio::test]
async fn workspace_git_status_stage_unstage_and_commit() {
    if !git_available() {
        return;
    }
    let app = app().await;
    let token = register_and_login(&app, "workspace-git-flow@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Workspace Git").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    init_git_repo(root.path());

    std::fs::write(root.path().join("tracked.txt"), b"changed").unwrap();
    std::fs::write(root.path().join("new.txt"), b"new").unwrap();

    let (status, git_status) = send(
        &app,
        authed("GET", &workspace_git_url(group_id, "status"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(git_status["available"], true);
    assert_eq!(git_status["status"], "ready");
    assert_eq!(git_status["clean"], false);
    assert!(git_status["branch"].as_str().is_some());
    assert_eq!(git_status["stash_count"], 0);
    assert_eq!(git_status["dirty_counts"]["staged"], 0);
    assert_eq!(git_status["dirty_counts"]["unstaged"], 1);
    assert_eq!(git_status["dirty_counts"]["untracked"], 1);
    assert_eq!(git_status["dirty_counts"]["conflicted"], 0);
    let tracked = git_status_file(&git_status, "tracked.txt");
    assert_eq!(tracked["status"], " M");
    assert_eq!(tracked["staged"], false);
    assert_eq!(tracked["unstaged"], true);
    assert_eq!(tracked["untracked"], false);
    assert_eq!(tracked["conflicted"], false);
    assert!(tracked["old_path"].is_null());
    let new_file = git_status_file(&git_status, "new.txt");
    assert_eq!(new_file["status"], "??");
    assert_eq!(new_file["staged"], false);
    assert_eq!(new_file["unstaged"], true);
    assert_eq!(new_file["untracked"], true);
    assert_eq!(new_file["conflicted"], false);

    let (status, staged) = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, "stage"),
            &token,
            json!({"paths": ["tracked.txt"]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let tracked = git_status_file(&staged, "tracked.txt");
    assert_eq!(tracked["status"], "M ");
    assert_eq!(tracked["staged"], true);
    assert_eq!(tracked["unstaged"], false);

    let (status, unstaged) = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, "unstage"),
            &token,
            json!({"paths": ["tracked.txt"]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let tracked = git_status_file(&unstaged, "tracked.txt");
    assert_eq!(tracked["status"], " M");
    assert_eq!(tracked["staged"], false);
    assert_eq!(tracked["unstaged"], true);

    let (status, staged_all) = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, "stage"),
            &token,
            json!({"paths": []}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(git_status_file(&staged_all, "tracked.txt")["status"], "M ");
    assert_eq!(git_status_file(&staged_all, "new.txt")["status"], "A ");

    let (status, committed) = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, "commit"),
            &token,
            json!({"message": "save workspace changes"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(committed["available"], true);
    assert_eq!(committed["status"], "ready");
    assert_eq!(committed["clean"], true);
    assert_eq!(committed["files"].as_array().unwrap().len(), 0);
    assert_eq!(committed["dirty_counts"]["staged"], 0);
    assert_eq!(committed["dirty_counts"]["unstaged"], 0);
    assert_eq!(committed["dirty_counts"]["untracked"], 0);
    assert_eq!(committed["dirty_counts"]["conflicted"], 0);
}

#[tokio::test]
async fn workspace_git_branch_and_repository_operations() {
    if !git_available() {
        return;
    }
    let app = app().await;
    let token = register_and_login(&app, "workspace-git-operations@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Workspace Git Ops").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    let (status, initialized) = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, "init"),
            &token,
            json!({"branch":"main"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{initialized:?}");
    assert_eq!(initialized["status"], "ready");
    run_git(root.path(), &["config", "user.email", "tests@example.com"]);
    run_git(root.path(), &["config", "user.name", "Tests"]);
    std::fs::write(root.path().join("tracked.txt"), b"initial").unwrap();
    run_git(root.path(), &["add", "tracked.txt"]);
    run_git(root.path(), &["commit", "-m", "initial"]);

    let (status, created) = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, "branches"),
            &token,
            json!({"name":"review","start_point":"main"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created:?}");
    assert!(created["branches"]
        .as_array()
        .unwrap()
        .iter()
        .any(|branch| branch["name"] == "review" && branch["kind"] == "local"));
    let (status, switched) = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, "branches/switch"),
            &token,
            json!({"name":"review","kind":"local"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{switched:?}");
    assert_eq!(switched["branch"], "review");
    let (status, renamed) = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, "branches/rename"),
            &token,
            json!({"old":"review","new":"review-renamed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renamed:?}");
    run_git(root.path(), &["switch", "main"]);
    let (status, deleted) = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, "branches/delete"),
            &token,
            json!({"name":"review-renamed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{deleted:?}");
    assert!(!deleted["branches"]
        .as_array()
        .unwrap()
        .iter()
        .any(|branch| branch["name"] == "review-renamed"));

    std::fs::write(root.path().join("tracked.txt"), b"changed").unwrap();
    std::fs::write(root.path().join("untracked.txt"), b"untracked").unwrap();
    let (status, discarded) = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, "discard"),
            &token,
            json!({"paths":["tracked.txt"],"all":false}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{discarded:?}");
    assert_eq!(
        std::fs::read(root.path().join("tracked.txt")).unwrap(),
        b"initial"
    );
    let (status, ambiguous) = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, "discard"),
            &token,
            json!({"paths":[],"all":false}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(ambiguous["error"]["code"], "invalid_input");
    let (status, ignored) = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, "ignore"),
            &token,
            json!({"path":"untracked.txt"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ignored:?}");
    let _ = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, "ignore"),
            &token,
            json!({"path":"untracked.txt"}),
        ),
    )
    .await;
    assert_eq!(
        std::fs::read_to_string(root.path().join(".gitignore"))
            .unwrap()
            .matches("untracked.txt")
            .count(),
        1
    );

    std::fs::write(root.path().join("stash.txt"), b"stash").unwrap();
    let (status, stashed) = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, "stash/push"),
            &token,
            json!({"message":"save"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{stashed:?}");
    assert_eq!(stashed["stash_count"], 1);
    let (status, popped) = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, "stash/pop"),
            &token,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{popped:?}");
    assert!(root.path().join("stash.txt").exists());

    let (status, missing_remote) = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, "fetch"),
            &token,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(missing_remote["error"]["code"], "missing_remote");
    assert_eq!(
        missing_remote["error"]["message"],
        "git remote is not configured; set a remote URL before fetch, pull, or push"
    );
    let (status, remote) = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, "set-remote"),
            &token,
            json!({"remote_url":"https://example.invalid/repo.git"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{remote:?}");
    assert_eq!(remote["remote_name"], "origin");
}

#[tokio::test]
async fn workspace_git_diff_returns_worktree_staged_branch_and_validates_path() {
    if !git_available() {
        return;
    }
    let app = app().await;
    let token = register_and_login(&app, "workspace-git-diff@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Workspace Git").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    init_git_repo(root.path());

    std::fs::write(root.path().join("tracked.txt"), b"changed\n").unwrap();

    let (status, worktree) = send(
        &app,
        authed(
            "GET",
            &format!(
                "{}?mode=worktree&path=tracked.txt",
                workspace_git_url(group_id, "diff")
            ),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(worktree["mode"], "worktree");
    assert_eq!(worktree["path"], "tracked.txt");
    assert!(worktree["patch"].as_str().unwrap().contains("+changed"));
    assert_eq!(worktree["truncated"], false);

    let (status, staged_empty) = send(
        &app,
        authed(
            "GET",
            &format!("{}?mode=staged", workspace_git_url(group_id, "diff")),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(staged_empty["patch"], "");

    run_git(root.path(), &["add", "tracked.txt"]);
    let (status, staged) = send(
        &app,
        authed(
            "GET",
            &format!("{}?mode=staged", workspace_git_url(group_id, "diff")),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(staged["mode"], "staged");
    assert!(staged["patch"].as_str().unwrap().contains("+changed"));

    run_git(root.path(), &["commit", "-m", "change"]);
    let (status, branch) = send(
        &app,
        authed(
            "GET",
            &format!("{}?mode=branch", workspace_git_url(group_id, "diff")),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(branch["mode"], "branch");
    assert_eq!(branch["head_ref"], "HEAD");
    assert!(branch["patch"].as_str().unwrap().contains("+changed"));

    let (status, invalid_path) = send(
        &app,
        authed(
            "GET",
            &format!(
                "{}?mode=worktree&path=..%2Fsecret.txt",
                workspace_git_url(group_id, "diff")
            ),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(invalid_path["error"]["code"], "invalid_input");
}

#[tokio::test]
async fn workspace_git_diff_marks_large_patch_as_truncated() {
    if !git_available() {
        return;
    }
    let app = app().await;
    let token = register_and_login(&app, "workspace-git-diff-large@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Workspace Git").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    init_git_repo(root.path());

    std::fs::write(root.path().join("tracked.txt"), "changed\n".repeat(30_000)).unwrap();
    let (status, body) = send(
        &app,
        authed(
            "GET",
            &format!("{}?mode=worktree", workspace_git_url(group_id, "diff")),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["truncated"], true);
    assert!(body["patch"].as_str().unwrap().contains("[diff truncated]"));
}

#[tokio::test]
async fn workspace_git_log_returns_newest_commits_with_pagination() {
    if !git_available() {
        return;
    }
    let app = app().await;
    let token = register_and_login(&app, "workspace-git-log@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Workspace Git").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    init_git_repo(root.path());
    std::fs::write(root.path().join("tracked.txt"), b"second").unwrap();
    run_git(root.path(), &["add", "tracked.txt"]);
    run_git(root.path(), &["commit", "-m", "second commit"]);

    let (status, first_page) = send(
        &app,
        authed(
            "GET",
            &format!("{}?limit=1&skip=0", workspace_git_url(group_id, "log")),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {first_page:?}");
    assert_eq!(first_page["commits"].as_array().unwrap().len(), 1);
    assert_eq!(first_page["commits"][0]["subject"], "second commit");
    assert_eq!(first_page["has_more"], true);
    assert_eq!(first_page["commits"][0]["sha"].as_str().unwrap().len(), 40);

    let (status, second_page) = send(
        &app,
        authed(
            "GET",
            &format!("{}?limit=1&skip=1", workspace_git_url(group_id, "log")),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {second_page:?}");
    assert_eq!(second_page["commits"][0]["subject"], "initial");
    assert_eq!(second_page["has_more"], false);

    let (status, invalid) = send(
        &app,
        authed(
            "GET",
            &format!("{}?limit=101", workspace_git_url(group_id, "log")),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(invalid["error"]["code"], "invalid_input");
}

#[tokio::test]
async fn workspace_git_commit_details_diff_and_branch_creation_validate_inputs() {
    if !git_available() {
        return;
    }
    let app = app().await;
    let token = register_and_login(&app, "workspace-git-commit@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Workspace Git").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    init_git_repo(root.path());
    std::fs::write(root.path().join("tracked.txt"), b"changed\n").unwrap();
    run_git(root.path(), &["add", "tracked.txt"]);
    run_git(root.path(), &["commit", "-m", "change tracked file"]);
    let sha = Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .current_dir(root.path())
        .output()
        .unwrap();
    let sha = String::from_utf8(sha.stdout).unwrap().trim().to_string();

    let (status, details) = send(
        &app,
        authed(
            "GET",
            &workspace_git_url(group_id, &format!("commits/{sha}")),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {details:?}");
    assert_eq!(details["sha"], sha);
    assert_eq!(details["subject"], "change tracked file");
    assert_eq!(details["files"][0]["path"], "tracked.txt");
    assert_eq!(details["insertions"], 1);
    assert_eq!(details["deletions"], 1);

    let (status, diff) = send(
        &app,
        authed(
            "GET",
            &format!(
                "{}?path=tracked.txt",
                workspace_git_url(group_id, &format!("commits/{sha}/diff"))
            ),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {diff:?}");
    assert_eq!(diff["mode"], "commit");
    assert!(diff["patch"].as_str().unwrap().contains("+changed"));

    let (status, created) = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, &format!("commits/{sha}/create-branch")),
            &token,
            json!({"name": "review/commit"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "body: {created:?}");
    let branch = Command::new("git")
        .args(["rev-parse", "review/commit"])
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(branch.status.success());

    for (url, body) in [
        (workspace_git_url(group_id, "commits/HEAD"), None),
        (
            workspace_git_url(
                group_id,
                &format!("commits/{sha}/diff?path=..%2Fsecret.txt"),
            ),
            None,
        ),
        (
            workspace_git_url(group_id, &format!("commits/{sha}/create-branch")),
            Some(json!({"name": "-bad"})),
        ),
    ] {
        let request = match body {
            Some(body) => authed_json("POST", &url, &token, body),
            None => authed("GET", &url, &token),
        };
        let (status, response) = send(&app, request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {response:?}");
        assert_eq!(response["error"]["code"], "invalid_input");
    }
}

#[tokio::test]
async fn workspace_git_commit_message_generates_from_staged_diff() {
    if !git_available() {
        return;
    }
    let app = app().await;
    let token = register_and_login(&app, "workspace-git-ai-message@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Workspace Git").await;
    let provider_body = format!(
        "data: {}\ndata: [DONE]\n",
        json!({"choices": [{"delta": {"content": "```\n\"Update tracked file\"\n```"}}]})
    );
    let provider = create_llm_provider(&app, &token, &fake_provider(provider_body).await).await;
    let agent = create_llm_agent(&app, &token, &workspace, &provider, "Committer").await;
    let group =
        create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[agent.as_str()]).await;
    let group_id = group["id"].as_str().unwrap();
    init_git_repo(root.path());

    std::fs::write(root.path().join("tracked.txt"), b"changed").unwrap();
    run_git(root.path(), &["add", "tracked.txt"]);

    let (status, body) = send(
        &app,
        authed(
            "POST",
            &workspace_git_url(group_id, "commit-message"),
            &token,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["message"], "Update tracked file");
}

#[tokio::test]
async fn workspace_git_commit_message_requires_staged_changes() {
    if !git_available() {
        return;
    }
    let app = app().await;
    let token = register_and_login(&app, "workspace-git-ai-no-staged@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Workspace Git").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    init_git_repo(root.path());

    let (status, body) = send(
        &app,
        authed(
            "POST",
            &workspace_git_url(group_id, "commit-message"),
            &token,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("stage changes"));
}

#[tokio::test]
async fn workspace_git_commit_message_requires_active_provider() {
    if !git_available() {
        return;
    }
    let app = app().await;
    let token = register_and_login(&app, "workspace-git-ai-no-provider@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Workspace Git").await;
    let agent = create_agent(&app, &token, &workspace, "No Provider").await;
    let group =
        create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[agent.as_str()]).await;
    let group_id = group["id"].as_str().unwrap();
    init_git_repo(root.path());

    std::fs::write(root.path().join("tracked.txt"), b"changed").unwrap();
    run_git(root.path(), &["add", "tracked.txt"]);

    let (status, body) = send(
        &app,
        authed(
            "POST",
            &workspace_git_url(group_id, "commit-message"),
            &token,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("LLM provider"));
}

#[tokio::test]
async fn workspace_git_reports_non_repo_failures_and_rejects_unsafe_paths() {
    if !git_available() {
        return;
    }
    let app = app().await;
    let token = register_and_login(&app, "workspace-git-errors@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Workspace Git").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    let (status, non_repo) = send(
        &app,
        authed("GET", &workspace_git_url(group_id, "status"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(non_repo["available"], false);
    assert_eq!(non_repo["status"], "not_repo");
    assert_eq!(non_repo["clean"], true);
    assert_eq!(non_repo["stash_count"], 0);
    assert_eq!(non_repo["dirty_counts"]["staged"], 0);
    assert_eq!(non_repo["dirty_counts"]["unstaged"], 0);
    assert_eq!(non_repo["dirty_counts"]["untracked"], 0);
    assert_eq!(non_repo["dirty_counts"]["conflicted"], 0);
    assert!(non_repo["message"]
        .as_str()
        .unwrap()
        .contains("Git repository"));

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, "stage"),
            &token,
            json!({"paths": ["../secret.txt"]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");

    init_git_repo(root.path());

    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), b"secret").unwrap();
    if create_dir_symlink(outside.path(), &root.path().join("link")).is_ok() {
        let (status, body) = send(
            &app,
            authed_json(
                "POST",
                &workspace_git_url(group_id, "stage"),
                &token,
                json!({"paths": ["link/secret.txt"]}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_input");
    }

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, "commit"),
            &token,
            json!({"message": ""}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            &workspace_git_url(group_id, "push"),
            &token,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "missing_remote");
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("set a remote URL"));
}

#[tokio::test]
async fn workspace_files_cloud_or_unbound_workspace_returns_client_errors() {
    let app = app().await;
    let token = register_and_login(&app, "workspace-files-workspace-errors@example.com").await;
    let cloud_workspace = create_workspace(&app, &token).await;
    let cloud_group =
        create_group_with_initial_agents(&app, &token, &cloud_workspace, "mesh", &[]).await;
    let cloud_group_id = cloud_group["id"].as_str().unwrap();

    assert_workspace_file_route_errors(
        &app,
        &token,
        cloud_group_id,
        StatusCode::BAD_REQUEST,
        "invalid_input",
    )
    .await;

    let (_root, local_workspace) = create_local_workspace(&app, &token, "Workspace Files").await;
    let local_group =
        create_group_with_initial_agents(&app, &token, &local_workspace, "mesh", &[]).await;
    let local_group_id = local_group["id"].as_str().unwrap();
    let (status, _) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{local_group_id}"),
            &token,
            json!({"workspace_id": Value::Null}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    assert_workspace_file_route_errors(
        &app,
        &token,
        local_group_id,
        StatusCode::BAD_REQUEST,
        "invalid_input",
    )
    .await;
}

#[tokio::test]
async fn workspace_files_cross_owner_all_routes_are_rejected() {
    let app = app().await;
    let token_a = register_and_login(&app, "workspace-files-cross-a@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token_a, "Workspace Files").await;
    let group = create_group_with_initial_agents(&app, &token_a, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    std::fs::write(root.path().join("missing.txt"), b"private").unwrap();
    let token_b = register_and_login(&app, "workspace-files-cross-b@example.com").await;

    assert_workspace_file_route_errors(
        &app,
        &token_b,
        group_id,
        StatusCode::FORBIDDEN,
        "permission_denied",
    )
    .await;
    assert!(!group_upload_file(root.path(), "blocked.txt").exists());
}

#[tokio::test]
async fn workspace_files_rejects_explicit_dot_path() {
    let app = app().await;
    let token = register_and_login(&app, "workspace-files-dot-path@example.com").await;
    let (_root, workspace) = create_local_workspace(&app, &token, "Workspace Files").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    let unsafe_path_message =
        "workspace file paths must be relative and stay inside the group workspace";

    for request in [
        authed("GET", &workspace_file_url(group_id, "."), &token),
        authed(
            "GET",
            &format!("/api/v2/groups/{group_id}/workspace-files/preview?path=."),
            &token,
        ),
        authed(
            "GET",
            &format!("/api/v2/groups/{group_id}/workspace-files/download?path=."),
            &token,
        ),
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/workspace-files/rename?path=."),
            &token,
            json!({"new_path": "renamed.txt"}),
        ),
        authed("DELETE", &workspace_file_url(group_id, "."), &token),
    ] {
        let (status, body) = send(&app, request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_input");
        assert_eq!(body["error"]["message"], unsafe_path_message);
    }
}

#[tokio::test]
async fn workspace_files_reject_path_traversal_and_symlink_escapes() {
    let app = app().await;
    let token = register_and_login(&app, "workspace-files-path-safety@example.com").await;
    let (root, workspace) = create_local_workspace(&app, &token, "Workspace Files").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    for path in [
        "..",
        "../secret.txt",
        "/absolute.txt",
        "C:%5Csecret.txt",
        "%5C%5Cserver%5Cshare.txt",
        "dir//file.txt",
        "dir/./file.txt",
    ] {
        let (status, body) = send(
            &app,
            authed("GET", &workspace_file_url(group_id, path), &token),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "path {path:?}");
        assert_eq!(body["error"]["code"], "invalid_input");
    }

    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), b"secret").unwrap();
    if create_dir_symlink(outside.path(), &root.path().join("link")).is_ok() {
        let (status, body) = send(
            &app,
            authed(
                "GET",
                &format!("/api/v2/groups/{group_id}/workspace-files/preview?path=link/secret.txt"),
                &token,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_input");
    }

    let outside_uploads = tempfile::tempdir().unwrap();
    let uploads_link = root.path().join("uploads");
    if create_dir_symlink(outside_uploads.path(), &uploads_link).is_ok() {
        let (status, body) = send(
            &app,
            authed_multipart_file(
                &format!("/api/v2/groups/{group_id}/workspace-files/upload"),
                &token,
                "file",
                "leak.txt",
                Some("text/plain"),
                b"leak",
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_input");
        assert!(!outside_uploads.path().join("leak.txt").exists());
        remove_symlink(&uploads_link).unwrap();
    }

    let uploads = root.path().join("uploads");
    std::fs::create_dir_all(&uploads).unwrap();
    let dangling_target = outside.path().join("missing-upload-target.txt");
    let upload_path = uploads.join("escape.txt");
    if create_file_symlink(&dangling_target, &upload_path).is_ok() {
        let (status, body) = send(
            &app,
            authed_multipart_file(
                &format!("/api/v2/groups/{group_id}/workspace-files/upload"),
                &token,
                "file",
                "escape.txt",
                Some("text/plain"),
                b"escape",
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_input");
        assert!(!dangling_target.exists());
        assert!(std::fs::symlink_metadata(&upload_path)
            .unwrap()
            .file_type()
            .is_symlink());
    }
}

#[tokio::test]
async fn group_create_initial_agents_inserts_topology_defaults() {
    let (app, state) = app_with_state().await;
    let token = register_and_login(&app, "initial-agents@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let agent_a = create_agent(&app, &token, &workspace, "Alpha").await;
    let agent_b = create_agent(&app, &token, &workspace, "Beta").await;

    for mode in ["star", "hierarchical", "ring"] {
        let (status, group) = send(
            &app,
            authed_json(
                "POST",
                "/api/v2/groups",
                &token,
                json!({
                    "name": format!("{mode} group"),
                    "workspace_id": workspace,
                    "communication_mode": mode,
                    "initial_agents": [agent_a, agent_b]
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let group_id = group["id"].as_str().unwrap();
        assert_eq!(group["communication_mode"], mode);

        let first = group_agent_row(&state, group_id, &agent_a).await;
        let second = group_agent_row(&state, group_id, &agent_b).await;
        assert_eq!(first.response_mode, "mentioned_only");
        assert_eq!(second.response_mode, "mentioned_only");
        assert_shared_group_workspace(&first.context_scope_json);
        assert_shared_group_workspace(&second.context_scope_json);

        match mode {
            "star" => {
                assert_eq!(first.topology_role.as_deref(), Some("hub"));
                assert_eq!(first.speaking_order, None);
                assert_eq!(second.topology_role, None);
                assert_eq!(second.speaking_order, None);
            }
            "hierarchical" => {
                assert_eq!(first.topology_role.as_deref(), Some("worker"));
                assert_eq!(second.topology_role.as_deref(), Some("worker"));
                assert_eq!(first.speaking_order, None);
                assert_eq!(second.speaking_order, None);
            }
            "ring" => {
                assert_eq!(first.topology_role, None);
                assert_eq!(second.topology_role, None);
                assert_eq!(first.speaking_order, Some(1));
                assert_eq!(second.speaking_order, Some(2));
            }
            _ => unreachable!(),
        }
    }
}

#[tokio::test]
async fn group_patch_ring_to_star_normalizes_existing_topology() {
    let (app, state) = app_with_state().await;
    let token = register_and_login(&app, "ring-to-star@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let agent_a = create_agent(&app, &token, &workspace, "Alpha").await;
    let agent_b = create_agent(&app, &token, &workspace, "Beta").await;
    let group = create_group_with_initial_agents(
        &app,
        &token,
        &workspace,
        "ring",
        &[agent_a.as_str(), agent_b.as_str()],
    )
    .await;
    let group_id = group["id"].as_str().unwrap();

    let updated = patch_group_mode(&app, &token, group_id, "star").await;
    assert_eq!(updated["communication_mode"], "star");

    let rows = [
        group_agent_row(&state, group_id, &agent_a).await,
        group_agent_row(&state, group_id, &agent_b).await,
    ];
    assert_eq!(
        rows.iter()
            .filter(|row| row.topology_role.as_deref() == Some("hub"))
            .count(),
        1
    );
    assert_eq!(rows[0].topology_role.as_deref(), Some("hub"));
    assert!(rows.iter().all(|row| row.speaking_order.is_none()));
}

#[tokio::test]
async fn group_patch_star_to_ring_assigns_speaking_orders() {
    let (app, state) = app_with_state().await;
    let token = register_and_login(&app, "star-to-ring@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let agent_a = create_agent(&app, &token, &workspace, "Alpha").await;
    let agent_b = create_agent(&app, &token, &workspace, "Beta").await;
    let group = create_group_with_initial_agents(
        &app,
        &token,
        &workspace,
        "star",
        &[agent_a.as_str(), agent_b.as_str()],
    )
    .await;
    let group_id = group["id"].as_str().unwrap();

    let updated = patch_group_mode(&app, &token, group_id, "ring").await;
    assert_eq!(updated["communication_mode"], "ring");

    let rows = [
        group_agent_row(&state, group_id, &agent_a).await,
        group_agent_row(&state, group_id, &agent_b).await,
    ];
    assert!(rows.iter().all(|row| row.topology_role.is_none()));
    let orders = rows
        .iter()
        .map(|row| row.speaking_order.expect("speaking order"))
        .collect::<Vec<_>>();
    assert_eq!(orders, vec![1, 2]);
}

#[tokio::test]
async fn group_patch_star_to_mesh_clears_topology() {
    let (app, state) = app_with_state().await;
    let token = register_and_login(&app, "star-to-mesh@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let agent_a = create_agent(&app, &token, &workspace, "Alpha").await;
    let agent_b = create_agent(&app, &token, &workspace, "Beta").await;
    let group = create_group_with_initial_agents(
        &app,
        &token,
        &workspace,
        "star",
        &[agent_a.as_str(), agent_b.as_str()],
    )
    .await;
    let group_id = group["id"].as_str().unwrap();

    let updated = patch_group_mode(&app, &token, group_id, "mesh").await;
    assert_eq!(updated["communication_mode"], "mesh");

    let rows = [
        group_agent_row(&state, group_id, &agent_a).await,
        group_agent_row(&state, group_id, &agent_b).await,
    ];
    assert!(rows.iter().all(|row| row.topology_role.is_none()));
    assert!(rows.iter().all(|row| row.speaking_order.is_none()));
}

#[tokio::test]
async fn group_agents_add_and_list_return_group_agent_read() {
    let app = app().await;
    let token = register_and_login(&app, "group-agents-add-list@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let agent = create_agent(&app, &token, &workspace, "Alpha").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    let (status, added) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/agents"),
            &token,
            json!({"agent_id": agent}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(added["id"], format!("{group_id}:{agent}"));
    assert_eq!(added["group_id"], group_id);
    assert_eq!(added["agent_id"], agent);
    assert_eq!(added["display_name"], "Alpha");
    assert_eq!(added["role"], Value::Null);
    assert_eq!(added["topology_role"], Value::Null);
    assert_eq!(added["speaking_order"], Value::Null);
    assert_eq!(added["response_mode"], "mentioned_only");
    assert_eq!(added["share_group_workspace"], true);
    assert_eq!(added["context_usage"], Value::Null);
    assert_eq!(added["status"], "active");
    assert!(added["joined_at"].as_str().is_some());

    let (status, list) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}/agents"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let rows = list.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], added["id"]);
    assert_eq!(rows[0]["display_name"], "Alpha");
    assert_eq!(rows[0]["share_group_workspace"], true);
}

#[tokio::test]
async fn group_agents_reject_foreign_agent_add() {
    let app = app().await;
    let token_a = register_and_login(&app, "group-agents-owner@example.com").await;
    let workspace_a = create_workspace(&app, &token_a).await;
    let group = create_group_with_initial_agents(&app, &token_a, &workspace_a, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    let token_b = register_and_login(&app, "group-agents-foreign@example.com").await;
    let workspace_b = create_workspace(&app, &token_b).await;
    let foreign_agent = create_agent(&app, &token_b, &workspace_b, "Foreign").await;

    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/agents"),
            &token_a,
            json!({"agent_id": foreign_agent}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");
}

#[tokio::test]
async fn group_agents_duplicate_conflict_and_readd_removed_agent() {
    let app = app().await;
    let token = register_and_login(&app, "group-agents-readd@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let agent = create_agent(&app, &token, &workspace, "Alpha").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    let agents_url = format!("/api/v2/groups/{group_id}/agents");
    let agent_url = format!("{agents_url}/{agent}");

    let (status, _) = send(
        &app,
        authed_json(
            "POST",
            &agents_url,
            &token,
            json!({"agent_id": agent, "share_group_workspace": false}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = send(
        &app,
        authed_json("POST", &agents_url, &token, json!({"agent_id": agent})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "conflict");
    assert_eq!(body["error"]["message"], "agent already in group");

    let (status, body) = send(&app, authed("DELETE", &agent_url, &token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);

    let (status, readded) = send(
        &app,
        authed_json(
            "POST",
            &agents_url,
            &token,
            json!({"agent_id": agent, "share_group_workspace": true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(readded["status"], "active");
    assert_eq!(readded["share_group_workspace"], true);

    let (status, list) = send(&app, authed("GET", &agents_url, &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn group_agents_concurrent_duplicate_add_returns_created_and_conflict() {
    let app = app().await;
    let token = register_and_login(&app, "group-agents-concurrent@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let agent = create_agent(&app, &token, &workspace, "Alpha").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    let agents_url = format!("/api/v2/groups/{group_id}/agents");

    let req_a = authed_json(
        "POST",
        &agents_url,
        &token,
        json!({"agent_id": agent.clone()}),
    );
    let req_b = authed_json(
        "POST",
        &agents_url,
        &token,
        json!({"agent_id": agent.clone()}),
    );

    let (first, second) = tokio::join!(send(&app, req_a), send(&app, req_b));
    let responses = vec![first, second];

    assert!(
        responses
            .iter()
            .all(|(status, _)| *status != StatusCode::INTERNAL_SERVER_ERROR),
        "responses: {responses:?}"
    );
    assert_eq!(
        responses
            .iter()
            .filter(|(status, _)| *status == StatusCode::CREATED)
            .count(),
        1,
        "responses: {responses:?}"
    );
    assert_eq!(
        responses
            .iter()
            .filter(|(status, _)| *status == StatusCode::CONFLICT)
            .count(),
        1,
        "responses: {responses:?}"
    );

    for (status, body) in responses {
        match status {
            StatusCode::CREATED => {
                assert_eq!(body["group_id"], group_id);
                assert_eq!(body["agent_id"], agent);
                assert_eq!(body["status"], "active");
            }
            StatusCode::CONFLICT => {
                assert_eq!(body["error"]["code"], "conflict");
                assert_eq!(body["error"]["message"], "agent already in group");
            }
            other => panic!("unexpected status {other}: {body:?}"),
        }
    }
}

#[tokio::test]
async fn group_agents_delete_hides_agent_and_clears_muted_and_admin_ids() {
    let (app, state) = app_with_state().await;
    let token = register_and_login(&app, "group-agents-delete@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let agent = create_agent(&app, &token, &workspace, "Alpha").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    let (status, _) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/agents"),
            &token,
            json!({"agent_id": agent}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/agents/{agent}/mute"),
            &token,
            json!({"muted": true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    sqlx::query("UPDATE groups SET admin_agent_ids_json = ? WHERE id = ?")
        .bind(serde_json::to_string(&vec![agent.clone()]).unwrap())
        .bind(group_id)
        .execute(state.db.pool())
        .await
        .unwrap();

    let (status, _) = send(
        &app,
        authed(
            "DELETE",
            &format!("/api/v2/groups/{group_id}/agents/{agent}"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, list) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}/agents"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(list.as_array().unwrap().is_empty());

    let (status, group) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_json_array_missing(&group["muted_agent_ids"], &agent);
    assert_json_array_missing(&group["admin_agent_ids"], &agent);
}

#[tokio::test]
async fn group_agents_mute_updates_group_read_and_unmute_removes_it() {
    let app = app().await;
    let token = register_and_login(&app, "group-agents-mute@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let agent = create_agent(&app, &token, &workspace, "Alpha").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    let (status, _) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/agents"),
            &token,
            json!({"agent_id": agent}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, muted) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/agents/{agent}/mute"),
            &token,
            json!({"muted": true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(muted["agent_id"], agent);

    let (status, group) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_json_array_contains(&group["muted_agent_ids"], &agent);

    let (status, _) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/agents/{agent}/mute"),
            &token,
            json!({"muted": false}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, group) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_json_array_missing(&group["muted_agent_ids"], &agent);
}

#[tokio::test]
async fn group_agents_workspace_sharing_toggles_response() {
    let app = app().await;
    let token = register_and_login(&app, "group-agents-workspace@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let agent = create_agent(&app, &token, &workspace, "Alpha").await;
    let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();

    let (status, added) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/agents"),
            &token,
            json!({"agent_id": agent, "share_group_workspace": false}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(added["share_group_workspace"], false);

    let (status, shared) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/agents/{agent}/workspace-sharing"),
            &token,
            json!({"share_group_workspace": true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(shared["share_group_workspace"], true);

    let (status, unshared) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/agents/{agent}/workspace-sharing"),
            &token,
            json!({"share_group_workspace": false}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unshared["share_group_workspace"], false);
}

#[tokio::test]
async fn group_topology_agent_patch_validates_star_hierarchical_and_ring() {
    let app = app().await;
    let token = register_and_login(&app, "group-topology-agent@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let agent_a = create_agent(&app, &token, &workspace, "Alpha").await;
    let agent_b = create_agent(&app, &token, &workspace, "Beta").await;

    let star_group = create_group_with_initial_agents(&app, &token, &workspace, "star", &[]).await;
    let star_group_id = star_group["id"].as_str().unwrap();
    let star_agents_url = format!("/api/v2/groups/{star_group_id}/agents");
    let (status, star_a) = send(
        &app,
        authed_json(
            "POST",
            &star_agents_url,
            &token,
            json!({"agent_id": agent_a}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(star_a["topology_role"], "hub");
    let (status, star_b) = send(
        &app,
        authed_json(
            "POST",
            &star_agents_url,
            &token,
            json!({"agent_id": agent_b}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(star_b["topology_role"], Value::Null);

    let (status, promoted) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{star_group_id}/agents/{agent_b}/topology"),
            &token,
            json!({"topology_role": "hub", "speaking_order": Value::Null}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(promoted["topology_role"], "hub");
    let (status, star_list) = send(&app, authed("GET", &star_agents_url, &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        star_list
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["topology_role"] == "hub")
            .count(),
        1
    );
    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{star_group_id}/agents/{agent_b}/topology"),
            &token,
            json!({"speaking_order": 1}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");

    let hierarchical_group =
        create_group_with_initial_agents(&app, &token, &workspace, "hierarchical", &[]).await;
    let hierarchical_group_id = hierarchical_group["id"].as_str().unwrap();
    let (status, worker) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{hierarchical_group_id}/agents"),
            &token,
            json!({"agent_id": agent_a}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(worker["topology_role"], "worker");
    let (status, leader) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{hierarchical_group_id}/agents/{agent_a}/topology"),
            &token,
            json!({"topology_role": "leader"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(leader["topology_role"], "leader");
    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{hierarchical_group_id}/agents/{agent_a}/topology"),
            &token,
            json!({"topology_role": "hub"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");

    let ring_group = create_group_with_initial_agents(&app, &token, &workspace, "ring", &[]).await;
    let ring_group_id = ring_group["id"].as_str().unwrap();
    let ring_agents_url = format!("/api/v2/groups/{ring_group_id}/agents");
    let (status, ring_a) = send(
        &app,
        authed_json(
            "POST",
            &ring_agents_url,
            &token,
            json!({"agent_id": agent_a}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(ring_a["speaking_order"], 1);
    let (status, ring_b) = send(
        &app,
        authed_json(
            "POST",
            &ring_agents_url,
            &token,
            json!({"agent_id": agent_b}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(ring_b["speaking_order"], 2);
    let (status, reordered) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{ring_group_id}/agents/{agent_b}/topology"),
            &token,
            json!({"speaking_order": 5}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reordered["topology_role"], Value::Null);
    assert_eq!(reordered["speaking_order"], 5);
    for invalid in [
        json!({"topology_role": "leader"}),
        json!({"speaking_order": 0}),
    ] {
        let (status, body) = send(
            &app,
            authed_json(
                "PATCH",
                &format!("/api/v2/groups/{ring_group_id}/agents/{agent_b}/topology"),
                &token,
                invalid,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_input");
    }
}

#[tokio::test]
async fn group_agents_cross_owner_group_mutation_is_rejected() {
    let app = app().await;
    let token_a = register_and_login(&app, "group-agents-cross-a@example.com").await;
    let workspace_a = create_workspace(&app, &token_a).await;
    let agent = create_agent(&app, &token_a, &workspace_a, "Alpha").await;
    let group = create_group_with_initial_agents(&app, &token_a, &workspace_a, "mesh", &[]).await;
    let group_id = group["id"].as_str().unwrap();
    let (status, _) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/agents"),
            &token_a,
            json!({"agent_id": agent}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let token_b = register_and_login(&app, "group-agents-cross-b@example.com").await;
    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}/agents/{agent}/mute"),
            &token_b,
            json!({"muted": true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");
}

struct GroupAgentRow {
    topology_role: Option<String>,
    speaking_order: Option<i64>,
    response_mode: String,
    context_scope_json: Option<String>,
}

async fn group_agent_row(state: &AppState, group_id: &str, agent_id: &str) -> GroupAgentRow {
    let row = sqlx::query_as::<_, (Option<String>, Option<i64>, String, Option<String>)>(
        "SELECT topology_role, speaking_order, response_mode, context_scope_json \
         FROM group_agents WHERE group_id = ? AND agent_id = ? AND status = 'active'",
    )
    .bind(group_id)
    .bind(agent_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    GroupAgentRow {
        topology_role: row.0,
        speaking_order: row.1,
        response_mode: row.2,
        context_scope_json: row.3,
    }
}

fn assert_shared_group_workspace(raw: &Option<String>) {
    let value: Value = serde_json::from_str(raw.as_deref().unwrap()).unwrap();
    assert_eq!(value["share_group_workspace"], true);
}

fn assert_json_array_contains(value: &Value, expected: &str) {
    assert!(value
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str() == Some(expected)));
}

fn assert_json_array_missing(value: &Value, expected: &str) {
    if value.is_null() {
        return;
    }
    assert!(!value
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str() == Some(expected)));
}

#[tokio::test]
async fn group_list_is_owner_scoped() {
    let app = app().await;
    let token_a = register_and_login(&app, "lista@example.com").await;
    let workspace_a = create_workspace(&app, &token_a).await;

    let (status, group) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/groups",
            &token_a,
            json!({"name": "A's Group", "workspace_id": workspace_a}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let group_id = group["id"].as_str().unwrap().to_string();

    let token_b = register_and_login(&app, "listb@example.com").await;
    let (status, list_b) = send(&app, authed("GET", "/api/v2/groups", &token_b)).await;
    assert_eq!(status, StatusCode::OK);
    let b_ids: Vec<&str> = list_b
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["id"].as_str().unwrap())
        .collect();
    assert!(!b_ids.contains(&group_id.as_str()));
}

#[tokio::test]
async fn direct_container_is_isolated_from_group_endpoints() {
    let (app, state) = app_with_state().await;
    let email = "direct-container-owner@example.com";
    let token = register_and_login(&app, email).await;
    let owner_id = owner_id(&state, email).await;
    let direct_id = Uuid::new_v4().to_string();
    let now = "2026-07-19T00:00:00Z";

    sqlx::query(
        "INSERT INTO groups (id, owner_id, name, conversation_kind, title_source, status, created_at, updated_at) \
         VALUES (?, ?, 'Direct container', 'direct', 'automatic', 'active', ?, ?)",
    )
    .bind(&direct_id)
    .bind(&owner_id)
    .bind(now)
    .bind(now)
    .execute(state.db.pool())
    .await
    .unwrap();

    let (status, group_list) = send(&app, authed("GET", "/api/v2/groups", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(group_list
        .as_array()
        .unwrap()
        .iter()
        .all(|group| group["id"] != direct_id));

    for request in [
        authed("GET", &format!("/api/v2/groups/{direct_id}"), &token),
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{direct_id}"),
            &token,
            json!({"name": "Must not update"}),
        ),
        authed("DELETE", &format!("/api/v2/groups/{direct_id}"), &token),
    ] {
        let (status, body) = send(&app, request).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "body: {body:?}");
        assert_eq!(body["error"]["code"], "not_found");
    }
}

#[tokio::test]
async fn group_scheduler_config_round_trips_and_defaults_legacy_off() {
    let app = app().await;
    let token = register_and_login(&app, "scheduler-config@example.com").await;
    let workspace = create_workspace(&app, &token).await;
    let provider = create_llm_provider(&app, &token, "http://127.0.0.1:1").await;

    let (status, group) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/groups",
            &token,
            json!({"name": "Legacy defaults", "workspace_id": workspace}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(group["scheduler_enabled"], false);
    assert_eq!(group["agent_mention_policy"], "display_only");
    assert_eq!(group["max_agent_steps"], Value::Null);
    assert_eq!(group["max_steps_per_agent"], 3);
    assert_eq!(group["max_scheduler_hops"], 5);
    assert_eq!(group["max_moderator_calls"], 4);
    assert_eq!(group["max_consecutive_failures"], 3);
    assert_eq!(group["max_total_failures"], 6);
    assert_eq!(group["max_total_tokens"], 120_000);
    assert_eq!(group["turn_timeout_seconds"], 300);
    assert_eq!(group["moderator_enabled"], false);
    assert_eq!(group["moderator_provider_id"], Value::Null);
    assert_eq!(group["moderator_model"], Value::Null);
    assert_eq!(group["free_speech"], false);
    assert_eq!(group["proactive_mode"], false);
    assert_eq!(group["allow_agent_free_mention"], true);

    let group_id = group["id"].as_str().unwrap();
    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}"),
            &token,
            json!({
                "scheduler_enabled": true,
                "agent_mention_policy": "bounded_schedule",
                "max_agent_steps": 18,
                "max_steps_per_agent": 4,
                "max_scheduler_hops": 7,
                "max_moderator_calls": 2,
                "max_consecutive_failures": 4,
                "max_total_failures": 8,
                "max_total_tokens": 240_000,
                "turn_timeout_seconds": 600,
                "moderator_enabled": true,
                "moderator_provider_id": provider,
                "moderator_model": "test-model"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {updated:?}");
    assert_eq!(updated["scheduler_enabled"], true);
    assert_eq!(updated["agent_mention_policy"], "bounded_schedule");
    assert_eq!(updated["max_agent_steps"], 18);
    assert_eq!(updated["max_steps_per_agent"], 4);
    assert_eq!(updated["max_scheduler_hops"], 7);
    assert_eq!(updated["max_moderator_calls"], 2);
    assert_eq!(updated["max_consecutive_failures"], 4);
    assert_eq!(updated["max_total_failures"], 8);
    assert_eq!(updated["max_total_tokens"], 240_000);
    assert_eq!(updated["turn_timeout_seconds"], 600);
    assert_eq!(updated["moderator_enabled"], true);
    assert_eq!(updated["moderator_provider_id"], provider);
    assert_eq!(updated["moderator_model"], "test-model");

    let (status, fetched) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["max_agent_steps"], 18);
    assert_eq!(fetched["max_total_tokens"], 240_000);
    assert_eq!(fetched["moderator_provider_id"], provider);

    let (status, reset) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}"),
            &token,
            json!({
                "scheduler_enabled": false,
                "agent_mention_policy": "display_only",
                "max_agent_steps": Value::Null,
                "moderator_enabled": false,
                "moderator_provider_id": Value::Null,
                "moderator_model": Value::Null
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {reset:?}");
    assert_eq!(reset["max_agent_steps"], Value::Null);
    assert_eq!(reset["moderator_provider_id"], Value::Null);
    assert_eq!(reset["moderator_model"], Value::Null);
}

#[tokio::test]
async fn group_scheduler_rejects_invalid_budget_and_moderator_provider() {
    let (app, state) = app_with_state().await;
    let owner_email = "scheduler-validation@example.com";
    let token = register_and_login(&app, owner_email).await;
    let workspace = create_workspace(&app, &token).await;
    let provider = create_llm_provider(&app, &token, "http://127.0.0.1:1").await;

    let (status, group) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/groups",
            &token,
            json!({"name": "Validated scheduler", "workspace_id": workspace}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let group_id = group["id"].as_str().unwrap();

    for invalid in [
        json!({"max_agent_steps": 0}),
        json!({"max_steps_per_agent": 0}),
        json!({"max_scheduler_hops": -1}),
        json!({"max_moderator_calls": -1}),
        json!({"max_consecutive_failures": 0}),
        json!({"max_total_failures": 0}),
        json!({"max_total_tokens": 0}),
        json!({"turn_timeout_seconds": 0}),
        json!({"turn_timeout_seconds": 3601}),
        json!({"agent_mention_policy": "unbounded"}),
    ] {
        let (status, body) = send(
            &app,
            authed_json(
                "PATCH",
                &format!("/api/v2/groups/{group_id}"),
                &token,
                invalid,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body:?}");
        assert_eq!(body["error"]["code"], "invalid_input");
    }

    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}"),
            &token,
            json!({"moderator_enabled": true, "moderator_model": "test-model"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body:?}");
    assert_eq!(body["error"]["code"], "invalid_input");

    let other_token = register_and_login(&app, "scheduler-other@example.com").await;
    let other_provider = create_llm_provider(&app, &other_token, "http://127.0.0.1:1").await;
    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}"),
            &token,
            json!({
                "moderator_enabled": true,
                "moderator_provider_id": other_provider,
                "moderator_model": "test-model"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body:?}");
    assert_eq!(body["error"]["code"], "permission_denied");

    sqlx::query("UPDATE llm_providers SET status = 'deleted' WHERE id = ?")
        .bind(&provider)
        .execute(state.db.pool())
        .await
        .unwrap();
    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}"),
            &token,
            json!({
                "moderator_enabled": true,
                "moderator_provider_id": provider,
                "moderator_model": "test-model"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body:?}");
    assert_eq!(body["error"]["code"], "invalid_input");
}

#[tokio::test]
async fn group_patch_updates_name_description_workspace_and_settings() {
    let app = app().await;
    let token = register_and_login(&app, "patch@example.com").await;
    let workspace_a = create_workspace(&app, &token).await;
    let workspace_b = create_workspace(&app, &token).await;

    let (status, group) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/groups",
            &token,
            json!({"name": "Before", "workspace_id": workspace_a}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let group_id = group["id"].as_str().unwrap().to_string();

    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}"),
            &token,
            json!({
                "name": "After",
                "description": "the team chat",
                "workspace_id": workspace_b,
                "free_speech": true,
                "proactive_mode": true,
                "proactive_reply_multiplier": 3,
                "allow_agent_free_mention": false,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["name"], "After");
    assert_eq!(updated["description"], "the team chat");
    assert_eq!(updated["workspace_id"], workspace_b);
    assert_eq!(updated["free_speech"], true);
    assert_eq!(updated["proactive_mode"], true);
    assert_eq!(updated["proactive_reply_multiplier"], 3);
    assert_eq!(updated["allow_agent_free_mention"], false);

    // Values round-trip through a fresh GET.
    let (status, fetched) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["name"], "After");
    assert_eq!(fetched["description"], "the team chat");
    assert_eq!(fetched["workspace_id"], workspace_b);
    assert_eq!(fetched["free_speech"], true);
    assert_eq!(fetched["proactive_mode"], true);
    assert_eq!(fetched["proactive_reply_multiplier"], 3);
    assert_eq!(fetched["allow_agent_free_mention"], false);

    // Explicit null clears the workspace binding.
    let (status, cleared) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}"),
            &token,
            json!({"workspace_id": Value::Null}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cleared["workspace_id"], Value::Null);
}

#[tokio::test]
async fn group_rejects_invalid_reply_multiplier() {
    let app = app().await;
    let token = register_and_login(&app, "multiplier@example.com").await;
    let workspace = create_workspace(&app, &token).await;

    // Create with multiplier 0 is rejected.
    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/groups",
            &token,
            json!({"name": "Bad", "workspace_id": workspace, "proactive_reply_multiplier": 0}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");

    // A valid group can still be created, then PATCH with 0 is rejected.
    let (status, group) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/groups",
            &token,
            json!({"name": "Good", "workspace_id": workspace}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let group_id = group["id"].as_str().unwrap().to_string();

    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{group_id}"),
            &token,
            json!({"proactive_reply_multiplier": 0}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");
}

#[tokio::test]
async fn group_delete_soft_deletes_and_hides_from_list() {
    let app = app().await;
    let token = register_and_login(&app, "delete@example.com").await;
    let workspace = create_workspace(&app, &token).await;

    let (status, group) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/groups",
            &token,
            json!({"name": "Doomed", "workspace_id": workspace}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let group_id = group["id"].as_str().unwrap().to_string();

    let (status, body) = send(
        &app,
        authed("DELETE", &format!("/api/v2/groups/{group_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);

    // Get now returns 404.
    let (status, body) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");

    // List omits it.
    let (status, list) = send(&app, authed("GET", "/api/v2/groups", &token)).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["id"].as_str().unwrap())
        .collect();
    assert!(!ids.contains(&group_id.as_str()));
}
