use qunica_backend::{
    api::router_with_state_for_tests,
    runtime::{
        sequence::{NewMessage, SequenceAllocator},
        StreamEvent, StreamEventKind,
    },
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use std::{path::Path, process::Command};
use tower::ServiceExt;
use uuid::Uuid;

const NOW: &str = "2026-08-14T00:00:00Z";

#[tokio::test]
async fn task_threads_create_list_filter_and_archive() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "task-threads@example.test";
    let token = register_and_login(&app, email).await;
    let owner_id: String = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    let group_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO groups (id, owner_id, name, created_at, updated_at) \
         VALUES (?, ?, 'Task group', ?, ?)",
    )
    .bind(&group_id)
    .bind(&owner_id)
    .bind(NOW)
    .bind(NOW)
    .execute(state.db.pool())
    .await
    .unwrap();

    let (status, first) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/threads"),
            &token,
            json!({"title": "First task"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(first["thread_type"], "task_thread");
    assert_eq!(first["title"], "First task");
    let first_id = first["id"].as_str().unwrap().to_owned();

    let (status, second) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/threads"),
            &token,
            json!({"title": "Second task"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let second_id = second["id"].as_str().unwrap().to_owned();

    let first_message = seed_message(&state, &group_id, &first_id, "first only").await;
    let second_message = seed_message(&state, &group_id, &second_id, "second only").await;

    let (status, filtered) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/groups/{group_id}/messages?limit=30&thread_id={first_id}"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(filtered.as_array().unwrap().len(), 1);
    assert_eq!(filtered[0]["content"], "first only");

    let (status, _) = send(
        &app,
        authed(
            "GET",
            &format!(
                "/api/v2/groups/{group_id}/messages?limit=30&thread_id={first_id}&before={second_message}"
            ),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send(
        &app,
        authed(
            "DELETE",
            &format!("/api/v2/groups/{group_id}/messages/{first_message}"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, filtered) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/groups/{group_id}/messages?limit=30&thread_id={first_id}"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(filtered.as_array().unwrap().is_empty());
    let first_status: String = sqlx::query_scalar("SELECT status FROM threads WHERE id = ?")
        .bind(&first_id)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(first_status, "active");

    let replacement = seed_message(&state, &group_id, &first_id, "clear only this task").await;
    let (status, cleared) = send(
        &app,
        authed(
            "POST",
            &format!("/api/v2/threads/{first_id}/messages/clear"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cleared["cleared_count"], 1);
    let statuses = sqlx::query_as::<_, (String, String)>(
        "SELECT id, status FROM messages WHERE id IN (?, ?) ORDER BY id",
    )
    .bind(&replacement)
    .bind(&second_message)
    .fetch_all(state.db.pool())
    .await
    .unwrap();
    assert_eq!(
        statuses
            .iter()
            .find(|(id, _)| id == &replacement)
            .unwrap()
            .1,
        "cleared"
    );
    assert_eq!(
        statuses
            .iter()
            .find(|(id, _)| id == &second_message)
            .unwrap()
            .1,
        "visible"
    );

    let (status, archived) = send(
        &app,
        authed(
            "POST",
            &format!("/api/v2/threads/{first_id}/archive"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(archived["status"], "archived");

    let late_message = NewMessage {
        id: Uuid::new_v4().to_string(),
        sender_type: "user".to_string(),
        sender_id: Some(owner_id.clone()),
        message_type: "text".to_string(),
        content: "late write".to_string(),
        content_json: None,
    };
    let late_event = StreamEvent::new(
        Uuid::new_v4(),
        0,
        StreamEventKind::UserMessage,
        json!({"message_id": late_message.id}),
    );
    let allocator = SequenceAllocator::new(state.db.pool().clone(), state.write_lock.clone());
    assert!(allocator
        .persist_message_with_event(&first_id, &group_id, &late_message, &late_event,)
        .await
        .is_err());

    let (status, threads) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}/threads"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(threads.as_array().unwrap().len(), 2);
    assert_eq!(threads[0]["id"], second_id);
    assert_eq!(threads[1]["id"], first_id);
    assert_eq!(threads[1]["status"], "archived");

    let (status, _) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/messages"),
            &token,
            json!({"content": "cannot append", "thread_id": first_id}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let stored: String = sqlx::query_scalar("SELECT content FROM messages WHERE id = ?")
        .bind(first_message)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(stored, "first only");
}

#[tokio::test]
async fn task_threads_bind_existing_or_new_git_branches_to_worktrees() {
    if !git_available() {
        return;
    }

    let (app, state) = router_with_state_for_tests().await;
    let email = "task-branches@example.test";
    let token = register_and_login(&app, email).await;
    let owner_id: String = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    let repository = tempfile::tempdir().unwrap();
    init_git_repo(repository.path());
    run_git(repository.path(), &["branch", "feature/existing"]);
    let repository_root = std::fs::canonicalize(repository.path()).unwrap();

    let workspace_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO workspaces \
         (id, owner_id, name, backend_type, local_path, status, created_at, updated_at) \
         VALUES (?, ?, 'Task repository', 'local', ?, 'active', ?, ?)",
    )
    .bind(&workspace_id)
    .bind(&owner_id)
    .bind(repository.path().to_string_lossy().into_owned())
    .bind(NOW)
    .bind(NOW)
    .execute(state.db.pool())
    .await
    .unwrap();
    let group_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO groups (id, owner_id, workspace_id, name, created_at, updated_at) \
         VALUES (?, ?, ?, 'Branched tasks', ?, ?)",
    )
    .bind(&group_id)
    .bind(&owner_id)
    .bind(&workspace_id)
    .bind(NOW)
    .bind(NOW)
    .execute(state.db.pool())
    .await
    .unwrap();

    let (status, existing) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/threads"),
            &token,
            json!({"title": "Existing branch", "git_branch": "feature/existing"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{existing:?}");
    assert_eq!(existing["git_branch"], "feature/existing");
    let existing_path: String =
        sqlx::query_scalar("SELECT worktree_path FROM threads WHERE id = ?")
            .bind(existing["id"].as_str().unwrap())
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert!(
        Path::new(&existing_path).starts_with(repository_root.join(".worktrees").join(&group_id))
    );
    assert_eq!(existing["worktree_path"], existing_path.as_str());
    assert_eq!(
        git_stdout(Path::new(&existing_path), &["branch", "--show-current"]),
        "feature/existing"
    );
    assert_eq!(
        git_stdout(repository.path(), &["status", "--porcelain"]),
        ""
    );

    let (status, created) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/threads"),
            &token,
            json!({"title": "New branch", "git_branch": "task/new"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["git_branch"], "task/new");
    let created_path: String = sqlx::query_scalar("SELECT worktree_path FROM threads WHERE id = ?")
        .bind(created["id"].as_str().unwrap())
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert!(
        Path::new(&created_path).starts_with(repository_root.join(".worktrees").join(&group_id))
    );
    assert_eq!(
        git_stdout(Path::new(&created_path), &["branch", "--show-current"]),
        "task/new"
    );

    let current = git_stdout(repository.path(), &["branch", "--show-current"]);
    let (status, error) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/threads"),
            &token,
            json!({"title": "Unsafe branch", "git_branch": current}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error["error"]["message"]
        .as_str()
        .unwrap()
        .contains("shared workspace"));

    // Deleting a task releases its worktree and frees the branch for a new one,
    // while the branch itself survives with whatever work the task produced.
    let created_id = created["id"].as_str().unwrap();
    let (status, _) = send(
        &app,
        authed(
            "POST",
            &format!("/api/v2/threads/{created_id}/archive"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app,
        authed("DELETE", &format!("/api/v2/threads/{created_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(!Path::new(&created_path).exists());
    assert!(git_stdout(repository.path(), &["branch", "--list", "task/new"]).contains("task/new"));

    let (status, rebound) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/threads"),
            &token,
            json!({"title": "Reused branch", "git_branch": "task/new"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(rebound["git_branch"], "task/new");
    let rebound_path: String = sqlx::query_scalar("SELECT worktree_path FROM threads WHERE id = ?")
        .bind(rebound["id"].as_str().unwrap())
        .fetch_one(state.db.pool())
        .await
        .unwrap();

    run_git(
        repository.path(),
        &["worktree", "remove", "--force", &existing_path],
    );
    run_git(
        repository.path(),
        &["worktree", "remove", "--force", &rebound_path],
    );
}

#[tokio::test]
async fn archived_task_threads_restore_and_delete() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "task-restore@example.test";
    let token = register_and_login(&app, email).await;
    let owner_id: String = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    let group_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO groups (id, owner_id, name, created_at, updated_at) \
         VALUES (?, ?, 'Restorable tasks', ?, ?)",
    )
    .bind(&group_id)
    .bind(&owner_id)
    .bind(NOW)
    .bind(NOW)
    .execute(state.db.pool())
    .await
    .unwrap();

    let (_, task) = send(
        &app,
        authed_json(
            "POST",
            &format!("/api/v2/groups/{group_id}/threads"),
            &token,
            json!({"title": "Restorable"}),
        ),
    )
    .await;
    let task_id = task["id"].as_str().unwrap().to_owned();
    let message_id = seed_message(&state, &group_id, &task_id, "kept while archived").await;

    let (status, _) = send(
        &app,
        authed("DELETE", &format!("/api/v2/threads/{task_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, _) = send(
        &app,
        authed(
            "POST",
            &format!("/api/v2/threads/{task_id}/archive"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, restored) = send(
        &app,
        authed(
            "POST",
            &format!("/api/v2/threads/{task_id}/unarchive"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(restored["status"], "active");

    let (status, again) = send(
        &app,
        authed(
            "POST",
            &format!("/api/v2/threads/{task_id}/unarchive"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["status"], "active");

    let (status, _) = send(
        &app,
        authed(
            "POST",
            &format!("/api/v2/threads/{task_id}/archive"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app,
        authed("DELETE", &format!("/api/v2/threads/{task_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, threads) = send(
        &app,
        authed("GET", &format!("/api/v2/groups/{group_id}/threads"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(threads.as_array().unwrap().is_empty());

    let message_status: String = sqlx::query_scalar("SELECT status FROM messages WHERE id = ?")
        .bind(&message_id)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(message_status, "cleared");

    let (status, _) = send(
        &app,
        authed("DELETE", &format!("/api/v2/threads/{task_id}"), &token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send(
        &app,
        authed(
            "POST",
            &format!("/api/v2/threads/{task_id}/unarchive"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

async fn seed_message(
    state: &qunica_backend::api::AppState,
    group_id: &str,
    thread_id: &str,
    content: &str,
) -> String {
    let id = Uuid::new_v4().to_string();
    let seq: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE thread_id = ?")
            .bind(thread_id)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO messages \
         (id, thread_id, group_id, seq, sender_type, message_type, content, status, created_at) \
         VALUES (?, ?, ?, ?, 'user', 'text', ?, 'visible', ?)",
    )
    .bind(&id)
    .bind(thread_id)
    .bind(group_id)
    .bind(seq)
    .bind(content)
    .bind(NOW)
    .execute(state.db.pool())
    .await
    .unwrap();
    id
}

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn init_git_repo(root: &Path) {
    run_git(root, &["init"]);
    run_git(root, &["config", "user.email", "tests@example.com"]);
    run_git(root, &["config", "user.name", "Tests"]);
    std::fs::write(root.join("tracked.txt"), "initial").unwrap();
    run_git(root, &["add", "tracked.txt"]);
    run_git(root, &["commit", "-m", "initial"]);
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_stdout(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

async fn register_and_login(app: &Router, email: &str) -> String {
    let (status, _) = send(
        app,
        json_request(
            "POST",
            "/api/v2/auth/register",
            None,
            json!({"email": email, "password": "supersecret", "name": "Task Tester"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = send(
        app,
        json_request(
            "POST",
            "/api/v2/auth/login",
            None,
            json!({"email": email, "password": "supersecret"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    body["access_token"].as_str().unwrap().to_owned()
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, body)
}

fn authed(method: &str, uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

fn authed_json(method: &str, uri: &str, token: &str, body: Value) -> Request<Body> {
    json_request(method, uri, Some(token), body)
}

fn json_request(method: &str, uri: &str, token: Option<&str>, body: Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}
