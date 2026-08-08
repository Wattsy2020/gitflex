use std::{
    debug_assert_matches,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    thread,
    time::Duration,
};

use sqlx::{
    ConnectOptions, Connection, Executor, QueryBuilder, Sqlite, SqliteConnection,
    sqlite::SqliteConnectOptions,
};
use thiserror::Error;

pub(super) const DATABASE_FILE_NAME: &str = "gitbranch-history.sqlite3";

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
type PruneOperation = fn(&Path, Vec<String>) -> Result<()>;

pub(super) type Result<T> = std::result::Result<T, Error>;

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

#[derive(Clone, Copy)]
pub(super) enum Table {
    Switch,
    Merge,
    Rebase,
}

impl Table {
    fn delete_all(self) -> &'static str {
        match self {
            Self::Switch => "DELETE FROM switch_history",
            Self::Merge => "DELETE FROM merge_history",
            Self::Rebase => "DELETE FROM rebase_history",
        }
    }

    fn delete_missing(self) -> &'static str {
        match self {
            Self::Switch => {
                "DELETE FROM switch_history WHERE branch NOT IN (SELECT branch FROM existing_branches)"
            }
            Self::Merge => {
                "DELETE FROM merge_history WHERE destination NOT IN (SELECT branch FROM existing_branches) OR source NOT IN (SELECT branch FROM existing_branches)"
            }
            Self::Rebase => {
                "DELETE FROM rebase_history WHERE source NOT IN (SELECT branch FROM existing_branches) OR target NOT IN (SELECT branch FROM existing_branches)"
            }
        }
    }
}

pub(super) fn prune(
    database_path: &Path,
    existing_branches: Vec<String>,
    table: Table,
) -> Result<()> {
    with_connection(database_path, |connection| {
        Box::pin(async move {
            if existing_branches.is_empty() {
                connection.execute(table.delete_all()).await?;
            } else {
                let mut query = QueryBuilder::<Sqlite>::new("WITH existing_branches(branch) AS (");
                query.push_values(existing_branches, |mut row, branch| {
                    row.push_bind(branch);
                });
                query.push(") ").push(table.delete_missing());
                query.build().execute(connection).await?;
            }
            Ok(())
        })
    })
}

pub(super) fn prune_in_background(
    database_path: &Path,
    existing_branches: Vec<String>,
    operation: PruneOperation,
) {
    let database_path = database_path.to_owned();
    let _ = thread::Builder::new()
        .name("gitbranch-history-prune".to_owned())
        .spawn(move || {
            let _ = operation(&database_path, existing_branches);
        });
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
