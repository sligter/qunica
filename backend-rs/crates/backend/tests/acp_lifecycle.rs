//! ACP runtime config, audit, and lifecycle tests (Task 9a + 9b).
//!
//! Config tests are pure (no I/O). Audit tests run against a migrated in-memory
//! SQLite database with the minimal FK rows seeded directly through the pool.
//! Lifecycle tests (Task 9b) spawn a real child process — this very integration
//! test binary, re-invoked with `ACP_FAKE_CHILD_MODE` set so the
//! `acp_lifecycle_fake_child_entrypoint` test speaks the ACP stdio JSON-RPC
//! protocol instead of running assertions. No Python/Node and no live network.

use ag_swarmer_backend::acp::{
    normalize_acp_runtime, run_acp_agent_stream, AcpConfigValue, AcpEventKind, AcpRunAudit,
    AcpRunContext, AcpRunRequest, AcpRuntimeConfig, AcpRuntimeProfile, PermissionPolicy,
    BLOCKED_ENV_KEYS, DEFAULT_TIMEOUT_SECONDS, MAX_TAIL_CHARS, MAX_TIMEOUT_SECONDS,
};
use ag_swarmer_backend::db::Db;
use serde_json::{json, Value};
use sqlx::SqlitePool;

// ---------------------------------------------------------------------------
// Config tests
// ---------------------------------------------------------------------------

fn err_message(raw: Value) -> String {
    normalize_acp_runtime(Some(&raw))
        .expect_err("expected config rejection")
        .to_string()
}

#[test]
fn acp_lifecycle_env_blocks_host_home_and_agent_config_leaks() {
    // Every blocked key is rejected, whichever one is present.
    for key in BLOCKED_ENV_KEYS {
        let message = err_message(json!({
            "command": "agent",
            "env": { key: "/tmp/leak" },
        }));
        assert_eq!(message, format!("ACP runtime env may not override {key}"));
    }

    // A non-blocked env key passes and survives normalization.
    let config = normalize_acp_runtime(Some(&json!({
        "command": "agent",
        "env": { "MY_AGENT_TOKEN": "abc" },
    })))
    .expect("benign env should normalize");
    assert_eq!(
        config.env.get("MY_AGENT_TOKEN").map(String::as_str),
        Some("abc")
    );
    // The blocked keys never leak in via the benign path either.
    for key in BLOCKED_ENV_KEYS {
        assert!(!config.env.contains_key(key));
    }

    // NUL in keys/values is rejected.
    assert_eq!(
        err_message(json!({ "command": "agent", "env": { "OK\0": "v" } })),
        "ACP runtime env key is invalid"
    );
    assert_eq!(
        err_message(json!({ "command": "agent", "env": { "OK": "v\0" } })),
        "ACP runtime env value is invalid"
    );
    // Non-string env values are rejected.
    assert_eq!(
        err_message(json!({ "command": "agent", "env": { "OK": 1 } })),
        "ACP runtime env keys and values must be strings"
    );
}

#[test]
fn acp_lifecycle_config_normalizes_settings_and_rejects_invalid_values() {
    // A fully specified config normalizes every field, trimming where Python does.
    let config = normalize_acp_runtime(Some(&json!({
        "command": "  my-agent  ",
        "args": ["--flag", "value"],
        "env": { "TOKEN": "secret" },
        "timeout_seconds": 120,
        "permission_policy": "auto_allow",
        "profile": "claude",
        "model": "  gpt-x  ",
        "mode": "plan",
        "thinking_effort": "high",
        "config_options": { "  reasoning  ": "  deep  ", "verbose": true },
    })))
    .expect("valid config should normalize");

    assert_eq!(config.command, "my-agent");
    assert_eq!(config.args, vec!["--flag", "value"]);
    assert_eq!(config.env.get("TOKEN").map(String::as_str), Some("secret"));
    assert_eq!(config.timeout_seconds, 120);
    assert_eq!(config.permission_policy, PermissionPolicy::AutoAllow);
    assert_eq!(config.profile, AcpRuntimeProfile::Claude);
    assert_eq!(config.model.as_deref(), Some("gpt-x"));
    assert_eq!(config.mode.as_deref(), Some("plan"));
    assert_eq!(config.thinking_effort.as_deref(), Some("high"));
    let options = config.config_options.expect("config_options present");
    assert_eq!(
        options.get("reasoning"),
        Some(&AcpConfigValue::Str("deep".to_string()))
    );
    assert_eq!(options.get("verbose"), Some(&AcpConfigValue::Bool(true)));

    // A minimal config takes all the Python defaults.
    let defaults = normalize_acp_runtime(Some(&json!({ "command": "agent" })))
        .expect("minimal config should normalize");
    assert_eq!(defaults.timeout_seconds, DEFAULT_TIMEOUT_SECONDS);
    assert_eq!(defaults.permission_policy, PermissionPolicy::Deny);
    assert_eq!(defaults.profile, AcpRuntimeProfile::Custom);
    assert!(defaults.args.is_empty());
    assert!(defaults.env.is_empty());
    assert!(defaults.model.is_none());
    assert!(defaults.mode.is_none());
    assert!(defaults.thinking_effort.is_none());
    assert!(defaults.config_options.is_none());

    // timeout_seconds == 0 falls back to the default (Python's `... or DEFAULT`).
    let zero_timeout =
        normalize_acp_runtime(Some(&json!({ "command": "agent", "timeout_seconds": 0 })))
            .expect("zero timeout falls back to default");
    assert_eq!(zero_timeout.timeout_seconds, DEFAULT_TIMEOUT_SECONDS);

    // Empty optional strings collapse to None.
    let blanks = normalize_acp_runtime(Some(&json!({
        "command": "agent",
        "model": "   ",
        "mode": "",
        "config_options": {},
    })))
    .expect("blank optionals should normalize");
    assert!(blanks.model.is_none());
    assert!(blanks.mode.is_none());
    assert!(blanks.config_options.is_none());

    // Rejections, each matching the Python oracle message.
    assert_eq!(
        normalize_acp_runtime(None).unwrap_err().to_string(),
        "ACP runtime config is required for ACP agents"
    );
    assert_eq!(
        err_message(json!({ "command": "agent", "adapter": "codex" })),
        "external CLI adapters are deprecated; configure this agent with an ACP \
         runtime command instead"
    );
    assert_eq!(
        err_message(json!({ "command": "agent", "adapter": "something" })),
        "ACP runtime config must not include an adapter field"
    );
    assert_eq!(err_message(json!({})), "ACP runtime command is required");
    assert_eq!(
        err_message(json!({ "command": "   " })),
        "ACP runtime command is required"
    );
    assert_eq!(
        err_message(json!({ "command": "bad\ncmd" })),
        "ACP runtime command is invalid"
    );
    assert_eq!(
        err_message(json!({ "command": "agent", "args": "nope" })),
        "ACP runtime args must be a list of strings"
    );
    assert_eq!(
        err_message(json!({ "command": "agent", "args": ["ok", 7] })),
        "ACP runtime args must be a list of strings"
    );
    assert_eq!(
        err_message(json!({ "command": "agent", "args": ["a\0b"] })),
        "ACP runtime arg is invalid"
    );
    assert_eq!(
        err_message(json!({ "command": "agent", "timeout_seconds": MAX_TIMEOUT_SECONDS + 1 })),
        "ACP runtime timeout_seconds is out of range"
    );
    assert_eq!(
        err_message(json!({ "command": "agent", "timeout_seconds": -5 })),
        "ACP runtime timeout_seconds is out of range"
    );
    assert_eq!(
        err_message(json!({ "command": "agent", "permission_policy": "maybe" })),
        "ACP runtime permission_policy must be deny or auto_allow"
    );
    assert_eq!(
        err_message(json!({ "command": "agent", "profile": "rogue" })),
        "ACP runtime profile must be custom, codex, or claude"
    );
    assert_eq!(
        err_message(json!({ "command": "agent", "model": 5 })),
        "ACP runtime model must be a string"
    );
    assert_eq!(
        err_message(json!({ "command": "agent", "config_options": [] })),
        "ACP runtime config_options must be an object"
    );
    assert_eq!(
        err_message(json!({ "command": "agent", "config_options": { "k": 1 } })),
        "ACP runtime config option values must be strings or booleans"
    );
    assert_eq!(
        err_message(json!({ "command": "agent", "config_options": { "  ": "v" } })),
        "ACP runtime config option keys must be strings"
    );
}

// ---------------------------------------------------------------------------
// Audit tests
// ---------------------------------------------------------------------------

/// Build a migrated in-memory database and seed the FK rows an audit run needs.
/// Returns the pool plus the seeded (owner_id, agent_id, group_id, thread_id).
async fn seeded_db() -> (SqlitePool, String, String, String, String) {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    let pool = db.pool().clone();

    let owner_id = uuid::Uuid::new_v4().to_string();
    let agent_id = uuid::Uuid::new_v4().to_string();
    let group_id = uuid::Uuid::new_v4().to_string();
    let thread_id = uuid::Uuid::new_v4().to_string();
    let now = "2024-01-01T00:00:00Z";

    sqlx::query(
        "INSERT INTO users (id, email, password_hash, name, created_at, updated_at) \
         VALUES (?, ?, 'hash', 'Tester', ?, ?)",
    )
    .bind(&owner_id)
    .bind(format!("{owner_id}@example.com"))
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO agents (id, owner_id, name, system_prompt, created_at, updated_at) \
         VALUES (?, ?, 'Agent', 'You are a test agent.', ?, ?)",
    )
    .bind(&agent_id)
    .bind(&owner_id)
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO groups (id, owner_id, name, created_at, updated_at) VALUES (?, ?, 'Team', ?, ?)",
    )
    .bind(&group_id)
    .bind(&owner_id)
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query("INSERT INTO threads (id, group_id, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(&thread_id)
        .bind(&group_id)
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();

    (pool, owner_id, agent_id, group_id, thread_id)
}

#[derive(sqlx::FromRow, Debug)]
struct RunRow {
    status: String,
    adapter: String,
    cwd: String,
    argv_json: String,
    exit_code: Option<i64>,
    stdout_tail: Option<String>,
    stderr_tail: Option<String>,
    error_message: Option<String>,
    started_at: String,
    ended_at: Option<String>,
}

async fn fetch_run(pool: &SqlitePool, id: &str) -> RunRow {
    sqlx::query_as::<_, RunRow>(
        "SELECT status, adapter, cwd, argv_json, exit_code, stdout_tail, stderr_tail, \
         error_message, started_at, ended_at FROM external_agent_runs WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn acp_lifecycle_audit_persists_running_and_completed_rows() {
    let (pool, owner_id, agent_id, group_id, thread_id) = seeded_db().await;

    let ctx = AcpRunContext {
        owner_id: owner_id.clone(),
        group_id: Some(group_id.clone()),
        agent_id: agent_id.clone(),
        thread_id: Some(thread_id.clone()),
        cwd: "/work/dir".to_string(),
        argv: vec!["my-agent".to_string(), "--flag".to_string()],
    };
    let audit = AcpRunAudit::start(&pool, &ctx).await.unwrap();

    // The initial row is `running` with argv recorded and no end time.
    let row = fetch_run(&pool, audit.id()).await;
    assert_eq!(row.status, "running");
    assert_eq!(row.adapter, "acp");
    assert_eq!(row.cwd, "/work/dir");
    assert_eq!(
        serde_json::from_str::<Value>(&row.argv_json).unwrap(),
        json!(["my-agent", "--flag"])
    );
    assert!(row.exit_code.is_none());
    assert!(row.ended_at.is_none());
    assert!(!row.started_at.is_empty());

    // A long stdout tail is bounded to MAX_TAIL_CHARS on completion.
    let long_stdout = "a".repeat(MAX_TAIL_CHARS + 1000);
    audit
        .complete(Some(0), Some(&long_stdout), Some("warn: ok"))
        .await
        .unwrap();

    let row = fetch_run(&pool, audit.id()).await;
    assert_eq!(row.status, "completed");
    assert_eq!(row.exit_code, Some(0));
    assert_eq!(
        row.stdout_tail.as_ref().unwrap().chars().count(),
        MAX_TAIL_CHARS
    );
    assert_eq!(row.stderr_tail.as_deref(), Some("warn: ok"));
    assert!(row.error_message.is_none());
    assert!(row.ended_at.is_some());
}

#[tokio::test]
async fn acp_lifecycle_audit_persists_failed_and_cancelled_rows() {
    let (pool, owner_id, agent_id, _group_id, _thread_id) = seeded_db().await;

    // A failed run records an exit code and an error message. group/thread null.
    let failed_ctx = AcpRunContext {
        owner_id: owner_id.clone(),
        group_id: None,
        agent_id: agent_id.clone(),
        thread_id: None,
        cwd: "/work".to_string(),
        argv: vec!["agent".to_string()],
    };
    let failed = AcpRunAudit::start(&pool, &failed_ctx).await.unwrap();
    failed
        .fail(Some(2), Some("partial"), Some("boom"), "process exited 2")
        .await
        .unwrap();

    let row = fetch_run(&pool, failed.id()).await;
    assert_eq!(row.status, "failed");
    assert_eq!(row.exit_code, Some(2));
    assert_eq!(row.stdout_tail.as_deref(), Some("partial"));
    assert_eq!(row.stderr_tail.as_deref(), Some("boom"));
    assert_eq!(row.error_message.as_deref(), Some("process exited 2"));
    assert!(row.ended_at.is_some());

    // A cancelled run has no exit code but keeps tails and an error message.
    let cancelled_ctx = AcpRunContext {
        owner_id,
        group_id: None,
        agent_id,
        thread_id: None,
        cwd: "/work".to_string(),
        argv: vec!["agent".to_string()],
    };
    let cancelled = AcpRunAudit::start(&pool, &cancelled_ctx).await.unwrap();
    cancelled
        .cancel(Some("so far"), None, "run was cancelled")
        .await
        .unwrap();

    let row = fetch_run(&pool, cancelled.id()).await;
    assert_eq!(row.status, "cancelled");
    assert!(row.exit_code.is_none());
    assert_eq!(row.stdout_tail.as_deref(), Some("so far"));
    assert!(row.stderr_tail.is_none());
    assert_eq!(row.error_message.as_deref(), Some("run was cancelled"));
    assert!(row.ended_at.is_some());
}

// ---------------------------------------------------------------------------
// Lifecycle tests (Task 9b): real child process via the fake-child entrypoint
// ---------------------------------------------------------------------------

/// Build an [`AcpRuntimeConfig`] whose command is this test binary re-invoked to
/// run only [`acp_lifecycle_fake_child_entrypoint`], with `ACP_FAKE_CHILD_MODE`
/// selecting the child's behavior. `extra` merges extra config fields.
fn fake_child_config(mode: &str, profile: &str, extra: Value) -> AcpRuntimeConfig {
    let exe = std::env::current_exe().expect("current test binary path");
    let mut obj = serde_json::Map::new();
    obj.insert("command".into(), json!(exe.to_string_lossy()));
    obj.insert(
        "args".into(),
        json!(["--exact", "acp_lifecycle_fake_child_entrypoint"]),
    );
    obj.insert("env".into(), json!({ "ACP_FAKE_CHILD_MODE": mode }));
    obj.insert("profile".into(), json!(profile));
    if let Value::Object(extra) = extra {
        for (key, value) in extra {
            if key == "env" {
                match value {
                    Value::Object(extra_env) => {
                        let base_env = obj
                            .entry("env")
                            .or_insert_with(|| Value::Object(serde_json::Map::new()));
                        if let Value::Object(base) = base_env {
                            for (env_key, env_value) in extra_env {
                                base.insert(env_key, env_value);
                            }
                        }
                    }
                    other => {
                        obj.insert(key, other);
                    }
                }
                continue;
            }
            obj.insert(key, value);
        }
    }
    normalize_acp_runtime(Some(&Value::Object(obj))).expect("fake child config normalizes")
}

#[tokio::test]
async fn acp_lifecycle_run_persists_running_and_completed_audit_rows() {
    let (pool, owner_id, agent_id, group_id, thread_id) = seeded_db().await;
    let cwd = tempfile::tempdir().unwrap();

    let config = fake_child_config("normal", "custom", json!({ "timeout_seconds": 30 }));
    let mut run = run_acp_agent_stream(
        pool.clone(),
        AcpRunRequest {
            owner_id,
            group_id: Some(group_id),
            agent_id,
            thread_id: Some(thread_id),
            config,
            cwd: cwd.path().to_path_buf(),
            prompt: "hi".to_string(),
        },
    )
    .await
    .expect("run starts");
    let run_id = run.run_id().to_string();

    // The row is persisted `running` before the turn finishes.
    let row = fetch_run(&pool, &run_id).await;
    assert_eq!(row.status, "running");
    assert!(row.ended_at.is_none());

    // The first event is the `running` run marker carrying the run id.
    let first = run.next_event().await.expect("running event");
    assert_eq!(first.kind, AcpEventKind::Run);
    assert_eq!(first.data["status"], json!("running"));
    assert_eq!(first.data["run_id"], json!(run_id));

    // Drain the rest of the stream to completion.
    let mut tokens = String::new();
    let mut saw_reasoning = false;
    let mut saw_tool_start = false;
    let mut saw_tool_result = false;
    let mut saw_usage = false;
    let mut terminal_status = None;
    while let Some(event) = run.next_event().await {
        match event.kind {
            AcpEventKind::Token => tokens.push_str(event.data.as_str().unwrap_or_default()),
            AcpEventKind::Reasoning => saw_reasoning = true,
            AcpEventKind::ToolCallStart => saw_tool_start = true,
            AcpEventKind::ToolCallResult => saw_tool_result = true,
            AcpEventKind::Usage => saw_usage = true,
            AcpEventKind::Run => {
                terminal_status = event.data["status"].as_str().map(str::to_string)
            }
        }
    }

    assert!(
        tokens.contains("hello"),
        "expected token text, got {tokens:?}"
    );
    assert!(saw_reasoning, "expected a reasoning event");
    assert!(saw_tool_start, "expected a tool_call_start event");
    assert!(saw_tool_result, "expected a tool_call_result event");
    assert!(saw_usage, "expected a usage event");
    assert_eq!(terminal_status.as_deref(), Some("completed"));

    // The terminal row is `completed` with a clean exit code.
    let row = fetch_run(&pool, &run_id).await;
    assert_eq!(row.status, "completed");
    assert_eq!(row.exit_code, Some(0));
    let stdout_tail = row.stdout_tail.as_deref().unwrap_or_default();
    assert!(
        stdout_tail.contains("\"sessionUpdate\":\"agent_message_chunk\""),
        "expected protocol stdout tail, got {stdout_tail:?}"
    );
    assert!(
        stdout_tail.contains("\"stopReason\":\"end_turn\""),
        "expected final prompt response in stdout tail, got {stdout_tail:?}"
    );
    assert!(row.error_message.is_none());
    assert!(row.ended_at.is_some());
}

#[tokio::test]
async fn acp_lifecycle_timeout_kills_child_and_persists_failed_status() {
    let (pool, owner_id, agent_id, _group_id, _thread_id) = seeded_db().await;
    let cwd = tempfile::tempdir().unwrap();

    // The `timeout` child never answers `session/prompt`; with a 1s timeout the
    // run is killed and persisted `failed`.
    let config = fake_child_config("timeout", "custom", json!({ "timeout_seconds": 1 }));
    let mut run = run_acp_agent_stream(
        pool.clone(),
        AcpRunRequest {
            owner_id,
            group_id: None,
            agent_id,
            thread_id: None,
            config,
            cwd: cwd.path().to_path_buf(),
            prompt: "hi".to_string(),
        },
    )
    .await
    .expect("run starts");
    let run_id = run.run_id().to_string();

    let mut statuses = Vec::new();
    while let Some(event) = run.next_event().await {
        if event.kind == AcpEventKind::Run {
            statuses.push(
                event.data["status"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            );
        }
    }
    assert_eq!(statuses, vec!["running".to_string(), "failed".to_string()]);

    let row = fetch_run(&pool, &run_id).await;
    assert_eq!(row.status, "failed");
    assert!(
        row.stdout_tail
            .as_deref()
            .unwrap_or_default()
            .contains("\"sessionId\":\"sess-fake\""),
        "expected setup responses in stdout tail, got {:?}",
        row.stdout_tail
    );
    assert!(
        row.error_message
            .as_deref()
            .unwrap_or_default()
            .contains("timed out"),
        "expected timeout message, got {:?}",
        row.error_message
    );
    assert!(row.ended_at.is_some());
}

#[tokio::test]
async fn acp_lifecycle_failed_child_exit_code_is_persisted() {
    const EXIT_CODE: i64 = 23;

    let (pool, owner_id, agent_id, _group_id, _thread_id) = seeded_db().await;
    let cwd = tempfile::tempdir().unwrap();

    let config = fake_child_config("fail_exit", "custom", json!({ "timeout_seconds": 30 }));
    let mut run = run_acp_agent_stream(
        pool.clone(),
        AcpRunRequest {
            owner_id,
            group_id: None,
            agent_id,
            thread_id: None,
            config,
            cwd: cwd.path().to_path_buf(),
            prompt: "hi".to_string(),
        },
    )
    .await
    .expect("run starts");
    let run_id = run.run_id().to_string();

    while run.next_event().await.is_some() {}

    let row = fetch_run(&pool, &run_id).await;
    assert_eq!(row.status, "failed");
    assert_eq!(row.exit_code, Some(EXIT_CODE));
    assert!(row.ended_at.is_some());
}

#[tokio::test]
async fn acp_lifecycle_stream_cancel_kills_child_and_persists_cancelled_status() {
    let (pool, owner_id, agent_id, _group_id, _thread_id) = seeded_db().await;
    let cwd = tempfile::tempdir().unwrap();

    // The `cancel` child holds the prompt open; a generous timeout ensures the
    // run ends via cancellation, not timeout.
    let config = fake_child_config("cancel", "custom", json!({ "timeout_seconds": 60 }));
    let mut run = run_acp_agent_stream(
        pool.clone(),
        AcpRunRequest {
            owner_id,
            group_id: None,
            agent_id,
            thread_id: None,
            config,
            cwd: cwd.path().to_path_buf(),
            prompt: "hi".to_string(),
        },
    )
    .await
    .expect("run starts");
    let run_id = run.run_id().to_string();
    let control = run.control();

    // Consume the `running` marker, then cancel mid-flight.
    let first = run.next_event().await.expect("running event");
    assert_eq!(first.data["status"], json!("running"));
    control.cancel();

    // A cancelled run emits no terminal run event, so the stream just closes.
    while run.next_event().await.is_some() {}

    let row = fetch_run(&pool, &run_id).await;
    assert_eq!(row.status, "cancelled");
    assert_eq!(
        row.error_message.as_deref(),
        Some("ACP agent run was cancelled")
    );
    assert!(row.exit_code.is_none());
    assert!(row.ended_at.is_some());
}

#[tokio::test]
async fn acp_lifecycle_custom_profile_child_env_is_isolated() {
    let (pool, owner_id, agent_id, _group_id, _thread_id) = seeded_db().await;
    let cwd = tempfile::tempdir().unwrap();

    let config = fake_child_config(
        "env",
        "custom",
        json!({
            "timeout_seconds": 30,
            "env": { "SAFE_KEY": "safe" },
        }),
    );
    let mut run = run_acp_agent_stream(
        pool.clone(),
        AcpRunRequest {
            owner_id,
            group_id: None,
            agent_id,
            thread_id: None,
            config,
            cwd: cwd.path().to_path_buf(),
            prompt: "hi".to_string(),
        },
    )
    .await
    .expect("run starts");
    let run_id = run.run_id().to_string();

    let mut env_text = None;
    let mut terminal_status = None;
    while let Some(event) = run.next_event().await {
        match event.kind {
            AcpEventKind::Token => env_text = event.data.as_str().map(str::to_string),
            AcpEventKind::Run => {
                terminal_status = event.data["status"].as_str().map(str::to_string)
            }
            _ => {}
        }
    }

    assert_eq!(terminal_status.as_deref(), Some("completed"));
    let child_env: Value = serde_json::from_str(&env_text.expect("env child echoes environment"))
        .expect("env summary is JSON");

    assert_eq!(child_env["AG_SWARMER_ACP_AGENT"], json!("1"));
    assert_eq!(child_env["SAFE_KEY"], json!("safe"));
    let home = child_env["HOME"].as_str().expect("HOME set");
    assert!(
        home.contains("ag-swarmer-acp-"),
        "expected isolated temp home, got {home:?}"
    );
    assert_eq!(child_env["USERPROFILE"], json!(home));

    let home_path = std::path::Path::new(home);
    for key in [
        "APPDATA",
        "LOCALAPPDATA",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "CODEX_HOME",
        "CLAUDE_CONFIG_DIR",
        "CLAUDE_HOME",
    ] {
        let value = child_env[key].as_str().unwrap_or_default();
        assert!(
            std::path::Path::new(value).starts_with(home_path),
            "{key} should live under HOME; {key}={value:?}, HOME={home:?}"
        );
    }

    let row = fetch_run(&pool, &run_id).await;
    assert_eq!(row.status, "completed");
    assert!(row.ended_at.is_some());
}

#[tokio::test]
async fn acp_lifecycle_applies_session_settings_and_emits_updates() {
    let (pool, owner_id, agent_id, group_id, thread_id) = seeded_db().await;
    let cwd = tempfile::tempdir().unwrap();

    // The `settings` child rejects `session/set_model`/`session/set_mode` with
    // method-not-found (forcing the config-option fallbacks), records every
    // `session/set_config_option`, and echoes them back as an update on prompt.
    let config = fake_child_config(
        "settings",
        "claude",
        json!({
            "timeout_seconds": 30,
            "model": "gpt-x",
            "mode": "plan",
            "thinking_effort": "high",
            "config_options": { "verbose": true, "temperature": "0.5" },
        }),
    );
    let mut run = run_acp_agent_stream(
        pool.clone(),
        AcpRunRequest {
            owner_id,
            group_id: Some(group_id),
            agent_id,
            thread_id: Some(thread_id),
            config,
            cwd: cwd.path().to_path_buf(),
            prompt: "hi".to_string(),
        },
    )
    .await
    .expect("run starts");
    let run_id = run.run_id().to_string();

    let mut applied_text = None;
    let mut saw_usage = false;
    let mut terminal_status = None;
    while let Some(event) = run.next_event().await {
        match event.kind {
            AcpEventKind::Token => applied_text = event.data.as_str().map(str::to_string),
            AcpEventKind::Usage => saw_usage = true,
            AcpEventKind::Run => {
                terminal_status = event.data["status"].as_str().map(str::to_string)
            }
            _ => {}
        }
    }

    assert!(saw_usage, "expected a usage event");
    assert_eq!(terminal_status.as_deref(), Some("completed"));

    let applied: Value =
        serde_json::from_str(&applied_text.expect("settings child echoes applied options"))
            .expect("applied summary is JSON");
    let options = applied["applied"].as_array().expect("applied is an array");
    let ids: Vec<&str> = options
        .iter()
        .filter_map(|opt| opt["configId"].as_str())
        .collect();

    // Model fell back to the `model` config option; mode fell back to the
    // claude-order `mode`; thinking effort used the claude-first `effort`; the
    // explicit options came through verbatim.
    assert!(ids.contains(&"model"), "ids: {ids:?}");
    assert!(ids.contains(&"mode"), "ids: {ids:?}");
    assert!(ids.contains(&"effort"), "ids: {ids:?}");
    assert!(ids.contains(&"verbose"), "ids: {ids:?}");
    assert!(ids.contains(&"temperature"), "ids: {ids:?}");

    let model_opt = options.iter().find(|o| o["configId"] == "model").unwrap();
    assert_eq!(model_opt["value"], json!("gpt-x"));
    let verbose_opt = options.iter().find(|o| o["configId"] == "verbose").unwrap();
    assert_eq!(verbose_opt["value"], json!(true));
    assert_eq!(verbose_opt["type"], json!("boolean"));

    let row = fetch_run(&pool, &run_id).await;
    assert_eq!(row.status, "completed");
    assert!(row.ended_at.is_some());
}

// ---------------------------------------------------------------------------
// Fake ACP child process
// ---------------------------------------------------------------------------

/// When `ACP_FAKE_CHILD_MODE` is set, this "test" is the entrypoint for the
/// child process the lifecycle tests spawn: it speaks the ACP stdio JSON-RPC
/// protocol on stdin/stdout and then exits, never returning to the harness.
/// Without the env var (i.e. in the normal parent test run) it is a no-op.
#[test]
fn acp_lifecycle_fake_child_entrypoint() {
    let Ok(mode) = std::env::var("ACP_FAKE_CHILD_MODE") else {
        return;
    };
    if mode == "fail_exit" {
        std::process::exit(23);
    }
    run_fake_child(&mode);
    std::process::exit(0);
}

/// Read JSON-RPC request lines from stdin and respond per `mode`. Writes go
/// straight to the process stdout handle (bypassing libtest's `print!` capture);
/// the parent's protocol reader skips the harness's non-JSON header line.
fn run_fake_child(mode: &str) {
    use std::io::BufRead;

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut applied: Vec<Value> = Vec::new();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let id = message.get("id").cloned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);

        match method {
            "initialize" => write_line(&stdout, &rpc_result(&id, json!({ "protocolVersion": 1 }))),
            "session/new" => write_line(
                &stdout,
                &rpc_result(&id, json!({ "sessionId": "sess-fake" })),
            ),
            "session/set_model" | "session/set_mode" => {
                if mode == "settings" {
                    write_line(&stdout, &rpc_error(&id, -32601, "method not found"));
                } else {
                    write_line(&stdout, &rpc_result(&id, json!({})));
                }
            }
            "session/set_config_option" => {
                applied.push(params);
                write_line(&stdout, &rpc_result(&id, json!({})));
            }
            "session/prompt" => match mode {
                // Hold the turn open so the parent can time out or cancel it.
                "timeout" | "cancel" => {}
                "env" => {
                    let mut env = serde_json::Map::new();
                    for key in [
                        "AG_SWARMER_ACP_AGENT",
                        "HOME",
                        "USERPROFILE",
                        "APPDATA",
                        "LOCALAPPDATA",
                        "XDG_CONFIG_HOME",
                        "XDG_DATA_HOME",
                        "XDG_CACHE_HOME",
                        "CODEX_HOME",
                        "CLAUDE_CONFIG_DIR",
                        "CLAUDE_HOME",
                        "SAFE_KEY",
                    ] {
                        if let Ok(value) = std::env::var(key) {
                            env.insert(key.to_string(), json!(value));
                        }
                    }
                    write_line(
                        &stdout,
                        &session_update(json!({
                            "sessionUpdate": "agent_message_chunk",
                            "content": { "type": "text", "text": Value::Object(env).to_string() },
                        })),
                    );
                    write_line(
                        &stdout,
                        &rpc_result(&id, json!({ "stopReason": "end_turn" })),
                    );
                }
                "settings" => {
                    let summary = json!({ "applied": applied.clone() }).to_string();
                    write_line(
                        &stdout,
                        &session_update(json!({
                            "sessionUpdate": "agent_message_chunk",
                            "content": { "type": "text", "text": summary },
                        })),
                    );
                    write_line(
                        &stdout,
                        &session_update(json!({
                            "sessionUpdate": "usage_update", "used": 10, "size": 1000,
                        })),
                    );
                    write_line(
                        &stdout,
                        &rpc_result(&id, json!({ "stopReason": "end_turn" })),
                    );
                }
                _ => {
                    write_line(
                        &stdout,
                        &session_update(json!({
                            "sessionUpdate": "agent_message_chunk",
                            "content": { "type": "text", "text": "hello world" },
                        })),
                    );
                    write_line(
                        &stdout,
                        &session_update(json!({
                            "sessionUpdate": "agent_thought_chunk",
                            "content": { "type": "text", "text": "thinking" },
                        })),
                    );
                    write_line(
                        &stdout,
                        &session_update(json!({
                            "sessionUpdate": "tool_call", "toolCallId": "t1", "title": "do_thing",
                            "status": "in_progress", "rawInput": { "a": 1 },
                        })),
                    );
                    write_line(
                        &stdout,
                        &session_update(json!({
                            "sessionUpdate": "tool_call_update", "toolCallId": "t1", "title": "do_thing",
                            "status": "completed", "rawOutput": { "ok": true },
                        })),
                    );
                    write_line(
                        &stdout,
                        &session_update(json!({
                            "sessionUpdate": "usage_update", "used": 5, "size": 1000,
                        })),
                    );
                    write_line(
                        &stdout,
                        &rpc_result(&id, json!({ "stopReason": "end_turn" })),
                    );
                }
            },
            _ => {
                if matches!(&id, Some(value) if !value.is_null()) {
                    write_line(&stdout, &rpc_error(&id, -32601, "method not found"));
                }
            }
        }
    }
}

fn rpc_result(id: &Option<Value>, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id.clone().unwrap_or(Value::Null), "result": result })
}

fn rpc_error(id: &Option<Value>, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.clone().unwrap_or(Value::Null),
        "error": { "code": code, "message": message },
    })
}

fn session_update(update: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": { "sessionId": "sess-fake", "update": update },
    })
}

fn write_line(stdout: &std::io::Stdout, value: &Value) {
    use std::io::Write;
    let mut handle = stdout.lock();
    let _ = handle.write_all(value.to_string().as_bytes());
    let _ = handle.write_all(b"\n");
    let _ = handle.flush();
}
