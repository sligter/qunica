use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[tokio::test]
async fn rejected_commit_does_not_overwrite_an_edit_made_during_deletion() {
    let mut fixture = Fixture::new(true).await;
    let note = fixture.create().await;
    let id = note["id"].as_str().unwrap();
    let before = fixture.snapshot().await;
    let index = fixture.index();
    fixture.reject_commit("UPDATE").await;
    let observed = Arc::new(AtomicBool::new(false));
    // Db's file pool has eight connections. Install a test-local SQLite hook
    // on each so the real UPDATE can rendezvous with an external file edit.
    let mut connections = Vec::new();
    for _ in 0..8 {
        connections.push(fixture.state.db.pool().acquire().await.unwrap());
    }
    for connection in &mut connections {
        let path = fixture.note_path(id);
        let observed = observed.clone();
        connection
            .lock_handle()
            .await
            .unwrap()
            .set_update_hook(move |change| {
                if change.table == "group_notes" && !observed.swap(true, Ordering::SeqCst) {
                    fs::write(&path, "newer edit during deletion").unwrap();
                }
            });
    }
    drop(connections);
    let (status, body) = fixture.mutate("DELETE", Some(id), json!({})).await;
    assert!(
        observed.load(Ordering::SeqCst),
        "the SQLite UPDATE must be observed before checking file safety"
    );
    internal_error(status, &body, "failed to commit group note delete");
    for reopen in [false, true] {
        if reopen {
            fixture.reopen().await;
        }
        assert_eq!(fixture.snapshot().await, before);
        assert_eq!(fixture.index(), index);
        assert!(
            fixture.note_path(id).is_file(),
            "a rejected deletion must leave the concurrent edit"
        );
        assert_eq!(
            fs::read_to_string(fixture.note_path(id)).unwrap(),
            "newer edit during deletion"
        );
        assert_eq!(
            fixture.get(id).await.1["content"],
            "newer edit during deletion"
        );
    }
    fixture.state.db.pool().close().await;
}

#[cfg(windows)]
#[tokio::test]
async fn a_windows_delete_lock_reports_committed_cleanup_pending_without_replay() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new(false).await;
    let note = fixture.create().await;
    let id = note["id"].as_str().unwrap();
    fs::write(fixture.note_path(id), "native body").unwrap();
    let index = fixture.index();
    let before = fixture.snapshot().await;
    // Permit metadata/read/write, but deliberately withhold FILE_SHARE_DELETE.
    // Validation can inspect the real file; DeleteFile must fail.
    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(0x1 | 0x2)
        .open(fixture.note_path(id))
        .unwrap();
    let (status, body) = fixture.mutate("DELETE", Some(id), json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body,
        json!({"error":{
            "code":"note_cleanup_pending",
            "message":"group note was deleted but its file cleanup failed",
            "details":{"committed":true,"group_id":fixture.group,"note_id":id,"phase":"file_cleanup",
                "reason":"failed to delete group note content"}
        }})
    );
    let after = fixture.snapshot().await;
    assert_eq!(after.rows[0].3, "deleted");
    assert_eq!(after.events.len(), before.events.len() + 1);
    assert_eq!(after.watermark, before.watermark + 1);
    assert_eq!(
        fs::read_to_string(fixture.note_path(id)).unwrap(),
        "native body"
    );
    assert_eq!(
        fixture.index(),
        index,
        "a failed cleanup has not refreshed the index"
    );
    assert_eq!(fixture.get(id).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        fixture.mutate("DELETE", Some(id), json!({})).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fixture.snapshot().await,
        after,
        "a repeat cannot delete or emit again"
    );
    drop(handle);
    assert_eq!(
        fs::read_to_string(fixture.note_path(id)).unwrap(),
        "native body",
        "releasing an external lock does not automatically replay cleanup"
    );
}
