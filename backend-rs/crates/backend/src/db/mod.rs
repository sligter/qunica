use std::{str::FromStr, time::Duration};

use sqlx::{
    migrate::{Migration, Migrator},
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    SqlitePool,
};
use thiserror::Error;

static MIGRATOR: Migrator = sqlx::migrate!("./src/db/migrations");

const APPEARANCE_MIGRATION_VERSION: i64 = 2;
const APPEARANCE_MIGRATION_DESCRIPTION: &str = "system settings appearance";
const LEGACY_APPEARANCE_MIGRATION_DESCRIPTION: &str = "system_settings_appearance";

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
        reconcile_legacy_appearance_migration_checksum(&self.pool).await?;
        MIGRATOR.run(&self.pool).await?;
        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use super::{
        appearance_column_matches_target_schema, migration_by_version,
        reconcile_legacy_appearance_migration_checksum, Db, APPEARANCE_MIGRATION_DESCRIPTION,
        APPEARANCE_MIGRATION_VERSION,
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
}
