use std::{
    debug_assert_matches,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    time::Duration,
};

use sqlx::{ConnectOptions, Connection, Executor, SqliteConnection, sqlite::SqliteConnectOptions};
use thiserror::Error;

pub const DATABASE_FILE_NAME: &str = "gitflex-history.sqlite3";

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS switch_history (
    rank INTEGER PRIMARY KEY AUTOINCREMENT CHECK (rank > 0),
    branch TEXT NOT NULL UNIQUE CHECK (branch <> '')
) STRICT;

CREATE TABLE IF NOT EXISTS merge_history (
    rank INTEGER PRIMARY KEY AUTOINCREMENT CHECK (rank > 0),
    destination TEXT NOT NULL CHECK (destination <> ''),
    source TEXT NOT NULL CHECK (source <> ''),
    UNIQUE (destination, source)
) STRICT;

CREATE TABLE IF NOT EXISTS rebase_history (
    source TEXT PRIMARY KEY NOT NULL CHECK (source <> ''),
    target TEXT NOT NULL CHECK (target <> '')
) STRICT;
"#;

type ConnectionFuture<'connection, T> = Pin<Box<dyn Future<Output = Result<T>> + 'connection>>;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Runtime(#[from] std::io::Error),
    #[error("history database contains invalid rank {0}")]
    InvalidRank(i64),
}

pub(super) fn database_path(common_directory: &Path) -> PathBuf {
    common_directory.join(DATABASE_FILE_NAME)
}

pub(super) fn with_connection<T>(
    database_path: &Path,
    operation: impl for<'connection> FnOnce(
        &'connection mut SqliteConnection,
    ) -> ConnectionFuture<'connection, T>,
) -> Result<T> {
    let runtime = tokio::runtime::Builder::new_current_thread().build()?;
    runtime.block_on(async {
        let options = SqliteConnectOptions::new()
            .filename(database_path)
            .create_if_missing(true)
            .busy_timeout(Duration::from_secs(5));
        let mut connection = options.connect().await?;
        let result = async {
            connection.execute(SCHEMA).await?;
            operation(&mut connection).await
        }
        .await;

        // not much we can do if we fail to close the database
        let close_result = connection.close().await.map_err(Error::from);
        debug_assert_matches!(close_result, Ok(()));

        result
    })
}

pub(super) fn rank_from_database(rank: i64) -> Result<usize> {
    usize::try_from(rank)
        .ok()
        .filter(|rank| *rank > 0)
        .ok_or(Error::InvalidRank(rank))
}
