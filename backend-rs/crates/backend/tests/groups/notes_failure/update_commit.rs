use super::*;
use qunica_backend::tools::AppControlContext;
use std::sync::atomic::{AtomicBool, Ordering};

async fn on_commit(fixture: &Fixture, action: impl Fn() + Send + Sync + 'static) {
    let action = Arc::new(action);
    let once = Arc::new(AtomicBool::new(false));
    let written = Arc::new(AtomicBool::new(false));
    let mut held = Vec::new();
    for _ in 0..8 {
        held.push(fixture.state.db.pool().acquire().await.unwrap());
    }
    for connection in &mut held {
        let action = action.clone();
        let once = once.clone();
        let armed = written.clone();
        connection
            .lock_handle()
            .await
            .unwrap()
            .set_update_hook(move |change| {
                if change.table == "group_notes" {
                    armed.store(true, Ordering::SeqCst);
                }
            });
        let written = written.clone();
        connection
            .lock_handle()
            .await
            .unwrap()
            .set_commit_hook(move || {
                if written.swap(false, Ordering::SeqCst) && !once.swap(true, Ordering::SeqCst) {
                    action();
                }
                true
            });
    }
}

#[tokio::test]
async fn approved_update_with_pending_publication_is_applied_once_without_replay() {
    let fixture = Fixture::new(true).await;
    let note = fixture.create().await;
    let id = note["id"].as_str().unwrap();
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
    let proposed = executor.execute("AppPropose", json!({"target_kind":"group_note", "action":"update", "target_id":id, "payload":{"content":"accepted body"}})).await;
    assert_eq!(
        proposed.status,
        ToolStatus::ApprovalRequired,
        "{}",
        proposed.output
    );
    let proposed: Value = serde_json::from_str(&proposed.output).unwrap();
    let action = proposed["action_id"].as_str().unwrap();
    let path = fixture.note_path(id);
    let index = fixture.index();
    on_commit(&fixture, move || {
        fs::write(&path, "newer native body").unwrap()
    })
    .await;
    let approve = || {
        authed_json(
            "POST",
            &format!("/api/v2/app-actions/{action}/approve"),
            &fixture.token,
            json!({}),
        )
    };
    let (status, applied) = send(&fixture.app, approve()).await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["status"], "applied");
    let saved: Value = serde_json::from_str(applied["result_json"].as_str().unwrap()).unwrap();
    assert_eq!(saved["id"], id);
    assert_eq!(saved["content"], "accepted body");
    assert_eq!(saved["warning"]["code"], "note_content_pending");
    assert_eq!(fixture.get(id).await.1["content"], "newer native body");
    assert_eq!(fixture.index(), index);
    let snapshot = fixture.snapshot().await;
    let (status, repeated) = send(&fixture.app, approve()).await;
    assert_eq!(status, StatusCode::OK, "{repeated}");
    assert_eq!(repeated, applied);
    assert_eq!(fixture.snapshot().await, snapshot);
    assert_eq!(fixture.get(id).await.1["content"], "newer native body");
    assert_eq!(fixture.index(), index);
    fixture.state.db.pool().close().await;
}

#[tokio::test]
async fn membership_revoked_by_commit_prevents_publication_and_later_access() {
    let mut fixture = Fixture::new(false).await;
    let note = fixture.create().await;
    let id = note["id"].as_str().unwrap();
    fixture.token = register_and_login(&fixture.app, "publication-member@example.test").await;
    sqlx::query("INSERT INTO group_members(group_id,user_id,role,status,joined_at) SELECT ?,id,'member','active','now' FROM users WHERE email='publication-member@example.test'")
        .bind(&fixture.group).execute(fixture.state.db.pool()).await.unwrap();
    let (status, saved) = fixture
        .mutate("PATCH", Some(id), json!({"content":"member prior"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert!(saved.get("warning").is_none(), "{saved}");
    let index = fixture.index();
    sqlx::query("CREATE TRIGGER test_revoke_note_writer AFTER UPDATE ON group_notes BEGIN UPDATE group_members SET status='removed' WHERE group_id=NEW.group_id AND role='member'; END")
        .execute(fixture.state.db.pool()).await.unwrap();
    let (status, saved) = fixture
        .mutate("PATCH", Some(id), json!({"content":"accepted body"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["id"], id);
    assert_eq!(saved["content"], "accepted body");
    assert_eq!(
        saved["warning"],
        json!({"code":"note_content_pending", "reason":"group notes are not shared with this user"})
    );
    assert_eq!(
        fs::read_to_string(fixture.note_path(id)).unwrap(),
        "member prior"
    );
    assert_eq!(fixture.index(), index);
    assert_eq!(fixture.get(id).await.0, StatusCode::FORBIDDEN);
    let before = fixture.snapshot().await;
    assert_eq!(
        fixture
            .mutate("PATCH", Some(id), json!({"content":"forbidden body"}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(fixture.snapshot().await, before);
}

#[tokio::test]
async fn committed_update_preserves_a_newer_file_and_returns_its_accepted_identity() {
    let mut fixture = Fixture::new(true).await;
    let note = fixture.create().await;
    let id = note["id"].as_str().unwrap();
    let before = fixture.snapshot().await;
    let index = fixture.index();
    let path = fixture.note_path(id);
    on_commit(&fixture, move || {
        fs::write(&path, "native edit during COMMIT").unwrap()
    })
    .await;
    let (status, saved) = fixture
        .mutate(
            "PATCH",
            Some(id),
            json!({"title":"Accepted", "content":"accepted body"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["id"], id);
    assert_eq!(saved["content"], "accepted body");
    assert_eq!(saved["title"], "Accepted");
    assert_eq!(saved["warning"]["code"], "note_content_pending");
    for reopen in [false, true] {
        if reopen {
            fixture.reopen().await;
        }
        let snapshot = fixture.snapshot().await;
        assert_eq!(snapshot.rows[0].2, "accepted body");
        assert_eq!(snapshot.events.len(), before.events.len() + 1);
        assert_eq!(
            fixture.get(id).await.1["content"],
            "native edit during COMMIT"
        );
        assert_eq!(
            fixture.index(),
            index,
            "content failure must stop index writes"
        );
    }
    fixture.state.db.pool().close().await;
}

#[tokio::test]
async fn same_bytes_in_a_replacement_file_do_not_authorize_publication() {
    let fixture = Fixture::new(true).await;
    let note = fixture.create().await;
    let id = note["id"].as_str().unwrap();
    let path = fixture.note_path(id);
    let original = path.with_extension("original");
    let index = fixture.index();
    on_commit(&fixture, move || {
        fs::rename(&path, &original).unwrap();
        fs::write(&path, "db before").unwrap();
    })
    .await;
    let (status, saved) = fixture
        .mutate("PATCH", Some(id), json!({"content":"accepted body"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["warning"]["code"], "note_content_pending");
    assert_eq!(saved["content"], "accepted body");
    assert_eq!(fixture.get(id).await.1["content"], "db before");
    assert_eq!(fixture.index(), index);
    fixture.state.db.pool().close().await;
}

#[tokio::test]
async fn removed_file_is_not_resurrected_after_an_update_commits() {
    let fixture = Fixture::new(true).await;
    let note = fixture.create().await;
    let id = note["id"].as_str().unwrap();
    let path = fixture.note_path(id);
    on_commit(&fixture, move || fs::remove_file(&path).unwrap()).await;
    let (status, saved) = fixture
        .mutate("PATCH", Some(id), json!({"content":"accepted body"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["warning"]["code"], "note_content_pending");
    assert_eq!(saved["content"], "accepted body");
    assert!(!fixture.note_path(id).exists());
    assert_eq!(
        fixture.get(id).await.1["content"],
        "accepted body",
        "missing-file fallback is unchanged"
    );
    fixture.state.db.pool().close().await;
}

#[tokio::test]
async fn missing_file_updates_publish_only_after_commit_and_never_clobber_a_new_target() {
    for outcome in ["ok", "rejected", "occupied"] {
        let fixture = Fixture::new(true).await;
        let note = fixture.create().await;
        let id = note["id"].as_str().unwrap();
        let path = fixture.note_path(id);
        fs::remove_file(&path).unwrap();
        let before = fixture.snapshot().await;
        let index = fixture.index();
        if outcome == "rejected" {
            fixture.reject_commit("UPDATE").await;
        }
        if outcome == "occupied" {
            on_commit(&fixture, move || {
                fs::write(&path, "new target during COMMIT").unwrap()
            })
            .await;
        }
        let (status, saved) = fixture
            .mutate("PATCH", Some(id), json!({"content":"accepted body"}))
            .await;
        match outcome {
            "rejected" => {
                internal_error(status, &saved, "failed to commit group note update");
                assert!(!fixture.note_path(id).exists());
                assert_eq!(fixture.snapshot().await, before);
                assert_eq!(fixture.index(), index);
            }
            "occupied" => {
                assert_eq!(status, StatusCode::OK, "{saved}");
                assert_eq!(saved["warning"]["code"], "note_content_pending");
                assert_eq!(saved["content"], "accepted body");
                assert_eq!(
                    fixture.get(id).await.1["content"],
                    "new target during COMMIT"
                );
                assert_eq!(fixture.index(), index);
            }
            _ => {
                assert_eq!(status, StatusCode::OK, "{saved}");
                assert!(saved.get("warning").is_none());
                assert_eq!(
                    fs::read_to_string(fixture.note_path(id)).unwrap(),
                    "accepted body"
                );
            }
        }
        assert_eq!(
            fs::read_dir(fixture.root.path().join("Notes"))
                .unwrap()
                .count(),
            if outcome == "rejected" { 1 } else { 2 },
            "owned temporary files must be removed"
        );
        fixture.state.db.pool().close().await;
    }
}

#[tokio::test]
async fn title_only_update_does_not_materialize_a_missing_body_and_body_keeps_nul_bytes() {
    let fixture = Fixture::new(false).await; // single-connection publication must not deadlock
    let note = fixture.create().await;
    let id = note["id"].as_str().unwrap();
    fs::remove_file(fixture.note_path(id)).unwrap();
    let (status, saved) = fixture
        .mutate("PATCH", Some(id), json!({"title":"Title only"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert!(!fixture.note_path(id).exists());
    for content in ["", "body\0with NUL"] {
        let (status, saved) = fixture
            .mutate("PATCH", Some(id), json!({"content":content}))
            .await;
        assert_eq!(status, StatusCode::OK, "{saved}");
        assert!(saved.get("warning").is_none(), "{saved}");
        assert_eq!(fs::read(fixture.note_path(id)).unwrap(), content.as_bytes());
    }
}

#[tokio::test]
async fn publication_rechecks_registration_content_and_workspace_without_another_pool_connection() {
    for change in [
        "UPDATE group_notes SET status='deleted' WHERE id=NEW.id",
        "UPDATE group_notes SET content='newer accepted body' WHERE id=NEW.id",
        "UPDATE groups SET status='deleted' WHERE id=NEW.group_id",
        "UPDATE groups SET workspace_id=NULL WHERE id=NEW.group_id",
        "UPDATE workspaces SET status='deleted' WHERE id=(SELECT workspace_id FROM groups WHERE id=NEW.group_id)",
        "UPDATE workspaces SET local_path=NULL WHERE id=(SELECT workspace_id FROM groups WHERE id=NEW.group_id)",
    ] {
        let fixture = Fixture::new(false).await;
        let note = fixture.create().await;
        let id = note["id"].as_str().unwrap();
        let index = fixture.index();
        // Real committed DB changes; no production callback or mocked persistence.
        sqlx::query(&format!("CREATE TRIGGER test_supersede_publication AFTER UPDATE ON group_notes WHEN NEW.content='accepted body' BEGIN {change}; END"))
            .execute(fixture.state.db.pool()).await.unwrap();
        let (status, saved) = fixture.mutate("PATCH", Some(id), json!({"content":"accepted body"})).await;
        assert_eq!(status, StatusCode::OK, "{change}: {saved}");
        assert_eq!(saved["id"], id);
        assert_eq!(saved["content"], "accepted body");
        assert_eq!(saved["warning"]["code"], "note_content_pending", "{change}: {saved}");
        assert_eq!(fs::read_to_string(fixture.note_path(id)).unwrap(), "db before", "{change}");
        assert_eq!(fixture.index(), index, "{change}");
    }
}

#[cfg(windows)]
#[tokio::test]
async fn publication_reopens_the_file_so_new_read_only_permissions_are_respected() {
    let fixture = Fixture::new(true).await;
    let note = fixture.create().await;
    let id = note["id"].as_str().unwrap();
    let path = fixture.note_path(id);
    let original_permissions = fs::metadata(&path).unwrap().permissions();
    let index = fixture.index();
    on_commit(&fixture, move || {
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions).unwrap();
    })
    .await;
    let (status, saved) = fixture
        .mutate("PATCH", Some(id), json!({"content":"accepted body"}))
        .await;
    // Restore only this fixture's original permission before asserting/cleaning up.
    let read_only = fs::metadata(fixture.note_path(id))
        .unwrap()
        .permissions()
        .readonly();
    fs::set_permissions(fixture.note_path(id), original_permissions).unwrap();
    assert!(read_only);
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["warning"]["code"], "note_content_pending");
    assert_eq!(saved["content"], "accepted body");
    assert_eq!(fixture.get(id).await.1["content"], "db before");
    assert_eq!(fixture.index(), index);
    fixture.state.db.pool().close().await;
}
