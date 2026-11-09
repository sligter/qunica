use super::*;
use qunica_backend::tools::AppControlContext;
use std::sync::Mutex;

// SQLx's real COMMIT callback only observes/changes fixture files, never SQLite.
async fn on_commit(fixture: &Fixture, effect: Arc<dyn Fn(&std::path::Path) + Send + Sync>) {
    let mut connections = Vec::new();
    for _ in 0..8 {
        connections.push(fixture.state.db.pool().acquire().await.unwrap());
    }
    for connection in &mut connections {
        let root = fixture.root.path().to_owned();
        let effect = effect.clone();
        connection
            .lock_handle()
            .await
            .unwrap()
            .set_commit_hook(move || {
                effect(&root);
                true
            });
    }
}

#[tokio::test]
async fn created_markdown_is_not_published_before_database_commit() {
    let fixture = Fixture::new(true).await;
    let seen = Arc::new(Mutex::new(None));
    let observed = seen.clone();
    on_commit(
        &fixture,
        Arc::new(move |root| {
            let notes = fs::read_dir(root.join("Notes"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect::<Vec<_>>();
            *observed.lock().unwrap() = Some(
                notes
                    .iter()
                    .any(|path| path.extension().is_some_and(|ext| ext == "md")),
            );
        }),
    )
    .await;
    let saved = fixture.create().await;
    assert_eq!(
        *seen.lock().unwrap(),
        Some(false),
        "the final note must not precede COMMIT"
    );
    assert!(saved.get("warning").is_none());
    let id = saved["id"].as_str().unwrap();
    assert_eq!(
        fs::read_to_string(fixture.note_path(id)).unwrap(),
        "db before"
    );
    assert_eq!(
        fs::read_dir(fixture.root.path().join("Notes"))
            .unwrap()
            .count(),
        2
    );
    fixture.state.db.pool().close().await;
}

#[tokio::test]
async fn an_external_removal_at_commit_reports_saved_content_pending_without_replay() {
    for blocked_index in [false, true] {
        let mut fixture = Fixture::new(true).await;
        if blocked_index {
            fixture.block_index();
        }
        let removed = Arc::new(Mutex::new(None));
        let observed = removed.clone();
        on_commit(
            &fixture,
            Arc::new(move |root| {
                for entry in fs::read_dir(root.join("Notes")).unwrap() {
                    let path = entry.unwrap().path();
                    if path.is_file() && fs::read_to_string(&path).unwrap() == "db before" {
                        fs::remove_file(&path).unwrap();
                        *observed.lock().unwrap() = Some(path);
                    }
                }
            }),
        )
        .await;
        let before = fixture.snapshot().await;
        let saved = fixture.create().await;
        assert!(
            removed.lock().unwrap().is_some(),
            "the file fault must be injected during COMMIT"
        );
        let warning =
            json!({"code":"note_content_pending","reason":"failed to publish group note content"});
        assert_eq!(saved["warning"], warning);
        assert_eq!(fixture.index_path().is_dir(), blocked_index);
        assert_eq!(
            fixture.index_path().exists(),
            blocked_index,
            "publication failure must stop subsequent index writes"
        );
        let id = saved["id"].as_str().unwrap();
        assert_eq!(saved["content"], "db before");
        let after = fixture.snapshot().await;
        assert_eq!(after.rows.len(), 1);
        assert_eq!(after.events.len(), before.events.len() + 1);
        assert_eq!(after.watermark, before.watermark + 1);
        assert!(!fixture.note_path(id).exists());
        for reopen in [false, true] {
            if reopen {
                fixture.reopen().await;
            }
            let (status, read) = fixture.get(id).await;
            assert_eq!(status, StatusCode::OK, "{read}");
            assert_eq!(read["content"], "db before");
            assert_eq!(fixture.snapshot().await, after);
        }
        fixture.state.db.pool().close().await;
    }
}

#[tokio::test]
async fn approved_create_with_pending_content_remains_applied_and_is_not_replayed() {
    let fixture = Fixture::new(true).await;
    on_commit(
        &fixture,
        Arc::new(move |root| {
            if let Ok(entries) = fs::read_dir(root.join("Notes")) {
                for entry in entries {
                    let path = entry.unwrap().path();
                    if path.is_file() && fs::read_to_string(&path).unwrap() == "db before" {
                        fs::remove_file(path).unwrap();
                    }
                }
            }
        }),
    )
    .await;
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
    let proposed = executor
        .execute(
            "AppPropose",
            json!({"target_kind":"group_note","action":"create",
        "payload":{"group_id":fixture.group,"title":"Approved","content":"db before"}}),
        )
        .await;
    assert_eq!(
        proposed.status,
        ToolStatus::ApprovalRequired,
        "{}",
        proposed.output
    );
    let proposed: Value = serde_json::from_str(&proposed.output).unwrap();
    let action = proposed["action_id"].as_str().unwrap();
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
    assert_eq!(
        saved["warning"],
        json!({"code":"note_content_pending","reason":"failed to publish group note content"})
    );
    let id = saved["id"].as_str().unwrap();
    assert_eq!(fixture.get(id).await.1["content"], "db before");
    let snapshot = fixture.snapshot().await;
    assert_eq!(snapshot.rows.len(), 1);
    assert_eq!(snapshot.events.len(), 1);
    let (status, repeated) = send(&fixture.app, approve()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(repeated, applied);
    assert_eq!(fixture.snapshot().await, snapshot);
    fixture.state.db.pool().close().await;
}
