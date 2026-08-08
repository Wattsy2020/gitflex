use std::{collections::HashMap, path::Path};

use super::history::{self, Result};

#[derive(Debug, Eq, PartialEq)]
pub(super) struct MergeRecord {
    destination: String,
    source: String,
}

impl MergeRecord {
    pub(super) fn new(destination: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            destination: destination.into(),
            source: source.into(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MergeHistory {
    rankings: HashMap<(String, String), usize>,
}

impl MergeHistory {
    pub fn rank(&self, destination: &str, source: &str) -> Option<usize> {
        self.rankings
            .get(&(destination.to_owned(), source.to_owned()))
            .copied()
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        merges: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        let merges = merges
            .into_iter()
            .map(|(destination, source)| (destination.into(), source.into()))
            .collect::<Vec<_>>();
        Self {
            rankings: merges
                .into_iter()
                .rev()
                .enumerate()
                .map(|(rank, merge)| (merge, rank + 1))
                .collect(),
        }
    }
}

pub(super) fn read(database_path: &Path) -> Result<MergeHistory> {
    history::with_connection(database_path, |connection| {
        Box::pin(async move {
            let rankings = sqlx::query_as::<_, (String, String, i64)>(
                "SELECT destination, source, rank FROM merge_history",
            )
            .fetch_all(connection)
            .await?
            .into_iter()
            .map(|(destination, source, rank)| {
                Ok(((destination, source), history::rank_from_database(rank)?))
            })
            .collect::<Result<_>>()?;

            Ok(MergeHistory { rankings })
        })
    })
}

pub(super) fn write(database_path: &Path, record: MergeRecord) -> Result<()> {
    history::with_connection(database_path, |connection| {
        Box::pin(async move {
            sqlx::query(
                r#"
INSERT INTO merge_history (destination, source)
VALUES (?, ?)
ON CONFLICT (destination, source) DO UPDATE SET rank = excluded.rank
"#,
            )
            .bind(record.destination)
            .bind(record.source)
            .execute(connection)
            .await?;
            Ok(())
        })
    })
}

pub(super) fn prune(database_path: &Path, existing_branches: Vec<String>) -> Result<()> {
    history::prune(database_path, existing_branches, history::Table::Merge)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{MergeHistory, MergeRecord, prune, read, write};
    use crate::git::{
        history::database_path,
        rebase_history::{self, RebaseRecord},
        switch_history,
    };

    #[test]
    fn missing_history_is_empty_and_destinations_are_independent() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let database_path = database_path(directory.path());

        assert_eq!(
            read(&database_path).expect("missing history should be readable"),
            MergeHistory::default()
        );

        write(&database_path, MergeRecord::new("main", "feature"))
            .expect("first merge should be recorded");
        write(&database_path, MergeRecord::new("release", "feature"))
            .expect("second destination should be recorded");
        write(&database_path, MergeRecord::new("main", "feature"))
            .expect("repeated merge should be recorded");

        let history = read(&database_path).expect("history should be readable");
        assert_eq!(history.rank("main", "feature"), Some(3));
        assert_eq!(history.rank("release", "feature"), Some(2));
        assert_eq!(history.rank("main", "unknown"), None);
    }

    #[test]
    fn pruning_removes_rows_with_either_branch_missing_and_only_changes_merge_history() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let database_path = database_path(directory.path());
        write(&database_path, MergeRecord::new("main", "feature"))
            .expect("existing merge should be recorded");
        write(&database_path, MergeRecord::new("deleted", "feature"))
            .expect("stale destination should be recorded");
        write(&database_path, MergeRecord::new("main", "deleted"))
            .expect("stale source should be recorded");
        switch_history::write(&database_path, "deleted".to_owned())
            .expect("switch should be recorded");
        rebase_history::write(&database_path, RebaseRecord::new("main", "deleted"))
            .expect("rebase should be recorded");

        prune(
            &database_path,
            vec!["main".to_owned(), "feature".to_owned()],
        )
        .expect("merge history should be pruned");

        let merge_history = read(&database_path).expect("merge history should be readable");
        assert_eq!(merge_history.rank("main", "feature"), Some(1));
        assert_eq!(merge_history.rank("deleted", "feature"), None);
        assert_eq!(merge_history.rank("main", "deleted"), None);
        assert_eq!(
            switch_history::read(&database_path)
                .expect("switch history should be readable")
                .rank("deleted"),
            Some(1)
        );
        assert_eq!(
            rebase_history::read(&database_path)
                .expect("rebase history should be readable")
                .target_for("main"),
            Some("deleted")
        );

        prune(&database_path, Vec::new()).expect("empty repositories should clear merge history");
        assert_eq!(
            read(&database_path).expect("merge history should be readable"),
            MergeHistory::default()
        );
    }
}
