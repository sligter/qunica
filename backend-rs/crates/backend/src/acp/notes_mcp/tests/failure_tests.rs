use super::*;
use std::fs;

fn error_text(result: &Value) -> &str {
    assert_eq!(result["isError"], true, "{result}");
    result["content"][0]["text"].as_str().unwrap()
}

async fn event_count(context: &NotesContext) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM account_events WHERE group_id=? AND kind='note.changed'",
    )
    .bind(&context.group_id)
    .fetch_one(&context.pool)
    .await
    .unwrap()
}

async fn revoked_call(bridge: &NotesBridge, name: &str, args: Value) {
    let response = reqwest::Client::builder().no_proxy().build().unwrap()
        .post(&bridge.endpoint).bearer_auth(&bridge.state.token)
        .json(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":name,"arguments":args}}))
        .send().await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn pending_create_keeps_its_accepted_receipt_when_the_registration_is_deleted() {
    let (context, root) = fixture().await;
    sqlx::query("CREATE TRIGGER test_invalidate_created_note AFTER INSERT ON group_notes BEGIN UPDATE group_notes SET title='Later title',content='later body',status='deleted',updated_at='later' WHERE id=NEW.id; END")
        .execute(&context.pool).await.unwrap();
    let bridge = NotesBridge::start().await.unwrap();
    let lease = bridge.activate(context.clone());
    let notes = root.path().join("Notes");
    let mut connection = context.pool.acquire().await.unwrap();
    connection
        .lock_handle()
        .await
        .unwrap()
        .set_commit_hook(move || {
            for entry in fs::read_dir(&notes).unwrap() {
                let path = entry.unwrap().path();
                if path.is_file() && fs::read(&path).unwrap() == b"accepted body" {
                    fs::remove_file(path).unwrap();
                }
            }
            true
        });
    drop(connection);
    let result = call(
        &bridge,
        "CreateGroupNote",
        json!({"title":"Accepted", "content":"accepted body"}),
    )
    .await;
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT id,status FROM group_notes")
        .fetch_all(&context.pool)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, "deleted");
    let saved = output(&result);
    assert_eq!(saved["id"], rows[0].0);
    assert_eq!(saved["title"], "Accepted");
    assert_eq!(saved["content"], "accepted body");
    assert_eq!(saved["created_at"], saved["updated_at"]);
    assert_eq!(
        saved["warning"],
        json!({"code":"note_content_pending", "reason":"failed to publish group note content"})
    );
    assert_eq!(saved["path"], format!("{}.md", rows[0].0));
    assert!(!root
        .path()
        .join("Notes")
        .join(saved["path"].as_str().unwrap())
        .exists());
    assert!(!root.path().join("Notes/index.md").exists());
    assert!(
        output(&call(&bridge, "ReadGroupNotes", json!({})).await)["notes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let events = event_count(&context).await;
    drop(lease);
    revoked_call(
        &bridge,
        "CreateGroupNote",
        json!({"title":"Revoked", "content":"not allowed"}),
    )
    .await;
    assert_eq!(event_count(&context).await, events);
}

#[tokio::test]
async fn create_commit_failure_is_not_published_and_publication_failure_keeps_saved_identity() {
    for rejected_commit in [true, false] {
        let (context, root) = fixture().await;
        if rejected_commit {
            for statement in [
                "CREATE TABLE test_create_parent(id INTEGER PRIMARY KEY)",
                "CREATE TABLE test_create_child(parent_id INTEGER REFERENCES test_create_parent(id) DEFERRABLE INITIALLY DEFERRED)",
                "CREATE TRIGGER test_create_failure AFTER INSERT ON group_notes BEGIN INSERT INTO test_create_child(parent_id) VALUES(1); END",
            ] { sqlx::query(statement).execute(&context.pool).await.unwrap(); }
        } else {
            let notes = root.path().join("Notes");
            let mut connection = context.pool.acquire().await.unwrap();
            connection
                .lock_handle()
                .await
                .unwrap()
                .set_commit_hook(move || {
                    if let Ok(entries) = fs::read_dir(&notes) {
                        for entry in entries {
                            let path = entry.unwrap().path();
                            if path.is_file() && fs::read_to_string(&path).unwrap() == "saved body"
                            {
                                fs::remove_file(path).unwrap();
                            }
                        }
                    }
                    true
                });
        }
        let bridge = NotesBridge::start().await.unwrap();
        let lease = bridge.activate(context.clone());
        let events = event_count(&context).await;
        let result = call(
            &bridge,
            "CreateGroupNote",
            json!({"title":"Created","content":"saved body"}),
        )
        .await;
        if rejected_commit {
            assert_eq!(error_text(&result), "failed to commit group note create");
            assert_eq!(event_count(&context).await, events);
            assert_eq!(fs::read_dir(root.path().join("Notes")).unwrap().count(), 0);
            assert!(
                output(&call(&bridge, "ReadGroupNotes", json!({})).await)["notes"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        } else {
            let saved = output(&result);
            assert_eq!(
                saved["warning"],
                json!({"code":"note_content_pending","reason":"failed to publish group note content"})
            );
            let read =
                output(&call(&bridge, "ReadGroupNotes", json!({"path":saved["path"]})).await);
            assert_eq!(read["id"], saved["id"]);
            assert_eq!(read["content"], "saved body");
            assert_eq!(event_count(&context).await, events + 1);
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM group_notes")
            .fetch_one(&context.pool)
            .await
            .unwrap();
        assert_eq!(count, i64::from(!rejected_commit));
        drop(lease);
        revoked_call(
            &bridge,
            "CreateGroupNote",
            json!({"title":"Revoked","content":"not allowed"}),
        )
        .await;
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM group_notes")
                .fetch_one(&context.pool)
                .await
                .unwrap(),
            count
        );
    }
}

#[tokio::test]
async fn create_rechecks_the_lease_before_post_commit_publication() {
    let (context, root) = fixture().await;
    let bridge = NotesBridge::start().await.unwrap();
    let lease = bridge.activate(context.clone());
    let active = lease.0.clone();
    let mut connection = context.pool.acquire().await.unwrap();
    connection
        .lock_handle()
        .await
        .unwrap()
        .set_commit_hook(move || {
            active.store(false, Ordering::SeqCst);
            true
        });
    drop(connection);
    let events = event_count(&context).await;
    let saved = output(
        &call(
            &bridge,
            "CreateGroupNote",
            json!({"title":"Accepted","content":"saved body"}),
        )
        .await,
    );
    assert_eq!(
        saved["warning"],
        json!({"code":"note_content_pending","reason":"group notes access has expired"})
    );
    let notes = root.path().join("Notes");
    assert_eq!(
        fs::read_dir(&notes).unwrap().count(),
        0,
        "revocation during COMMIT must stop publication and index writes"
    );
    assert_eq!(event_count(&context).await, events + 1);
    let persisted: (String, String) = sqlx::query_as("SELECT id,content FROM group_notes")
        .fetch_one(&context.pool)
        .await
        .unwrap();
    assert_eq!(saved["id"], persisted.0);
    assert_eq!(persisted.1, "saved body");
    revoked_call(
        &bridge,
        "CreateGroupNote",
        json!({"title":"Denied","content":"not allowed"}),
    )
    .await;
    assert_eq!(event_count(&context).await, events + 1);
}

#[tokio::test]
async fn index_failure_reports_saved_create_and_edit_without_tool_error_and_revocation_blocks_writes(
) {
    let (context, root) = fixture().await;
    let bridge = NotesBridge::start().await.unwrap();
    let lease = bridge.activate(context.clone());
    fs::create_dir_all(root.path().join("Notes/index.md")).unwrap();
    let events = event_count(&context).await;
    let created = call(
        &bridge,
        "CreateGroupNote",
        json!({"title":"Created","content":"saved body"}),
    )
    .await;
    assert_ne!(created["isError"], true, "{created}");
    let saved = output(&created);
    assert_eq!(
        saved["warning"],
        json!({"code":"note_index_pending","reason":"group note index path is not a regular file"})
    );
    let listed = output(&call(&bridge, "ReadGroupNotes", json!({})).await);
    assert_eq!(listed["notes"].as_array().unwrap().len(), 1);
    let path = listed["notes"][0]["path"].as_str().unwrap();
    assert_eq!(saved["path"], path);
    let read = output(&call(&bridge, "ReadGroupNotes", json!({"path":path})).await);
    assert_eq!(saved["id"], read["id"]);
    assert_eq!(read["content"], "saved body");
    let persisted: String = sqlx::query_scalar("SELECT content FROM group_notes")
        .fetch_one(&context.pool)
        .await
        .unwrap();
    assert_eq!(persisted, "saved body");
    assert_eq!(
        fs::read_to_string(root.path().join("Notes").join(path)).unwrap(),
        "saved body"
    );
    assert_eq!(event_count(&context).await, events + 1);
    let edited = call(
        &bridge,
        "EditGroupNote",
        json!({"path":path,"edits":[{"old_text":"saved body","new_text":"edited body"}]}),
    )
    .await;
    assert_ne!(edited["isError"], true, "{edited}");
    let edited = output(&edited);
    assert_eq!(edited["id"], saved["id"]);
    assert_eq!(edited["path"], path);
    assert_eq!(edited["content"], "edited body");
    assert_eq!(edited["warning"], saved["warning"]);
    assert_eq!(event_count(&context).await, events + 2);
    drop(lease);
    revoked_call(
        &bridge,
        "CreateGroupNote",
        json!({"title":"Revoked","content":"not allowed"}),
    )
    .await;
    revoked_call(
        &bridge,
        "EditGroupNote",
        json!({"path":path,"edits":[{"old_text":"edited body","new_text":"not allowed"}]}),
    )
    .await;
    assert_eq!(event_count(&context).await, events + 2);
    assert_eq!(
        fs::read_to_string(root.path().join("Notes").join(path)).unwrap(),
        "edited body"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM group_notes")
            .fetch_one(&context.pool)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn update_rechecks_the_lease_before_post_commit_publication() {
    let (context, root) = fixture().await;
    let bridge = NotesBridge::start().await.unwrap();
    let lease = bridge.activate(context.clone());
    let created = output(
        &call(
            &bridge,
            "CreateGroupNote",
            json!({"title":"Before", "content":"before body"}),
        )
        .await,
    );
    let path = created["path"].as_str().unwrap();
    let index = fs::read_to_string(root.path().join("Notes/index.md")).unwrap();
    let events = event_count(&context).await;
    let active = lease.0.clone();
    let mut connection = context.pool.acquire().await.unwrap();
    connection
        .lock_handle()
        .await
        .unwrap()
        .set_commit_hook(move || {
            active.store(false, Ordering::SeqCst);
            true
        });
    drop(connection);
    let edit = json!({"path":path,"edits":[{"old_text":"before body","new_text":"after body"}]});
    let saved = output(&call(&bridge, "EditGroupNote", edit.clone()).await);
    assert_eq!(saved["id"], created["id"]);
    assert_eq!(saved["content"], "after body");
    assert_eq!(
        saved["warning"],
        json!({"code":"note_content_pending", "reason":"group notes access has expired"})
    );
    assert_eq!(
        fs::read_to_string(root.path().join("Notes").join(path)).unwrap(),
        "before body"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("Notes/index.md")).unwrap(),
        index
    );
    assert_eq!(event_count(&context).await, events + 1);
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT content FROM group_notes")
            .fetch_one(&context.pool)
            .await
            .unwrap(),
        "after body"
    );
    revoked_call(&bridge, "EditGroupNote", edit).await;
    assert_eq!(event_count(&context).await, events + 1);
}

#[tokio::test]
async fn failed_edit_commit_reports_error_and_keeps_the_original_file() {
    let (context, root) = fixture().await;
    let bridge = NotesBridge::start().await.unwrap();
    let lease = bridge.activate(context.clone());
    let created = output(
        &call(
            &bridge,
            "CreateGroupNote",
            json!({"title":"Before","content":"before body"}),
        )
        .await,
    );
    let path = created["path"].as_str().unwrap();
    let events = event_count(&context).await;
    let index = fs::read_to_string(root.path().join("Notes/index.md")).unwrap();
    for statement in [
        "CREATE TABLE test_note_commit_parent(id INTEGER PRIMARY KEY)",
        "CREATE TABLE test_note_commit_child(parent_id INTEGER REFERENCES test_note_commit_parent(id) DEFERRABLE INITIALLY DEFERRED)",
        "CREATE TRIGGER test_note_commit_failure AFTER UPDATE ON group_notes BEGIN INSERT INTO test_note_commit_child(parent_id) VALUES(1); END",
    ] { sqlx::query(statement).execute(&context.pool).await.unwrap(); }
    let edit = json!({"path":path,"edits":[{"old_text":"before body","new_text":"after body"}]});
    let failed = call(&bridge, "EditGroupNote", edit.clone()).await;
    assert_eq!(error_text(&failed), "failed to commit group note update");
    assert_eq!(event_count(&context).await, events);
    assert_eq!(
        fs::read_to_string(root.path().join("Notes/index.md")).unwrap(),
        index
    );
    assert_eq!(
        fs::read_to_string(root.path().join("Notes").join(path)).unwrap(),
        "before body"
    );
    let persisted: String = sqlx::query_scalar("SELECT content FROM group_notes")
        .fetch_one(&context.pool)
        .await
        .unwrap();
    assert_eq!(persisted, "before body");
    let read = output(&call(&bridge, "ReadGroupNotes", json!({"path":path})).await);
    assert_eq!(read["content"], "before body");
    let repeated = call(&bridge, "EditGroupNote", edit.clone()).await;
    assert_eq!(error_text(&repeated), "failed to commit group note update");
    assert_eq!(event_count(&context).await, events);
    assert_eq!(
        fs::read_to_string(root.path().join("Notes").join(path)).unwrap(),
        "before body"
    );
    drop(lease);
    revoked_call(&bridge, "EditGroupNote", edit).await;
    assert_eq!(event_count(&context).await, events);
}
