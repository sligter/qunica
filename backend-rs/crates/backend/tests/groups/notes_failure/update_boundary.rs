//! Commit rejection and publication preserve independent file edits and metadata.
use super::*;
use qunica_backend::tools::AppControlContext;
use std::sync::atomic::{AtomicBool, Ordering};

async fn connections(fixture: &Fixture) -> Vec<sqlx::pool::PoolConnection<sqlx::Sqlite>> {
    let mut held = Vec::new();
    for _ in 0..8 {
        held.push(fixture.state.db.pool().acquire().await.unwrap());
    }
    held
}

#[tokio::test]
async fn a_rejected_update_preserves_an_external_edit_made_during_the_statement() {
    let mut fixture = Fixture::new(true).await;
    let note = fixture.create().await;
    let id = note["id"].as_str().unwrap();
    let before = fixture.snapshot().await;
    let index = fixture.index();
    fixture.reject_commit("UPDATE").await;
    let observed = Arc::new(AtomicBool::new(false));
    let mut held = connections(&fixture).await;
    for connection in &mut held {
        let path = fixture.note_path(id);
        let observed = observed.clone();
        connection
            .lock_handle()
            .await
            .unwrap()
            .set_update_hook(move |change| {
                if change.table == "group_notes" && !observed.swap(true, Ordering::SeqCst) {
                    fs::write(&path, "external edit during SQL UPDATE").unwrap();
                }
            });
    }
    drop(held);
    let (status, body) = fixture
        .mutate("PATCH", Some(id), json!({"content":"request body"}))
        .await;
    internal_error(status, &body, "failed to commit group note update");
    assert!(observed.load(Ordering::SeqCst));
    for reopen in [false, true] {
        if reopen {
            fixture.reopen().await;
        }
        assert_eq!(fixture.snapshot().await, before);
        assert_eq!(fixture.index(), index);
        assert_eq!(
            fixture.get(id).await.1["content"],
            "external edit during SQL UPDATE",
            "a rejected database mutation must not write the target"
        );
    }
    fixture.state.db.pool().close().await;
}

#[tokio::test]
async fn a_commit_rejection_keeps_an_edit_made_during_commit() {
    let mut fixture = Fixture::new(true).await;
    let note = fixture.create().await;
    let id = note["id"].as_str().unwrap();
    let before = fixture.snapshot().await;
    let index = fixture.index();
    let observed = Arc::new(AtomicBool::new(false));
    let mut held = connections(&fixture).await;
    for connection in &mut held {
        let path = fixture.note_path(id);
        let observed = observed.clone();
        connection
            .lock_handle()
            .await
            .unwrap()
            .set_commit_hook(move || {
                fs::write(&path, "external edit during COMMIT").unwrap();
                observed.store(true, Ordering::SeqCst);
                false // Real SQLite rollback, not a mocked commit error.
            });
    }
    drop(held);
    let (status, body) = fixture
        .mutate("PATCH", Some(id), json!({"content":"request body"}))
        .await;
    internal_error(status, &body, "failed to commit group note update");
    assert!(observed.load(Ordering::SeqCst));
    for reopen in [false, true] {
        if reopen {
            fixture.reopen().await;
        }
        assert_eq!(fixture.snapshot().await, before);
        assert_eq!(fixture.index(), index);
        assert_eq!(
            fixture.get(id).await.1["content"],
            "external edit during COMMIT",
            "rollback compensation must not restore an older backup over this edit"
        );
    }
    fixture.state.db.pool().close().await;
}

#[tokio::test]
async fn failed_approval_keeps_the_file_and_reapproval_does_not_replay_it() {
    let fixture = Fixture::new(true).await;
    let note = fixture.create().await;
    let id = note["id"].as_str().unwrap();
    let before = fixture.snapshot().await;
    let owner: String = sqlx::query_scalar("SELECT owner_id FROM groups WHERE id=?")
        .bind(&fixture.group)
        .fetch_one(fixture.state.db.pool())
        .await
        .unwrap();
    let executor = ToolExecutor::without_workspace().with_app_control(AppControlContext::new(
        fixture.state.db.pool().clone(),
        owner,
        Uuid::new_v4().to_string(),
    ));
    let proposal = executor
        .execute(
            "AppPropose",
            json!({"target_kind":"group_note","action":"update","target_id":id,
        "payload":{"content":"approved request body"}}),
        )
        .await;
    assert_eq!(
        proposal.status,
        ToolStatus::ApprovalRequired,
        "{}",
        proposal.output
    );
    let proposal: Value = serde_json::from_str(&proposal.output).unwrap();
    let action = proposal["action_id"].as_str().unwrap();
    fixture.reject_commit("UPDATE").await;
    let approve = || {
        authed_json(
            "POST",
            &format!("/api/v2/app-actions/{action}/approve"),
            &fixture.token,
            json!({}),
        )
    };
    let (status, error) = send(&fixture.app, approve()).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{error}");
    assert_eq!(error["error"]["code"], "app_action_failed");
    let approval: (String, String) =
        sqlx::query_as("SELECT status,result_json FROM app_actions WHERE id=?")
            .bind(action)
            .fetch_one(fixture.state.db.pool())
            .await
            .unwrap();
    assert_eq!(approval.0, "failed");
    assert_eq!(
        serde_json::from_str::<Value>(&approval.1).unwrap(),
        json!({"error":"failed to commit group note update"})
    );
    let snapshot = fixture.snapshot().await;
    assert_eq!(snapshot.rows, before.rows);
    assert_eq!(snapshot.events, before.events);
    assert_eq!(
        fixture.get(id).await.1["content"],
        "db before",
        "a confirmed rejected update must not publish the approved content"
    );
    fs::write(fixture.note_path(id), "external edit after failure").unwrap();
    let (status, repeated) = send(&fixture.app, approve()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{repeated}");
    assert_eq!(
        repeated,
        json!({"error":{"code":"conflict","message":"this action was already failed","details":{}}})
    );
    assert_eq!(
        fixture.get(id).await.1["content"],
        "external edit after failure"
    );
    assert_eq!(fixture.snapshot().await, snapshot);
    fixture.state.db.pool().close().await;
}

#[cfg(windows)]
#[tokio::test]
async fn note_update_and_workspace_replacement_have_different_protected_acl_outcomes() {
    use std::process::Command;
    fn protected(path: &std::path::Path, protect: bool) -> bool {
        let result = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                r#"
$ErrorActionPreference = 'Stop'
if ($env:QUNICA_TEST_PROTECT_ACL -eq 'true') {
    $taskAcl = Get-Acl -LiteralPath $env:QUNICA_TEST_NOTE_PATH
    $taskAcl.SetAccessRuleProtection($true, $true)
    Set-Acl -LiteralPath $env:QUNICA_TEST_NOTE_PATH -AclObject $taskAcl
}
(Get-Acl -LiteralPath $env:QUNICA_TEST_NOTE_PATH).AreAccessRulesProtected
"#,
            ])
            .env("QUNICA_TEST_NOTE_PATH", path)
            // Use Windows PowerShell's own modules, not an inherited PS7 path.
            .env_remove("PSModulePath")
            .env(
                "QUNICA_TEST_PROTECT_ACL",
                if protect { "true" } else { "false" },
            )
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "ACL fixture failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        match String::from_utf8(result.stdout).unwrap().trim() {
            "True" => true,
            "False" => false,
            other => panic!("unexpected ACL protection response: {other:?}"),
        }
    }
    let fixture = Fixture::new(false).await;
    let note = fixture.create().await;
    let id = note["id"].as_str().unwrap();
    let path = fixture.note_path(id);
    assert!(protected(&path, true));
    let (status, saved) = fixture
        .mutate("PATCH", Some(id), json!({"content":"in-place note update"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert!(
        protected(&path, false),
        "current Notes update preserves DACL protection"
    );
    let relative = format!("Notes/{id}.md");
    let (status, read) = send(
        &fixture.app,
        authed(
            "GET",
            &workspace_file_text_url(&fixture.group, &relative),
            &fixture.token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{read}");
    let (status, replaced) = send(
        &fixture.app,
        authed_json(
            "PATCH",
            &workspace_file_text_save_url(&fixture.group, &relative),
            &fixture.token,
            json!({"content":"workspace replacement","version":read["version"]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replaced}");
    assert_eq!(fs::read_to_string(&path).unwrap(), "workspace replacement");
    assert!(!protected(&path, false), "observed Workspace replacement currently loses DACL protection; this is not a desired invariant");
}

#[tokio::test]
async fn note_update_and_workspace_replacement_have_different_hard_link_outcomes() {
    let fixture = Fixture::new(false).await;
    let note = fixture.create().await;
    let id = note["id"].as_str().unwrap();
    let path = fixture.note_path(id);
    let alias = fixture.root.path().join("note-alias.md");
    fs::hard_link(&path, &alias).unwrap();
    let (status, saved) = fixture
        .mutate("PATCH", Some(id), json!({"content":"in-place note update"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(fs::read_to_string(&alias).unwrap(), "in-place note update");
    let relative = format!("Notes/{id}.md");
    let (status, read) = send(
        &fixture.app,
        authed(
            "GET",
            &workspace_file_text_url(&fixture.group, &relative),
            &fixture.token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{read}");
    let (status, replaced) = send(
        &fixture.app,
        authed_json(
            "PATCH",
            &workspace_file_text_save_url(&fixture.group, &relative),
            &fixture.token,
            json!({"content":"workspace replacement","version":read["version"]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replaced}");
    assert_eq!(fs::read_to_string(&path).unwrap(), "workspace replacement");
    assert_eq!(
        fs::read_to_string(&alias).unwrap(),
        "in-place note update",
        "Workspace replacement currently changes file identity; Notes update does not"
    );
}
