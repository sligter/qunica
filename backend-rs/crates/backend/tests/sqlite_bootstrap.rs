use qunica_backend::db::Db;

#[tokio::test]
async fn sqlite_bootstrap_enables_wal_and_creates_core_tables() {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();

    let journal_mode: (String,) = sqlx::query_as("PRAGMA journal_mode")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert!(
        journal_mode.0.eq_ignore_ascii_case("wal") || journal_mode.0.eq_ignore_ascii_case("memory")
    );

    let table_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('users', 'messages', 'stream_events')",
    )
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(table_count.0, 3);

    let columns: Vec<(String, String, Option<String>, i64)> = sqlx::query_as(
        "SELECT name, type, dflt_value, \"notnull\" FROM pragma_table_info('groups') \
         WHERE name IN ('conversation_kind', 'direct_agent_id', 'title_source') ORDER BY name",
    )
    .fetch_all(db.pool())
    .await
    .unwrap();
    assert_eq!(
        columns,
        vec![
            (
                "conversation_kind".to_string(),
                "TEXT".to_string(),
                Some("'group'".to_string()),
                1
            ),
            ("direct_agent_id".to_string(), "TEXT".to_string(), None, 0),
            (
                "title_source".to_string(),
                "TEXT".to_string(),
                Some("'manual'".to_string()),
                1
            ),
        ]
    );
}

#[tokio::test]
async fn sqlite_history_indexes_avoid_payload_scans_and_snapshot_sorting() {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    let expiry_plan: Vec<(i64, i64, i64, String)> = sqlx::query_as(
        "EXPLAIN QUERY PLAN SELECT owner_id,seq FROM account_events WHERE occurred_at < '2026-01-01' ORDER BY occurred_at,owner_id,seq LIMIT 256",
    ).fetch_all(db.pool()).await.unwrap();
    assert!(
        expiry_plan
            .iter()
            .any(|row| row.3.contains("SEARCH") && row.3.contains("COVERING INDEX")),
        "{expiry_plan:?}"
    );
    assert!(
        !expiry_plan.iter().any(|row| row.3.contains("TEMP B-TREE")),
        "{expiry_plan:?}"
    );
    let snapshot_plan: Vec<(i64, i64, i64, String)> = sqlx::query_as(
        "EXPLAIN QUERY PLAN SELECT stream_id,MAX(seq) FROM stream_events WHERE thread_id='thread' GROUP BY stream_id",
    ).fetch_all(db.pool()).await.unwrap();
    assert!(
        snapshot_plan
            .iter()
            .any(|row| row.3.contains("COVERING INDEX")),
        "{snapshot_plan:?}"
    );
    assert!(
        !snapshot_plan
            .iter()
            .any(|row| row.3.contains("TEMP B-TREE")),
        "{snapshot_plan:?}"
    );
    let delete_plan: Vec<(i64, i64, i64, String)> = sqlx::query_as(
        "EXPLAIN QUERY PLAN DELETE FROM account_events WHERE (owner_id,seq) IN (VALUES ('owner',1),('owner',2)) AND (occurred_at < strftime('%Y-%m-%dT%H:%M:%fZ','now','-7 days') OR seq <= (SELECT MAX(c.retained_after,c.high_watermark-100000) FROM account_event_counters c WHERE c.owner_id=account_events.owner_id)) RETURNING owner_id,seq",
    ).fetch_all(db.pool()).await.unwrap();
    assert!(
        delete_plan
            .iter()
            .any(|row| row.3.contains("SEARCH account_events")
                && row.3.contains("owner_id=? AND seq=?")),
        "{delete_plan:?}"
    );
    assert!(
        !delete_plan
            .iter()
            .any(|row| row.3.contains("SCAN account_events")),
        "{delete_plan:?}"
    );
}
