//! SkillManager runtime tests.
//!
//! Every test name contains `skill_manager` so
//! `cargo test --workspace skill_manager` selects this focused suite.

use ag_swarmer_backend::tools::{MountedSkill, ToolExecutor, ToolStatus};
use serde_json::{json, Value};

fn mounted_skills() -> Vec<MountedSkill> {
    vec![
        MountedSkill {
            name: "citations".to_string(),
            description: Some("Adds citation discipline".to_string()),
            metadata: json!({
                "activation": "manual",
                "version": "1.0.0",
            }),
            body_markdown: "# Citation Instructions\nUse source-backed claims.".to_string(),
        },
        MountedSkill {
            name: "summarizer".to_string(),
            description: None,
            metadata: json!({
                "trigger": "long documents",
                "tools": ["read"],
            }),
            body_markdown: "# Summarizer Instructions\nCompress without losing decisions."
                .to_string(),
        },
    ]
}

fn parse_output(result: &ag_swarmer_backend::tools::ToolResult) -> Value {
    serde_json::from_str(&result.output).unwrap()
}

#[tokio::test]
async fn skill_manager_lists_metadata_without_instructions() {
    let executor = ToolExecutor::without_workspace_with_skills(mounted_skills());

    let result = executor.execute("SkillManager", json!({})).await;

    assert_eq!(result.status, ToolStatus::Completed);
    let payload = parse_output(&result);
    assert_eq!(payload["tool"], "SkillManager");
    assert_eq!(payload["status"], "COMPLETED");
    assert!(payload["message"]
        .as_str()
        .unwrap()
        .contains("inspect or activate"));

    let skills = payload["skills"].as_array().unwrap();
    assert_eq!(skills.len(), 2);
    assert_eq!(skills[0]["name"], "citations");
    assert_eq!(skills[0]["description"], "Adds citation discipline");
    assert_eq!(skills[0]["metadata"]["activation"], "manual");
    assert_eq!(skills[1]["name"], "summarizer");
    assert_eq!(skills[1]["description"], Value::Null);
    assert_eq!(skills[1]["metadata"]["trigger"], "long documents");

    for skill in skills {
        let object = skill.as_object().unwrap();
        assert!(!object.contains_key("instructions"));
        assert!(!object.contains_key("body_markdown"));
    }
    assert!(!result.output.contains("Use source-backed claims."));
    assert!(!result.output.contains("Compress without losing decisions."));
}

#[tokio::test]
async fn skill_manager_inspect_and_activate_include_instructions_without_execution() {
    let executor = ToolExecutor::without_workspace_with_skills(mounted_skills());

    for action in ["inspect", "activate"] {
        let result = executor
            .execute(
                "SkillManager",
                json!({ "action": action, "skill_name": "citations" }),
            )
            .await;

        assert_eq!(result.status, ToolStatus::Completed, "{action}");
        let payload = parse_output(&result);
        assert_eq!(payload["tool"], "SkillManager");
        assert_eq!(payload["status"], "COMPLETED");
        assert_eq!(
            payload["message"],
            "Skill runtime activation records intent only; no arbitrary code was loaded."
        );
        let skills = payload["skills"].as_array().unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0]["name"], "citations");
        assert_eq!(
            skills[0]["instructions"],
            "# Citation Instructions\nUse source-backed claims."
        );
        assert!(!result.output.contains("body_markdown"));
    }
}

#[tokio::test]
async fn skill_manager_missing_skill_returns_not_found() {
    let executor = ToolExecutor::without_workspace_with_skills(mounted_skills());

    let result = executor
        .execute(
            "SkillManager",
            json!({ "action": "inspect", "skill_name": "missing" }),
        )
        .await;

    assert_eq!(result.status, ToolStatus::Failed);
    let payload = parse_output(&result);
    assert_eq!(payload["tool"], "SkillManager");
    assert_eq!(payload["status"], "NOT_FOUND");
    assert_eq!(payload["skills"], json!([]));
}

#[tokio::test]
async fn skill_manager_rejects_unsupported_action() {
    let executor = ToolExecutor::without_workspace_with_skills(mounted_skills());

    let result = executor
        .execute(
            "SkillManager",
            json!({ "action": "install", "skill_name": "citations" }),
        )
        .await;

    assert_eq!(result.status, ToolStatus::Failed);
    let payload = parse_output(&result);
    assert_eq!(payload["tool"], "SkillManager");
    assert_eq!(payload["status"], "FAILED");
    assert!(payload["message"]
        .as_str()
        .unwrap()
        .contains("Unsupported skill action: install"));
}

#[tokio::test]
async fn skill_manager_existing_executor_constructors_still_work() {
    let without_workspace = ToolExecutor::without_workspace();
    let listed = without_workspace.execute("SkillManager", json!({})).await;
    assert_eq!(listed.status, ToolStatus::Completed);
    let payload = parse_output(&listed);
    assert_eq!(payload["skills"], json!([]));

    let new_without_workspace = ToolExecutor::new(None).unwrap();
    let listed = new_without_workspace
        .execute("SkillManager", json!({ "action": "list" }))
        .await;
    assert_eq!(listed.status, ToolStatus::Completed);
    let payload = parse_output(&listed);
    assert_eq!(payload["skills"], json!([]));

    let todos = without_workspace
        .execute("TodoWrite", json!({ "todos": ["keep existing behavior"] }))
        .await;
    assert_eq!(todos.status, ToolStatus::Completed);
    let payload = parse_output(&todos);
    assert_eq!(payload["status"], "COMPLETED");
    assert_eq!(payload["todos"], json!(["keep existing behavior"]));
}
