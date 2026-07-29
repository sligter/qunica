use std::io::{Cursor, Write};

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use tower::ServiceExt;
use zip::{write::SimpleFileOptions, ZipWriter};

async fn app() -> Router {
    ag_swarmer_backend::api::router_for_tests().await
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

async fn create_manual_skill(app: &Router, token: &str, name: &str) -> Value {
    let (status, skill) = send(
        app,
        authed_json(
            "POST",
            "/api/v2/skills",
            token,
            json!({
                "name": name,
                "description": "first draft",
                "body_markdown": "# Instructions\nDo the thing.",
                "metadata": {"level": "basic"}
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    skill
}

async fn import_zip_skill(app: &Router, token: &str) -> Value {
    let zip = zip_bytes(&[
        (
            "skill-pack/SKILL.md",
            "---\nname: Zip Skill\ndescription: packaged\nkind: demo\n---\nUse the references.",
        ),
        ("skill-pack/references/readme.md", "hello from a reference"),
        ("skill-pack/assets/data.txt", "asset text"),
    ]);
    let (status, skill) = send(
        app,
        authed_json(
            "POST",
            "/api/v2/skills/import-package",
            token,
            json!({"filename": "skill.zip", "content_base64": STANDARD.encode(zip)}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    skill
}

fn zip_bytes(entries: &[(&str, &str)]) -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    for (path, content) in entries {
        writer
            .start_file(*path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(content.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn codeload_zip_bytes(entries: &[(&str, Option<&str>)]) -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    for (path, content) in entries {
        match content {
            Some(content) => {
                writer
                    .start_file(*path, SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(content.as_bytes()).unwrap();
            }
            None => writer
                .add_directory(*path, SimpleFileOptions::default())
                .unwrap(),
        }
    }
    writer.finish().unwrap().into_inner()
}

#[tokio::test]
async fn skills_manual_crud_is_owner_scoped() {
    let app = app().await;
    let token_a = register_and_login(&app, "skills-crud-a@example.com").await;
    let token_b = register_and_login(&app, "skills-crud-b@example.com").await;

    let skill = create_manual_skill(&app, &token_a, "Manual Skill").await;
    let skill_id = skill["id"].as_str().unwrap().to_string();
    assert_eq!(skill["name"], "Manual Skill");
    assert_eq!(skill["source"], "manual");
    assert_eq!(skill["files"], json!([]));

    let (status, list) = send(&app, authed("GET", "/api/v2/skills", &token_a)).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|skill| skill["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&skill_id.as_str()));

    let (status, fetched) = send(
        &app,
        authed("GET", &format!("/api/v2/skills/{skill_id}"), &token_a),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["id"], skill_id);

    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/skills/{skill_id}"),
            &token_a,
            json!({
                "name": "Manual Skill v2",
                "description": Value::Null,
                "body_markdown": "# Updated\nUse this.",
                "metadata": {"level": "advanced"}
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["name"], "Manual Skill v2");
    assert_eq!(updated["description"], Value::Null);
    assert_eq!(updated["metadata"], json!({"level": "advanced"}));

    let (status, body) = send(
        &app,
        authed("GET", &format!("/api/v2/skills/{skill_id}"), &token_b),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");

    let (status, body) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/skills/{skill_id}"),
            &token_b,
            json!({"name": "stolen"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");

    let (status, body) = send(
        &app,
        authed("DELETE", &format!("/api/v2/skills/{skill_id}"), &token_b),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "permission_denied");

    let (status, body) = send(
        &app,
        authed("DELETE", &format!("/api/v2/skills/{skill_id}"), &token_a),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);

    let (status, body) = send(
        &app,
        authed("GET", &format!("/api/v2/skills/{skill_id}"), &token_a),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
}

#[tokio::test]
async fn skills_import_raw_markdown_parses_metadata_and_body() {
    let app = app().await;
    let token = register_and_login(&app, "skills-raw@example.com").await;

    let raw = "---\nname: Research Skill\ndescription: Reads docs\ntags:\n  - docs\npriority: 2\n---\n# Body\nFollow the citation rules.";
    let (status, skill) = send(
        &app,
        authed_json("POST", "/api/v2/skills/import", &token, json!({"raw": raw})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(skill["name"], "Research Skill");
    assert_eq!(skill["description"], "Reads docs");
    assert_eq!(skill["source"], "markdown");
    assert_eq!(skill["body_markdown"], "# Body\nFollow the citation rules.");
    assert_eq!(skill["metadata"]["tags"], json!(["docs"]));
    assert_eq!(skill["metadata"]["priority"], json!(2));
}

#[tokio::test]
async fn skills_import_zip_rejects_traversal_and_persists_resources() {
    let app = app().await;
    let token = register_and_login(&app, "skills-zip@example.com").await;

    let unsafe_zip = zip_bytes(&[
        (
            "SKILL.md",
            "---\nname: Unsafe\ndescription: bad\n---\nBody text",
        ),
        ("../payload.txt", "escape"),
    ]);
    let (status, body) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/skills/import-package",
            &token,
            json!({"content_base64": STANDARD.encode(unsafe_zip)}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_input");

    let skill = import_zip_skill(&app, &token).await;
    let skill_id = skill["id"].as_str().unwrap().to_string();
    assert_eq!(skill["source"], "package");
    assert_eq!(skill["name"], "Zip Skill");
    assert!(skill["storage_path"].as_str().unwrap().contains(&skill_id));
    assert!(skill["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file["path"] == "references/readme.md"));

    let (status, resources) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/skills/{skill_id}/resources"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(resources
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["path"] == "references/readme.md"
            && resource["category"] == "references"
            && resource["is_text"] == true));

    let (status, resource) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/skills/{skill_id}/resources/references/readme.md"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resource["path"], "references/readme.md");
    assert_eq!(resource["content"], "hello from a reference");
    assert_eq!(resource["is_text"], true);
}

#[tokio::test]
async fn skills_import_package_accepts_safe_directory_entries() {
    let app = app().await;
    let token = register_and_login(&app, "skills-dir-zip@example.com").await;
    let zip = codeload_zip_bytes(&[
        ("skill-pack/", None),
        (
            "skill-pack/SKILL.md",
            Some("---\nname: Directory Zip\ndescription: packaged\n---\nUse the references."),
        ),
        ("skill-pack/references/", None),
        (
            "skill-pack/references/readme.md",
            Some("directory entry survived"),
        ),
    ]);

    let (status, skill) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/skills/import-package",
            &token,
            json!({"filename": "skill.zip", "content_base64": STANDARD.encode(zip)}),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(skill["name"], "Directory Zip");
    assert!(skill["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file["path"] == "references/readme.md"));
}

#[tokio::test]
async fn skills_import_package_accepts_a_nested_skill_root() {
    let app = app().await;
    let token = register_and_login(&app, "skills-nested-zip@example.com").await;
    let zip = zip_bytes(&[
        (
            "skills/academic-pptx/SKILL.md",
            "---\nname: academic-pptx\ndescription: Academic presentations. Triggers include: conference talk.\n---\nUse the bundled guidance.",
        ),
        (
            "skills/academic-pptx/content_guidelines.md",
            "Build a clear argument.",
        ),
        ("skills/academic-pptx/assets/example.txt", "example"),
    ]);

    let (status, skill) = send(
        &app,
        authed_json(
            "POST",
            "/api/v2/skills/import-package",
            &token,
            json!({"filename": "academic-pptx.zip", "content_base64": STANDARD.encode(zip)}),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(skill["name"], "academic-pptx");
    assert_eq!(
        skill["description"],
        "Academic presentations. Triggers include: conference talk."
    );
    assert!(skill["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file["path"] == "content_guidelines.md"));
}

#[tokio::test]
async fn skills_resource_update_is_path_safe_and_updates_size() {
    let app = app().await;
    let token = register_and_login(&app, "skills-resource-update@example.com").await;
    let skill = import_zip_skill(&app, &token).await;
    let skill_id = skill["id"].as_str().unwrap().to_string();

    for bad_path in [
        "../references/readme.md",
        "/absolute/path.txt",
        "C:%5Ctemp%5Csecret.txt",
        "%5C%5Cserver%5Cshare%5Csecret.txt",
    ] {
        let (status, _) = send(
            &app,
            authed_json(
                "PATCH",
                &format!("/api/v2/skills/{skill_id}/resources/{bad_path}"),
                &token,
                json!({"content": "bad"}),
            ),
        )
        .await;
        assert!(status.is_client_error(), "{bad_path} returned {status}");
    }

    let updated_content = "new reference body";
    let (status, updated) = send(
        &app,
        authed_json(
            "PATCH",
            &format!("/api/v2/skills/{skill_id}/resources/references/readme.md"),
            &token,
            json!({"content": updated_content}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["content"], updated_content);
    assert_eq!(updated["size"], updated_content.len() as u64);

    let (status, fetched) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/skills/{skill_id}/resources/references/readme.md"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["content"], updated_content);
    assert_eq!(fetched["size"], updated_content.len() as u64);
}

mod skills {
    use super::*;

    #[tokio::test]
    async fn deleting_skill_prunes_agent_skill_ids() {
        let app = app().await;
        let token = register_and_login(&app, "skills-prune@example.com").await;
        let workspace = create_workspace(&app, &token).await;
        let skill_a = create_manual_skill(&app, &token, "Skill A").await;
        let skill_b = create_manual_skill(&app, &token, "Skill B").await;
        let skill_a_id = skill_a["id"].as_str().unwrap().to_string();
        let skill_b_id = skill_b["id"].as_str().unwrap().to_string();

        let (status, agent) = send(
            &app,
            authed_json(
                "POST",
                "/api/v2/agents",
                &token,
                json!({
                    "name": "Agent",
                    "workspace_id": workspace,
                    "skill_ids": [skill_a_id, skill_b_id],
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let agent_id = agent["id"].as_str().unwrap().to_string();

        let (status, _) = send(
            &app,
            authed("DELETE", &format!("/api/v2/skills/{skill_a_id}"), &token),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (status, agent) = send(
            &app,
            authed("GET", &format!("/api/v2/agents/{agent_id}"), &token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(agent["skill_ids"], json!([skill_b_id]));
    }
}
