use super::{
    cancel_queued_job_in_tx, cancel_turn_effects_in_tx, insert_message_in_tx, payload_hash,
    reopen_paused_thread_in_tx, Command, MessageAcceptance, MessageAuthor, PreparedMessage,
};
use crate::{
    db::Db,
    runtime::group::{AttachmentKind, MessageAttachment},
};
use serde_json::{json, Value};
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

const NOW: &str = "2026-11-08T00:00:00Z";

async fn fixture() -> Db {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    for sql in [
        "INSERT INTO users(id,email,password_hash,name,created_at,updated_at) VALUES('owner','acceptance@example.test','hash','Owner','now','now')",
        "INSERT INTO groups(id,owner_id,name,created_at,updated_at) VALUES('group','owner','Team','now','now')",
        "INSERT INTO threads(id,group_id,next_seq,created_at,updated_at) VALUES('thread','group',6,'now','now')",
    ] {
        sqlx::query(sql).execute(db.pool()).await.unwrap();
    }
    db
}

fn command() -> Command {
    Command {
        request_id: Uuid::new_v4().to_string(),
        name: "message.send".into(),
        target: json!({"conversation_id":"group","thread_id":"thread"}),
        payload: json!({"content":"Hello"}),
    }
}

fn prepared() -> PreparedMessage {
    PreparedMessage {
        content: "Hello".into(),
        attachments: vec![MessageAttachment {
            id: "attachment".into(),
            path: "docs/example.txt".into(),
            name: "example.txt".into(),
            mime_type: "text/plain".into(),
            size: 42,
            kind: AttachmentKind::File,
        }],
        model_override: Some("test-model".into()),
        effort_override: Some("high".into()),
    }
}

async fn queue(tx: &mut Transaction<'_, Sqlite>, command: &Command) -> Value {
    insert_message_in_tx(
        tx,
        MessageAcceptance {
            owner: "owner",
            command,
            digest: &payload_hash(command),
            group: "group",
            thread: "thread",
            prepared: &prepared(),
            now: NOW,
            reuse_job: None,
            author: None,
        },
    )
    .await
    .unwrap()
}

async fn counts(db: &Db) -> Vec<i64> {
    let mut counts = Vec::new();
    for table in [
        "messages",
        "group_turns",
        "execution_jobs",
        "stream_events",
        "command_receipts",
        "account_events",
    ] {
        counts.push(
            sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(db.pool())
                .await
                .unwrap(),
        );
    }
    counts.push(
        sqlx::query_scalar("SELECT next_seq FROM threads WHERE id='thread'")
            .fetch_one(db.pool())
            .await
            .unwrap(),
    );
    counts
}

#[tokio::test]
async fn acceptance_preserves_identities_authorship_payload_and_unadmitted_history() {
    let db = fixture().await;
    let command = command();
    let mut tx = db.begin_write().await.unwrap();
    let response = queue(&mut tx, &command).await;
    tx.commit().await.unwrap();
    assert_eq!(
        response,
        json!({"request_id":command.request_id,"status":"accepted",
        "job_id":response["job_id"],"turn_id":response["turn_id"],
        "stream_id":command.request_id,"thread_id":"thread"})
    );
    for key in ["job_id", "turn_id", "stream_id"] {
        Uuid::parse_str(response[key].as_str().unwrap()).unwrap();
    }
    let (message, seq, sender, sender_id, kind, raw, admitted, turn): (String,i64,String,String,String,String,i64,String) =
        sqlx::query_as("SELECT id,seq,sender_type,sender_id,message_type,content_json,context_admitted,turn_id FROM messages")
            .fetch_one(db.pool()).await.unwrap();
    assert_eq!(
        (
            seq,
            sender.as_str(),
            sender_id.as_str(),
            kind.as_str(),
            admitted
        ),
        (6, "user", "owner", "text", 0)
    );
    assert_eq!(turn, response["turn_id"]);
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        json!({"version":1,"attachments":prepared().attachments})
    );
    let (job, request, status, generation, active, occupies, payload): (String,String,String,i64,i64,i64,String) =
        sqlx::query_as("SELECT id,request_id,status,generation,worker_active,occupies_thread,payload_json FROM execution_jobs")
            .fetch_one(db.pool()).await.unwrap();
    assert_eq!(job, response["job_id"]);
    assert_eq!(request, command.request_id);
    assert_eq!(
        (status.as_str(), generation, active, occupies),
        ("queued", 0, 0, 0)
    );
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::to_value(prepared()).unwrap()
    );
    let (status, strategy, config, topology): (String,String,String,String) =
        sqlx::query_as("SELECT status,scheduler_strategy,config_snapshot_json,topology_snapshot_json FROM group_turns")
            .fetch_one(db.pool()).await.unwrap();
    assert_eq!(
        (
            status.as_str(),
            strategy.as_str(),
            config.as_str(),
            topology.as_str()
        ),
        ("pending", "bounded", "{}", "{}")
    );
    let (stream, seq, id, kind, payload): (String, i64, String, String, String) =
        sqlx::query_as("SELECT stream_id,seq,event_id,kind,payload_json FROM stream_events")
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert_eq!(
        (stream.as_str(), seq, id.as_str(), kind.as_str()),
        (
            command.request_id.as_str(),
            0,
            format!("{}:0", command.request_id).as_str(),
            "user_message"
        )
    );
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        json!({
        "message_id":message,"thread_id":"thread","content":"Hello","attachments":prepared().attachments,
        "sender_type":"user","sender_id":"owner","message_type":"text"})
    );
    let (hash, raw): (String, String) =
        sqlx::query_as("SELECT payload_hash,response_json FROM command_receipts")
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert_eq!(hash, payload_hash(&command));
    assert_eq!(serde_json::from_str::<Value>(&raw).unwrap(), response);
}

#[tokio::test]
async fn server_authorship_and_metadata_do_not_turn_into_human_input() {
    for (sender, sender_id, kind, metadata) in [
        (
            "agent",
            Some("router"),
            "text",
            Some(json!({"router_dispatch":true})),
        ),
        (
            "system",
            None,
            "task_report",
            Some(json!({"task_reports":[{"task_id":"task"}]})),
        ),
        (
            "user",
            Some("owner"),
            "text",
            Some(json!({"automation":{"id":"timer"}})),
        ),
        ("system", None, "task_report", None),
    ] {
        let db = fixture().await;
        let command = command();
        let mut tx = db.begin_write().await.unwrap();
        insert_message_in_tx(
            &mut tx,
            MessageAcceptance {
                owner: "owner",
                command: &command,
                digest: &payload_hash(&command),
                group: "group",
                thread: "thread",
                prepared: &prepared(),
                now: NOW,
                reuse_job: None,
                author: Some(MessageAuthor {
                    sender_type: sender,
                    sender_id,
                    message_type: kind,
                    content_json: metadata.as_ref().map(Value::to_string),
                }),
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        let row: (String, Option<String>, String, Option<String>) =
            sqlx::query_as("SELECT sender_type,sender_id,message_type,content_json FROM messages")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(
            row,
            (
                sender.into(),
                sender_id.map(str::to_string),
                kind.into(),
                metadata.as_ref().map(Value::to_string)
            )
        );
        let raw: String = sqlx::query_scalar("SELECT payload_json FROM stream_events")
            .fetch_one(db.pool())
            .await
            .unwrap();
        let event: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(event["sender_type"], sender);
        assert_eq!(event["sender_id"], json!(sender_id));
        assert_eq!(event["message_type"], kind);
    }
}

#[tokio::test]
async fn only_active_turns_queue_a_new_reservation() {
    for (existing, expected) in [
        ("pending", "queued"),
        ("running", "queued"),
        ("waiting_for_user", "queued"),
        ("queued", "pending"),
        ("completed", "pending"),
        ("failed", "pending"),
        ("cancelled", "pending"),
        ("superseded", "pending"),
    ] {
        let db = fixture().await;
        sqlx::query("INSERT INTO group_turns(id,thread_id,group_id,status,scheduler_strategy,config_snapshot_json,topology_snapshot_json,created_at,updated_at) VALUES('old','thread','group',?,'bounded','{}','{}','now','now')")
            .bind(existing).execute(db.pool()).await.unwrap();
        let mut tx = db.begin_write().await.unwrap();
        let response = queue(&mut tx, &command()).await;
        tx.commit().await.unwrap();
        let status: String = sqlx::query_scalar("SELECT status FROM group_turns WHERE id=?")
            .bind(response["turn_id"].as_str())
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(status, expected, "existing status {existing}");
    }
}

#[tokio::test]
async fn a_used_stream_key_gets_a_new_stream_without_changing_the_receipt_identity() {
    for collision in ["job", "event"] {
        let db = fixture().await;
        let command = command();
        if collision == "job" {
            sqlx::query("INSERT INTO execution_jobs(id,owner_id,request_id,command_name,group_id,thread_id,turn_id,stream_id,payload_json,status,created_at,updated_at) VALUES('old','owner','different-request','message.send','group','thread','old-turn',?,'{}','succeeded','now','now')")
                .bind(&command.request_id).execute(db.pool()).await.unwrap();
        } else {
            sqlx::query("INSERT INTO stream_events(id,stream_id,thread_id,seq,event_id,kind,payload_json,created_at) VALUES('old',?,'thread',0,?,'done','{}','now')")
                .bind(&command.request_id).bind(format!("{}:0",command.request_id)).execute(db.pool()).await.unwrap();
        }
        let mut tx = db.begin_write().await.unwrap();
        let response = queue(&mut tx, &command).await;
        tx.commit().await.unwrap();
        assert_eq!(response["request_id"], command.request_id);
        assert_ne!(response["stream_id"], command.request_id);
        Uuid::parse_str(response["stream_id"].as_str().unwrap()).unwrap();
        let request: String =
            sqlx::query_scalar("SELECT request_id FROM execution_jobs WHERE id=?")
                .bind(response["job_id"].as_str())
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(request, command.request_id);
    }
}

#[tokio::test]
async fn caller_rollback_restores_every_record_event_and_sequence() {
    let db = fixture().await;
    let before = counts(&db).await;
    let mut tx = db.begin_write().await.unwrap();
    queue(&mut tx, &command()).await;
    tx.rollback().await.unwrap();
    assert_eq!(counts(&db).await, before);
}

#[tokio::test]
async fn a_write_failure_at_each_acceptance_stage_is_rolled_back_by_the_caller() {
    for table in [
        "messages",
        "group_turns",
        "stream_events",
        "execution_jobs",
        "command_receipts",
    ] {
        let db = fixture().await;
        let before = counts(&db).await;
        sqlx::query(&format!("CREATE TRIGGER inject_failure BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT,'acceptance write rejected'); END"))
            .execute(db.pool()).await.unwrap();
        let mut tx = db.begin_write().await.unwrap();
        let command = command();
        let result = insert_message_in_tx(
            &mut tx,
            MessageAcceptance {
                owner: "owner",
                command: &command,
                digest: &payload_hash(&command),
                group: "group",
                thread: "thread",
                prepared: &prepared(),
                now: NOW,
                reuse_job: None,
                author: None,
            },
        )
        .await;
        assert!(result.is_err(), "failed insert into {table}");
        tx.rollback().await.unwrap();
        assert_eq!(counts(&db).await, before, "failure at {table}");
    }
}

#[tokio::test]
async fn duplicate_request_never_commits_a_second_acceptance() {
    let db = fixture().await;
    let command = command();
    let mut tx = db.begin_write().await.unwrap();
    let response = queue(&mut tx, &command).await;
    tx.commit().await.unwrap();
    let before = counts(&db).await;
    let mut tx = db.begin_write().await.unwrap();
    let result = insert_message_in_tx(
        &mut tx,
        MessageAcceptance {
            owner: "owner",
            command: &command,
            digest: &payload_hash(&command),
            group: "group",
            thread: "thread",
            prepared: &prepared(),
            now: NOW,
            reuse_job: None,
            author: None,
        },
    )
    .await;
    assert!(result.is_err());
    tx.rollback().await.unwrap();
    assert_eq!(counts(&db).await, before);
    let receipt: String = sqlx::query_scalar("SELECT response_json FROM command_receipts")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&receipt).unwrap(), response);
}

#[tokio::test]
async fn reuse_job_keeps_generation_occupancy_and_original_receipt_while_rotating_the_stream() {
    let db = fixture().await;
    let original = command();
    let mut tx = db.begin_write().await.unwrap();
    let first = queue(&mut tx, &original).await;
    tx.commit().await.unwrap();
    sqlx::query("UPDATE execution_jobs SET status='waiting_for_user',generation=7,worker_active=1,occupies_thread=1,error='old error'")
        .execute(db.pool()).await.unwrap();
    sqlx::query("UPDATE group_turns SET status='waiting_for_user',agent_steps=5,total_tokens=123")
        .execute(db.pool())
        .await
        .unwrap();
    let next = command();
    let mut tx = db.begin_write().await.unwrap();
    let response = insert_message_in_tx(
        &mut tx,
        MessageAcceptance {
            owner: "owner",
            command: &next,
            digest: &payload_hash(&next),
            group: "group",
            thread: "thread",
            prepared: &prepared(),
            now: NOW,
            reuse_job: first["job_id"].as_str(),
            author: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(response["job_id"], first["job_id"]);
    assert_ne!(response["turn_id"], first["turn_id"]);
    assert_ne!(response["stream_id"], first["stream_id"]);
    let (request,generation,active,occupies,status,error,turn,stream):(String,i64,i64,i64,String,Option<String>,String,String)=
        sqlx::query_as("SELECT request_id,generation,worker_active,occupies_thread,status,error,turn_id,stream_id FROM execution_jobs")
            .fetch_one(db.pool()).await.unwrap();
    assert_eq!(
        (request, generation, active, occupies, status, error),
        (original.request_id, 7, 1, 1, "queued".into(), None)
    );
    assert_eq!(turn, response["turn_id"]);
    assert_eq!(stream, response["stream_id"]);
    let budget: (i64, i64) =
        sqlx::query_as("SELECT agent_steps,total_tokens FROM group_turns WHERE id=?")
            .bind(first["turn_id"].as_str())
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert_eq!(budget, (5, 123));
    let seqs: Vec<i64> = sqlx::query_scalar("SELECT seq FROM messages ORDER BY seq")
        .fetch_all(db.pool())
        .await
        .unwrap();
    assert_eq!(seqs, vec![6, 7]);
    let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM command_receipts")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(receipts, 2);
}

#[tokio::test]
async fn reopening_only_a_paused_thread_preserves_approvals_and_turns() {
    for status in ["paused", "active", "running", "archived"] {
        let db = fixture().await;
        let mut tx = db.begin_write().await.unwrap();
        queue(&mut tx, &command()).await;
        tx.commit().await.unwrap();
        sqlx::query("UPDATE threads SET status=?")
            .bind(status)
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("UPDATE messages SET status='interrupted'")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO pending_approvals(id,owner_id,group_id,thread_id,tool_call_id,action_hash,action_json,request_json,created_at,updated_at) VALUES('approval','owner','group','thread','call','hash','{}','{}','now','now')")
            .execute(db.pool()).await.unwrap();
        let mut tx = db.begin_write().await.unwrap();
        reopen_paused_thread_in_tx(&mut tx, "thread", NOW)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let actual:(String,String,String,String)=sqlx::query_as("SELECT t.status,m.status,a.status,r.status FROM threads t,messages m,pending_approvals a,group_turns r")
            .fetch_one(db.pool()).await.unwrap();
        assert_eq!(
            actual,
            (
                if status == "paused" { "active" } else { status }.into(),
                if status == "paused" {
                    "visible"
                } else {
                    "interrupted"
                }
                .into(),
                "pending".into(),
                "pending".into()
            )
        );
    }
}

#[tokio::test]
async fn cancelling_unclaimed_work_is_idempotent_and_ignores_claimed_workers() {
    for (status, worker, expected) in [
        ("pending", 0, "cancelled"),
        ("queued", 0, "cancelled"),
        ("running", 1, "running"),
    ] {
        let db = fixture().await;
        let mut tx = db.begin_write().await.unwrap();
        let response = queue(&mut tx, &command()).await;
        tx.commit().await.unwrap();
        sqlx::query("UPDATE group_turns SET status=?")
            .bind(status)
            .execute(db.pool())
            .await
            .unwrap();
        if worker == 1 {
            sqlx::query(
                "UPDATE execution_jobs SET status='running',worker_active=1,occupies_thread=1",
            )
            .execute(db.pool())
            .await
            .unwrap();
        }
        for _ in 0..2 {
            let mut tx = db.begin_write().await.unwrap();
            cancel_queued_job_in_tx(&mut tx, response["job_id"].as_str().unwrap())
                .await
                .unwrap();
            tx.commit().await.unwrap();
        }
        let result:(String,String,i64,i64)=sqlx::query_as("SELECT r.status,j.status,j.worker_active,j.occupies_thread FROM group_turns r,execution_jobs j")
            .fetch_one(db.pool()).await.unwrap();
        assert_eq!(result, (expected.into(), expected.into(), worker, worker));
        let events: Vec<(i64, String, String, String)> =
            sqlx::query_as("SELECT seq,event_id,kind,payload_json FROM stream_events ORDER BY seq")
                .fetch_all(db.pool())
                .await
                .unwrap();
        if worker == 1 {
            assert_eq!(events.len(), 1);
        } else {
            assert_eq!(events.len(), 3);
            assert_eq!(events[1].0, 1);
            assert_eq!(
                events[1].1,
                format!("{}:1", response["stream_id"].as_str().unwrap())
            );
            assert_eq!(events[1].2, "turn_cancelled");
            assert_eq!(
                serde_json::from_str::<Value>(&events[1].3).unwrap(),
                json!({"turn_id":response["turn_id"],"reason":"superseded"})
            );
            assert_eq!((events[2].0, events[2].2.as_str()), (2, "done"));
        }
    }
}

#[tokio::test]
async fn cancellation_preserves_worker_occupancy_and_only_cancels_its_checkpoint_approval() {
    let db = fixture().await;
    let mut tx = db.begin_write().await.unwrap();
    let response = queue(&mut tx, &command()).await;
    tx.commit().await.unwrap();
    sqlx::query("UPDATE group_turns SET status='running'")
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE execution_jobs SET status='running',worker_active=1,occupies_thread=1")
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE messages SET status='interrupted',content_json=?")
        .bind(json!({"tool_calls":[{"tool_call_id":"own-call"}]}).to_string())
        .execute(db.pool())
        .await
        .unwrap();
    for call in ["own-call", "newer-call"] {
        sqlx::query("INSERT INTO pending_approvals(id,owner_id,group_id,thread_id,tool_call_id,action_hash,action_json,request_json,created_at,updated_at) VALUES(?,'owner','group','thread',?,'hash','{}','{}','now','now')")
            .bind(call).bind(call).execute(db.pool()).await.unwrap();
    }
    let mut tx = db.begin_write().await.unwrap();
    cancel_turn_effects_in_tx(&mut tx, response["turn_id"].as_str().unwrap(), "thread")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let result:(String,String,i64,i64)=sqlx::query_as("SELECT r.status,j.status,j.worker_active,j.occupies_thread FROM group_turns r,execution_jobs j")
        .fetch_one(db.pool()).await.unwrap();
    assert_eq!(result, ("cancelled".into(), "cancelled".into(), 1, 1));
    let approvals: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT tool_call_id,status,version FROM pending_approvals ORDER BY tool_call_id",
    )
    .fetch_all(db.pool())
    .await
    .unwrap();
    assert_eq!(
        approvals,
        vec![
            ("newer-call".into(), "pending".into(), 1),
            ("own-call".into(), "cancelled".into(), 2)
        ]
    );
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM stream_events")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(events, 1, "a live worker owns its own terminal events");
}

#[tokio::test]
async fn failure_to_write_the_cancelled_stream_rolls_back_state_and_approval_changes() {
    let db = fixture().await;
    let mut tx = db.begin_write().await.unwrap();
    let response = queue(&mut tx, &command()).await;
    tx.commit().await.unwrap();
    sqlx::query("UPDATE messages SET status='interrupted',content_json=?")
        .bind(json!({"tool_calls":[{"tool_call_id":"call"}]}).to_string())
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO pending_approvals(id,owner_id,group_id,thread_id,tool_call_id,action_hash,action_json,request_json,created_at,updated_at) VALUES('approval','owner','group','thread','call','hash','{}','{}','now','now')")
        .execute(db.pool()).await.unwrap();
    let before = counts(&db).await;
    sqlx::query("CREATE TRIGGER reject_done BEFORE INSERT ON stream_events WHEN NEW.kind='done' BEGIN SELECT RAISE(ABORT,'terminal rejected'); END")
        .execute(db.pool()).await.unwrap();
    let mut tx = db.begin_write().await.unwrap();
    assert!(
        cancel_turn_effects_in_tx(&mut tx, response["turn_id"].as_str().unwrap(), "thread")
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    assert_eq!(counts(&db).await, before);
    let state: (String, String) =
        sqlx::query_as("SELECT r.status,j.status FROM group_turns r,execution_jobs j")
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert_eq!(state, ("pending".into(), "queued".into()));
    let approval: (String, i64) = sqlx::query_as("SELECT status,version FROM pending_approvals")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(approval, ("pending".into(), 1));
}
