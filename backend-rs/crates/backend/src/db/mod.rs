use std::{str::FromStr, time::Duration};

use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    SqlitePool,
};
use thiserror::Error;

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
        sqlx::migrate!("./src/db/migrations")
            .run(&self.pool)
            .await?;
        Ok(())
    }
}
