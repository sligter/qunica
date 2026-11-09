//! Characterization of existing failure behavior, including partial completion.
//! These assertions describe compatibility; they are not a desired atomicity policy.
use super::*;
use qunica_backend::{
    api,
    db::Db,
    tools::{ToolExecutor, ToolStatus},
};
use std::fs;

#[path = "notes_failure/create_commit.rs"]
mod create_commit;
#[path = "notes_failure/delete_commit.rs"]
mod delete_commit;
#[path = "notes_failure/index_result.rs"]
mod index_result;
#[path = "notes_failure/update_boundary.rs"]
mod update_boundary;
#[path = "notes_failure/update_commit.rs"]
mod update_commit;

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    rows: Vec<(String, String, String, String, String, String)>,
    events: Vec<(i64, String, String, String)>,
    watermark: i64,
}

struct Fixture {
    app: Router,
    state: AppState,
    token: String,
    group: String,
    root: tempfile::TempDir,
    storage: tempfile::TempDir,
    url: Option<String>,
}

impl Fixture {
    async fn new(persistent: bool) -> Self {
        let (mut app, mut state) = app_with_state().await;
        let storage = tempfile::tempdir().unwrap();
        let url = persistent.then(|| {
            format!(
                "sqlite:{}?mode=rwc",
                storage.path().join("notes.sqlite3").display()
            )
        });
        if let Some(url) = &url {
            state.db = Db::connect(url).await.unwrap();
            state.db.migrate().await.unwrap();
            app = api::router(state.clone());
        }
        let token = register_and_login(&app, "notes-failure@example.test").await;
        let (root, workspace) = create_local_workspace(&app, &token, "Notes failures").await;
        let group = create_group_with_initial_agents(&app, &token, &workspace, "mesh", &[]).await;
        Self {
            app,
            state,
            token,
            group: group["id"].as_str().unwrap().into(),
            root,
            storage,
            url,
        }
    }

    async fn create(&self) -> Value {
        create_group_note(&self.app, &self.token, &self.group, "Before", "db before").await
    }

    async fn mutate(&self, method: &str, id: Option<&str>, payload: Value) -> (StatusCode, Value) {
        let suffix = id.map(|id| format!("/{id}")).unwrap_or_default();
        send(
            &self.app,
            authed_json(
                method,
                &format!("/api/v2/groups/{}/notes{suffix}", self.group),
                &self.token,
                payload,
            ),
        )
        .await
    }

    async fn get(&self, id: &str) -> (StatusCode, Value) {
        send(
            &self.app,
            authed(
                "GET",
                &format!("/api/v2/groups/{}/notes/{id}", self.group),
                &self.token,
            ),
        )
        .await
    }

    async fn list(&self) -> Vec<Value> {
        let (status, rows) = send(
            &self.app,
            authed(
                "GET",
                &format!("/api/v2/groups/{}/notes", self.group),
                &self.token,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        rows.as_array().unwrap().clone()
    }

    fn note_path(&self, id: &str) -> PathBuf {
        group_note_file(self.root.path(), id)
    }
    fn index_path(&self) -> PathBuf {
        self.root.path().join("Notes/index.md")
    }
    fn index(&self) -> String {
        fs::read_to_string(self.index_path()).unwrap()
    }

    fn block_index(&self) {
        let index = self.index_path();
        fs::create_dir_all(index.parent().unwrap()).unwrap();
        if index.is_file() {
            fs::remove_file(&index).unwrap();
        }
        fs::create_dir(index).unwrap();
    }

    async fn snapshot(&self) -> Snapshot {
        Snapshot {
            rows: sqlx::query_as("SELECT id,title,content,status,created_at,updated_at FROM group_notes WHERE group_id=? ORDER BY id")
                .bind(&self.group).fetch_all(self.state.db.pool()).await.unwrap(),
            events: sqlx::query_as("SELECT seq,event_id,resource_id,payload_json FROM account_events WHERE group_id=? AND kind='note.changed' ORDER BY seq")
                .bind(&self.group).fetch_all(self.state.db.pool()).await.unwrap(),
            watermark: sqlx::query_scalar("SELECT high_watermark FROM account_event_counters WHERE owner_id=(SELECT owner_id FROM groups WHERE id=?)")
                .bind(&self.group).fetch_one(self.state.db.pool()).await.unwrap(),
        }
    }

    async fn reject_statement(&self, operation: &str) {
        sqlx::query(&format!("CREATE TRIGGER test_note_statement_failure BEFORE {operation} ON group_notes BEGIN SELECT RAISE(ABORT,'private note write failure'); END"))
            .execute(self.state.db.pool()).await.unwrap();
    }

    async fn reject_commit(&self, operation: &str) {
        assert_eq!(
            sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
                .fetch_one(self.state.db.pool())
                .await
                .unwrap(),
            1
        );
        for statement in [
            "CREATE TABLE test_note_commit_parent(id INTEGER PRIMARY KEY)",
            "CREATE TABLE test_note_commit_child(parent_id INTEGER REFERENCES test_note_commit_parent(id) DEFERRABLE INITIALLY DEFERRED)",
        ] {
            sqlx::query(statement).execute(self.state.db.pool()).await.unwrap();
        }
        sqlx::query(&format!("CREATE TRIGGER test_note_commit_failure AFTER {operation} ON group_notes BEGIN INSERT INTO test_note_commit_child(parent_id) VALUES(1); END"))
            .execute(self.state.db.pool()).await.unwrap();
    }

    async fn reopen(&mut self) {
        assert!(self.storage.path().is_dir());
        self.state.db.pool().close().await;
        self.state.db = Db::connect(self.url.as_ref().unwrap()).await.unwrap();
        self.state.db.migrate().await.unwrap();
        self.app = api::router(self.state.clone());
    }
}

fn internal_error(status: StatusCode, value: &Value, message: &str) {
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{value}");
    assert_eq!(
        *value,
        json!({"error":{"code":"internal_error","message":message,"details":{}}})
    );
}

fn index_warning(value: &Value) {
    assert_eq!(
        value["warning"],
        json!({"code":"note_index_pending","reason":"group note index path is not a regular file"})
    );
}

#[tokio::test]
async fn rejected_sql_statements_leave_database_file_index_and_events_unchanged() {
    for (method, operation, message) in [
        ("POST", "INSERT", "failed to create group note"),
        ("PATCH", "UPDATE", "failed to update group note"),
        ("DELETE", "UPDATE", "failed to delete group note"),
    ] {
        let fixture = Fixture::new(false).await;
        let note = fixture.create().await;
        let id = note["id"].as_str().unwrap();
        fs::write(fixture.note_path(id), "file before").unwrap();
        let before = fixture.snapshot().await;
        let index = fixture.index();
        fixture.reject_statement(operation).await;
        let (status, body) = fixture
            .mutate(
                method,
                (method != "POST").then_some(id),
                json!({"title":"After","content":"after"}),
            )
            .await;
        internal_error(status, &body, message);
        assert_eq!(fixture.snapshot().await, before);
        assert_eq!(fixture.index(), index);
        assert_eq!(
            fs::read_to_string(fixture.note_path(id)).unwrap(),
            "file before"
        );
        assert_eq!(
            fs::read_dir(fixture.root.path().join("Notes"))
                .unwrap()
                .count(),
            2
        );
    }
}

#[tokio::test]
async fn an_invalid_notes_directory_rolls_back_create_and_account_events() {
    let fixture = Fixture::new(false).await;
    let before = fixture.snapshot().await;
    fs::write(fixture.root.path().join("Notes"), "external obstruction").unwrap();
    let (status, body) = fixture
        .mutate("POST", None, json!({"title":"After","content":"after"}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], "invalid_input");
    assert_eq!(fixture.snapshot().await, before);
    assert_eq!(
        fs::read_to_string(fixture.root.path().join("Notes")).unwrap(),
        "external obstruction"
    );
}

#[tokio::test]
async fn a_non_file_note_path_rolls_back_update_and_delete() {
    for method in ["PATCH", "DELETE"] {
        let fixture = Fixture::new(false).await;
        let note = fixture.create().await;
        let id = note["id"].as_str().unwrap();
        let before = fixture.snapshot().await;
        let index = fixture.index();
        fs::remove_file(fixture.note_path(id)).unwrap();
        fs::create_dir(fixture.note_path(id)).unwrap();
        let (status, body) = fixture
            .mutate(method, Some(id), json!({"title":"After","content":"after"}))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"]["code"], "invalid_input");
        assert_eq!(fixture.snapshot().await, before);
        assert_eq!(fixture.index(), index);
        assert!(fixture.note_path(id).is_dir());
    }
}

#[tokio::test]
async fn a_deferred_constraint_rejects_commit_after_successful_statement_execution() {
    let fixture = Fixture::new(false).await;
    fixture.reject_commit("INSERT").await;
    let before = fixture.snapshot().await;
    let mut tx = fixture.state.db.begin_write().await.unwrap();
    sqlx::query("INSERT INTO group_notes(id,group_id,author_id,title,content,created_at,updated_at) SELECT ?,id,owner_id,'Control','control','now','now' FROM groups WHERE id=?")
        .bind(Uuid::new_v4().to_string()).bind(&fixture.group).execute(&mut *tx).await.unwrap();
    let inserted: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM group_notes")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(inserted, 1);
    let error = tx.commit().await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("787")
    );
    assert_eq!(fixture.snapshot().await, before);
}

#[tokio::test]
async fn failed_create_commit_leaves_no_published_note_or_registration_after_reopen() {
    let mut fixture = Fixture::new(true).await;
    let before = fixture.snapshot().await;
    fixture.reject_commit("INSERT").await;
    let (status, body) = fixture
        .mutate(
            "POST",
            None,
            json!({"title":"After","content":"orphan body"}),
        )
        .await;
    internal_error(status, &body, "failed to commit group note create");
    assert_eq!(fixture.snapshot().await, before);
    assert!(!fixture.index_path().exists());
    for reopen in [false, true] {
        if reopen {
            fixture.reopen().await;
        }
        assert_eq!(fixture.snapshot().await, before);
        assert!(fixture.list().await.is_empty());
        assert_eq!(
            fs::read_dir(fixture.root.path().join("Notes"))
                .unwrap()
                .count(),
            0,
            "a rejected COMMIT must not publish Markdown or retain its temporary file"
        );
        assert!(!fixture.index_path().exists());
    }
    fixture.state.db.pool().close().await;
}

#[tokio::test]
async fn failed_update_commit_preserves_the_original_file_even_after_reopen() {
    let mut fixture = Fixture::new(true).await;
    let note = fixture.create().await;
    let id = note["id"].as_str().unwrap();
    fs::write(fixture.note_path(id), "file before").unwrap();
    let before = fixture.snapshot().await;
    let index = fixture.index();
    fixture.reject_commit("UPDATE").await;
    let (status, body) = fixture
        .mutate(
            "PATCH",
            Some(id),
            json!({"title":"After","content":"after"}),
        )
        .await;
    internal_error(status, &body, "failed to commit group note update");
    for restart in [false, true] {
        if restart {
            fixture.reopen().await;
        }
        assert_eq!(fixture.snapshot().await, before);
        assert_eq!(
            fs::read_to_string(fixture.note_path(id)).unwrap(),
            "file before"
        );
        assert_eq!(fixture.index(), index);
        let (status, note) = fixture.get(id).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(note["title"], "Before");
        assert_eq!(note["content"], "file before");
        let list = fixture.list().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["title"], "Before");
        assert_eq!(
            list[0]["content"], "",
            "listing intentionally omits the body"
        );
    }
    fixture.state.db.pool().close().await;
}

#[tokio::test]
async fn failed_title_only_commit_keeps_the_independently_edited_file() {
    let fixture = Fixture::new(false).await;
    let note = fixture.create().await;
    let id = note["id"].as_str().unwrap();
    fs::write(fixture.note_path(id), "file before").unwrap();
    let before = fixture.snapshot().await;
    let index = fixture.index();
    fixture.reject_commit("UPDATE").await;
    let (status, body) = fixture
        .mutate("PATCH", Some(id), json!({"title":"After"}))
        .await;
    internal_error(status, &body, "failed to commit group note update");
    assert_eq!(fixture.snapshot().await, before);
    assert_eq!(fixture.index(), index);
    assert_eq!(fixture.get(id).await.1["content"], "file before");
}

#[tokio::test]
async fn failed_delete_commit_preserves_native_edits_after_reopen() {
    let mut fixture = Fixture::new(true).await;
    let note = fixture.create().await;
    let id = note["id"].as_str().unwrap();
    let executor = ToolExecutor::new(None)
        .unwrap()
        .with_group_notes(Some(fixture.root.path().join("Notes")))
        .unwrap();
    let edited = executor
        .execute(
            "EditGroupNote",
            json!({
                "path":format!("{id}.md"),
                "edits":[{"oldText":"db before","newText":"file-only newer body"}]
            }),
        )
        .await;
    assert_eq!(edited.status, ToolStatus::Completed, "{}", edited.output);
    assert_eq!(fixture.get(id).await.1["content"], "file-only newer body");
    let before = fixture.snapshot().await;
    let index = fixture.index();
    fixture.reject_commit("UPDATE").await;
    let (status, body) = fixture.mutate("DELETE", Some(id), json!({})).await;
    internal_error(status, &body, "failed to commit group note delete");
    for restart in [false, true] {
        if restart {
            fixture.reopen().await;
        }
        assert_eq!(fixture.snapshot().await, before);
        assert_eq!(fixture.index(), index);
        assert!(
            fixture.note_path(id).is_file(),
            "COMMIT rejection must preserve the native edit"
        );
        assert_eq!(
            fs::read_to_string(fixture.note_path(id)).unwrap(),
            "file-only newer body"
        );
        let (status, note) = fixture.get(id).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            note["content"], "file-only newer body",
            "failed deletion must not replace a native edit with the database fallback"
        );
    }
    fixture.state.db.pool().close().await;
}

#[tokio::test]
async fn a_post_commit_index_failure_acknowledges_the_created_identity_without_replaying_create() {
    let fixture = Fixture::new(false).await;
    fixture.block_index();
    let before = fixture.snapshot().await;
    let (status, body) = fixture
        .mutate("POST", None, json!({"title":"After","content":"after"}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    index_warning(&body);
    let id = body["id"].as_str().unwrap();
    assert_eq!(body["group_id"], fixture.group);
    assert_eq!(body["content"], "after");
    let snapshot = fixture.snapshot().await;
    assert_eq!(snapshot.rows.len(), 1);
    assert_eq!(snapshot.rows[0].0, id);
    assert_eq!(snapshot.events.len(), before.events.len() + 1);
    assert_eq!(snapshot.watermark, before.watermark + 1);
    assert_eq!(fs::read_to_string(fixture.note_path(id)).unwrap(), "after");
    assert_eq!(fixture.get(id).await.1["content"], "after");
    let list = fixture.list().await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], id);
    fs::remove_dir(fixture.index_path()).unwrap();
    assert_eq!(fixture.list().await.len(), 1);
    assert!(
        !fixture.index_path().exists(),
        "reads do not regenerate the index"
    );
    let (status, body) = fixture
        .mutate("PATCH", Some(id), json!({"title":"Revised"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.get("warning").is_none(),
        "ordinary responses stay compatible"
    );
    assert_eq!(body["id"], id);
    assert_eq!(body["content"], "after");
    assert_eq!(fixture.snapshot().await.rows.len(), 1);
    let index = fixture.index();
    for row in fixture.snapshot().await.rows {
        assert!(index.contains(&format!("({}.md)", row.0)));
    }
    let executor = ToolExecutor::new(None)
        .unwrap()
        .with_group_notes(Some(fixture.root.path().join("Notes")))
        .unwrap();
    let native = executor.execute("ReadGroupNotes", json!({})).await;
    assert_eq!(native.status, ToolStatus::Completed, "{}", native.output);
    assert!(native.output.contains(&format!("({id}.md)")));
}

#[tokio::test]
async fn a_post_commit_index_failure_keeps_the_update_and_account_event() {
    let fixture = Fixture::new(false).await;
    let note = fixture.create().await;
    let id = note["id"].as_str().unwrap();
    fixture.block_index();
    let before = fixture.snapshot().await;
    let (status, body) = fixture
        .mutate(
            "PATCH",
            Some(id),
            json!({"title":"After","content":"after"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    index_warning(&body);
    assert_eq!(body["id"], id);
    assert_eq!(body["content"], "after");
    let snapshot = fixture.snapshot().await;
    assert_eq!(snapshot.rows.len(), 1);
    assert_eq!(
        (
            snapshot.rows[0].1.as_str(),
            snapshot.rows[0].2.as_str(),
            snapshot.rows[0].3.as_str()
        ),
        ("After", "after", "active")
    );
    assert_eq!(snapshot.events.len(), before.events.len() + 1);
    assert_eq!(snapshot.watermark, before.watermark + 1);
    assert_eq!(fs::read_to_string(fixture.note_path(id)).unwrap(), "after");
    assert_eq!(fixture.get(id).await.1["content"], "after");
    fs::remove_dir(fixture.index_path()).unwrap();
    assert_eq!(fixture.get(id).await.1["title"], "After");
    assert!(!fixture.index_path().exists());
}

#[tokio::test]
async fn a_post_commit_index_failure_keeps_the_delete_and_a_repeat_delete_is_not_found() {
    let fixture = Fixture::new(false).await;
    let note = fixture.create().await;
    let id = note["id"].as_str().unwrap();
    fixture.block_index();
    let before = fixture.snapshot().await;
    let (status, body) = fixture.mutate("DELETE", Some(id), json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body,
        json!({"error":{
            "code":"note_index_pending",
            "message":"group note was deleted but its index could not be refreshed",
            "details":{"committed":true,"group_id":fixture.group,"note_id":id,
                "phase":"index_sync","reason":"group note index path is not a regular file"}
        }})
    );
    let snapshot = fixture.snapshot().await;
    assert_eq!(snapshot.rows[0].3, "deleted");
    assert_eq!(snapshot.events.len(), before.events.len() + 1);
    assert_eq!(snapshot.watermark, before.watermark + 1);
    assert!(!fixture.note_path(id).exists());
    assert_eq!(fixture.get(id).await.0, StatusCode::NOT_FOUND);
    assert!(fixture.list().await.is_empty());
    assert_eq!(
        fixture.mutate("DELETE", Some(id), json!({})).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fixture.snapshot().await,
        snapshot,
        "retrying the deletion does not emit a second event"
    );
}
