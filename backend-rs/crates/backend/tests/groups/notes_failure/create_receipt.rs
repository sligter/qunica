use super::*;
use qunica_backend::tools::AppControlContext;
use std::sync::atomic::{AtomicBool, Ordering};

// Arm only for an actual Note mutation: approval also commits its own ledger.
// The COMMIT callback changes owned files/pool admission, never queries SQLite.
async fn fail_publication(fixture: &Fixture, close_pool: bool) {
    let written = Arc::new(AtomicBool::new(false));
    let mut held = Vec::new();
    for _ in 0..8 {
        held.push(fixture.state.db.pool().acquire().await.unwrap());
    }
    for connection in &mut held {
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
        let notes = fixture.root.path().join("Notes");
        let pool = fixture.state.db.pool().clone();
        connection
            .lock_handle()
            .await
            .unwrap()
            .set_commit_hook(move || {
                if written.swap(false, Ordering::SeqCst) {
                    for entry in fs::read_dir(&notes).unwrap() {
                        let path = entry.unwrap().path();
                        if path.is_file() && fs::read(&path).unwrap() == b"accepted body" {
                            fs::remove_file(path).unwrap();
                        }
                    }
                    if close_pool {
                        // close() immediately fences new acquisition; the checked-out
                        // COMMIT is unaffected. Await closure outside the callback.
                        drop(pool.close());
                    }
                }
                true
            });
    }
}

async fn invalidate_registration(fixture: &Fixture) {
    sqlx::query("CREATE TRIGGER test_replace_created_registration AFTER INSERT ON group_notes BEGIN UPDATE group_notes SET title='Later title',content='later body',status='deleted',updated_at='later' WHERE id=NEW.id; END")
        .execute(fixture.state.db.pool()).await.unwrap();
}

fn accepted_receipt(saved: &Value, group: &str, id: &str) {
    assert_eq!(saved["id"], id);
    assert_eq!(saved["group_id"], group);
    assert_eq!(saved["title"], "Accepted");
    assert_eq!(saved["content"], "accepted body");
    assert_eq!(saved["created_at"], saved["updated_at"]);
    assert_ne!(saved["updated_at"], "later");
    assert_eq!(
        saved["warning"],
        json!({"code":"note_content_pending", "reason":"failed to publish group note content"})
    );
}

#[tokio::test]
async fn pending_create_receipt_survives_pool_shutdown_after_the_accepted_commit() {
    for blocked_index in [false, true] {
        let mut fixture = Fixture::new(true).await;
        if blocked_index {
            fixture.block_index();
        }
        let before = fixture.snapshot().await;
        fail_publication(&fixture, true).await;
        let (status, saved) = fixture
            .mutate(
                "POST",
                None,
                json!({"title":"Accepted", "content":"accepted body"}),
            )
            .await;
        assert!(fixture.state.db.pool().is_closed());
        // Reopen the physical DB before inspecting the response, including on a
        // red test. This independently proves COMMIT and closes owned handles.
        fixture.reopen().await;
        let snapshot = fixture.snapshot().await;
        assert_eq!(snapshot.rows.len(), 1);
        assert_eq!(snapshot.events.len(), before.events.len() + 1);
        let id = &snapshot.rows[0].0;
        assert_eq!(snapshot.rows[0].2, "accepted body");
        assert_eq!(snapshot.rows[0].3, "active");
        assert!(!fixture.note_path(id).exists());
        assert_eq!(fixture.index_path().exists(), blocked_index);
        assert_eq!(fixture.index_path().is_dir(), blocked_index);
        assert_eq!(fixture.get(id).await.1["content"], "accepted body");
        assert_eq!(status, StatusCode::CREATED, "{saved}");
        accepted_receipt(&saved, &fixture.group, id);
        assert_eq!(saved["created_at"], snapshot.rows[0].4);
        fixture.state.db.pool().close().await;
    }
}

#[tokio::test]
async fn pending_create_returns_its_accepted_identity_when_registration_is_no_longer_active() {
    let fixture = Fixture::new(true).await;
    invalidate_registration(&fixture).await;
    fail_publication(&fixture, false).await;
    let (status, saved) = fixture
        .mutate(
            "POST",
            None,
            json!({"title":"Accepted", "content":"accepted body"}),
        )
        .await;
    let snapshot = fixture.snapshot().await;
    assert_eq!(snapshot.rows.len(), 1);
    let id = &snapshot.rows[0].0;
    assert_eq!(snapshot.rows[0].1, "Later title");
    assert_eq!(snapshot.rows[0].3, "deleted");
    assert!(!fixture.note_path(id).exists());
    assert!(!fixture.index_path().exists());
    assert_eq!(fixture.get(id).await.0, StatusCode::NOT_FOUND);
    assert_eq!(status, StatusCode::CREATED, "{saved}");
    accepted_receipt(&saved, &fixture.group, id);
    fixture.state.db.pool().close().await;
}

#[tokio::test]
async fn pending_create_approval_keeps_accepted_identity_and_reapproval_never_replays_it() {
    let fixture = Fixture::new(true).await;
    invalidate_registration(&fixture).await;
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
    let proposed = executor.execute("AppPropose", json!({"target_kind":"group_note", "action":"create", "payload":{"group_id":fixture.group, "title":"Accepted", "content":"accepted body"}})).await;
    assert_eq!(
        proposed.status,
        ToolStatus::ApprovalRequired,
        "{}",
        proposed.output
    );
    let proposed: Value = serde_json::from_str(&proposed.output).unwrap();
    let action = proposed["action_id"].as_str().unwrap();
    fail_publication(&fixture, false).await;
    let approve = || {
        authed_json(
            "POST",
            &format!("/api/v2/app-actions/{action}/approve"),
            &fixture.token,
            json!({}),
        )
    };
    let (status, applied) = send(&fixture.app, approve()).await;
    let snapshot = fixture.snapshot().await;
    assert_eq!(snapshot.rows.len(), 1);
    let id = &snapshot.rows[0].0;
    assert_eq!(snapshot.rows[0].3, "deleted");
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["status"], "applied");
    let saved: Value = serde_json::from_str(applied["result_json"].as_str().unwrap()).unwrap();
    accepted_receipt(&saved, &fixture.group, id);
    let (status, repeated) = send(&fixture.app, approve()).await;
    assert_eq!(status, StatusCode::OK, "{repeated}");
    assert_eq!(repeated, applied);
    assert_eq!(fixture.snapshot().await, snapshot);
    assert!(!fixture.note_path(id).exists());
    assert!(!fixture.index_path().exists());
    fixture.state.db.pool().close().await;
}
