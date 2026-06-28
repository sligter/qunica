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
            json!({"agent_id": agent, "share_group_workspace": true}),
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
