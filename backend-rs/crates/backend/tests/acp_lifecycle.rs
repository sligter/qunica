//! ACP runtime config + audit foundation tests (Task 9a).
//!
//! Config tests are pure (no I/O). Audit tests run against a migrated in-memory
//! SQLite database with the minimal FK rows seeded directly through the pool.
//! No external commands or child processes are spawned in this slice.

use ag_swarmer_backend::acp::{
    normalize_acp_runtime, AcpConfigValue, AcpRunAudit, AcpRunContext, AcpRuntimeProfile,
    PermissionPolicy, BLOCKED_ENV_KEYS, DEFAULT_TIMEOUT_SECONDS, MAX_TAIL_CHARS,
    MAX_TIMEOUT_SECONDS,
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
    assert_eq!(config.env.get("MY_AGENT_TOKEN").map(String::as_str), Some("abc"));
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

    sqlx::query(
        "INSERT INTO threads (id, group_id, created_at, updated_at) VALUES (?, ?, ?, ?)",
    )
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
    assert_eq!(row.stdout_tail.as_ref().unwrap().chars().count(), MAX_TAIL_CHARS);
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
