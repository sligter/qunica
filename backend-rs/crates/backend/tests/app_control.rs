//! App-control tool tests: the read surface the built-in Assistant uses to
//! inspect the user's configuration.
//!
//! These run the tools directly against a migrated in-memory database. The
//! properties that matter are owner scoping (no cross-tenant reads, by id or by
//! list) and secret containment (no API key or MCP header value reaches the
//! model, at any depth, in any field).

use ag_swarmer_backend::{
    api::router_with_state_for_tests,
    tools::{AppControlContext, ToolExecutor, ToolResult, ToolStatus},
};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use uuid::Uuid;

const SECRET_KEY: &str = "sk-super-secret-do-not-leak";
const SECRET_HEADER: &str = "Bearer tok-do-not-leak";

async fn execute(executor: &ToolExecutor, name: &str, args: Value) -> ToolResult {
    executor.execute(name, args).await
}

fn parsed(result: &ToolResult) -> Value {
    serde_json::from_str(&result.output)
        .unwrap_or_else(|error| panic!("tool output is not JSON ({error}): {}", result.output))
}

async fn seed_user(pool: &SqlitePool, email: &str) -> String {
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, name, avatar_url, created_at, updated_at) \
         VALUES (?, ?, 'x', 'Tester', NULL, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .bind(&id)
    .bind(email)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn seed_workspace(pool: &SqlitePool, owner_id: &str, name: &str) -> String {
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO workspaces (id, owner_id, name, backend_type, local_path, status, created_at, updated_at) \
         VALUES (?, ?, ?, 'local', 'C:/tmp/ws', 'active', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .bind(&id)
    .bind(owner_id)
    .bind(name)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn seed_provider(pool: &SqlitePool, owner_id: &str, name: &str) -> String {
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO llm_providers \
         (id, owner_id, name, kind, base_url, api_key, default_model, status, created_at, updated_at) \
         VALUES (?, ?, ?, 'openai-compatible', 'https://example.invalid/v1', ?, 'gpt-4o-mini', \
                 'active', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .bind(&id)
    .bind(owner_id)
    .bind(name)
    .bind(SECRET_KEY)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn seed_agent(pool: &SqlitePool, owner_id: &str, workspace_id: &str, name: &str) -> String {
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO agents \
         (id, owner_id, workspace_id, name, system_prompt, runtime_kind, skill_ids_json, status, \
          is_system, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 'You are helpful.', 'llm_chat', '[]', 'active', 0, \
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .bind(&id)
    .bind(owner_id)
    .bind(workspace_id)
    .bind(name)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn seed_mcp_server(pool: &SqlitePool, owner_id: &str, name: &str) -> String {
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO mcp_servers \
         (id, owner_id, name, transport, url, headers_json, timeout_seconds, status, created_at, updated_at) \
         VALUES (?, ?, ?, 'http', 'https://mcp.invalid/rpc', ?, 60, 'active', \
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .bind(&id)
    .bind(owner_id)
    .bind(name)
    .bind(json!({ "Authorization": SECRET_HEADER }).to_string())
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn seed_skill(pool: &SqlitePool, owner_id: &str, name: &str) -> String {
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO skills (id, owner_id, name, description, body_markdown, status, created_at, updated_at) \
         VALUES (?, ?, ?, 'A skill', '# Body', 'active', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .bind(&id)
    .bind(owner_id)
    .bind(name)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn seed_group(pool: &SqlitePool, owner_id: &str, name: &str) -> String {
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO groups (id, owner_id, name, conversation_kind, status, created_at, updated_at) \
         VALUES (?, ?, ?, 'group', 'active', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .bind(&id)
    .bind(owner_id)
    .bind(name)
    .execute(pool)
    .await
    .unwrap();
    id
}

fn executor_for(pool: &SqlitePool, owner_id: &str) -> ToolExecutor {
    ToolExecutor::without_workspace().with_app_control(AppControlContext::new(
        pool.clone(),
        owner_id.to_string(),
        Uuid::new_v4().to_string(),
    ))
}

#[tokio::test]
async fn app_control_lists_every_kind_scoped_to_the_owner() {
    let (_app, state) = router_with_state_for_tests().await;
    let pool = state.db.pool();
    let owner = seed_user(pool, "app-list@example.com").await;
    let stranger = seed_user(pool, "app-list-other@example.com").await;

    let workspace = seed_workspace(pool, &owner, "Mine").await;
    seed_agent(pool, &owner, &workspace, "Researcher").await;
    seed_provider(pool, &owner, "Primary").await;
    seed_mcp_server(pool, &owner, "Weather").await;
    seed_skill(pool, &owner, "Review").await;
    seed_group(pool, &owner, "Team").await;

    // The stranger's rows must never appear in the owner's listings.
    let stranger_workspace = seed_workspace(pool, &stranger, "Theirs").await;
    seed_agent(pool, &stranger, &stranger_workspace, "TheirAgent").await;
    seed_provider(pool, &stranger, "TheirProvider").await;

    let executor = executor_for(pool, &owner);
    for kind in [
        "agent",
        "provider",
        "mcp",
        "skill",
        "workspace",
        "group",
        "group_template",
        "group_note",
        "chat",
    ] {
        let result = execute(&executor, "AppList", json!({ "kind": kind })).await;
        assert_eq!(
            result.status,
            ToolStatus::Completed,
            "kind {kind}: {}",
            result.output
        );
        let value = parsed(&result);
        assert_eq!(value["kind"], kind);
        assert!(value["items"].is_array(), "kind {kind}: {value}");
        assert!(
            !result.output.contains("Theirs"),
            "kind {kind} leaked another owner's workspace"
        );
        assert!(
            !result.output.contains("TheirAgent") && !result.output.contains("TheirProvider"),
            "kind {kind} leaked another owner's row"
        );
    }
}

#[tokio::test]
async fn app_control_reads_group_templates_and_current_shared_note_content() {
    let (_app, state) = router_with_state_for_tests().await;
    let pool = state.db.pool();
    let owner = seed_user(pool, "app-group-resources@example.com").await;
    let stranger = seed_user(pool, "app-group-resources-other@example.com").await;
    let root = tempfile::tempdir().unwrap();
    let workspace = seed_workspace(pool, &owner, "Local").await;
    sqlx::query("UPDATE workspaces SET local_path = ? WHERE id = ?")
        .bind(root.path().to_string_lossy().as_ref())
        .bind(&workspace)
        .execute(pool)
        .await
        .unwrap();
    let group = seed_group(pool, &owner, "Team").await;
    sqlx::query("UPDATE groups SET workspace_id = ? WHERE id = ?")
        .bind(&workspace)
        .bind(&group)
        .execute(pool)
        .await
        .unwrap();

    let template = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO group_templates (id, owner_id, name, config_json, created_at, updated_at) \
         VALUES (?, ?, 'Review team', '{\"free_speech\":true}', \
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .bind(&template)
    .bind(&owner)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO group_templates (id, owner_id, name, config_json, created_at, updated_at) \
         VALUES (?, ?, 'Their template', '{}', \
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&stranger)
    .execute(pool)
    .await
    .unwrap();

    let note = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO group_notes \
         (id, group_id, author_id, title, content, status, created_at, updated_at) \
         VALUES (?, ?, ?, 'Plan', 'stale database fallback', 'active', \
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .bind(&note)
    .bind(&group)
    .bind(&owner)
    .execute(pool)
    .await
    .unwrap();
    std::fs::create_dir_all(root.path().join("Notes")).unwrap();
    std::fs::write(
        root.path().join("Notes").join(format!("{note}.md")),
        "fresh file content",
    )
    .unwrap();

    let executor = executor_for(pool, &owner);
    let templates = execute(&executor, "AppList", json!({"kind": "group_template"})).await;
    assert!(templates.output.contains("Review team"));
    assert!(!templates.output.contains("Their template"));
    let template_detail = execute(
        &executor,
        "AppGet",
        json!({"kind": "group_template", "id": template}),
    )
    .await;
    assert_eq!(
        parsed(&template_detail)["item"]["config"]["free_speech"],
        true
    );

    let notes = execute(&executor, "AppList", json!({"kind": "group_note"})).await;
    assert!(notes.output.contains("Plan"));
    let note_detail = execute(
        &executor,
        "AppGet",
        json!({"kind": "group_note", "id": note}),
    )
    .await;
    assert_eq!(
        parsed(&note_detail)["item"]["content"],
        "fresh file content"
    );
}

#[tokio::test]
async fn app_control_rejects_an_unknown_kind() {
    let (_app, state) = router_with_state_for_tests().await;
    let pool = state.db.pool();
    let owner = seed_user(pool, "app-bad-kind@example.com").await;
    let executor = executor_for(pool, &owner);

    let result = execute(&executor, "AppList", json!({ "kind": "password" })).await;
    assert_eq!(result.status, ToolStatus::Failed);
    assert!(result.output.contains("kind"), "{}", result.output);
}

#[tokio::test]
async fn app_control_cannot_read_another_owners_row_by_id() {
    let (_app, state) = router_with_state_for_tests().await;
    let pool = state.db.pool();
    let owner = seed_user(pool, "app-get-owner@example.com").await;
    let stranger = seed_user(pool, "app-get-stranger@example.com").await;
    let stranger_provider = seed_provider(pool, &stranger, "TheirProvider").await;

    let executor = executor_for(pool, &owner);
    let result = execute(
        &executor,
        "AppGet",
        json!({"kind": "provider", "id": stranger_provider}),
    )
    .await;

    assert_eq!(result.status, ToolStatus::Failed);
    assert!(!result.output.contains(SECRET_KEY));
    assert!(!result.output.contains("TheirProvider"));
}

#[tokio::test]
async fn app_control_never_returns_provider_or_mcp_secrets() {
    let (_app, state) = router_with_state_for_tests().await;
    let pool = state.db.pool();
    let owner = seed_user(pool, "app-secrets@example.com").await;
    let provider = seed_provider(pool, &owner, "Primary").await;
    let server = seed_mcp_server(pool, &owner, "Weather").await;
    let executor = executor_for(pool, &owner);

    let result = execute(
        &executor,
        "AppGet",
        json!({"kind": "provider", "id": provider}),
    )
    .await;
    assert_eq!(result.status, ToolStatus::Completed, "{}", result.output);
    assert!(
        !result.output.contains(SECRET_KEY),
        "provider api key leaked: {}",
        result.output
    );
    assert_eq!(parsed(&result)["item"]["api_key_configured"], json!(true));

    let result = execute(&executor, "AppGet", json!({"kind": "mcp", "id": server})).await;
    assert_eq!(result.status, ToolStatus::Completed, "{}", result.output);
    assert!(
        !result.output.contains("tok-do-not-leak"),
        "mcp header value leaked: {}",
        result.output
    );
    // Header *names* are safe and useful; the values are not.
    assert_eq!(
        parsed(&result)["item"]["header_names"],
        json!(["Authorization"])
    );

    // The list surface must hold the same line as the detail surface.
    let result = execute(&executor, "AppList", json!({"kind": "provider"})).await;
    assert!(!result.output.contains(SECRET_KEY));
    let result = execute(&executor, "AppList", json!({"kind": "mcp"})).await;
    assert!(!result.output.contains("tok-do-not-leak"));
}

#[tokio::test]
async fn app_state_reports_what_first_run_setup_is_missing() {
    let (_app, state) = router_with_state_for_tests().await;
    let pool = state.db.pool();
    let owner = seed_user(pool, "app-state@example.com").await;
    let executor = executor_for(pool, &owner);

    let result = execute(&executor, "AppState", json!({})).await;
    assert_eq!(result.status, ToolStatus::Completed, "{}", result.output);
    let value = parsed(&result);
    assert_eq!(value["has_provider"], json!(false));
    assert_eq!(value["has_workspace"], json!(false));
    assert_eq!(value["has_agent"], json!(false));

    let workspace = seed_workspace(pool, &owner, "Mine").await;
    seed_provider(pool, &owner, "Primary").await;
    seed_agent(pool, &owner, &workspace, "Researcher").await;

    let value = parsed(&execute(&executor, "AppState", json!({})).await);
    assert_eq!(value["has_provider"], json!(true));
    assert_eq!(value["has_workspace"], json!(true));
    assert_eq!(value["has_agent"], json!(true));
    assert_eq!(value["counts"]["agent"], json!(1));
}

#[tokio::test]
async fn app_control_tools_report_setup_required_without_a_context() {
    // A regular agent has no app-control context. The tools must degrade to a
    // controlled result rather than panicking or reaching the database.
    let executor = ToolExecutor::without_workspace();
    for (name, args) in [
        ("AppList", json!({"kind": "agent"})),
        ("AppGet", json!({"kind": "agent", "id": "x"})),
        ("AppState", json!({})),
    ] {
        let result = execute(&executor, name, args).await;
        assert_eq!(result.status, ToolStatus::SetupRequired, "{name}");
    }
}

#[tokio::test]
async fn app_control_hides_the_assistant_from_agent_listings() {
    let (_app, state) = router_with_state_for_tests().await;
    let pool = state.db.pool();
    let owner = seed_user(pool, "app-list-assistant@example.com").await;
    let workspace = seed_workspace(pool, &owner, "Mine").await;
    seed_agent(pool, &owner, &workspace, "Researcher").await;
    sqlx::query(
        "INSERT INTO agents \
         (id, owner_id, workspace_id, name, system_prompt, runtime_kind, skill_ids_json, status, \
          is_system, created_at, updated_at) \
         VALUES (?, ?, NULL, 'AG Assistant', 'p', 'llm_chat', '[]', 'active', 1, \
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&owner)
    .execute(pool)
    .await
    .unwrap();

    let executor = executor_for(pool, &owner);
    let result = execute(&executor, "AppList", json!({"kind": "agent"})).await;

    // The Assistant reporting on itself as a configurable agent invites it to
    // propose changes to its own tools.
    assert!(!result.output.contains("AG Assistant"), "{}", result.output);
    assert!(result.output.contains("Researcher"));
}

// ---------------------------------------------------------------------------
// AppDocs: the bundled usage guide
// ---------------------------------------------------------------------------

#[test]
fn every_bundled_doc_has_a_title_and_a_body() {
    let docs = ag_swarmer_backend::docs::all();
    assert!(!docs.is_empty());
    for doc in docs {
        assert!(!doc.slug.is_empty());
        assert!(!doc.title.is_empty(), "{} has no title", doc.slug);
        assert!(!doc.body.trim().is_empty(), "{} has no body", doc.slug);
    }
    // Slugs address documents in tool output; a duplicate would make one
    // unreachable.
    let mut slugs: Vec<&str> = docs.iter().map(|doc| doc.slug).collect();
    slugs.sort_unstable();
    let count = slugs.len();
    slugs.dedup();
    assert_eq!(slugs.len(), count, "duplicate doc slug");
}

#[tokio::test]
async fn app_docs_finds_a_real_feature_and_stays_bounded() {
    let (_app, state) = router_with_state_for_tests().await;
    let pool = state.db.pool();
    let owner = seed_user(pool, "app-docs@example.com").await;
    let executor = executor_for(pool, &owner);

    let result = execute(&executor, "AppDocs", json!({"query": "mcp stdio server"})).await;
    assert_eq!(result.status, ToolStatus::Completed, "{}", result.output);
    assert!(
        result.output.contains("mcp-servers"),
        "expected the mcp-servers doc: {}",
        result.output
    );
    assert!(
        result.output.len() <= ag_swarmer_backend::docs::MAX_DOCS_OUTPUT_BYTES,
        "output was {} bytes",
        result.output.len()
    );
}

#[tokio::test]
async fn app_docs_can_return_one_document_whole() {
    let (_app, state) = router_with_state_for_tests().await;
    let pool = state.db.pool();
    let owner = seed_user(pool, "app-docs-slug@example.com").await;
    let executor = executor_for(pool, &owner);

    let result = execute(&executor, "AppDocs", json!({"slug": "getting-started"})).await;
    assert_eq!(result.status, ToolStatus::Completed, "{}", result.output);
    assert_eq!(parsed(&result)["documents"][0]["slug"], "getting-started");

    let result = execute(&executor, "AppDocs", json!({"slug": "no-such-page"})).await;
    assert_eq!(result.status, ToolStatus::Failed);
}

#[tokio::test]
async fn app_docs_says_so_rather_than_returning_an_arbitrary_page() {
    let (_app, state) = router_with_state_for_tests().await;
    let pool = state.db.pool();
    let owner = seed_user(pool, "app-docs-miss@example.com").await;
    let executor = executor_for(pool, &owner);

    // An unmatched query must not hand back whichever doc scored least badly:
    // the model would present it as the answer.
    let result = execute(
        &executor,
        "AppDocs",
        json!({"query": "kubernetes helm chart ingress"}),
    )
    .await;
    assert_eq!(result.status, ToolStatus::Completed, "{}", result.output);
    let value = parsed(&result);
    assert_eq!(value["documents"].as_array().unwrap().len(), 0);
    assert!(
        value["message"]
            .as_str()
            .unwrap_or_default()
            .contains("no matching"),
        "{value}"
    );
    // The index is still offered so the model can pick a page by name.
    assert!(value["available"].is_array());
}

#[tokio::test]
async fn app_docs_requires_a_query_or_slug() {
    let (_app, state) = router_with_state_for_tests().await;
    let pool = state.db.pool();
    let owner = seed_user(pool, "app-docs-empty@example.com").await;
    let executor = executor_for(pool, &owner);

    let result = execute(&executor, "AppDocs", json!({})).await;
    assert_eq!(result.status, ToolStatus::Failed);
}

#[tokio::test]
async fn app_state_reports_whether_auto_created_workspaces_are_possible() {
    let (_app, state) = router_with_state_for_tests().await;
    let pool = state.db.pool();
    let owner = seed_user(pool, "app-state-root@example.com").await;
    let executor = executor_for(pool, &owner);

    // `auto_create` needs a configured root. Without knowing that, the
    // Assistant proposes a workspace that fails on approval with an error it
    // cannot explain.
    let value = parsed(&execute(&executor, "AppState", json!({})).await);
    assert_eq!(value["can_auto_create_workspace"], json!(false));

    sqlx::query(
        "INSERT INTO system_settings (id, owner_id, group_workspace_root, created_at, updated_at)          VALUES (?, ?, 'C:/tmp/roots', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&owner)
    .execute(pool)
    .await
    .unwrap();

    let value = parsed(&execute(&executor, "AppState", json!({})).await);
    assert_eq!(value["can_auto_create_workspace"], json!(true));
}
