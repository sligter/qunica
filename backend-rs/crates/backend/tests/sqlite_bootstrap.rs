use ag_swarmer_backend::db::Db;

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
}
