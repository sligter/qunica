use super::*;

#[tokio::test]
async fn failed_acceptance_keeps_http_error_contract_and_leaves_no_partial_work() {
    for table in [
        "messages",
        "group_turns",
        "stream_events",
        "execution_jobs",
        "command_receipts",
    ] {
        let (app, state, token, group) = fixture().await;
        sqlx::query(&format!("CREATE TRIGGER reject_acceptance BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT,'private storage failure'); END"))
            .execute(state.db.pool()).await.unwrap();
        let (status, body) = request(
            &app,
            "POST",
            "/api/v2/commands",
            Some(&token),
            json!({
                "request_id":Uuid::new_v4().to_string(),"name":"message.send",
                "target":{"conversation_id":group},"payload":{"content":"Hello"}
            }),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "insert into {table}: {body}"
        );
        assert_eq!(
            body,
            json!({"error":{"code":"internal_error","message":"database error","details":{}}})
        );
        for table in [
            "threads",
            "messages",
            "group_turns",
            "stream_events",
            "execution_jobs",
            "command_receipts",
        ] {
            let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(state.db.pool())
                .await
                .unwrap();
            assert_eq!(count, 0, "partial {table}");
        }
    }
}

#[tokio::test]
async fn cancellation_storage_errors_keep_their_distinct_messages_and_roll_back_the_turn() {
    for (table, message) in [
        ("group_turns", "failed to cancel turn"),
        ("execution_jobs", "database error"),
    ] {
        let (app, state, token, group) = fixture().await;
        sqlx::query(
            "INSERT INTO threads(id,group_id,created_at,updated_at) VALUES('thread',?,'now','now')",
        )
        .bind(&group)
        .execute(state.db.pool())
        .await
        .unwrap();
        sqlx::query("INSERT INTO group_turns(id,thread_id,group_id,status,scheduler_strategy,config_snapshot_json,topology_snapshot_json,created_at,updated_at) VALUES('turn','thread',?,'pending','bounded','{}','{}','now','now')")
            .bind(&group).execute(state.db.pool()).await.unwrap();
        sqlx::query("INSERT INTO execution_jobs(id,owner_id,request_id,command_name,group_id,thread_id,turn_id,stream_id,payload_json,status,created_at,updated_at) SELECT 'job',owner_id,'original-request','message.send',id,'thread','turn','stream','{}','queued','now','now' FROM groups WHERE id=?")
            .bind(&group).execute(state.db.pool()).await.unwrap();
        sqlx::query(&format!("CREATE TRIGGER reject_cancellation BEFORE UPDATE ON {table} BEGIN SELECT RAISE(ABORT,'private cancellation failure'); END"))
            .execute(state.db.pool()).await.unwrap();
        let (status, body) = request(&app, "POST", "/api/v2/commands", Some(&token), json!({
            "request_id":Uuid::new_v4().to_string(),"name":"turn.cancel","target":{"turn_id":"turn"},"payload":{}
        })).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            body,
            json!({"error":{"code":"internal_error","message":message,"details":{}}})
        );
        let state_row: (String, String) =
            sqlx::query_as("SELECT r.status,j.status FROM group_turns r,execution_jobs j")
                .fetch_one(state.db.pool())
                .await
                .unwrap();
        assert_eq!(state_row, ("pending".into(), "queued".into()));
        for table in ["stream_events", "command_receipts"] {
            let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(state.db.pool())
                .await
                .unwrap();
            assert_eq!(count, 0, "partial {table}");
        }
    }
}
