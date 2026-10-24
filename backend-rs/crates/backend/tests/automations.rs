//! Durable trigger acceptance, timer recovery and thread-level job arbitration.
//! These exercise the real API, SQLite schema and runtime worker instead of a
//! mocked scheduler: the acceptance promise is that a 202 exists only after the
//! occurrence, thread, message, turn, job and receipt are committed together.

use std::path::Path;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use chrono::{TimeZone, Utc};
use hmac::{Hmac, Mac};
use qunica_backend::{
    api::{self, automations, AppState},
    db::Db,
    server::{build_state, ServerConfig},
};
use serde_json::{json, Value};
use sha2::Sha256;
use tower::ServiceExt;
use uuid::Uuid;

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        app.clone()
            .oneshot(builder.body(Body::from(body.to_string())).unwrap()),
    )
    .await
    .unwrap()
    .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn sign(secret: &str, timestamp: i64, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    format!("sha256={:x}", mac.finalize().into_bytes())
}

async fn webhook(
    app: &Router,
    hook: &str,
    secret: &str,
    timestamp: i64,
    body: &[u8],
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/v2/automation-hooks/{hook}"))
        .header("content-type", "application/json")
        .header("x-qunica-timestamp", timestamp.to_string())
        .header("x-qunica-signature", sign(secret, timestamp, body))
        .body(Body::from(body.to_vec()))
        .unwrap();
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        app.clone().oneshot(request),
    )
    .await
    .unwrap()
    .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn sign_github(secret: &str, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    format!("sha256={:x}", mac.finalize().into_bytes())
}

async fn github_webhook(
    app: &Router,
    hook: &str,
    event: &str,
    delivery: &str,
    signature: &str,
    body: &[u8],
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/v2/automation-hooks/{hook}"))
        .header("content-type", "application/json")
        .header("x-github-event", event)
        .header("x-github-delivery", delivery)
        .header("x-hub-signature-256", signature)
        .body(Body::from(body.to_vec()))
        .unwrap();
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        app.clone().oneshot(request),
    )
    .await
    .unwrap()
    .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn fixture() -> (Router, AppState, String, String, String) {
    let (app, state) = api::router_with_state_for_tests().await;
    let (_, user) = request(
        &app,
        "POST",
        "/api/v2/auth/register",
        None,
        json!({"email":"automations@example.test","password":"supersecret","name":"Automations"}),
    )
    .await;
    let (_, login) = request(
        &app,
        "POST",
        "/api/v2/auth/login",
        None,
        json!({"email":"automations@example.test","password":"supersecret","device_label":"tests"}),
    )
    .await;
    let token = login["access_token"].as_str().unwrap().to_string();
    let owner = user["id"].as_str().unwrap().to_string();
    let group = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO groups(id,owner_id,name,status,conversation_kind,created_at,updated_at) \
         VALUES(?,?,'Group','active','group','2026-10-18T00:00:00Z','2026-10-18T00:00:00Z')",
    )
    .bind(&group)
    .bind(&owner)
    .execute(state.db.pool())
    .await
    .unwrap();
    (app, state, token, owner, group)
}

async fn create(app: &Router, token: &str, group: &str, name: &str, trigger: Value) -> Value {
    let (status, body) = request(
        app,
        "POST",
        "/api/v2/automations",
        Some(token),
        json!({
            "group_id": group,
            "name": name,
            "prompt": "Run the scheduled check",
            "trigger": trigger,
            "timezone": "UTC",
            "budget": {"max_agent_steps": 4, "max_total_tokens": 32000, "turn_timeout_seconds": 60}
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create failed: {body}");
    body
}

async fn set_due(state: &AppState, id: &str, due: i64) {
    sqlx::query("UPDATE automations SET next_run_at=? WHERE id=?")
        .bind(due)
        .bind(id)
        .execute(state.db.pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn preview_is_authenticated_read_only_and_rejects_invalid_schedules() {
    let (app, state, token, _owner, _group) = fixture().await;
    let body = json!({
        "trigger": {"kind": "cron", "expression": "0 9 * * MON-FRI"},
        "timezone": "Asia/Shanghai"
    });
    let (status, preview) = request(
        &app,
        "POST",
        "/api/v2/automations/preview",
        Some(&token),
        body.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["preview"].as_array().unwrap().len(), 5);
    assert!(preview["preview"][0]
        .as_str()
        .is_some_and(|value| value.ends_with("+08:00")));

    let (status, error) = request(
        &app,
        "POST",
        "/api/v2/automations/preview",
        Some(&token),
        json!({"trigger": {"kind": "cron", "expression": "not cron"}, "timezone": "UTC"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"]["code"], "invalid_input");
    let (status, _) = request(&app, "POST", "/api/v2/automations/preview", None, body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let persisted: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM automations")
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(persisted, 0, "preview must not create a task");
}

#[tokio::test]
async fn timer_tick_accepts_once_without_clients_and_atomically_writes_the_receipt() {
    let (app, state, token, _owner, group) = fixture().await;
    let automation = create(
        &app,
        &token,
        &group,
        "Every minute",
        json!({"kind":"cron","expression":"* * * * *"}),
    )
    .await;
    let automation_id = automation["id"].as_str().unwrap();
    let due = Utc.with_ymd_and_hms(2026, 10, 18, 12, 0, 0).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 10, 18, 12, 0, 3).unwrap();
    set_due(&state, automation_id, due.timestamp()).await;
    // The automation snapshots a lower token ceiling; the effective runtime
    // limit must be the intersection with the group's current configuration.
    sqlx::query("UPDATE groups SET max_total_tokens=64000 WHERE id=?")
        .bind(&group)
        .execute(state.db.pool())
        .await
        .unwrap();

    assert_eq!(automations::tick(&state, now, false).await.unwrap(), 1);
    let occurrence: (String, Option<String>, Option<String>, String) = sqlx::query_as(
        "SELECT source,job_id,thread_id,response_json FROM automation_occurrences o \
         LEFT JOIN execution_jobs j ON j.id=o.job_id WHERE o.automation_id=?",
    )
    .bind(automation_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(occurrence.0, "timer");
    let job_id = occurrence.1.expect("accepted occurrence must keep its job");
    let thread_id = occurrence
        .2
        .expect("accepted occurrence must keep its thread");
    let response: Value = serde_json::from_str(&occurrence.3).unwrap();
    assert_eq!(response["job_id"], json!(job_id));
    assert_eq!(response["thread_id"], json!(thread_id));

    let receipt: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM command_receipts WHERE owner_id=? AND request_id=?",
    )
    .bind(
        sqlx::query_scalar::<_, String>("SELECT owner_id FROM automations WHERE id=?")
            .bind(automation_id)
            .fetch_one(state.db.pool())
            .await
            .unwrap(),
    )
    .bind(response["request_id"].as_str().unwrap())
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(
        receipt, 1,
        "the acceptance receipt is part of the same commit"
    );
    let message: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE thread_id=?")
        .bind(&thread_id)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(message, 1);

    // The same instant must not create a second occurrence; next_run already
    // advanced in the committing transaction.
    assert_eq!(automations::tick(&state, now, false).await.unwrap(), 0);
    let occurrences: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM automation_occurrences WHERE automation_id=?")
            .bind(automation_id)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(occurrences, 1);
    let next_run: Option<i64> =
        sqlx::query_scalar("SELECT next_run_at FROM automations WHERE id=?")
            .bind(automation_id)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(
        next_run,
        Some(
            Utc.with_ymd_and_hms(2026, 10, 18, 12, 1, 0)
                .unwrap()
                .timestamp()
        )
    );

    wait_status(&state, &job_id, "succeeded").await;
    let snapshot: String = sqlx::query_scalar(
        "SELECT config_snapshot_json FROM group_turns \
         WHERE id=(SELECT turn_id FROM execution_jobs WHERE id=?)",
    )
    .bind(&job_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    let snapshot: Value = serde_json::from_str(&snapshot).unwrap();
    assert_eq!(
        snapshot["max_total_tokens"],
        json!(32000),
        "automation budget must cap the group runtime: {snapshot}"
    );
}

#[tokio::test]
async fn webhook_replay_returns_the_original_job_and_changed_content_conflicts() {
    let (app, state, token, _owner, group) = fixture().await;
    let automation = create(
        &app,
        &token,
        &group,
        "Signed hook",
        json!({"kind":"webhook"}),
    )
    .await;
    let automation_id = automation["id"].as_str().unwrap().to_string();
    let hook = automation["hook_id"].as_str().unwrap().to_string();
    let (status, rotated) = request(
        &app,
        "POST",
        &format!("/api/v2/automations/{automation_id}/secret"),
        Some(&token),
        json!({"action":"rotate","expected_version":1}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rotated}");
    let secret = rotated["secret"].as_str().unwrap().to_string();

    let body = br#"{"event_id":"delivery-1","payload":{"text":"hello"}}"#;
    let timestamp = Utc::now().timestamp();
    let (status, first) = webhook(&app, &hook, &secret, timestamp, body).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    let (status, replay) = webhook(&app, &hook, &secret, timestamp, body).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(first["job_id"], replay["job_id"]);
    assert_eq!(first["occurrence_id"], replay["occurrence_id"]);
    let occurrences: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM automation_occurrences WHERE automation_id=?")
            .bind(&automation_id)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    let jobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM execution_jobs WHERE automation_id=?")
        .bind(&automation_id)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!((occurrences, jobs), (1, 1));

    // A captured signature cannot rewrite identity: changing the body breaks
    // the signature first. With a valid signature for the same event id but
    // different content, dedup reports an idempotency conflict.
    let changed = br#"{"event_id":"delivery-1","payload":{"text":"different"}}"#;
    let (status, conflict) = webhook(&app, &hook, &secret, timestamp, changed).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["error"]["code"], "idempotency_conflict");
    let (status, unauthorized) = webhook(
        &app,
        &hook,
        "wrong-secret-at-least-32-bytes-long",
        timestamp,
        body,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(unauthorized["error"]["code"], "webhook_unauthorized");
    let (status, stale) = webhook(&app, &hook, &secret, timestamp - 301, body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(stale["error"]["code"], "webhook_unauthorized");
    let oversized = vec![b' '; 64 * 1024 + 1];
    let (status, too_large) = webhook(&app, &hook, &secret, timestamp, &oversized).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(too_large["error"]["code"], "webhook_too_large");

    // Concurrent delivery of one event must still create exactly one job.
    let concurrent = br#"{"event_id":"delivery-2","payload":{"text":"parallel"}}"#;
    let (left, right) = tokio::join!(
        webhook(&app, &hook, &secret, timestamp, concurrent),
        webhook(&app, &hook, &secret, timestamp, concurrent)
    );
    assert_eq!(left.0, StatusCode::ACCEPTED);
    assert_eq!(right.0, StatusCode::ACCEPTED);
    assert_eq!(left.1["job_id"], right.1["job_id"]);
    let deliveries: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM automation_occurrences WHERE automation_id=? AND source='webhook'",
    )
    .bind(&automation_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(deliveries, 2);
}

#[tokio::test]
async fn github_delivery_uses_native_signature_and_normalized_payload_dedup() {
    let (app, state, token, _owner, group) = fixture().await;
    let automation = create(
        &app,
        &token,
        &group,
        "GitHub push",
        json!({"kind":"github","events":["push"]}),
    )
    .await;
    let automation_id = automation["id"].as_str().unwrap().to_string();
    let hook = automation["hook_id"].as_str().unwrap().to_string();
    let (status, rotated) = request(
        &app,
        "POST",
        &format!("/api/v2/automations/{automation_id}/secret"),
        Some(&token),
        json!({"action":"rotate","expected_version":1}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rotated}");
    let secret = rotated["secret"].as_str().unwrap().to_string();

    let body = br#"{"ref":"refs/heads/main","after":"abc"}"#;
    let signature = sign_github(&secret, body);
    let (status, first) = github_webhook(&app, &hook, "push", "delivery-1", &signature, body).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    let occurrence: (String, String) = sqlx::query_as(
        "SELECT event_key,response_json FROM automation_occurrences WHERE automation_id=?",
    )
    .bind(&automation_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(occurrence.0, "github:delivery-1");
    let response: Value = serde_json::from_str(&occurrence.1).unwrap();
    assert_eq!(response["job_id"], first["job_id"]);
    let content: String = sqlx::query_scalar(
        "SELECT content FROM messages WHERE group_id=? AND sender_type='user' ORDER BY created_at DESC,rowid DESC LIMIT 1",
    )
    .bind(&group)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert!(content.contains("GitHub push"), "{content}");
    assert!(content.contains("github:delivery-1"), "{content}");

    // The delivery header is unsigned. Reusing the same signed payload under a
    // new id must not create a second task.
    let (status, replay) =
        github_webhook(&app, &hook, "push", "delivery-changed", &signature, body).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{replay}");
    assert_eq!(replay["job_id"], first["job_id"]);
    let occurrences: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM automation_occurrences WHERE automation_id=?")
            .bind(&automation_id)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(occurrences, 1);

    // A reused delivery id with different signed content is a conflict.
    let changed = br#"{"ref":"refs/heads/other","after":"def"}"#;
    let (status, conflict) = github_webhook(
        &app,
        &hook,
        "push",
        "delivery-1",
        &sign_github(&secret, changed),
        changed,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["error"]["code"], "idempotency_conflict");

    // Subscribed events only; an otherwise valid `issues` delivery is a 202
    // without an occurrence so GitHub does not retry it.
    let issues = br#"{"action":"opened"}"#;
    let (status, ignored) = github_webhook(
        &app,
        &hook,
        "issues",
        "delivery-2",
        &sign_github(&secret, issues),
        issues,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(ignored["status"], "ignored");
    let occurrences: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM automation_occurrences WHERE automation_id=?")
            .bind(&automation_id)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(occurrences, 1);

    let (status, _) = github_webhook(&app, &hook, "push", "delivery-3", "", body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = github_webhook(&app, &hook, "", "delivery-3", &signature, body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = github_webhook(&app, &hook, "push", "", &signature, body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = github_webhook(
        &app,
        &hook,
        "push",
        "delivery-4",
        &sign_github("another-fixture-secret-at-least-32-bytes", body),
        body,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, invalid) = request(
        &app,
        "POST",
        "/api/v2/automations",
        Some(&token),
        json!({
            "group_id":group,
            "name":"Invalid GitHub",
            "prompt":"x",
            "trigger":{"kind":"github","events":["not an event"]},
            "timezone":"UTC"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{invalid}");
}

#[tokio::test]
async fn restart_tick_coalesces_missed_runs_inside_the_catchup_window() {
    let (app, state, token, _owner, group) = fixture().await;
    let automation = create(
        &app,
        &token,
        &group,
        "Catch up",
        json!({"kind":"cron","expression":"* * * * *"}),
    )
    .await;
    let automation_id = automation["id"].as_str().unwrap();
    let now = Utc.with_ymd_and_hms(2026, 10, 18, 12, 0, 3).unwrap();
    set_due(&state, automation_id, now.timestamp() - 2 * 24 * 3600).await;

    assert_eq!(automations::tick(&state, now, true).await.unwrap(), 1);
    let timer_occurrences: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM automation_occurrences WHERE automation_id=? AND source='timer'",
    )
    .bind(automation_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    let skipped: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM automation_occurrences WHERE automation_id=? AND source='skipped'",
    )
    .bind(automation_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!((timer_occurrences, skipped), (1, 1));
    let scheduled: i64 = sqlx::query_scalar(
        "SELECT scheduled_at FROM automation_occurrences WHERE automation_id=? AND source='timer'",
    )
    .bind(automation_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(
        scheduled,
        Utc.with_ymd_and_hms(2026, 10, 18, 12, 0, 0)
            .unwrap()
            .timestamp()
    );
    assert_eq!(automations::tick(&state, now, true).await.unwrap(), 0);
}

#[tokio::test]
async fn pausing_cancels_only_unclaimed_automatic_work() {
    let (app, state, token, owner, group) = fixture().await;
    let automation = create(
        &app,
        &token,
        &group,
        "Pausable",
        json!({"kind":"cron","expression":"* * * * *"}),
    )
    .await;
    let automation_id = automation["id"].as_str().unwrap();
    // Seed accepted work directly so pausing deterministically races neither
    // the timer nor the worker that claims a newly committed occurrence.
    sqlx::query(
        "INSERT INTO execution_jobs(id,owner_id,request_id,command_name,group_id,thread_id,turn_id,stream_id,payload_json,status,generation,occupies_thread,worker_active,created_at,updated_at) \
         VALUES('busy-job',?, 'busy-request','message.send',?,'busy-thread','busy-turn','busy-stream','{}','running',1,1,1,'2026-10-18T11:59:00Z','2026-10-18T11:59:00Z')",
    )
    .bind(&owner)
    .bind(&group)
    .execute(state.db.pool())
    .await
    .unwrap();

    for (id, status, occupied) in [("unclaimed", "queued", 0), ("claimed", "running", 1)] {
        sqlx::query("INSERT INTO threads(id,group_id,status,created_at,updated_at) VALUES(?,?,'active','now','now')")
            .bind(id).bind(&group).execute(state.db.pool()).await.unwrap();
        sqlx::query("INSERT INTO group_turns(id,thread_id,group_id,status,scheduler_strategy,config_snapshot_json,topology_snapshot_json,created_at,updated_at) VALUES(?,?,?,?,'bounded','{}','{}','now','now')")
            .bind(id).bind(id).bind(&group).bind(status).execute(state.db.pool()).await.unwrap();
        sqlx::query("INSERT INTO execution_jobs(id,owner_id,request_id,command_name,group_id,thread_id,turn_id,stream_id,payload_json,status,occupies_thread,worker_active,automation_id,created_at,updated_at) VALUES(?,?,?,'message.send',?,?,?,?,'{}',?,?,?,?,'now','now')")
            .bind(id).bind(&owner).bind(id).bind(&group).bind(id).bind(id).bind(id)
            .bind(status).bind(occupied).bind(occupied).bind(automation_id)
            .execute(state.db.pool()).await.unwrap();
    }
    let job: (String, i64) =
        sqlx::query_as("SELECT status,occupies_thread FROM execution_jobs WHERE id='unclaimed'")
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(job, ("queued".to_owned(), 0));

    let (status, paused) = request(
        &app,
        "POST",
        &format!("/api/v2/automations/{automation_id}/toggle"),
        Some(&token),
        json!({"enabled":false,"expected_version":1}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{paused}");
    let cancelled: (String, String) = sqlx::query_as(
        "SELECT j.status,g.status FROM execution_jobs j JOIN group_turns g ON g.id=j.turn_id \
         WHERE j.id='unclaimed'",
    )
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(cancelled, ("cancelled".to_owned(), "cancelled".to_owned()));
    let busy: String = sqlx::query_scalar("SELECT status FROM execution_jobs WHERE id='busy-job'")
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(busy, "running", "pause must not stop another task's worker");
    let claimed: String =
        sqlx::query_scalar("SELECT status FROM execution_jobs WHERE id='claimed'")
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(claimed, "running", "pause preserves claimed automatic work");
    let next_run: Option<i64> =
        sqlx::query_scalar("SELECT next_run_at FROM automations WHERE id=?")
            .bind(automation_id)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(next_run, None);
}

#[tokio::test]
async fn restart_preserves_queued_work_and_blocks_only_the_interrupted_thread() {
    let directory = tempfile::tempdir().unwrap();
    let database_url = sqlite_url(&directory.path().join("automations.sqlite3"));
    let db = Db::connect(&database_url).await.unwrap();
    db.migrate().await.unwrap();
    let pool = db.pool().clone();
    let password_hash = bcrypt::hash("supersecret", 4).unwrap();
    sqlx::query(
        "INSERT INTO users(id,email,password_hash,name,created_at,updated_at) \
         VALUES('owner-1','restart@example.test',?,'Owner','2026-10-18T00:00:00Z','2026-10-18T00:00:00Z')",
    )
    .bind(&password_hash)
    .execute(&pool)
    .await
    .unwrap();
    for (group, name) in [("group-a", "Queued"), ("group-b", "Interrupted")] {
        sqlx::query(
            "INSERT INTO groups(id,owner_id,name,status,conversation_kind,created_at,updated_at) \
             VALUES(?,'owner-1',?,'active','group','2026-10-18T00:00:00Z','2026-10-18T00:00:00Z')",
        )
        .bind(group)
        .bind(name)
        .execute(&pool)
        .await
        .unwrap();
    }
    for thread in ["thread-a", "thread-b", "thread-b2"] {
        let group = if thread == "thread-a" {
            "group-a"
        } else {
            "group-b"
        };
        sqlx::query(
            "INSERT INTO threads(id,group_id,status,next_seq,created_at,updated_at) \
             VALUES(?,?,'active',1,'2026-10-18T00:00:00Z','2026-10-18T00:00:00Z')",
        )
        .bind(thread)
        .bind(group)
        .execute(&pool)
        .await
        .unwrap();
    }
    for (turn, thread, group, status) in [
        ("turn-a", "thread-a", "group-a", "queued"),
        ("turn-b", "thread-b", "group-b", "running"),
        ("turn-b2", "thread-b2", "group-b", "queued"),
        ("turn-b3", "thread-b", "group-b", "queued"),
    ] {
        sqlx::query(
            "INSERT INTO group_turns(id,thread_id,group_id,status,scheduler_strategy,config_snapshot_json,topology_snapshot_json,created_at,updated_at) \
             VALUES(?,?,?,?,'bounded','{}','{}','2026-10-18T00:00:00Z','2026-10-18T00:00:00Z')",
        )
        .bind(turn)
        .bind(thread)
        .bind(group)
        .bind(status)
        .execute(&pool)
        .await
        .unwrap();
    }
    for (id, group, thread, turn, status, occupies, worker) in [
        ("job-a", "group-a", "thread-a", "turn-a", "queued", 0, 0),
        ("job-b", "group-b", "thread-b", "turn-b", "running", 1, 1),
        ("job-b2", "group-b", "thread-b2", "turn-b2", "queued", 0, 0),
        ("job-b3", "group-b", "thread-b", "turn-b3", "queued", 0, 0),
    ] {
        sqlx::query(
            "INSERT INTO execution_jobs(id,owner_id,request_id,command_name,group_id,thread_id,turn_id,stream_id,payload_json,status,generation,occupies_thread,worker_active,created_at,updated_at) \
             VALUES(?,'owner-1',?,'message.send',?,?,?,?,'{\"content\":\"restart\",\"attachments\":[],\"model_override\":null,\"effort_override\":null}',?,?,?,?,'2026-10-18T11:00:00Z','2026-10-18T11:00:00Z')",
        )
        .bind(id)
        .bind(format!("request-{id}"))
        .bind(group)
        .bind(thread)
        .bind(turn)
        .bind(Uuid::new_v4().to_string())
        .bind(status)
        .bind(1)
        .bind(occupies)
        .bind(worker)
        .execute(&pool)
        .await
        .unwrap();
    }
    drop(pool);
    drop(db);

    let state = build_state(&ServerConfig {
        host: "127.0.0.1".to_owned(),
        port: 0,
        database_url,
        secret_key: "test-secret".to_owned(),
        registration_enabled: true,
        initial_user: None,
        app_data_dir: Some(directory.path().join("app-data")),
        web_dir: None,
        workspaces_dir: None,
    })
    .await
    .unwrap();
    let app = api::router(state.clone());

    wait_status(&state, "job-a", "succeeded").await;
    wait_status(&state, "job-b2", "succeeded").await;
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM execution_jobs WHERE id='job-b'")
            .fetch_one(state.db.pool())
            .await
            .unwrap(),
        "interrupted"
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM execution_jobs WHERE id='job-b3'")
            .fetch_one(state.db.pool())
            .await
            .unwrap(),
        "queued",
        "an unconfirmed interrupted job blocks its own thread while other tasks run"
    );

    let (_, login) = request(
        &app,
        "POST",
        "/api/v2/auth/login",
        None,
        json!({"email":"restart@example.test","password":"supersecret","device_label":"tests"}),
    )
    .await;
    let token = login["access_token"].as_str().unwrap();
    let (status, acknowledged) = request(
        &app,
        "POST",
        "/api/v2/commands",
        Some(token),
        json!({
            "request_id": Uuid::new_v4().to_string(),
            "name": "turn.acknowledge_stopped",
            "target": {"turn_id":"turn-b"},
            "payload": {}
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{acknowledged}");
    assert_eq!(acknowledged["jobs"], json!(["job-b"]));
    wait_status(&state, "job-b3", "succeeded").await;
}

async fn wait_status(state: &AppState, job: &str, expected: &str) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let status: String = sqlx::query_scalar("SELECT status FROM execution_jobs WHERE id=?")
                .bind(job)
                .fetch_one(state.db.pool())
                .await
                .unwrap();
            if status == expected {
                return;
            }
            let error: Option<String> =
                sqlx::query_scalar("SELECT error FROM execution_jobs WHERE id=?")
                    .bind(job)
                    .fetch_one(state.db.pool())
                    .await
                    .unwrap();
            let events: Vec<(String, String)> = sqlx::query_as(
                "SELECT kind,payload_json FROM stream_events WHERE stream_id=(SELECT stream_id FROM execution_jobs WHERE id=?) ORDER BY seq",
            )
            .bind(job)
            .fetch_all(state.db.pool())
            .await
            .unwrap();
            assert!(
                ["queued", "running"].contains(&status.as_str()),
                "job {job} ended as {status} ({error:?}) before becoming {expected}; events={events:?}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

fn sqlite_url(path: &Path) -> String {
    let mut display = path.to_string_lossy().replace('\\', "/");
    if !display.starts_with('/') && !display.starts_with("//") {
        display = format!("/{display}");
    }
    format!("sqlite://{display}?mode=rwc")
}
