use std::{str::FromStr, time::Duration};

use sqlx::{
    migrate::{Migration, Migrator},
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    SqlitePool,
};
use thiserror::Error;

static MIGRATOR: Migrator = sqlx::migrate!("./src/db/migrations");

type SchemaMatchFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool, sqlx::Error>> + Send + 'a>>;
type SchemaMatchFn = for<'a> fn(&'a SqlitePool) -> SchemaMatchFuture<'a>;

const APPEARANCE_MIGRATION_VERSION: i64 = 2;
const APPEARANCE_MIGRATION_DESCRIPTION: &str = "system settings appearance";
const LEGACY_APPEARANCE_MIGRATION_DESCRIPTION: &str = "system_settings_appearance";
const DISPATCH_IDEMPOTENCY_MIGRATION_VERSION: i64 = 27;
const DISPATCH_IDEMPOTENCY_MIGRATION_DESCRIPTION: &str = "dispatch idempotency";
const INITIAL_MIGRATION_VERSION: i64 = 1;
const INITIAL_MIGRATION_DESCRIPTION: &str = "initial";
const SCHEDULER_MIGRATION_VERSION: i64 = 3;
const SCHEDULER_MIGRATION_DESCRIPTION: &str = "bounded group scheduler";

#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

#[derive(Debug, Error)]
pub enum DbError {
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

impl Db {
    pub async fn connect(database_url: &str) -> Result<Self, DbError> {
        let options = SqliteConnectOptions::from_str(database_url)?
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_millis(5000))
            .foreign_keys(true);

        // An in-memory database lives only inside a single connection, so pin the
        // pool to one persistent connection. Otherwise migrations and later queries
        // could land on different connections that each see an empty database.
        let is_memory = database_url.contains(":memory:");
        let pool = if is_memory {
            SqlitePoolOptions::new()
                .max_connections(1)
                .min_connections(1)
                .idle_timeout(None)
                .max_lifetime(None)
                .connect_with(options)
                .await?
        } else {
            SqlitePoolOptions::new()
                .max_connections(8)
                .connect_with(options)
                .await?
        };
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn migrate(&self) -> Result<(), DbError> {
        reconcile_legacy_initial_migration_checksum(&self.pool).await?;
        reconcile_legacy_appearance_migration_checksum(&self.pool).await?;
        reconcile_legacy_scheduler_migration_checksum(&self.pool).await?;
        reconcile_legacy_dispatch_idempotency_checksum(&self.pool).await?;
        MIGRATOR.run(&self.pool).await?;
        Ok(())
    }
}

async fn reconcile_legacy_scheduler_migration_checksum(
    pool: &SqlitePool,
) -> Result<(), sqlx::Error> {
    reconcile_checksum_when_schema_matches(
        pool,
        SCHEDULER_MIGRATION_VERSION,
        SCHEDULER_MIGRATION_DESCRIPTION,
        scheduler_schema_matches_target,
    )
    .await
}

async fn reconcile_legacy_dispatch_idempotency_checksum(
    pool: &SqlitePool,
) -> Result<(), sqlx::Error> {
    reconcile_checksum_when_schema_matches(
        pool,
        DISPATCH_IDEMPOTENCY_MIGRATION_VERSION,
        DISPATCH_IDEMPOTENCY_MIGRATION_DESCRIPTION,
        legacy_dispatch_idempotency_schema_matches,
    )
    .await
}

async fn reconcile_checksum_when_schema_matches(
    pool: &SqlitePool,
    version: i64,
    description: &str,
    schema_matches: SchemaMatchFn,
) -> Result<(), sqlx::Error> {
    let Some(migration) = migration_by_version(version, description) else {
        return Ok(());
    };
    if !table_exists(pool, "_sqlx_migrations").await? {
        return Ok(());
    }
    let applied: Option<(Vec<u8>, bool)> =
        sqlx::query_as("SELECT checksum, success FROM _sqlx_migrations WHERE version = ?1")
            .bind(version)
            .fetch_optional(pool)
            .await?;
    let Some((checksum, success)) = applied else {
        return Ok(());
    };
    if !success || checksum == migration.checksum.as_ref() || !schema_matches(pool).await? {
        return Ok(());
    }
    tracing::warn!(
        version,
        description,
        "repairing legacy checksum for already-applied alpha migration"
    );
    sqlx::query("UPDATE _sqlx_migrations SET checksum = ?1 WHERE version = ?2")
        .bind(migration.checksum.as_ref())
        .bind(version)
        .execute(pool)
        .await?;
    Ok(())
}

async fn reconcile_legacy_initial_migration_checksum(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let Some(migration) =
        migration_by_version(INITIAL_MIGRATION_VERSION, INITIAL_MIGRATION_DESCRIPTION)
    else {
        return Ok(());
    };
    if !table_exists(pool, "_sqlx_migrations").await? {
        return Ok(());
    }

    let applied: Option<(Vec<u8>, bool)> =
        sqlx::query_as("SELECT checksum, success FROM _sqlx_migrations WHERE version = ?1")
            .bind(INITIAL_MIGRATION_VERSION)
            .fetch_optional(pool)
            .await?;
    let Some((checksum, success)) = applied else {
        return Ok(());
    };
    if !success
        || checksum == migration.checksum.as_ref()
        || !initial_schema_matches_target(pool).await?
    {
        return Ok(());
    }

    tracing::warn!(
        version = INITIAL_MIGRATION_VERSION,
        description = INITIAL_MIGRATION_DESCRIPTION,
        "repairing legacy checksum for already-applied alpha initial migration"
    );
    sqlx::query("UPDATE _sqlx_migrations SET checksum = ?1 WHERE version = ?2")
        .bind(migration.checksum.as_ref())
        .bind(INITIAL_MIGRATION_VERSION)
        .execute(pool)
        .await?;
    Ok(())
}

async fn reconcile_legacy_appearance_migration_checksum(
    pool: &SqlitePool,
) -> Result<(), sqlx::Error> {
    let Some(migration) = migration_by_version(
        APPEARANCE_MIGRATION_VERSION,
        APPEARANCE_MIGRATION_DESCRIPTION,
    ) else {
        return Ok(());
    };

    if !table_exists(pool, "_sqlx_migrations").await? {
        return Ok(());
    }

    let applied: Option<(String, Vec<u8>, bool)> = sqlx::query_as(
        "SELECT description, checksum, success FROM _sqlx_migrations WHERE version = ?1",
    )
    .bind(APPEARANCE_MIGRATION_VERSION)
    .fetch_optional(pool)
    .await?;
    let Some((description, checksum, success)) = applied else {
        return Ok(());
    };

    if !is_appearance_migration_description(&description) || !success {
        return Ok(());
    }
    if checksum == migration.checksum.as_ref() {
        return Ok(());
    }
    if !appearance_column_matches_target_schema(pool).await? {
        return Ok(());
    }

    tracing::warn!(
        version = APPEARANCE_MIGRATION_VERSION,
        description = APPEARANCE_MIGRATION_DESCRIPTION,
        "repairing legacy checksum for already-applied alpha migration"
    );
    sqlx::query("UPDATE _sqlx_migrations SET checksum = ?1 WHERE version = ?2")
        .bind(migration.checksum.as_ref())
        .bind(APPEARANCE_MIGRATION_VERSION)
        .execute(pool)
        .await?;
    Ok(())
}

fn migration_by_version(version: i64, description: &str) -> Option<&'static Migration> {
    MIGRATOR
        .iter()
        .find(|migration| migration.version == version && migration.description == description)
}

fn is_appearance_migration_description(description: &str) -> bool {
    description == APPEARANCE_MIGRATION_DESCRIPTION
        || description == LEGACY_APPEARANCE_MIGRATION_DESCRIPTION
}

async fn table_exists(pool: &SqlitePool, table_name: &str) -> Result<bool, sqlx::Error> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1")
            .bind(table_name)
            .fetch_one(pool)
            .await?;
    Ok(count > 0)
}

async fn appearance_column_matches_target_schema(pool: &SqlitePool) -> Result<bool, sqlx::Error> {
    let row: Option<(String, i64, Option<String>)> = sqlx::query_as(
        r#"
        SELECT type, "notnull", dflt_value
        FROM pragma_table_info('system_settings')
        WHERE name = 'appearance'
        "#,
    )
    .fetch_optional(pool)
    .await?;
    let Some((column_type, not_null, default_value)) = row else {
        return Ok(false);
    };

    Ok(column_type.eq_ignore_ascii_case("TEXT")
        && not_null == 1
        && default_value.as_deref().is_some_and(|value| {
            value.eq_ignore_ascii_case("'system'") || value.eq_ignore_ascii_case("system")
        }))
}

async fn initial_schema_matches_target(pool: &SqlitePool) -> Result<bool, sqlx::Error> {
    let required = [
        ("llm_providers", "context_window_tokens"),
        ("llm_providers", "context_output_reserve_ratio"),
        ("llm_providers", "description"),
        ("skills", "source"),
        ("system_settings", "id"),
        ("system_settings", "tavily_api_key"),
        ("system_settings", "tavily_search_url"),
        ("groups", "announcement"),
        ("groups", "communication_mode"),
        ("group_agents", "topology_role"),
        ("group_agents", "response_mode"),
    ];
    for (table, column) in required {
        if !column_exists(pool, table, column).await? {
            return Ok(false);
        }
    }
    if !table_exists(pool, "group_notes").await? {
        return Ok(false);
    }
    table_exists(pool, "group_files").await
}

async fn column_exists(pool: &SqlitePool, table: &str, column: &str) -> Result<bool, sqlx::Error> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2")
            .bind(table)
            .bind(column)
            .fetch_one(pool)
            .await?;
    Ok(count > 0)
}

fn scheduler_schema_matches_target(
    pool: &SqlitePool,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool, sqlx::Error>> + Send + '_>> {
    Box::pin(async move {
        let required = [
            ("groups", "scheduler_enabled"),
            ("groups", "agent_mention_policy"),
            ("groups", "max_agent_steps"),
            ("groups", "turn_timeout_seconds"),
            ("groups", "moderator_model"),
            ("messages", "turn_id"),
            ("messages", "dispatch_id"),
            ("messages", "reply_to_message_id"),
        ];
        for (table, column) in required {
            if !column_exists(pool, table, column).await? {
                return Ok(false);
            }
        }
        Ok(table_exists(pool, "group_turns").await?
            && table_exists(pool, "agent_dispatches").await?)
    })
}

fn legacy_dispatch_idempotency_schema_matches(
    pool: &SqlitePool,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool, sqlx::Error>> + Send + '_>> {
    Box::pin(column_exists(pool, "agent_dispatches", "dispatch_key"))
}

#[cfg(test)]
mod tests {
    use super::{
        appearance_column_matches_target_schema, migration_by_version,
        reconcile_legacy_appearance_migration_checksum,
        reconcile_legacy_dispatch_idempotency_checksum,
        reconcile_legacy_scheduler_migration_checksum, scheduler_schema_matches_target, Db,
        APPEARANCE_MIGRATION_DESCRIPTION, APPEARANCE_MIGRATION_VERSION,
        DISPATCH_IDEMPOTENCY_MIGRATION_DESCRIPTION, DISPATCH_IDEMPOTENCY_MIGRATION_VERSION,
        SCHEDULER_MIGRATION_DESCRIPTION, SCHEDULER_MIGRATION_VERSION,
    };
    use sqlx::SqlitePool;

    #[tokio::test]
    async fn repairs_legacy_appearance_checksum_when_schema_matches() {
        let pool = legacy_migration_pool(true).await;
        let old_checksum = applied_checksum(&pool).await;

        reconcile_legacy_appearance_migration_checksum(&pool)
            .await
            .unwrap();

        let repaired_checksum = applied_checksum(&pool).await;
        let expected_checksum = migration_by_version(
            APPEARANCE_MIGRATION_VERSION,
            APPEARANCE_MIGRATION_DESCRIPTION,
        )
        .unwrap()
        .checksum
        .as_ref()
        .to_vec();

        assert_ne!(old_checksum, expected_checksum);
        assert_eq!(repaired_checksum, expected_checksum);
    }

    #[tokio::test]
    async fn repairs_legacy_appearance_checksum_with_underscore_description() {
        let pool = legacy_migration_pool_with_description(
            true,
            super::LEGACY_APPEARANCE_MIGRATION_DESCRIPTION,
        )
        .await;

        reconcile_legacy_appearance_migration_checksum(&pool)
            .await
            .unwrap();

        let expected_checksum = migration_by_version(
            APPEARANCE_MIGRATION_VERSION,
            APPEARANCE_MIGRATION_DESCRIPTION,
        )
        .unwrap()
        .checksum
        .as_ref()
        .to_vec();

        assert_eq!(applied_checksum(&pool).await, expected_checksum);
    }

    #[tokio::test]
    async fn leaves_legacy_appearance_checksum_when_schema_does_not_match() {
        let pool = legacy_migration_pool(false).await;
        let old_checksum = applied_checksum(&pool).await;

        reconcile_legacy_appearance_migration_checksum(&pool)
            .await
            .unwrap();

        assert_eq!(applied_checksum(&pool).await, old_checksum);
        assert!(!appearance_column_matches_target_schema(&pool)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn repairs_legacy_scheduler_checksum_when_schema_matches() {
        let pool = scheduler_migration_pool(true).await;
        reconcile_legacy_scheduler_migration_checksum(&pool)
            .await
            .unwrap();

        let expected_checksum =
            migration_by_version(SCHEDULER_MIGRATION_VERSION, SCHEDULER_MIGRATION_DESCRIPTION)
                .unwrap()
                .checksum
                .as_ref()
                .to_vec();
        assert_eq!(scheduler_checksum(&pool).await, expected_checksum);
    }

    #[tokio::test]
    async fn leaves_legacy_scheduler_checksum_when_schema_does_not_match() {
        let pool = scheduler_migration_pool(false).await;
        let old_checksum = scheduler_checksum(&pool).await;
        reconcile_legacy_scheduler_migration_checksum(&pool)
            .await
            .unwrap();

        assert_eq!(scheduler_checksum(&pool).await, old_checksum);
        assert!(!scheduler_schema_matches_target(&pool).await.unwrap());
    }

    #[tokio::test]
    async fn repairs_removed_dispatch_idempotency_migration() {
        let db = Db::connect("sqlite::memory:").await.unwrap();
        let pool = db.pool();
        sqlx::query(
            r#"
            CREATE TABLE _sqlx_migrations (
                version BIGINT PRIMARY KEY,
                description TEXT NOT NULL,
                installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                success BOOLEAN NOT NULL,
                checksum BLOB NOT NULL,
                execution_time BIGINT NOT NULL
            );
            CREATE TABLE agent_dispatches (id TEXT PRIMARY KEY, dispatch_key TEXT);
            INSERT INTO _sqlx_migrations
                (version, description, success, checksum, execution_time)
            VALUES (?1, ?2, TRUE, X'010203', 0);
            "#,
        )
        .bind(DISPATCH_IDEMPOTENCY_MIGRATION_VERSION)
        .bind(DISPATCH_IDEMPOTENCY_MIGRATION_DESCRIPTION)
        .execute(pool)
        .await
        .unwrap();

        reconcile_legacy_dispatch_idempotency_checksum(pool)
            .await
            .unwrap();

        let actual: Vec<u8> =
            sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = ?1")
                .bind(DISPATCH_IDEMPOTENCY_MIGRATION_VERSION)
                .fetch_one(pool)
                .await
                .unwrap();
        let expected = migration_by_version(
            DISPATCH_IDEMPOTENCY_MIGRATION_VERSION,
            DISPATCH_IDEMPOTENCY_MIGRATION_DESCRIPTION,
        )
        .unwrap()
        .checksum
        .as_ref()
        .to_vec();
        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn unified_scheduler_migration_converts_only_disabled_rows() {
        let db = Db::connect("sqlite::memory:").await.unwrap();
        db.migrate().await.unwrap();
        let pool = db.pool();
        sqlx::query(
            "INSERT INTO users (id, email, password_hash, name, created_at, updated_at) \
             VALUES ('owner', 'owner@example.com', 'hash', 'Owner', 'now', 'now')",
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO groups (id, owner_id, name, scheduler_enabled, agent_mention_policy, \
                max_steps_per_agent, max_scheduler_hops, max_moderator_calls, \
                max_consecutive_failures, max_total_failures, moderator_enabled, \
                allow_agent_free_mention, agent_free_mention_max_dispatches, created_at, updated_at) \
             VALUES ('legacy', 'owner', 'Legacy', 0, 'bounded_schedule', 3, 5, 4, 3, 6, 1, 1, 8, 'now', 'now'), \
                    ('bounded', 'owner', 'Bounded', 1, 'bounded_schedule', 4, 7, 2, 4, 8, 1, 1, 9, 'now', 'now')",
        )
        .execute(pool)
        .await
        .unwrap();

        sqlx::query(include_str!("migrations/0014_unified_group_scheduler.sql"))
            .execute(pool)
            .await
            .unwrap();

        let legacy: (
            i64,
            String,
            Option<i64>,
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
        ) = sqlx::query_as(
            "SELECT scheduler_enabled, agent_mention_policy, max_agent_steps, \
                        max_steps_per_agent, max_scheduler_hops, max_moderator_calls, \
                        max_consecutive_failures, max_total_failures, moderator_enabled, \
                        allow_agent_free_mention, agent_free_mention_max_dispatches \
                 FROM groups WHERE id = 'legacy'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(
            legacy,
            (1, "display_only".to_owned(), None, 1, 0, 0, 1, 1, 0, 0, 0)
        );

        let bounded: (i64, String, i64, i64) = sqlx::query_as(
            "SELECT scheduler_enabled, agent_mention_policy, max_steps_per_agent, max_scheduler_hops \
             FROM groups WHERE id = 'bounded'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(bounded, (1, "bounded_schedule".to_owned(), 4, 7));
    }

    async fn legacy_migration_pool(with_appearance_column: bool) -> SqlitePool {
        legacy_migration_pool_with_description(
            with_appearance_column,
            APPEARANCE_MIGRATION_DESCRIPTION,
        )
        .await
    }

    async fn legacy_migration_pool_with_description(
        with_appearance_column: bool,
        description: &str,
    ) -> SqlitePool {
        let db = Db::connect("sqlite::memory:").await.unwrap();
        let pool = db.pool().clone();
        sqlx::query(
            r#"
            CREATE TABLE _sqlx_migrations (
                version BIGINT PRIMARY KEY,
                description TEXT NOT NULL,
                installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                success BOOLEAN NOT NULL,
                checksum BLOB NOT NULL,
                execution_time BIGINT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(if with_appearance_column {
            r#"
            CREATE TABLE system_settings (
                id TEXT PRIMARY KEY,
                appearance TEXT NOT NULL DEFAULT 'system'
            )
            "#
        } else {
            r#"
            CREATE TABLE system_settings (
                id TEXT PRIMARY KEY
            )
            "#
        })
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO _sqlx_migrations
                (version, description, success, checksum, execution_time)
            VALUES
                (?1, ?2, TRUE, ?3, 0)
            "#,
        )
        .bind(APPEARANCE_MIGRATION_VERSION)
        .bind(description)
        .bind(vec![1_u8, 2, 3])
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn applied_checksum(pool: &SqlitePool) -> Vec<u8> {
        sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = ?1")
            .bind(APPEARANCE_MIGRATION_VERSION)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn scheduler_migration_pool(with_complete_schema: bool) -> SqlitePool {
        let db = Db::connect("sqlite::memory:").await.unwrap();
        let pool = db.pool().clone();
        sqlx::query(
            r#"
            CREATE TABLE _sqlx_migrations (
                version BIGINT PRIMARY KEY,
                description TEXT NOT NULL,
                installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                success BOOLEAN NOT NULL,
                checksum BLOB NOT NULL,
                execution_time BIGINT NOT NULL
            );
            CREATE TABLE groups (
                scheduler_enabled INTEGER,
                agent_mention_policy TEXT,
                max_agent_steps INTEGER,
                turn_timeout_seconds INTEGER,
                moderator_model TEXT
            );
            CREATE TABLE messages (
                turn_id TEXT,
                dispatch_id TEXT,
                reply_to_message_id TEXT
            );
            CREATE TABLE group_turns (id TEXT);
            CREATE TABLE agent_dispatches (id TEXT);
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        if !with_complete_schema {
            sqlx::query("DROP TABLE agent_dispatches")
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (?1, ?2, TRUE, ?3, 0)",
        )
        .bind(SCHEDULER_MIGRATION_VERSION)
        .bind(SCHEDULER_MIGRATION_DESCRIPTION)
        .bind(vec![4_u8, 5, 6])
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn scheduler_checksum(pool: &SqlitePool) -> Vec<u8> {
        sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = ?1")
            .bind(SCHEDULER_MIGRATION_VERSION)
            .fetch_one(pool)
            .await
            .unwrap()
    }
}
