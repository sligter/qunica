use axum::{
    body::Body,
    http::{HeaderMap, Request, StatusCode},
    Router,
};
use qunica_backend::api::{router_with_state_for_tests, AppState};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tower::ServiceExt;

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

fn request(method: &str, uri: &str, token: Option<&str>, body: Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

fn authed(method: &str, uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

async fn stream_events(app: &Router, uri: &str, token: &str, body: Value) -> Vec<Value> {
    let response = app
        .clone()
        .oneshot(request("POST", uri, Some(token), body))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let text = String::from_utf8(
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    text.split("\n\n")
        .filter_map(|frame| frame.lines().find_map(|line| line.strip_prefix("data: ")))
        .map(|raw| serde_json::from_str(raw).unwrap())
        .collect()
}

async fn register(app: &Router, email: &str) -> String {
    let (status, _) = send(
        app,
        request(
            "POST",
            "/api/v2/auth/register",
            None,
            json!({"email": email, "password": "supersecret", "name": "Tester"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = send(
        app,
        request(
            "POST",
            "/api/v2/auth/login",
            None,
            json!({"email": email, "password": "supersecret"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    body["access_token"].as_str().unwrap().to_string()
}

async fn create_workspace(app: &Router, token: &str) -> String {
    let (status, body) = send(
        app,
        request(
            "POST",
            "/api/v2/workspaces",
            Some(token),
            json!({"name":"Workspace", "backend_type":"cloud_sandbox"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    body["id"].as_str().unwrap().to_string()
}

async fn create_local_workspace(
    app: &Router,
    token: &str,
    name: &str,
) -> (tempfile::TempDir, String) {
    let root = tempfile::tempdir().unwrap();
    let (status, body) = send(
        app,
        request(
            "POST",
            "/api/v2/workspaces",
            Some(token),
            json!({
                "name": name,
                "backend_type": "local",
                "local_path": root.path().to_string_lossy()
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {body:?}");
    (root, body["id"].as_str().unwrap().to_string())
}

async fn create_agent(app: &Router, token: &str, workspace_id: &str, name: &str) -> String {
    let (status, body) = send(
        app,
        request(
            "POST",
            "/api/v2/agents",
            Some(token),
            json!({"name":name, "workspace_id":workspace_id}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    body["id"].as_str().unwrap().to_string()
}

async fn create_chat(app: &Router, token: &str, agent_id: &str) -> Value {
    let (status, body) = send(
        app,
        request(
            "POST",
            "/api/v2/direct-chats",
            Some(token),
            json!({"agent_id": agent_id}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {body:?}");
    body
}

fn direct_workspace_file_url(chat_id: &str, suffix: &str, path: &str) -> String {
    format!("/api/v2/direct-chats/{chat_id}/workspace-files{suffix}?path={path}")
}

fn direct_workspace_file_route_requests(chat_id: &str, token: &str) -> Vec<Request<Body>> {
    vec![
        authed(
            "GET",
            &format!("/api/v2/direct-chats/{chat_id}/workspace-files/root"),
            token,
        ),
        authed(
            "GET",
            &direct_workspace_file_url(chat_id, "", "fixture.txt"),
            token,
        ),
        authed(
            "GET",
            &direct_workspace_file_url(chat_id, "/preview", "fixture.txt"),
            token,
        ),
        authed(
            "GET",
            &direct_workspace_file_url(chat_id, "/download", "fixture.txt"),
            token,
        ),
        authed(
            "GET",
            &direct_workspace_file_url(chat_id, "/text", "fixture.txt"),
            token,
        ),
        request(
            "PATCH",
            &direct_workspace_file_url(chat_id, "/text/save", "fixture.txt"),
            Some(token),
            json!({"content": "blocked", "version": "deadbeef"}),
        ),
    ]
}

async fn assert_direct_workspace_file_route_errors(
    app: &Router,
    token: &str,
    chat_id: &str,
    expected_status: StatusCode,
    expected_code: &str,
) {
    for request in direct_workspace_file_route_requests(chat_id, token) {
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

#[tokio::test]
async fn direct_workspace_files_list_stream_and_text_save_round_trip() {
    let (app, _state) = router_with_state_for_tests().await;
    let token = register(&app, "direct-workspace-files@example.com").await;
    let (_root, workspace_id) = create_local_workspace(&app, &token, "Direct Workspace").await;
    let agent_id = create_agent(&app, &token, &workspace_id, "Local Agent").await;
    let chat = create_chat(&app, &token, &agent_id).await;
    let chat_id = chat["id"].as_str().unwrap();
    let chat_root = direct_workspace_root(&app, &token, chat_id).await;
    std::fs::create_dir(chat_root.join("docs")).unwrap();
    std::fs::write(chat_root.join(".hidden"), b"hidden").unwrap();
    std::fs::write(chat_root.join("photo.png"), b"\x89PNG\r\n").unwrap();
    std::fs::write(chat_root.join("manual.pdf"), b"%PDF-1.4\n").unwrap();
    std::fs::write(chat_root.join("page.html"), b"<html>preview</html>").unwrap();
    let note_path = chat_root.join("note.txt");
    let original = "direct UTF-8 文本 👋\n";
    std::fs::write(&note_path, original).unwrap();

    let (status, workspace_root) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/direct-chats/{chat_id}/workspace-files/root"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        workspace_root["root"],
        chat_root.to_string_lossy().to_string()
    );

    let (status, list) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/direct-chats/{chat_id}/workspace-files"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {list:?}");
    let rows = list.as_array().unwrap();
    for expected in ["docs", "manual.pdf", "note.txt", "page.html", "photo.png"] {
        assert!(
            rows.iter().any(|row| row["name"] == expected),
            "missing {expected}: {rows:?}"
        );
    }
    assert!(!rows.iter().any(|row| row["name"] == ".hidden"));
    assert_eq!(
        rows.iter().find(|row| row["name"] == "docs").unwrap()["is_dir"],
        true
    );

    let (status, hidden_list) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/direct-chats/{chat_id}/workspace-files?show_hidden=true"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {hidden_list:?}");
    assert!(hidden_list
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["name"] == ".hidden"));

    let streamed: [(&str, &str, &[u8]); 3] = [
        ("photo.png", "image/png", b"\x89PNG\r\n"),
        ("manual.pdf", "application/pdf", b"%PDF-1.4\n"),
        ("page.html", "text/html", b"<html>preview</html>"),
    ];
    for (path, mime_type, expected_bytes) in streamed {
        let (status, headers, bytes) = send_bytes(
            &app,
            authed(
                "GET",
                &direct_workspace_file_url(chat_id, "/download", path),
                &token,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "path {path}");
        assert_eq!(bytes, expected_bytes);
        assert_eq!(
            headers.get("content-type").unwrap().to_str().unwrap(),
            mime_type
        );
        assert!(headers
            .get("content-disposition")
            .unwrap()
            .to_str()
            .unwrap()
            .contains(&format!("filename=\"{path}\"")));
    }

    let (status, read) = send(
        &app,
        authed(
            "GET",
            &direct_workspace_file_url(chat_id, "/text", "note.txt"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {read:?}");
    assert_eq!(read["content"], original);
    assert_eq!(read["is_text"], true);
    assert_eq!(read["truncated"], false);
    let first_version = read["version"].as_str().unwrap().to_owned();

    let updated = "direct save 已完成 ✅\n";
    let (status, saved) = send(
        &app,
        request(
            "PATCH",
            &direct_workspace_file_url(chat_id, "/text/save", "note.txt"),
            Some(&token),
            json!({"content": updated, "version": first_version.clone()}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {saved:?}");
    assert_eq!(saved["content"], updated);
    assert_ne!(saved["version"], first_version);
    assert_eq!(std::fs::read_to_string(&note_path).unwrap(), updated);
}

#[tokio::test]
async fn direct_attachment_message_persists_local_workspace_metadata() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "direct-attachment-message@example.com").await;
    let (_root, workspace_id) = create_local_workspace(&app, &token, "Direct Attachments").await;
    let agent_id = create_agent(&app, &token, &workspace_id, "Attachment Agent").await;
    let chat = create_chat(&app, &token, &agent_id).await;
    let chat_id = chat["id"].as_str().unwrap();
    let attachment_path = direct_workspace_root(&app, &token, chat_id)
        .await
        .join("diagram.png");
    std::fs::write(&attachment_path, b"PNG!").unwrap();

    let (status, body) = send(
        &app,
        request(
            "POST",
            &format!("/api/v2/direct-chats/{chat_id}/messages"),
            Some(&token),
            json!({"content": "", "attachments": [{"path": "diagram.png"}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {body:?}");
    let attachments = &body["user_message"]["attachments"];
    assert_eq!(attachments[0]["path"], "diagram.png");
    assert_eq!(attachments[0]["name"], "diagram.png");
    assert_eq!(attachments[0]["mime_type"], "image/png");
    assert_eq!(attachments[0]["size"], 4);
    assert_eq!(attachments[0]["kind"], "image");

    let user_message_id = body["user_message"]["id"].as_str().unwrap();
    let event_payload: String = sqlx::query_scalar(
        "SELECT se.payload_json FROM stream_events se \
         JOIN messages m ON m.thread_id = se.thread_id \
         WHERE se.kind = 'user_message' \
           AND m.group_id = ? \
           AND m.id = ? \
           AND json_extract(se.payload_json, '$.message_id') = ?",
    )
    .bind(chat_id)
    .bind(user_message_id)
    .bind(user_message_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    let event_payload: Value = serde_json::from_str(&event_payload).unwrap();
    assert_eq!(event_payload["attachments"], *attachments);

    std::fs::remove_file(&attachment_path).unwrap();
    let (status, messages) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/direct-chats/{chat_id}/messages"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(messages[0]["attachments"], *attachments);
}

#[tokio::test]
async fn direct_workspace_files_reject_cloud_and_never_fall_back_to_the_agent_workspace() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "direct-workspace-errors@example.com").await;

    let cloud_workspace_id = create_workspace(&app, &token).await;
    let cloud_agent_id = create_agent(&app, &token, &cloud_workspace_id, "Cloud Agent").await;
    let cloud_chat = create_chat(&app, &token, &cloud_agent_id).await;
    let cloud_chat_id = cloud_chat["id"].as_str().unwrap();
    assert_direct_workspace_file_route_errors(
        &app,
        &token,
        cloud_chat_id,
        StatusCode::BAD_REQUEST,
        "invalid_input",
    )
    .await;

    let (_root, local_workspace_id) =
        create_local_workspace(&app, &token, "Unbound Direct Workspace").await;
    let local_agent_id = create_agent(&app, &token, &local_workspace_id, "Unbound Agent").await;
    let local_chat = create_chat(&app, &token, &local_agent_id).await;
    let local_chat_id = local_chat["id"].as_str().unwrap();
    sqlx::query("UPDATE groups SET workspace_id = NULL WHERE id = ?")
        .bind(local_chat_id)
        .execute(state.db.pool())
        .await
        .unwrap();

    let (status, chat) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/direct-chats/{local_chat_id}"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {chat:?}");
    assert_eq!(chat["workspace_id"], Value::Null);

    let (status, files) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/direct-chats/{local_chat_id}/workspace-files"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {files:?}");
    assert_eq!(files["error"]["code"], "invalid_input");

    let (status, roots) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/direct-chats/{local_chat_id}/workspace-roots"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {roots:?}");
    assert!(roots.as_array().unwrap().is_empty());
}

async fn direct_workspace_root(app: &Router, token: &str, chat_id: &str) -> PathBuf {
    let (status, body) = send(
        app,
        authed(
            "GET",
            &format!("/api/v2/direct-chats/{chat_id}/workspace-files/root"),
            token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body:?}");
    PathBuf::from(body["root"].as_str().unwrap())
}

#[tokio::test]
async fn direct_workspace_files_and_attachments_reject_unsafe_paths_and_symlink_escapes() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "direct-workspace-path-safety@example.com").await;
    let (_root, workspace_id) = create_local_workspace(&app, &token, "Direct Path Safety").await;
    let agent_id = create_agent(&app, &token, &workspace_id, "Path Agent").await;
    let chat = create_chat(&app, &token, &agent_id).await;
    let chat_id = chat["id"].as_str().unwrap();
    let chat_root = direct_workspace_root(&app, &token, chat_id).await;

    for path in [
        "../secret.txt",
        "/absolute.txt",
        "C:%5Csecret.txt",
        "%5C%5Cserver%5Cshare.txt",
    ] {
        for request in [
            authed("GET", &direct_workspace_file_url(chat_id, "", path), &token),
            authed(
                "GET",
                &direct_workspace_file_url(chat_id, "/preview", path),
                &token,
            ),
            authed(
                "GET",
                &direct_workspace_file_url(chat_id, "/download", path),
                &token,
            ),
            authed(
                "GET",
                &direct_workspace_file_url(chat_id, "/text", path),
                &token,
            ),
            request(
                "PATCH",
                &direct_workspace_file_url(chat_id, "/text/save", path),
                Some(&token),
                json!({"content": "blocked", "version": "0".repeat(64)}),
            ),
        ] {
            let (status, body) = send(&app, request).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "path {path:?}: {body:?}");
            assert_eq!(body["error"]["code"], "invalid_input");
        }
    }

    for path in [
        "../secret.txt",
        "/absolute.txt",
        r"C:\secret.txt",
        r"\\server\share.txt",
    ] {
        let (status, body) = send(
            &app,
            request(
                "POST",
                &format!("/api/v2/direct-chats/{chat_id}/messages"),
                Some(&token),
                json!({"content": "", "attachments": [{"path": path}]}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "path {path:?}: {body:?}");
        assert_eq!(body["error"]["code"], "invalid_input");
    }

    let outside = tempfile::tempdir().unwrap();
    let outside_file = outside.path().join("secret.txt");
    std::fs::write(&outside_file, "secret").unwrap();
    if create_dir_symlink(outside.path(), &chat_root.join("link")).is_ok() {
        for request in [
            authed(
                "GET",
                &direct_workspace_file_url(chat_id, "", "link"),
                &token,
            ),
            authed(
                "GET",
                &direct_workspace_file_url(chat_id, "/preview", "link/secret.txt"),
                &token,
            ),
            authed(
                "GET",
                &direct_workspace_file_url(chat_id, "/download", "link/secret.txt"),
                &token,
            ),
            authed(
                "GET",
                &direct_workspace_file_url(chat_id, "/text", "link/secret.txt"),
                &token,
            ),
            request(
                "PATCH",
                &direct_workspace_file_url(chat_id, "/text/save", "link/secret.txt"),
                Some(&token),
                json!({"content": "blocked", "version": "0".repeat(64)}),
            ),
        ] {
            let route = request.uri().to_string();
            let (status, body) = send(&app, request).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{route}: {body:?}");
            assert_eq!(body["error"]["code"], "invalid_input");
        }
        let (status, body) = send(
            &app,
            request(
                "POST",
                &format!("/api/v2/direct-chats/{chat_id}/messages"),
                Some(&token),
                json!({"content": "", "attachments": [{"path": "link/secret.txt"}]}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body:?}");
        assert_eq!(body["error"]["code"], "invalid_input");
        assert_eq!(std::fs::read_to_string(&outside_file).unwrap(), "secret");
    }

    let message_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE group_id = ?")
        .bind(chat_id)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(message_count, 0);
}

#[tokio::test]
async fn direct_chat_lifecycle_creates_independent_sessions_and_graph() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "direct-lifecycle@example.com").await;
    let workspace_id = create_workspace(&app, &token).await;
    let agent_id = create_agent(&app, &token, &workspace_id, "Atlas").await;
    let first = create_chat(&app, &token, &agent_id).await;
    let second = create_chat(&app, &token, &agent_id).await;
    assert_ne!(first["id"], second["id"]);
    assert_eq!(first["title"], "New chat with Atlas");
    assert_eq!(first["title_source"], "automatic");
    assert_ne!(first["workspace_id"], workspace_id);
    assert_ne!(first["workspace_id"], second["workspace_id"]);

    let first_id = first["id"].as_str().unwrap();
    let graph: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM group_members WHERE group_id = ? AND status = 'active'), \
                (SELECT COUNT(*) FROM group_agents WHERE group_id = ? AND agent_id = ? AND status = 'active'), \
                (SELECT COUNT(*) FROM threads WHERE group_id = ? AND status = 'active')",
    ).bind(first_id).bind(first_id).bind(&agent_id).bind(first_id).fetch_one(state.db.pool()).await.unwrap();
    assert_eq!(graph, (1, 1, 1));

    let (status, list) = send(&app, authed("GET", "/api/v2/direct-chats", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 2);

    let (status, renamed) = send(
        &app,
        request(
            "PATCH",
            &format!("/api/v2/direct-chats/{first_id}"),
            Some(&token),
            json!({"title":"  Manual title  "}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["title"], "Manual title");
    assert_eq!(renamed["title_source"], "manual");

    let (status, _) = send(
        &app,
        authed(
            "DELETE",
            &format!("/api/v2/direct-chats/{first_id}"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = send(
        &app,
        authed("GET", &format!("/api/v2/direct-chats/{first_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
}

#[tokio::test]
async fn direct_chat_lifecycle_enforces_agent_state_ownership_and_kind() {
    let (app, state) = router_with_state_for_tests().await;
    let token_a = register(&app, "direct-owner@example.com").await;
    let workspace_a = create_workspace(&app, &token_a).await;
    let agent_a = create_agent(&app, &token_a, &workspace_a, "Owned").await;
    let chat = create_chat(&app, &token_a, &agent_a).await;
    let chat_id = chat["id"].as_str().unwrap();
    let token_b = register(&app, "direct-other@example.com").await;

    assert_direct_workspace_file_route_errors(
        &app,
        &token_b,
        chat_id,
        StatusCode::FORBIDDEN,
        "permission_denied",
    )
    .await;

    for call in [
        authed("GET", &format!("/api/v2/direct-chats/{chat_id}"), &token_b),
        request(
            "PATCH",
            &format!("/api/v2/direct-chats/{chat_id}"),
            Some(&token_b),
            json!({"title":"No"}),
        ),
        authed(
            "DELETE",
            &format!("/api/v2/direct-chats/{chat_id}"),
            &token_b,
        ),
    ] {
        let (status, _) = send(&app, call).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    let other_workspace = create_workspace(&app, &token_b).await;
    let other_agent = create_agent(&app, &token_b, &other_workspace, "Foreign").await;
    let (status, body) = send(
        &app,
        request(
            "POST",
            "/api/v2/direct-chats",
            Some(&token_a),
            json!({"agent_id":other_agent}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");

    sqlx::query("UPDATE agents SET status = 'deleted' WHERE id = ?")
        .bind(&agent_a)
        .execute(state.db.pool())
        .await
        .unwrap();
    let (status, existing) = send(
        &app,
        authed("GET", &format!("/api/v2/direct-chats/{chat_id}"), &token_a),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(existing["agent_status"], "deleted");
    let (status, _) = send(
        &app,
        request(
            "POST",
            "/api/v2/direct-chats",
            Some(&token_a),
            json!({"agent_id":agent_a}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, group) = send(
        &app,
        request(
            "POST",
            "/api/v2/groups",
            Some(&token_a),
            json!({"name":"Normal group", "workspace_id":workspace_a}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let group_id = group["id"].as_str().unwrap();
    let (status, _) = send(
        &app,
        authed("GET", &format!("/api/v2/direct-chats/{group_id}"), &token_a),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    assert_direct_workspace_file_route_errors(
        &app,
        &token_a,
        group_id,
        StatusCode::NOT_FOUND,
        "not_found",
    )
    .await;
}

#[tokio::test]
async fn direct_chat_titles_validate_and_follow_account_language() {
    let (app, _state): (Router, AppState) = router_with_state_for_tests().await;
    let token = register(&app, "direct-language@example.com").await;
    let workspace_id = create_workspace(&app, &token).await;
    let agent_id = create_agent(&app, &token, &workspace_id, "中文 Agent").await;
    let (status, _) = send(
        &app,
        request(
            "PATCH",
            "/api/v2/settings/system",
            Some(&token),
            json!({"language":"zh-CN"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let chat = create_chat(&app, &token, &agent_id).await;
    assert_eq!(chat["title"], "与 中文 Agent 的新对话");
    let chat_id = chat["id"].as_str().unwrap();
    for title in ["", " ", &"x".repeat(121)] {
        let (status, body) = send(
            &app,
            request(
                "PATCH",
                &format!("/api/v2/direct-chats/{chat_id}"),
                Some(&token),
                json!({"title": title}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body:?}");
        assert_eq!(body["error"]["code"], "invalid_input");
    }
}

#[tokio::test]
async fn direct_message_endpoints_are_kind_safe_and_preserve_unavailable_history() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "direct-messages@example.com").await;
    let workspace_id = create_workspace(&app, &token).await;
    let agent_id = create_agent(&app, &token, &workspace_id, "Solo").await;
    let chat = create_chat(&app, &token, &agent_id).await;
    let chat_id = chat["id"].as_str().unwrap();

    let (status, initial_messages) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/direct-chats/{chat_id}/messages"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(initial_messages, json!([]));

    let (status, sent) = send(
        &app,
        request(
            "POST",
            &format!("/api/v2/direct-chats/{chat_id}/messages"),
            Some(&token),
            json!({"content":"hello direct"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {sent:?}");
    assert_eq!(sent["user_message"]["content"], "hello direct");

    let (status, body) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{chat_id}/messages"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");

    let (status, group) = send(
        &app,
        request(
            "POST",
            "/api/v2/groups",
            Some(&token),
            json!({"name":"ordinary", "workspace_id":workspace_id}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let group_id = group["id"].as_str().unwrap();
    let (status, body) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/direct-chats/{group_id}/messages"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");

    sqlx::query("UPDATE agents SET status = 'deleted' WHERE id = ?")
        .bind(&agent_id)
        .execute(state.db.pool())
        .await
        .unwrap();
    let (status, history) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/direct-chats/{chat_id}/messages"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(history.as_array().unwrap()[0]["content"], "hello direct");
    let (status, body) = send(
        &app,
        request(
            "POST",
            &format!("/api/v2/direct-chats/{chat_id}/messages"),
            Some(&token),
            json!({"content":"blocked"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "conflict");
}

#[tokio::test]
async fn direct_context_reset_preserves_history_and_starts_a_new_thread() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "direct-context-reset@example.com").await;
    let workspace_id = create_workspace(&app, &token).await;
    let agent_id = create_agent(&app, &token, &workspace_id, "Solo").await;
    let chat = create_chat(&app, &token, &agent_id).await;
    let chat_id = chat["id"].as_str().unwrap();

    let (status, first) = send(
        &app,
        request(
            "POST",
            &format!("/api/v2/direct-chats/{chat_id}/messages"),
            Some(&token),
            json!({"content":"remember this"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {first:?}");
    let first_thread_id = first["user_message"]["thread_id"].as_str().unwrap();

    let (status, old_latest) = send(
        &app,
        request(
            "POST",
            &format!("/api/v2/direct-chats/{chat_id}/messages"),
            Some(&token),
            json!({"content":"still old context"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {old_latest:?}");

    let (status, history_before) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/direct-chats/{chat_id}/messages"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(
        &app,
        authed(
            "POST",
            &format!("/api/v2/direct-chats/{chat_id}/context/reset"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "body: {body:?}");

    let old_thread_status: String = sqlx::query_scalar("SELECT status FROM threads WHERE id = ?")
        .bind(first_thread_id)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(old_thread_status, "cleared");

    let (status, history_after) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/direct-chats/{chat_id}/messages"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(history_after, history_before);

    let (status, second) = send(
        &app,
        request(
            "POST",
            &format!("/api/v2/direct-chats/{chat_id}/messages"),
            Some(&token),
            json!({"content":"start fresh"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {second:?}");
    assert_ne!(
        second["user_message"]["thread_id"].as_str().unwrap(),
        first_thread_id
    );

    let (status, latest) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/direct-chats/{chat_id}/messages?limit=1"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(latest.as_array().unwrap()[0]["content"], "start fresh");
}

#[tokio::test]
async fn direct_sessions_keep_threads_and_group_management_boundaries_isolated() {
    let (app, state) = router_with_state_for_tests().await;
    let token = register(&app, "direct-isolation@example.com").await;
    let workspace_id = create_workspace(&app, &token).await;
    let agent_id = create_agent(&app, &token, &workspace_id, "Solo").await;
    let first = create_chat(&app, &token, &agent_id).await;
    let second = create_chat(&app, &token, &agent_id).await;
    let first_id = first["id"].as_str().unwrap();
    let second_id = second["id"].as_str().unwrap();

    for (chat_id, content) in [(first_id, "first session"), (second_id, "second session")] {
        let (status, _) = send(
            &app,
            request(
                "POST",
                &format!("/api/v2/direct-chats/{chat_id}/messages"),
                Some(&token),
                json!({"content": content}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let threads: Vec<(String, String)> = sqlx::query_as(
        "SELECT group_id, id FROM threads WHERE group_id IN (?, ?) AND status = 'active' ORDER BY group_id",
    )
    .bind(first_id)
    .bind(second_id)
    .fetch_all(state.db.pool())
    .await
    .unwrap();
    assert_eq!(threads.len(), 2);
    assert_ne!(threads[0].1, threads[1].1);

    let (status, first_history) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/direct-chats/{first_id}/messages"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        first_history.as_array().unwrap()[0]["content"],
        "first session"
    );

    for call in [
        authed("GET", &format!("/api/v2/groups/{first_id}/members"), &token),
        authed("GET", &format!("/api/v2/groups/{first_id}/notes"), &token),
        authed("GET", &format!("/api/v2/groups/{first_id}/agents"), &token),
        request(
            "PATCH",
            &format!("/api/v2/groups/{first_id}"),
            Some(&token),
            json!({"scheduler_enabled": true}),
        ),
    ] {
        let (status, body) = send(&app, call).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "body: {body:?}");
    }
}

#[tokio::test]
async fn direct_stream_generates_first_title_and_replays_conversation_update() {
    let (app, _state) = router_with_state_for_tests().await;
    let token = register(&app, "direct-title-stream@example.com").await;
    let workspace_id = create_workspace(&app, &token).await;
    let agent_id = create_agent(&app, &token, &workspace_id, "Solo").await;
    let chat = create_chat(&app, &token, &agent_id).await;
    let chat_id = chat["id"].as_str().unwrap();
    let content = "   12345678901234567890123456789012 extra   spaces  ";
    let events = stream_events(
        &app,
        &format!("/api/v2/direct-chats/{chat_id}/messages/stream"),
        &token,
        json!({"content": content}),
    )
    .await;
    let update = events
        .iter()
        .find(|event| event["kind"] == "conversation_updated")
        .expect("conversation update event");
    assert_eq!(
        update["payload"]["title"],
        "12345678901234567890123456789012"
    );
    assert_eq!(update["payload"]["title_source"], "automatic");
    let user_event_id = events
        .iter()
        .find(|event| event["kind"] == "user_message")
        .and_then(|event| event["event_id"].as_str())
        .expect("user message event id")
        .to_owned();

    let (status, renamed) = send(
        &app,
        request(
            "PATCH",
            &format!("/api/v2/direct-chats/{chat_id}"),
            Some(&token),
            json!({"title":"Pinned title"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["title_source"], "manual");
    let second = stream_events(
        &app,
        &format!("/api/v2/direct-chats/{chat_id}/messages/stream"),
        &token,
        json!({"content":"another message"}),
    )
    .await;
    let second_update = second
        .iter()
        .find(|event| event["kind"] == "conversation_updated")
        .unwrap();
    assert_eq!(second_update["payload"]["title"], "Pinned title");
    assert_eq!(second_update["payload"]["title_source"], "manual");

    let replay = Request::builder()
        .method("POST")
        .uri(format!("/api/v2/direct-chats/{chat_id}/messages/stream"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .header("last-event-id", user_event_id)
        .body(Body::from(json!({"content":"must not start"}).to_string()))
        .unwrap();
    let response = app.clone().oneshot(replay).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let replay_text = String::from_utf8(
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(replay_text.contains("conversation_updated"));
}

/// Direct chats have the same file mutations groups do, served by the same
/// services under their own URL namespace, and still share the Git routes.
#[tokio::test]
async fn direct_workspace_supports_file_mutations_and_git_through_shared_routes() {
    let (app, _state) = router_with_state_for_tests().await;
    let token = register(&app, "direct-workspace-mutations@example.com").await;
    let (_root, workspace_id) =
        create_local_workspace(&app, &token, "Direct Mutations Workspace").await;
    let agent_id = create_agent(&app, &token, &workspace_id, "Local Agent").await;
    let chat = create_chat(&app, &token, &agent_id).await;
    let chat_id = chat["id"].as_str().unwrap();
    let chat_root = direct_workspace_root(&app, &token, chat_id).await;
    std::fs::write(chat_root.join("before.txt"), b"before").unwrap();
    std::fs::create_dir(chat_root.join("empty")).unwrap();
    let foreign_token = register(&app, "direct-workspace-foreign@example.com").await;

    let (status, body) = send(
        &app,
        request(
            "PATCH",
            &format!("/api/v2/direct-chats/{chat_id}/workspace-files/rename?path=before.txt"),
            Some(&foreign_token),
            json!({"new_path": "stolen.txt"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body:?}");
    assert!(chat_root.join("before.txt").is_file());

    let (status, body) = send(
        &app,
        authed(
            "DELETE",
            &format!("/api/v2/direct-chats/{chat_id}/workspace-files?path=empty"),
            &foreign_token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body:?}");
    assert!(chat_root.join("empty").is_dir());

    let (status, body) = send(
        &app,
        request(
            "POST",
            &format!("/api/v2/direct-chats/{chat_id}/workspace-files/create"),
            Some(&foreign_token),
            json!({"path": "stolen", "kind": "directory"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body:?}");
    assert!(!chat_root.join("stolen").exists());

    let (status, created) = send(
        &app,
        request(
            "POST",
            &format!("/api/v2/direct-chats/{chat_id}/workspace-files/create"),
            Some(&token),
            json!({"path": "drafts", "kind": "directory"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {created:?}");
    assert_eq!(created["path"], "drafts");
    assert!(chat_root.join("drafts").is_dir());

    let (status, created) = send(
        &app,
        request(
            "POST",
            &format!("/api/v2/direct-chats/{chat_id}/workspace-files/create"),
            Some(&token),
            json!({"path": "drafts/notes.md", "kind": "file"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {created:?}");
    assert_eq!(created["path"], "drafts/notes.md");
    assert!(chat_root.join("drafts").join("notes.md").is_file());

    let (status, renamed) = send(
        &app,
        request(
            "PATCH",
            &format!("/api/v2/direct-chats/{chat_id}/workspace-files/rename?path=before.txt"),
            Some(&token),
            json!({"new_path": "after.txt"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {renamed:?}");
    assert_eq!(renamed["path"], "after.txt");
    assert!(chat_root.join("after.txt").is_file());

    let (status, body) = send(
        &app,
        authed(
            "DELETE",
            &format!("/api/v2/direct-chats/{chat_id}/workspace-files?path=empty"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "body: {body:?}");
    assert!(!chat_root.join("empty").exists());

    std::fs::create_dir(chat_root.join("nested")).unwrap();
    std::fs::write(chat_root.join("nested").join("child.txt"), b"child").unwrap();
    let (status, body) = send(
        &app,
        request(
            "POST",
            &format!("/api/v2/direct-chats/{chat_id}/workspace-files/actions"),
            Some(&token),
            json!({"action": "delete", "paths": ["nested"]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "body: {body:?}");
    assert!(!chat_root.join("nested").exists());

    let (status, initialized) = send(
        &app,
        request(
            "POST",
            &format!("/api/v2/groups/{chat_id}/workspace-git/init"),
            Some(&token),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {initialized:?}");
    assert_eq!(initialized["available"], true);

    let (status, body) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/groups/{chat_id}/workspace-git/status"),
            &foreign_token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body:?}");
    assert_eq!(body["error"]["code"], "permission_denied");
}

/// Direct chats keep their own workspace when the agent is rebound.
#[tokio::test]
async fn direct_chat_workspace_is_isolated_from_agent_rebinding() {
    let (app, _state) = router_with_state_for_tests().await;
    let token = register(&app, "direct-workspace-follows@example.com").await;
    let (first_root, first_workspace) = create_local_workspace(&app, &token, "First").await;
    let agent_id = create_agent(&app, &token, &first_workspace, "Mover").await;
    let chat = create_chat(&app, &token, &agent_id).await;
    let chat_id = chat["id"].as_str().unwrap().to_string();
    let chat_workspace = chat["workspace_id"].as_str().unwrap().to_string();
    assert_ne!(chat_workspace, first_workspace);
    let chat_root = direct_workspace_root(&app, &token, &chat_id).await;
    std::fs::write(chat_root.join("chat.txt"), b"chat").unwrap();

    let (second_root, second_workspace) = create_local_workspace(&app, &token, "Second").await;
    std::fs::write(second_root.path().join("agent.txt"), b"agent").unwrap();
    let (status, updated) = send(
        &app,
        request(
            "PATCH",
            &format!("/api/v2/agents/{agent_id}"),
            Some(&token),
            json!({ "workspace_id": second_workspace }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {updated:?}");

    let (status, chat) = send(
        &app,
        authed("GET", &format!("/api/v2/direct-chats/{chat_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {chat:?}");
    assert_eq!(chat["workspace_id"], chat_workspace);

    // The private conversation still sees only its own directory.
    let (status, files) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/direct-chats/{chat_id}/workspace-files"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {files:?}");
    let names = files
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["name"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["chat.txt"]);
    assert!(!first_root.path().join("chat.txt").is_file());
}

/// A direct chat can upload just like a group chat; a one-on-one is where a
/// user is most likely to drop a file in.
#[tokio::test]
async fn direct_workspace_upload_writes_into_the_conversation_workspace() {
    let (app, _state) = router_with_state_for_tests().await;
    let token = register(&app, "direct-workspace-upload@example.com").await;
    let (_root, workspace_id) = create_local_workspace(&app, &token, "Uploads").await;
    let agent_id = create_agent(&app, &token, &workspace_id, "Uploader").await;
    let chat = create_chat(&app, &token, &agent_id).await;
    let chat_id = chat["id"].as_str().unwrap();
    let chat_root = direct_workspace_root(&app, &token, chat_id).await;

    let boundary = "boundary123";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"notes.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--{boundary}--\r\n"
    );
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/v2/direct-chats/{chat_id}/workspace-files/upload"
                ))
                .header("authorization", format!("Bearer {token}"))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(axum::body::Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(chat_root.join("uploads/notes.txt").is_file());
}

// ---------------------------------------------------------------------------
// Assistant-generated titles
// ---------------------------------------------------------------------------

fn openai_sse_body(text: &str) -> String {
    format!(
        "data: {}\ndata: [DONE]\n",
        json!({"choices": [{"delta": {"content": text}}]})
    )
}

/// Start a local OpenAI-style SSE server that answers queued bodies in order.
async fn queued_fake_provider(
    bodies: Vec<String>,
) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use axum::response::IntoResponse;
    use std::collections::VecDeque;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let queue = Arc::new(tokio::sync::Mutex::new(VecDeque::from(bodies)));
    let calls = Arc::new(AtomicUsize::new(0));
    let app_calls = Arc::clone(&calls);
    let app = Router::new().fallback(move || {
        let queue = Arc::clone(&queue);
        let calls = Arc::clone(&app_calls);
        async move {
            calls.fetch_add(1, Ordering::SeqCst);
            let body = queue
                .lock()
                .await
                .pop_front()
                .unwrap_or_else(|| "data: [DONE]\n".to_owned());
            (
                [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                body,
            )
                .into_response()
        }
    });
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), calls)
}

async fn create_provider_backed_chat(
    app: &Router,
    state: &AppState,
    email: &str,
    bodies: Vec<String>,
) -> (
    tempfile::TempDir,
    String,
    String,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    let token = register(app, email).await;
    let owner_id: String = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    let (base_url, calls) = queued_fake_provider(bodies).await;
    let provider_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO llm_providers \
         (id, owner_id, name, kind, base_url, api_key, default_model, reasoning_passback, \
          status, created_at, updated_at) \
         VALUES (?, ?, 'Fake', 'openai-compatible', ?, 'test-key', 'test-model', 0, \
                 'active', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
    )
    .bind(&provider_id)
    .bind(&owner_id)
    .bind(&base_url)
    .execute(state.db.pool())
    .await
    .unwrap();

    let (root, workspace_id) = create_local_workspace(app, &token, "Titles").await;
    let (status, agent) = send(
        app,
        request(
            "POST",
            "/api/v2/agents",
            Some(&token),
            json!({
                "name": "Solo",
                "workspace_id": workspace_id,
                "llm_provider_id": provider_id,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {agent:?}");
    let agent_id = agent["id"].as_str().unwrap();
    let chat = create_chat(app, &token, agent_id).await;
    (root, token, chat["id"].as_str().unwrap().to_owned(), calls)
}

/// The opening message of a direct chat makes the runtime ask the chat's own
/// agent to write a title, announced as a second `conversation_updated`
/// before the reply starts.
#[tokio::test]
async fn direct_stream_titles_new_chats_from_the_opening_message_via_llm() {
    let (app, state) = router_with_state_for_tests().await;
    // The first provider response names the chat; the second answers the agent.
    let (_root, token, chat_id, calls) = create_provider_backed_chat(
        &app,
        &state,
        "direct-llm-title@example.com",
        vec![
            openai_sse_body("\u{201c}Rust \u{6240}\u{6709}\u{6743}\u{201d}"),
            openai_sse_body("Hello! How can I help?"),
        ],
    )
    .await;

    let events = stream_events(
        &app,
        &format!("/api/v2/direct-chats/{chat_id}/messages/stream"),
        &token,
        json!({"content": "explain rust ownership please"}),
    )
    .await;

    let updates: Vec<&Value> = events
        .iter()
        .filter(|event| event["kind"] == "conversation_updated")
        .collect();
    assert_eq!(updates.len(), 2, "events: {events:?}");
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "events: {events:?}"
    );
    assert_eq!(updates[0]["payload"]["title_source"], "automatic");
    assert_eq!(
        updates[1]["payload"]["title"],
        format!("Rust \u{6240}\u{6709}\u{6743}")
    );
    assert_eq!(updates[1]["payload"]["conversation_id"], chat_id);

    let (status, fetched) = send(
        &app,
        authed("GET", &format!("/api/v2/direct-chats/{chat_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["title"], format!("Rust \u{6240}\u{6709}\u{6743}"));
}

#[tokio::test]
async fn a_manual_title_skips_the_opening_title_provider_call() {
    let (app, state) = router_with_state_for_tests().await;
    let (_root, token, chat_id, calls) = create_provider_backed_chat(
        &app,
        &state,
        "direct-manual-title@example.com",
        vec![openai_sse_body("Agent reply")],
    )
    .await;

    let (status, renamed) = send(
        &app,
        request(
            "PATCH",
            &format!("/api/v2/direct-chats/{chat_id}"),
            Some(&token),
            json!({"title": "Pinned before first message"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["title_source"], "manual");

    let events = stream_events(
        &app,
        &format!("/api/v2/direct-chats/{chat_id}/messages/stream"),
        &token,
        json!({"content": "private opening message"}),
    )
    .await;

    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "events: {events:?}"
    );
    let updates: Vec<&Value> = events
        .iter()
        .filter(|event| event["kind"] == "conversation_updated")
        .collect();
    assert_eq!(updates.len(), 1, "events: {events:?}");
    assert_eq!(
        updates[0]["payload"]["title"],
        "Pinned before first message"
    );
    assert_eq!(updates[0]["payload"]["title_source"], "manual");
}
