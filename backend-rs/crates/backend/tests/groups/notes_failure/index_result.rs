use super::*;
use qunica_backend::tools::AppControlContext;

#[cfg(windows)]
#[tokio::test]
async fn an_index_write_sharing_failure_reports_committed_operations_without_overwriting_the_index()
{
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new(false).await;
    let original = fixture.create().await;
    let original_id = original["id"].as_str().unwrap();
    let old_index = fixture.index();
    // Allow readers and deletion, but deny writes to the existing index file.
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0x1 | 0x4)
        .open(fixture.index_path())
        .unwrap();
    let before = fixture.snapshot().await;
    let (status, created) = fixture
        .mutate("POST", None, json!({"title":"New","content":"new"}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(
        created["warning"],
        json!({"code":"note_index_pending","reason":"failed to write group note index"})
    );
    let new_id = created["id"].as_str().unwrap();
    let (status, updated) = fixture
        .mutate("PATCH", Some(original_id), json!({"content":"updated"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["id"], original_id);
    assert_eq!(updated["warning"], created["warning"]);
    let (status, deleted) = fixture.mutate("DELETE", Some(original_id), json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{deleted}");
    assert_eq!(deleted["error"]["code"], "note_index_pending");
    assert_eq!(
        deleted["error"]["details"],
        json!({"committed":true,"group_id":fixture.group,"note_id":original_id,"phase":"index_sync","reason":"failed to write group note index"})
    );
    assert_eq!(fixture.index(), old_index);
    assert_eq!(fixture.get(original_id).await.0, StatusCode::NOT_FOUND);
    assert!(!fixture.note_path(original_id).exists());
    assert_eq!(fixture.get(new_id).await.1["content"], "new");
    let after = fixture.snapshot().await;
    assert_eq!(after.rows.len(), 2);
    assert_eq!(after.events.len(), before.events.len() + 3);
    assert_eq!(after.watermark, before.watermark + 3);
    drop(lock);
    let (status, saved) = fixture
        .mutate("PATCH", Some(new_id), json!({"title":"Revised"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert!(saved.get("warning").is_none());
    let repaired = fixture.index();
    assert!(repaired.contains(&format!("({new_id}.md)")));
    assert!(!repaired.contains(&format!("({original_id}.md)")));
}

#[tokio::test]
async fn approved_note_writes_with_index_failure_are_applied_once_with_saved_identity() {
    let fixture = Fixture::new(false).await;
    fixture.block_index();
    let owner: String = sqlx::query_scalar("SELECT owner_id FROM groups WHERE id=?")
        .bind(&fixture.group)
        .fetch_one(fixture.state.db.pool())
        .await
        .unwrap();
    let executor = ToolExecutor::without_workspace().with_app_control(AppControlContext::new(
        fixture.state.db.pool().clone(),
        owner,
        uuid::Uuid::new_v4().to_string(),
    ));
    let before = fixture.snapshot().await;
    let mut note_id = None;
    for (step, action) in ["create", "update"].into_iter().enumerate() {
        let payload = if step == 0 {
            json!({"group_id":fixture.group,"title":"Approved","content":"first"})
        } else {
            json!({"content":"second"})
        };
        let staged = executor
            .execute(
                "AppPropose",
                json!({
                    "target_kind":"group_note","action":action,"target_id":note_id,"payload":payload
                }),
            )
            .await;
        assert_eq!(
            staged.status,
            ToolStatus::ApprovalRequired,
            "{}",
            staged.output
        );
        let staged: Value = serde_json::from_str(&staged.output).unwrap();
        let action_id = staged["action_id"].as_str().unwrap();
        let approve = || {
            authed_json(
                "POST",
                &format!("/api/v2/app-actions/{action_id}/approve"),
                &fixture.token,
                json!({}),
            )
        };
        let (status, applied) = send(&fixture.app, approve()).await;
        assert_eq!(status, StatusCode::OK, "{applied}");
        assert_eq!(applied["status"], "applied");
        let saved: Value = serde_json::from_str(applied["result_json"].as_str().unwrap()).unwrap();
        index_warning(&saved);
        assert_eq!(saved["group_id"], fixture.group);
        if let Some(id) = &note_id {
            assert_eq!(saved["id"], *id);
        } else {
            note_id = Some(saved["id"].as_str().unwrap().to_owned());
        }
        assert_eq!(saved["content"], if step == 0 { "first" } else { "second" });
        let snapshot = fixture.snapshot().await;
        assert_eq!(snapshot.rows.len(), 1);
        assert_eq!(snapshot.events.len(), before.events.len() + step + 1);
        let (status, repeated) = send(&fixture.app, approve()).await;
        assert_eq!(status, StatusCode::OK, "{repeated}");
        assert_eq!(repeated, applied);
        assert_eq!(
            fixture.snapshot().await,
            snapshot,
            "approval must not replay a committed write"
        );
    }
}
