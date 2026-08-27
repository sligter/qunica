//! ACP runtime config, audit, and lifecycle tests (Task 9a + 9b).
//!
//! Config tests are pure (no I/O). Audit tests run against a migrated in-memory
//! SQLite database with the minimal FK rows seeded directly through the pool.
//! Lifecycle tests (Task 9b) spawn a real child process — this very integration
//! test binary, re-invoked with `ACP_FAKE_CHILD_MODE` set so the
//! `acp_lifecycle_fake_child_entrypoint` test speaks the ACP stdio JSON-RPC
//! protocol instead of running assertions. No Python/Node and no live network.

use qunica_backend::acp::{
    normalize_acp_runtime, probe_acp_runtime_capabilities, run_acp_agent_stream,
    shutdown_reusable_acp_session, shutdown_reusable_acp_sessions, AcpCapabilityError,
    AcpConfigValue, AcpEventKind, AcpImage, AcpRunAudit, AcpRunContext, AcpRunRequest,
    AcpRuntimeConfig, AcpRuntimeProfile, PermissionPolicy, BLOCKED_ENV_KEYS,
    DEFAULT_TIMEOUT_SECONDS, MAX_TAIL_CHARS,
};
use qunica_backend::db::Db;
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

    let pi = normalize_acp_runtime(Some(&json!({
        "command": "pi-acp",
        "profile": "pi",
    })))
    .expect("pi profile should normalize");
    assert_eq!(pi.profile, AcpRuntimeProfile::Pi);

    let opencode = normalize_acp_runtime(Some(&json!({
        "command": "opencode",
        "args": ["acp"],
        "profile": "opencode",
    })))
    .expect("opencode profile should normalize");
    assert_eq!(opencode.profile, AcpRuntimeProfile::Opencode);

    let dsh = normalize_acp_runtime(Some(&json!({
        "command": "dsh-acp-demo",
        "profile": "dsh",
    })))
    .expect("dsh profile should normalize");
    assert_eq!(dsh.profile, AcpRuntimeProfile::Dsh);

    let migrated_codex = normalize_acp_runtime(Some(&json!({
        "command": "npx",
        "profile": "codex",
        "args": ["@zed-industries/codex-acp"],
    })))
    .expect("legacy Codex config normalizes");
    assert_eq!(migrated_codex.command, "npx");
    assert_eq!(
        migrated_codex.args,
        vec!["-y", "@agentclientprotocol/codex-acp"]
    );

    // Rejections with stable user-facing messages.
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
    let uncapped_timeout = normalize_acp_runtime(Some(&json!({
        "command": "agent",
        "timeout_seconds": u64::MAX,
    })))
    .expect("positive timeouts have no policy-level cap");
    assert_eq!(uncapped_timeout.timeout_seconds, u64::MAX);
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
        "ACP runtime profile must be custom, codex, claude, pi, opencode, or dsh"
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
    let (pool, owner_id, agent_id, _group_id, _thread_id) = seeded_db().await;
    let cwd = tempfile::tempdir().unwrap();

    let config = fake_child_config("normal", "custom", json!({ "timeout_seconds": 30 }));
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
            prompt_images: Vec::new(),
            prompt_has_image_attachments: false,
            incremental_prompt: None,
            incremental_prompt_images: Vec::new(),
            incremental_prompt_has_image_attachments: false,
            context_hash: None,
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
            AcpEventKind::Warning => {}
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
            prompt_images: Vec::new(),
            prompt_has_image_attachments: false,
            incremental_prompt: None,
            incremental_prompt_images: Vec::new(),
            incremental_prompt_has_image_attachments: false,
            context_hash: None,
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
            prompt_images: Vec::new(),
            prompt_has_image_attachments: false,
            incremental_prompt: None,
            incremental_prompt_images: Vec::new(),
            incremental_prompt_has_image_attachments: false,
            context_hash: None,
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
            prompt_images: Vec::new(),
            prompt_has_image_attachments: false,
            incremental_prompt: None,
            incremental_prompt_images: Vec::new(),
            incremental_prompt_has_image_attachments: false,
            context_hash: None,
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

async fn assert_child_env_is_isolated_for_profile(
    pool: SqlitePool,
    owner_id: &str,
    agent_id: &str,
    cwd: &std::path::Path,
    profile: &str,
) {
    let config = fake_child_config(
        "env",
        profile,
        json!({
            "timeout_seconds": 30,
            "env": { "SAFE_KEY": "safe" },
        }),
    );
    let mut run = run_acp_agent_stream(
        pool.clone(),
        AcpRunRequest {
            owner_id: owner_id.to_string(),
            group_id: None,
            agent_id: agent_id.to_string(),
            thread_id: None,
            config,
            cwd: cwd.to_path_buf(),
            prompt: "hi".to_string(),
            prompt_images: Vec::new(),
            prompt_has_image_attachments: false,
            incremental_prompt: None,
            incremental_prompt_images: Vec::new(),
            incremental_prompt_has_image_attachments: false,
            context_hash: None,
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
    run.join().await.expect("run joins");

    assert_eq!(terminal_status.as_deref(), Some("completed"));
    let child_env: Value = serde_json::from_str(&env_text.expect("env child echoes environment"))
        .expect("env summary is JSON");

    assert_eq!(child_env["QUNICA_ACP_AGENT"], json!("1"));
    assert_eq!(child_env["SAFE_KEY"], json!("safe"));
    let home = child_env["HOME"].as_str().expect("HOME set");
    assert!(
        home.contains("qunica-acp-"),
        "{profile}: expected isolated temp home, got {home:?}"
    );

    let home_path = std::path::Path::new(home);
    for key in [
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
    ] {
        let value = child_env[key].as_str().unwrap_or_default();
        assert!(
            std::path::Path::new(value).starts_with(home_path),
            "{profile}: {key} should live under HOME; {key}={value:?}, HOME={home:?}"
        );
    }

    let row = fetch_run(&pool, &run_id).await;
    assert_eq!(row.status, "completed");
    assert!(row.ended_at.is_some());
}

#[tokio::test]
async fn acp_lifecycle_child_env_is_isolated_for_untrusted_profiles() {
    let (pool, owner_id, agent_id, _group_id, _thread_id) = seeded_db().await;
    let cwd = tempfile::tempdir().unwrap();

    for profile in ["custom", "opencode"] {
        assert_child_env_is_isolated_for_profile(
            pool.clone(),
            &owner_id,
            &agent_id,
            cwd.path(),
            profile,
        )
        .await;
    }
}

#[tokio::test]
async fn acp_lifecycle_applies_session_settings_and_emits_updates() {
    let (pool, owner_id, agent_id, _group_id, _thread_id) = seeded_db().await;
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
            group_id: None,
            agent_id,
            thread_id: None,
            config,
            cwd: cwd.path().to_path_buf(),
            prompt: "hi".to_string(),
            prompt_images: Vec::new(),
            prompt_has_image_attachments: false,
            incremental_prompt: None,
            incremental_prompt_images: Vec::new(),
            incremental_prompt_has_image_attachments: false,
            context_hash: None,
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
            AcpEventKind::Warning => {}
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

#[tokio::test]
async fn acp_lifecycle_warns_instead_of_failing_when_settings_are_unimplemented() {
    let (pool, owner_id, agent_id, _group_id, _thread_id) = seeded_db().await;
    let cwd = tempfile::tempdir().unwrap();

    // A prompt-only ACP surface: `session/set_model`, `session/set_mode`, and
    // `session/set_config_option` are all method-not-found. Configuring a
    // model must not take the whole turn down with it.
    let config = fake_child_config(
        "settings_unsupported",
        "custom",
        json!({
            "timeout_seconds": 30,
            "model": "deepseek-v4-pro",
            "mode": "workspace-write",
            "thinking_effort": "high",
            "config_options": { "verbose": true },
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
            prompt_images: Vec::new(),
            prompt_has_image_attachments: false,
            incremental_prompt: None,
            incremental_prompt_images: Vec::new(),
            incremental_prompt_has_image_attachments: false,
            context_hash: None,
        },
    )
    .await
    .expect("run starts");
    let run_id = run.run_id().to_string();

    let mut warnings: Vec<Value> = Vec::new();
    let mut tokens = String::new();
    let mut terminal_status = None;
    while let Some(event) = run.next_event().await {
        match event.kind {
            AcpEventKind::Token => tokens.push_str(event.data.as_str().unwrap_or_default()),
            AcpEventKind::Warning => warnings.push(event.data),
            AcpEventKind::Run => {
                terminal_status = event.data["status"].as_str().map(str::to_string)
            }
            _ => {}
        }
    }

    // The turn ran to completion and the ignored settings were reported.
    assert_eq!(terminal_status.as_deref(), Some("completed"));
    assert!(tokens.contains("hello"), "tokens: {tokens:?}");
    let settings: Vec<&str> = warnings
        .iter()
        .filter_map(|warning| warning["setting"].as_str())
        .collect();
    assert!(settings.contains(&"model"), "warnings: {warnings:?}");
    assert!(settings.contains(&"mode"), "warnings: {warnings:?}");
    assert!(
        settings.contains(&"thinking effort"),
        "warnings: {warnings:?}"
    );
    assert!(
        settings.contains(&"config option"),
        "warnings: {warnings:?}"
    );
    assert_eq!(warnings[0]["code"], json!("acp_setting_unsupported"));

    let row = fetch_run(&pool, &run_id).await;
    assert_eq!(row.status, "completed");
}

#[tokio::test]
async fn acp_lifecycle_fails_when_an_implemented_config_option_is_rejected() {
    let (pool, owner_id, agent_id, _group_id, _thread_id) = seeded_db().await;
    let cwd = tempfile::tempdir().unwrap();

    // `session/set_config_option` exists here but rejects the id. That is a
    // real misconfiguration, not an unsupported surface, so it still fails.
    let config = fake_child_config(
        "settings_rejected",
        "custom",
        json!({
            "timeout_seconds": 30,
            "model": "deepseek-v4-pro",
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
            prompt_images: Vec::new(),
            prompt_has_image_attachments: false,
            incremental_prompt: None,
            incremental_prompt_images: Vec::new(),
            incremental_prompt_has_image_attachments: false,
            context_hash: None,
        },
    )
    .await
    .expect("run starts");
    let run_id = run.run_id().to_string();

    while run.next_event().await.is_some() {}

    let row = fetch_run(&pool, &run_id).await;
    assert_eq!(row.status, "failed");
}

#[tokio::test]
async fn acp_lifecycle_reuses_keyed_session_and_sends_incremental_prompt() {
    let (pool, owner_id, agent_id, group_id, thread_id) = seeded_db().await;
    let cwd = tempfile::tempdir().unwrap();
    let config = fake_child_config("reuse", "custom", json!({ "timeout_seconds": 30 }));

    let first = run_and_collect_tokens(
        pool.clone(),
        AcpRunRequest {
            owner_id: owner_id.clone(),
            group_id: Some(group_id.clone()),
            agent_id: agent_id.clone(),
            thread_id: Some(thread_id.clone()),
            config: config.clone(),
            cwd: cwd.path().to_path_buf(),
            prompt: "FULL_CONTEXT_ONE".to_string(),
            prompt_images: Vec::new(),
            prompt_has_image_attachments: false,
            incremental_prompt: Some("INCREMENT_ONE".to_string()),
            incremental_prompt_images: Vec::new(),
            incremental_prompt_has_image_attachments: false,
            context_hash: Some("ctx-a".to_string()),
        },
    )
    .await;
    let second = run_and_collect_tokens(
        pool.clone(),
        AcpRunRequest {
            owner_id,
            group_id: Some(group_id),
            agent_id,
            thread_id: Some(thread_id),
            config,
            cwd: cwd.path().to_path_buf(),
            prompt: "FULL_CONTEXT_TWO".to_string(),
            prompt_images: Vec::new(),
            prompt_has_image_attachments: false,
            incremental_prompt: Some("INCREMENT_TWO".to_string()),
            incremental_prompt_images: Vec::new(),
            incremental_prompt_has_image_attachments: false,
            context_hash: Some("ctx-a".to_string()),
        },
    )
    .await;

    let first_payload: Value = serde_json::from_str(&first).expect("first token json");
    let second_payload: Value = serde_json::from_str(&second).expect("second token json");
    assert_eq!(first_payload["new_count"], json!(1));
    assert_eq!(first_payload["prompt_count"], json!(1));
    assert_eq!(first_payload["prompt"], json!("FULL_CONTEXT_ONE"));
    assert_eq!(second_payload["new_count"], json!(1));
    assert_eq!(second_payload["prompt_count"], json!(2));
    assert_eq!(second_payload["prompt"], json!("INCREMENT_TWO"));
    shutdown_reusable_acp_sessions().await;
}

/// A turn against a live agent process, returning its streamed text and any
/// warnings raised about the session itself.
async fn run_and_collect_turn(pool: SqlitePool, request: AcpRunRequest) -> (String, Vec<Value>) {
    let mut run = run_acp_agent_stream(pool, request)
        .await
        .expect("run starts");
    let mut tokens = String::new();
    let mut warnings = Vec::new();
    let mut terminal_status = None;
    while let Some(event) = run.next_event().await {
        match event.kind {
            AcpEventKind::Token => tokens.push_str(event.data.as_str().unwrap_or_default()),
            AcpEventKind::Warning => warnings.push(event.data.clone()),
            AcpEventKind::Run => {
                terminal_status = event.data["status"].as_str().map(str::to_string)
            }
            _ => {}
        }
    }
    run.join().await.expect("run joins");
    assert_eq!(terminal_status.as_deref(), Some("completed"));
    (tokens, warnings)
}

/// The session id stored for a conversation's agent, if any.
async fn stored_session_id(pool: &SqlitePool, thread_id: &str, agent_id: &str) -> Option<String> {
    sqlx::query_as::<_, (String,)>(
        "SELECT session_id FROM acp_sessions WHERE thread_id = ? AND agent_id = ?",
    )
    .bind(thread_id)
    .bind(agent_id)
    .fetch_optional(pool)
    .await
    .expect("read stored ACP session")
    .map(|(session_id,)| session_id)
}

/// One conversation's ACP agent, so successive turns of a resume test address
/// the same session key.
struct ResumedConversation<'a> {
    owner_id: &'a str,
    agent_id: &'a str,
    group_id: &'a str,
    thread_id: &'a str,
    config: &'a AcpRuntimeConfig,
    cwd: &'a std::path::Path,
}

impl ResumedConversation<'_> {
    fn turn(&self, full_prompt: &str, incremental_prompt: &str) -> AcpRunRequest {
        AcpRunRequest {
            owner_id: self.owner_id.to_string(),
            group_id: Some(self.group_id.to_string()),
            agent_id: self.agent_id.to_string(),
            thread_id: Some(self.thread_id.to_string()),
            config: self.config.clone(),
            cwd: self.cwd.to_path_buf(),
            prompt: full_prompt.to_string(),
            prompt_images: Vec::new(),
            prompt_has_image_attachments: false,
            incremental_prompt: Some(incremental_prompt.to_string()),
            incremental_prompt_images: Vec::new(),
            incremental_prompt_has_image_attachments: false,
            context_hash: Some("ctx-a".to_string()),
        }
    }

    /// Drop the live child and everything the process knew about the session,
    /// exactly as closing and reopening the app does.
    async fn restart(&self) {
        shutdown_reusable_acp_session(self.group_id, self.thread_id, self.agent_id).await;
    }
}

#[tokio::test]
async fn acp_lifecycle_reopens_the_stored_session_after_a_restart() {
    let (pool, owner_id, agent_id, group_id, thread_id) = seeded_db().await;
    let cwd = tempfile::tempdir().unwrap();
    let config = fake_child_config("resume", "custom", json!({ "timeout_seconds": 30 }));
    let conversation = ResumedConversation {
        owner_id: &owner_id,
        agent_id: &agent_id,
        group_id: &group_id,
        thread_id: &thread_id,
        config: &config,
        cwd: cwd.path(),
    };

    let (first, _) = run_and_collect_turn(
        pool.clone(),
        conversation.turn("FULL_CONTEXT_ONE", "INCREMENT_ONE"),
    )
    .await;
    assert_eq!(
        stored_session_id(&pool, &thread_id, &agent_id)
            .await
            .as_deref(),
        Some("sess-fake"),
        "the session id is stored as soon as the agent issues it"
    );

    conversation.restart().await;

    let (second, warnings) = run_and_collect_turn(
        pool.clone(),
        conversation.turn("FULL_CONTEXT_TWO", "INCREMENT_TWO"),
    )
    .await;

    let first_payload: Value = serde_json::from_str(&first).expect("first token json");
    let second_payload: Value = serde_json::from_str(&second).expect("second token json");
    assert_eq!(first_payload["new_count"], json!(1));
    assert_eq!(first_payload["load_count"], json!(0));
    assert_eq!(first_payload["prompt"], json!("FULL_CONTEXT_ONE"));
    // The relaunched agent reopens the same session and is told only what has
    // happened since, rather than being handed the transcript as a stranger.
    assert_eq!(second_payload["new_count"], json!(0));
    assert_eq!(second_payload["load_count"], json!(1));
    assert_eq!(second_payload["session_id"], json!("sess-fake"));
    assert_eq!(second_payload["prompt"], json!("INCREMENT_TWO"));
    // A load replays the whole session back at the client; replaying it into
    // the turn would look like the agent saying it all over again.
    assert!(
        !second.contains("REPLAYED_HISTORY"),
        "history replayed by the load must not stream into the turn: {second}"
    );
    assert!(warnings.is_empty(), "a clean resume warns about nothing");
    conversation.restart().await;
}

#[tokio::test]
async fn acp_lifecycle_starts_fresh_when_the_stored_session_is_refused() {
    let (pool, owner_id, agent_id, group_id, thread_id) = seeded_db().await;
    let cwd = tempfile::tempdir().unwrap();
    let config = fake_child_config("resume_refused", "custom", json!({ "timeout_seconds": 30 }));
    let conversation = ResumedConversation {
        owner_id: &owner_id,
        agent_id: &agent_id,
        group_id: &group_id,
        thread_id: &thread_id,
        config: &config,
        cwd: cwd.path(),
    };

    run_and_collect_turn(
        pool.clone(),
        conversation.turn("FULL_CONTEXT_ONE", "INCREMENT_ONE"),
    )
    .await;
    conversation.restart().await;

    let (second, warnings) = run_and_collect_turn(
        pool.clone(),
        conversation.turn("FULL_CONTEXT_TWO", "INCREMENT_TWO"),
    )
    .await;

    let payload: Value = serde_json::from_str(&second).expect("second token json");
    // The agent refused the session it once issued, so the turn falls back to
    // the full transcript instead of failing.
    assert_eq!(payload["load_count"], json!(1));
    assert_eq!(payload["new_count"], json!(1));
    assert_eq!(payload["prompt"], json!("FULL_CONTEXT_TWO"));
    assert_eq!(
        warnings
            .iter()
            .filter_map(|warning| warning["code"].as_str())
            .collect::<Vec<_>>(),
        vec!["acp_session_resume_failed"],
        "the user is told the agent came back without its earlier working context"
    );
    // The dead id is not retried on every later turn.
    assert_eq!(
        stored_session_id(&pool, &thread_id, &agent_id)
            .await
            .as_deref(),
        Some("sess-fake"),
        "the fresh session replaces the refused one"
    );
    conversation.restart().await;
}

#[tokio::test]
async fn acp_lifecycle_sends_images_as_standard_prompt_blocks() {
    let (pool, owner_id, agent_id, _group_id, _thread_id) = seeded_db().await;
    let cwd = tempfile::tempdir().unwrap();
    let config = fake_child_config("images", "codex", json!({ "timeout_seconds": 30 }));

    let response = run_and_collect_tokens(
        pool,
        AcpRunRequest {
            owner_id,
            group_id: None,
            agent_id,
            thread_id: None,
            config,
            cwd: cwd.path().to_path_buf(),
            prompt: "Describe the attached image.".to_string(),
            prompt_images: vec![AcpImage {
                mime_type: "image/png".to_string(),
                data_base64: "AQIDBA==".to_string(),
            }],
            prompt_has_image_attachments: true,
            incremental_prompt: None,
            incremental_prompt_images: Vec::new(),
            incremental_prompt_has_image_attachments: false,
            context_hash: None,
        },
    )
    .await;

    let blocks: Value = serde_json::from_str(&response).expect("prompt blocks json");
    assert_eq!(blocks[0]["type"], "text");
    assert!(blocks[0]["text"]
        .as_str()
        .unwrap()
        .contains("Describe the attached image."));
    assert!(blocks[0]["text"]
        .as_str()
        .unwrap()
        .contains("report only details visible in those image pixels"));
    assert_eq!(
        blocks[1],
        json!({ "type": "image", "data": "AQIDBA==", "mimeType": "image/png" })
    );
}

#[tokio::test]
async fn acp_lifecycle_omits_images_when_agent_does_not_advertise_support() {
    let (pool, owner_id, agent_id, _group_id, _thread_id) = seeded_db().await;
    let cwd = tempfile::tempdir().unwrap();
    let config = fake_child_config(
        "images_unsupported",
        "custom",
        json!({ "timeout_seconds": 30 }),
    );

    let response = run_and_collect_tokens(
        pool,
        AcpRunRequest {
            owner_id,
            group_id: None,
            agent_id,
            thread_id: None,
            config,
            cwd: cwd.path().to_path_buf(),
            prompt: "Describe the attached image.".to_string(),
            prompt_images: vec![AcpImage {
                mime_type: "image/png".to_string(),
                data_base64: "AQIDBA==".to_string(),
            }],
            prompt_has_image_attachments: true,
            incremental_prompt: None,
            incremental_prompt_images: Vec::new(),
            incremental_prompt_has_image_attachments: false,
            context_hash: None,
        },
    )
    .await;

    let blocks: Value = serde_json::from_str(&response).expect("prompt blocks json");
    assert_eq!(blocks.as_array().unwrap().len(), 1);
    assert_eq!(blocks[0]["type"], "text");
    assert!(blocks[0]["text"]
        .as_str()
        .unwrap()
        .contains("native image input is unavailable"));
}

#[tokio::test]
async fn acp_lifecycle_blocks_visual_claims_when_attachment_bytes_are_unavailable() {
    let (pool, owner_id, agent_id, _group_id, _thread_id) = seeded_db().await;
    let cwd = tempfile::tempdir().unwrap();
    let config = fake_child_config("images", "codex", json!({ "timeout_seconds": 30 }));

    let response = run_and_collect_tokens(
        pool,
        AcpRunRequest {
            owner_id,
            group_id: None,
            agent_id,
            thread_id: None,
            config,
            cwd: cwd.path().to_path_buf(),
            prompt: "Extract the text from the attachment.".to_string(),
            prompt_images: Vec::new(),
            prompt_has_image_attachments: true,
            incremental_prompt: None,
            incremental_prompt_images: Vec::new(),
            incremental_prompt_has_image_attachments: false,
            context_hash: None,
        },
    )
    .await;

    let blocks: Value = serde_json::from_str(&response).expect("prompt blocks json");
    assert_eq!(blocks.as_array().unwrap().len(), 1);
    assert!(blocks[0]["text"]
        .as_str()
        .unwrap()
        .contains("native image input is unavailable"));
}

#[tokio::test]
async fn acp_lifecycle_context_hash_change_restarts_keyed_session() {
    let (pool, owner_id, agent_id, group_id, thread_id) = seeded_db().await;
    let cwd = tempfile::tempdir().unwrap();
    let config = fake_child_config("reuse", "custom", json!({ "timeout_seconds": 30 }));

    let _ = run_and_collect_tokens(
        pool.clone(),
        AcpRunRequest {
            owner_id: owner_id.clone(),
            group_id: Some(group_id.clone()),
            agent_id: agent_id.clone(),
            thread_id: Some(thread_id.clone()),
            config: config.clone(),
            cwd: cwd.path().to_path_buf(),
            prompt: "FULL_CONTEXT_ONE".to_string(),
            prompt_images: Vec::new(),
            prompt_has_image_attachments: false,
            incremental_prompt: Some("INCREMENT_ONE".to_string()),
            incremental_prompt_images: Vec::new(),
            incremental_prompt_has_image_attachments: false,
            context_hash: Some("ctx-a".to_string()),
        },
    )
    .await;
    let second = run_and_collect_tokens(
        pool.clone(),
        AcpRunRequest {
            owner_id,
            group_id: Some(group_id),
            agent_id,
            thread_id: Some(thread_id),
            config,
            cwd: cwd.path().to_path_buf(),
            prompt: "FULL_CONTEXT_TWO".to_string(),
            prompt_images: Vec::new(),
            prompt_has_image_attachments: false,
            incremental_prompt: Some("INCREMENT_TWO".to_string()),
            incremental_prompt_images: Vec::new(),
            incremental_prompt_has_image_attachments: false,
            context_hash: Some("ctx-b".to_string()),
        },
    )
    .await;

    let second_payload: Value = serde_json::from_str(&second).expect("second token json");
    assert_eq!(second_payload["new_count"], json!(1));
    assert_eq!(second_payload["prompt_count"], json!(1));
    assert_eq!(second_payload["prompt"], json!("FULL_CONTEXT_TWO"));
    shutdown_reusable_acp_sessions().await;
}

async fn run_and_collect_tokens(pool: SqlitePool, request: AcpRunRequest) -> String {
    let mut run = run_acp_agent_stream(pool, request)
        .await
        .expect("run starts");
    let mut tokens = String::new();
    let mut terminal_status = None;
    while let Some(event) = run.next_event().await {
        match event.kind {
            AcpEventKind::Token => tokens.push_str(event.data.as_str().unwrap_or_default()),
            AcpEventKind::Run => {
                terminal_status = event.data["status"].as_str().map(str::to_string)
            }
            _ => {}
        }
    }
    run.join().await.expect("run joins");
    assert_eq!(terminal_status.as_deref(), Some("completed"));
    tokens
}

#[tokio::test]
async fn acp_capability_probe_normalizes_initial_options_without_prompt() {
    let cwd = tempfile::tempdir().unwrap();
    let log = cwd.path().join("probe.log");
    let config = fake_child_config(
        "capabilities_initial",
        "custom",
        json!({ "env": { "ACP_FAKE_LOG": log.to_string_lossy() } }),
    );

    let capabilities = probe_acp_runtime_capabilities(config, None)
        .await
        .expect("capability probe succeeds");

    assert_eq!(capabilities.models[0].value, "gpt-5.5");
    assert_eq!(capabilities.models[0].label, "GPT-5.5");
    assert_eq!(
        capabilities.models[0].description.as_deref(),
        Some("Primary model")
    );
    assert_eq!(capabilities.modes[0].value, "auto");
    assert_eq!(capabilities.thinking_efforts[0].value, "low");
    assert_eq!(capabilities.current_model.as_deref(), Some("gpt-5.5"));
    assert_eq!(capabilities.current_mode.as_deref(), Some("auto"));
    assert_eq!(capabilities.current_thinking_effort.as_deref(), Some("low"));
    assert_eq!(capabilities.source, "acp");
    assert_eq!(capabilities.warning, None);

    let log = std::fs::read_to_string(log).expect("read fake ACP log");
    assert!(log.find("initialize").unwrap() < log.find("session/new").unwrap());
    assert!(!log.contains("session/prompt"));
    assert!(log.contains("ACP_FAKE_EXIT"));
}

#[tokio::test]
async fn acp_capability_probe_reads_session_model_catalog() {
    let config = fake_child_config("capabilities_model_catalog", "custom", json!({}));

    let capabilities = probe_acp_runtime_capabilities(config, None)
        .await
        .expect("model catalog probe succeeds");

    assert!(capabilities
        .models
        .iter()
        .any(|model| model.value == "gpt-5.6-terra[high]"));
    assert!(capabilities
        .models
        .iter()
        .any(|model| model.value == "gpt-5.5"));
    assert_eq!(capabilities.current_model.as_deref(), Some("gpt-5.5"));
}

#[tokio::test]
async fn acp_capability_probe_keeps_opencode_model_config_options_scoped() {
    let config = fake_child_config("capabilities_model_catalog", "opencode", json!({}));

    let capabilities = probe_acp_runtime_capabilities(config, None)
        .await
        .expect("OpenCode capability probe succeeds");

    assert!(capabilities
        .models
        .iter()
        .any(|model| model.value == "gpt-5.5"));
    assert!(capabilities.models.iter().all(|model| {
        !matches!(
            model.value.as_str(),
            "gpt-5.6-terra[low]" | "gpt-5.6-terra[high]"
        )
    }));
}

#[tokio::test]
async fn acp_capability_probe_applies_only_the_selected_model() {
    let cwd = tempfile::tempdir().unwrap();
    let log_path = cwd.path().join("probe.log");
    let config = fake_child_config(
        "capabilities_update",
        "custom",
        json!({
            "env": { "ACP_FAKE_LOG": log_path.to_string_lossy() },
            "model": "saved-model-must-not-apply",
            "mode": "saved-mode-must-not-apply",
            "thinking_effort": "saved-effort-must-not-apply",
            "config_options": { "custom_runtime_option": "preserved" },
        }),
    );

    let capabilities = probe_acp_runtime_capabilities(config, Some("gpt-5.5".to_string()))
        .await
        .expect("model-dependent capability probe succeeds");

    assert_eq!(capabilities.current_model.as_deref(), Some("gpt-5.5"));
    assert_eq!(capabilities.thinking_efforts[0].value, "xhigh");
    assert_eq!(
        capabilities.current_thinking_effort.as_deref(),
        Some("xhigh")
    );

    let log = std::fs::read_to_string(log_path).expect("read fake ACP log");
    assert!(log.contains("session/set_model"));
    assert!(log.contains("gpt-5.5"));
    assert!(!log.contains("saved-model-must-not-apply"));
    assert!(!log.contains("\"method\":\"session/set_mode\""));
    assert!(!log.contains("saved-mode-must-not-apply"));
    assert!(!log.contains("saved-effort-must-not-apply"));
    assert!(!log.contains("custom_runtime_option"));
    assert!(!log.contains("preserved"));
    assert!(!log.contains("session/set_config_option"));
    assert!(!log.contains("session/prompt"));
}

#[tokio::test]
async fn acp_capability_probe_falls_back_to_model_config_option() {
    let cwd = tempfile::tempdir().unwrap();
    let log_path = cwd.path().join("probe.log");
    let config = fake_child_config(
        "capabilities_fallback",
        "custom",
        json!({ "env": { "ACP_FAKE_LOG": log_path.to_string_lossy() } }),
    );

    let capabilities = probe_acp_runtime_capabilities(config, Some("gpt-5.5".to_string()))
        .await
        .expect("fallback capability probe succeeds");

    assert_eq!(capabilities.current_model.as_deref(), Some("gpt-5.5"));
    assert_eq!(capabilities.thinking_efforts[0].value, "xhigh");
    let log = std::fs::read_to_string(log_path).expect("read fake ACP log");
    let set_model = log.find("session/set_model").unwrap();
    let fallback = log.find("\"configId\":\"model\"").unwrap();
    assert!(set_model < fallback);
    assert!(!log.contains("session/prompt"));
}

#[tokio::test]
async fn acp_capability_probe_survives_a_runtime_with_no_settings_methods() {
    // dsh answers method-not-found for `session/set_model` *and* for the
    // `session/set_config_option` fallback: its model rides the composition it
    // launched with. A runtime with no wire-level selector must probe as "no
    // choices discovered", not as a rejected probe — the settings panel
    // otherwise shows a discovery error for a runtime that works.
    let cwd = tempfile::tempdir().unwrap();
    let log_path = cwd.path().join("probe.log");
    let config = fake_child_config(
        "settings_unsupported",
        "custom",
        json!({ "env": { "ACP_FAKE_LOG": log_path.to_string_lossy() } }),
    );

    let capabilities = probe_acp_runtime_capabilities(config, Some("gpt-5.5".to_string()))
        .await
        .expect("a runtime without settings methods still probes");

    assert!(capabilities.warning.is_none());
    // The user's pick stays selected even though nothing came back over the
    // wire; the choices themselves come from the runtime preset.
    assert_eq!(capabilities.current_model.as_deref(), Some("gpt-5.5"));
    assert!(capabilities.models.is_empty());
    let log = std::fs::read_to_string(log_path).expect("read fake ACP log");
    assert!(log.contains("session/set_model"));
    assert!(log.contains("\"configId\":\"model\""));
    assert!(!log.contains("session/prompt"));
}

#[tokio::test]
async fn acp_capability_probe_preserves_wire_order_for_updates_and_responses() {
    let config = fake_child_config("capabilities_ordering", "custom", json!({}));

    let capabilities = probe_acp_runtime_capabilities(config, Some("gpt-5.5".to_string()))
        .await
        .expect("ordered capability probe succeeds");

    assert_eq!(capabilities.thinking_efforts[0].value, "medium");
    assert_eq!(
        capabilities.current_thinking_effort.as_deref(),
        Some("medium")
    );
}

#[tokio::test]
async fn acp_capability_probe_uses_latest_mode_source_on_the_wire() {
    for (fake_mode, expected_mode) in [
        ("capabilities_mode_legacy_latest", "legacy-latest"),
        ("capabilities_mode_config_latest", "config-latest"),
    ] {
        let config = fake_child_config(fake_mode, "custom", json!({}));
        let capabilities = probe_acp_runtime_capabilities(config, Some("gpt-5.5".to_string()))
            .await
            .expect("mode ordering probe succeeds");

        assert_eq!(capabilities.current_mode.as_deref(), Some(expected_mode));
        assert_eq!(capabilities.modes[0].value, expected_mode);
    }
}

#[tokio::test]
async fn acp_capability_probe_merges_partial_config_updates_by_category() {
    let config = fake_child_config("capabilities_partial", "custom", json!({}));

    let capabilities = probe_acp_runtime_capabilities(config, Some("gpt-5.5".to_string()))
        .await
        .expect("partial capability update succeeds");

    assert_eq!(capabilities.current_model.as_deref(), Some("gpt-5.5"));
    assert_eq!(capabilities.current_mode.as_deref(), Some("auto"));
    assert_eq!(capabilities.current_thinking_effort.as_deref(), Some("low"));
    assert_eq!(capabilities.modes[0].value, "auto");
    assert_eq!(capabilities.thinking_efforts[0].value, "low");
}

#[tokio::test]
async fn acp_capability_probe_applies_latest_current_mode_update() {
    let config = fake_child_config("capabilities_mode_update", "custom", json!({}));

    let capabilities = probe_acp_runtime_capabilities(config, None)
        .await
        .expect("current mode capability update succeeds");

    assert_eq!(capabilities.current_mode.as_deref(), Some("manual"));
    assert!(
        capabilities.modes.iter().any(|mode| mode.value == "manual"),
        "modes: {:?}",
        capabilities.modes
    );
}

#[tokio::test]
async fn acp_capability_probe_uses_isolated_cwd_home_and_minimal_environment() {
    let output = tempfile::tempdir().unwrap();
    let log_path = output.path().join("probe-env.log");
    let config = fake_child_config(
        "capabilities_env",
        "custom",
        json!({ "env": { "ACP_FAKE_LOG": log_path.to_string_lossy() } }),
    );

    probe_acp_runtime_capabilities(config, None)
        .await
        .expect("environment probe succeeds");

    let log = std::fs::read_to_string(log_path).expect("read probe environment log");
    let env_line = log
        .lines()
        .find_map(|line| line.strip_prefix("ACP_FAKE_ENV "))
        .expect("fake child logged its environment");
    let summary: Value = serde_json::from_str(env_line).expect("environment summary is JSON");
    let cwd = summary["cwd"].as_str().expect("cwd present");
    let env = summary["env"].as_object().expect("env present");
    let home = env["HOME"].as_str().expect("isolated HOME present");
    assert!(cwd.contains("qunica-acp-probe-"));
    assert!(home.contains("qunica-acp-probe-"));
    assert_ne!(cwd, home);
    assert_eq!(
        std::path::Path::new(cwd).parent(),
        std::path::Path::new(home).parent()
    );

    let allowed = [
        "PATH",
        "PATHEXT",
        "SYSTEMROOT",
        "WINDIR",
        "COMSPEC",
        "LD_LIBRARY_PATH",
        "DYLD_LIBRARY_PATH",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "TMP",
        "TEMP",
        "TMPDIR",
        "CODEX_HOME",
        "CLAUDE_CONFIG_DIR",
        "CLAUDE_HOME",
        "QUNICA_ACP_AGENT",
        "ACP_FAKE_CHILD_MODE",
        "ACP_FAKE_LOG",
    ];
    for key in env.keys() {
        assert!(
            allowed
                .iter()
                .any(|allowed_key| key.eq_ignore_ascii_case(allowed_key)),
            "unexpected inherited probe environment key: {key}"
        );
    }
}

#[tokio::test]
async fn acp_capability_probe_redacts_reflected_runtime_environment_values() {
    let secret = "TOP_SECRET_CAPABILITY_VALUE";
    let config = fake_child_config(
        "capabilities_reflection",
        "custom",
        json!({ "env": { "ACP_FAKE_SECRET": secret } }),
    );

    let capabilities = probe_acp_runtime_capabilities(config, None)
        .await
        .expect("reflected values are safely redacted");
    let serialized = serde_json::to_string(&capabilities).unwrap();
    assert!(!serialized.contains(secret));
    assert!(capabilities.models.is_empty());
    assert!(capabilities.current_model.is_none());
    assert_eq!(capabilities.modes[0].value, "safe-mode");
    assert_eq!(capabilities.modes[0].label, "safe-mode");
    assert!(capabilities.modes[0].description.is_none());
    assert!(capabilities.current_mode.is_none());
}

#[tokio::test]
async fn acp_capability_probe_backpressures_observation_floods_without_reordering() {
    let config = fake_child_config("capabilities_flood", "custom", json!({}));

    let capabilities = probe_acp_runtime_capabilities(config, None)
        .await
        .expect("bounded observation flood is drained");

    assert_eq!(
        capabilities.current_thinking_effort.as_deref(),
        Some("medium")
    );
    assert_eq!(capabilities.thinking_efforts[0].value, "medium");
}

#[tokio::test]
async fn acp_capability_probe_redacts_protocol_errors() {
    let config = fake_child_config(
        "capabilities_error",
        "custom",
        json!({ "env": { "ACP_FAKE_SECRET": "TOP_SECRET_VALUE" } }),
    );

    let error = probe_acp_runtime_capabilities(config, None)
        .await
        .expect_err("capability probe should fail");

    assert!(matches!(error, AcpCapabilityError::Protocol { .. }));
    assert!(!error.to_string().contains("TOP_SECRET_VALUE"));
}

#[tokio::test]
async fn acp_capability_probe_timeout_reaps_child() {
    let cwd = tempfile::tempdir().unwrap();
    let port_path = cwd.path().join("probe.port");
    let log_path = cwd.path().join("probe.log");
    let config = fake_child_config(
        "capabilities_timeout",
        "custom",
        json!({
            "env": {
                "ACP_FAKE_LOG": log_path.to_string_lossy(),
                "ACP_FAKE_PORT_FILE": port_path.to_string_lossy(),
            }
        }),
    );

    let error = probe_acp_runtime_capabilities(config, None)
        .await
        .expect_err("capability probe should time out");

    assert!(matches!(error, AcpCapabilityError::Timeout));
    let address = std::fs::read_to_string(port_path).expect("fake child wrote listener address");
    let rebound = std::net::TcpListener::bind(address.trim())
        .expect("fake child listener released after timeout cleanup");
    drop(rebound);
    let log = std::fs::read_to_string(log_path).expect("read fake ACP log");
    assert!(log.contains("initialize"));
    assert!(!log.contains("session/prompt"));
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

    let _listener = std::env::var("ACP_FAKE_PORT_FILE").ok().map(|path| {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind fake ACP port");
        std::fs::write(path, listener.local_addr().unwrap().to_string())
            .expect("write fake ACP port");
        listener
    });
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut applied: Vec<Value> = Vec::new();
    let mut new_count = 0;
    let mut prompt_count = 0;
    let mut load_count = 0;
    let mut loaded_session_id = String::new();

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
        append_fake_log(&message.to_string());

        match method {
            "initialize" => match mode {
                "capabilities_timeout" => {}
                "capabilities_error" => {
                    let secret = std::env::var("ACP_FAKE_SECRET").unwrap_or_default();
                    write_line(
                        &stdout,
                        &rpc_error(&id, -32602, &format!("rejected {secret}")),
                    );
                }
                "images" => write_line(
                    &stdout,
                    &rpc_result(
                        &id,
                        json!({
                            "protocolVersion": 1,
                            "agentCapabilities": { "promptCapabilities": { "image": true } },
                        }),
                    ),
                ),
                "resume" | "resume_refused" => write_line(
                    &stdout,
                    &rpc_result(
                        &id,
                        json!({
                            "protocolVersion": 1,
                            "agentCapabilities": { "loadSession": true },
                        }),
                    ),
                ),
                _ => write_line(&stdout, &rpc_result(&id, json!({ "protocolVersion": 1 }))),
            },
            "session/new" => {
                new_count += 1;
                if mode == "capabilities_env" {
                    let summary = json!({
                        "cwd": std::env::current_dir().unwrap(),
                        "env": std::env::vars().collect::<std::collections::BTreeMap<_, _>>(),
                    });
                    append_fake_log(&format!("ACP_FAKE_ENV {summary}"));
                }
                if mode == "capabilities_flood" {
                    for index in 0..256 {
                        let effort = if index % 2 == 0 { "low" } else { "xhigh" };
                        write_line(
                            &stdout,
                            &session_update(json!({
                                "sessionUpdate": "config_option_update",
                                "configOptions": capability_config_options("gpt-5.4", effort),
                            })),
                        );
                    }
                }
                let result = if mode == "capabilities_reflection" {
                    capability_reflection_state(
                        &std::env::var("ACP_FAKE_SECRET").unwrap_or_default(),
                    )
                } else if mode == "capabilities_model_catalog" {
                    capability_model_catalog_state()
                } else if mode == "capabilities_flood" {
                    capability_session_state("gpt-5.4", "medium")
                } else if mode.starts_with("capabilities_") {
                    let model = if mode == "capabilities_initial" {
                        "gpt-5.5"
                    } else {
                        "gpt-5.4"
                    };
                    capability_session_state(model, "low")
                } else {
                    json!({ "sessionId": "sess-fake" })
                };
                write_line(&stdout, &rpc_result(&id, result));
                if mode == "capabilities_mode_update" {
                    write_line(
                        &stdout,
                        &session_update(json!({
                            "sessionUpdate": "current_mode_update",
                            "currentModeId": "manual",
                        })),
                    );
                }
            }
            "session/load" => {
                load_count += 1;
                loaded_session_id = params
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if mode == "resume_refused" {
                    write_line(&stdout, &rpc_error(&id, -32603, "unknown session"));
                    continue;
                }
                // A real agent replays the loaded session's history back at the
                // client before answering the load.
                write_line(
                    &stdout,
                    &session_update(json!({
                        "sessionUpdate": "agent_message_chunk",
                        "content": { "type": "text", "text": "REPLAYED_HISTORY" },
                    })),
                );
                write_line(&stdout, &rpc_result(&id, json!({})));
            }
            "session/set_model" | "session/set_mode" => {
                if mode == "settings"
                    || mode == "capabilities_fallback"
                    || mode == "settings_unsupported"
                    || mode == "settings_rejected"
                {
                    write_line(&stdout, &rpc_error(&id, -32601, "method not found"));
                } else if mode == "capabilities_mode_legacy_latest" {
                    write_line(
                        &stdout,
                        &rpc_result(&id, json!({ "modes": legacy_mode_state("legacy-latest") })),
                    );
                } else if mode == "capabilities_mode_config_latest" {
                    write_line(
                        &stdout,
                        &session_update(json!({
                            "sessionUpdate": "available_modes_update",
                            "modes": legacy_mode_state("legacy-before-config"),
                        })),
                    );
                    write_line(
                        &stdout,
                        &rpc_result(
                            &id,
                            json!({ "configOptions": [mode_config_option("config-latest")] }),
                        ),
                    );
                } else if mode == "capabilities_ordering" && method == "session/set_model" {
                    write_line(
                        &stdout,
                        &session_update(json!({
                            "sessionUpdate": "config_option_update",
                            "configOptions": capability_config_options("gpt-5.5", "xhigh"),
                        })),
                    );
                    write_line(
                        &stdout,
                        &rpc_result(
                            &id,
                            json!({
                                "configOptions": capability_config_options("gpt-5.5", "medium"),
                            }),
                        ),
                    );
                } else if mode == "capabilities_partial" && method == "session/set_model" {
                    let model_option = capability_config_options("gpt-5.5", "xhigh")
                        .into_iter()
                        .next()
                        .unwrap();
                    write_line(
                        &stdout,
                        &rpc_result(&id, json!({ "configOptions": [model_option] })),
                    );
                } else {
                    write_line(&stdout, &rpc_result(&id, json!({})));
                    if mode == "capabilities_update" && method == "session/set_model" {
                        write_line(
                            &stdout,
                            &session_update(json!({
                                "sessionUpdate": "config_option_update",
                                "configOptions": capability_config_options("gpt-5.5", "xhigh"),
                            })),
                        );
                    }
                }
            }
            "session/set_config_option" => {
                applied.push(params);
                // A runtime whose ACP surface is prompt-only answers
                // method-not-found for every setting; one that implements the
                // method but dislikes the id answers invalid-params.
                if mode == "settings_unsupported" {
                    write_line(&stdout, &rpc_error(&id, -32601, "method not found"));
                    continue;
                }
                if mode == "settings_rejected" {
                    write_line(&stdout, &rpc_error(&id, -32602, "unknown config option"));
                    continue;
                }
                let result = if mode == "capabilities_fallback" {
                    json!({
                        "configOptions": capability_config_options("gpt-5.5", "xhigh"),
                    })
                } else {
                    json!({})
                };
                write_line(&stdout, &rpc_result(&id, result));
            }
            "session/prompt" => match mode {
                // Hold the turn open so the parent can time out or cancel it.
                "timeout" | "cancel" => {}
                "env" => {
                    let mut env = serde_json::Map::new();
                    for key in [
                        "QUNICA_ACP_AGENT",
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
                "reuse" | "resume" | "resume_refused" => {
                    prompt_count += 1;
                    let prompt = params
                        .get("prompt")
                        .and_then(Value::as_array)
                        .and_then(|items| items.first())
                        .and_then(|item| item.get("text"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let summary = json!({
                        "new_count": new_count,
                        "prompt_count": prompt_count,
                        "load_count": load_count,
                        "session_id": loaded_session_id,
                        "prompt": prompt,
                    })
                    .to_string();
                    write_line(
                        &stdout,
                        &session_update(json!({
                            "sessionUpdate": "agent_message_chunk",
                            "content": { "type": "text", "text": summary },
                        })),
                    );
                    write_line(
                        &stdout,
                        &rpc_result(&id, json!({ "stopReason": "end_turn" })),
                    );
                }
                "images" | "images_unsupported" => {
                    let prompt = params.get("prompt").cloned().unwrap_or(Value::Null);
                    write_line(
                        &stdout,
                        &session_update(json!({
                            "sessionUpdate": "agent_message_chunk",
                            "content": { "type": "text", "text": prompt.to_string() },
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
    append_fake_log("ACP_FAKE_EXIT");
}

fn capability_session_state(model: &str, effort: &str) -> Value {
    json!({
        "sessionId": "sess-fake",
        "configOptions": capability_config_options(model, effort),
        "modes": {
            "currentModeId": "legacy",
            "availableModes": [{ "id": "legacy", "name": "Legacy" }],
        },
    })
}

fn capability_model_catalog_state() -> Value {
    json!({
        "sessionId": "sess-fake",
        "models": {
            "currentModelId": "gpt-5.6-terra[high]",
            "availableModels": [
                { "modelId": "gpt-5.6-terra[low]", "name": "GPT-5.6-terra (low)" },
                { "modelId": "gpt-5.6-terra[high]", "name": "GPT-5.6-terra (high)" },
            ],
        },
        "configOptions": capability_config_options("gpt-5.5", "low"),
        "modes": {
            "currentModeId": "agent",
            "availableModes": [{ "id": "agent", "name": "Agent" }],
        },
    })
}

fn capability_reflection_state(secret: &str) -> Value {
    json!({
        "sessionId": "sess-fake",
        "configOptions": [
            {
                "id": "model",
                "category": "model",
                "type": "select",
                "currentValue": secret,
                "options": [{ "value": secret, "name": "reflected model" }],
            },
            {
                "id": "approval_preset",
                "type": "select",
                "currentValue": secret,
                "options": [{
                    "value": "safe-mode",
                    "name": format!("Mode {secret}"),
                    "description": format!("Description {secret}"),
                }],
            },
            {
                "id": "reasoning_effort",
                "category": "thought_level",
                "type": "select",
                "currentValue": "safe-effort",
                "options": [{ "value": "safe-effort", "name": "Safe effort" }],
            },
        ],
    })
}

fn legacy_mode_state(mode: &str) -> Value {
    json!({
        "currentModeId": mode,
        "availableModes": [{ "id": mode, "name": mode }],
    })
}

fn mode_config_option(mode: &str) -> Value {
    json!({
        "id": "approval_preset",
        "type": "select",
        "currentValue": mode,
        "options": [{ "value": mode, "name": mode }],
    })
}

fn capability_config_options(model: &str, effort: &str) -> Vec<Value> {
    let effort_options = match effort {
        "xhigh" => vec![json!({ "value": "xhigh", "name": "XHigh" })],
        "medium" => vec![json!({ "value": "medium", "name": "Medium" })],
        _ => vec![json!({ "value": "low", "name": "Low" })],
    };
    vec![
        json!({
            "id": "adapter_model_selector",
            "name": "Model",
            "category": "model",
            "type": "select",
            "currentValue": model,
            "options": [
                { "value": "gpt-5.5", "name": "GPT-5.5", "description": "Primary model" },
                { "value": "gpt-5.4", "name": "GPT-5.4" },
            ],
        }),
        json!({
            "id": "approval_preset",
            "name": "Mode",
            "type": "select",
            "currentValue": "auto",
            "options": [
                { "value": "auto", "name": "Default" },
                { "value": "manual", "name": "Manual" },
            ],
        }),
        json!({
            "id": "adapter_reasoning_selector",
            "name": "Thinking",
            "category": "thought_level",
            "type": "select",
            "currentValue": effort,
            "options": effort_options,
        }),
    ]
}

fn append_fake_log(line: &str) {
    use std::io::Write;

    let Ok(path) = std::env::var("ACP_FAKE_LOG") else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{line}");
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
