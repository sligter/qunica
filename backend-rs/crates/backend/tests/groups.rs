use ag_swarmer_backend::api::{router_with_state_for_tests, AppState};
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use tower::ServiceExt;

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

/// Register and log in a user, returning a bearer token.
async fn register_and_login(app: &Router, email: &str) -> String {
    let (status, _) = send(
        app,
        post_json(
            "/api/v2/auth/register",
            json!({"email": email, "password": "supersecret", "name": "Tester"}),
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

    let rows = vec![
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

    let rows = vec![
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

    let rows = vec![
        group_agent_row(&state, group_id, &agent_a).await,
        group_agent_row(&state, group_id, &agent_b).await,
    ];
    assert!(rows.iter().all(|row| row.topology_role.is_none()));
    assert!(rows.iter().all(|row| row.speaking_order.is_none()));
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
