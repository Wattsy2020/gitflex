use std::{collections::BTreeMap, path::Path};

use super::history::{self, Result};

#[derive(Debug, Eq, PartialEq)]
pub struct RebaseRecord {
    source: String,
    target: String,
}

impl RebaseRecord {
    pub fn new(source: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
        }
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
pub(super) struct RebaseHistory {
    targets: BTreeMap<String, String>,
}

impl RebaseHistory {
    pub(super) fn target_for(&self, source: &str) -> Option<&str> {
        self.targets.get(source).map(String::as_str)
    }
}

pub(super) fn read(database_path: &Path) -> Result<RebaseHistory> {
    history::with_connection(database_path, |connection| {
        Box::pin(async move {
            let targets =
                sqlx::query_as::<_, (String, String)>("SELECT source, target FROM rebase_history")
                    .fetch_all(connection)
                    .await?
                    .into_iter()
                    .collect();
            Ok(RebaseHistory { targets })
        })
    })
}

pub(super) fn write(database_path: &Path, record: RebaseRecord) -> Result<()> {
    history::with_connection(database_path, |connection| {
        Box::pin(async move {
            sqlx::query(
                r#"
INSERT INTO rebase_history (source, target)
VALUES (?, ?)
ON CONFLICT (source) DO UPDATE SET target = excluded.target
"#,
            )
            .bind(record.source)
            .bind(record.target)
            .execute(connection)
            .await?;
            Ok(())
        })
    })
}

pub(super) fn prune(database_path: &Path, existing_branches: Vec<String>) -> Result<()> {
    history::prune(database_path, existing_branches, history::Table::Rebase)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{RebaseHistory, RebaseRecord, prune, read, write};
    use crate::git::{
        history::database_path,
        merge_history::{self, MergeRecord},
        switch_history,
    };

    #[test]
    fn missing_history_is_empty_and_records_are_replaced_per_source() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let database_path = database_path(directory.path());

        assert_eq!(
            read(&database_path).expect("missing history should be readable"),
            RebaseHistory::default()
        );

        write(&database_path, RebaseRecord::new("feature", "main"))
            .expect("first target should be recorded");
        write(&database_path, RebaseRecord::new("other", "develop"))
            .expect("another source target should be recorded");
        write(&database_path, RebaseRecord::new("feature", "release"))
            .expect("the feature target should be replaced");

        let history = read(&database_path).expect("history should be readable");
        assert_eq!(history.target_for("feature"), Some("release"));
        assert_eq!(history.target_for("other"), Some("develop"));
    }

    #[test]
    fn pruning_removes_rows_with_either_branch_missing_and_only_changes_rebase_history() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let database_path = database_path(directory.path());
        write(&database_path, RebaseRecord::new("feature", "main"))
            .expect("existing rebase should be recorded");
        write(&database_path, RebaseRecord::new("deleted", "main"))
            .expect("stale source should be recorded");
        write(&database_path, RebaseRecord::new("other", "deleted"))
            .expect("stale target should be recorded");
        switch_history::write(&database_path, "deleted".to_owned())
            .expect("switch should be recorded");
        merge_history::write(&database_path, MergeRecord::new("main", "deleted"))
            .expect("merge should be recorded");

        prune(
            &database_path,
            vec!["main".to_owned(), "feature".to_owned(), "other".to_owned()],
        )
        .expect("rebase history should be pruned");

        let rebase_history = read(&database_path).expect("rebase history should be readable");
        assert_eq!(rebase_history.target_for("feature"), Some("main"));
        assert_eq!(rebase_history.target_for("deleted"), None);
        assert_eq!(rebase_history.target_for("other"), None);
        assert_eq!(
            switch_history::read(&database_path)
                .expect("switch history should be readable")
                .rank("deleted"),
            Some(1)
        );
        assert_eq!(
            merge_history::read(&database_path)
                .expect("merge history should be readable")
                .rank("main", "deleted"),
            Some(1)
        );

        prune(&database_path, Vec::new()).expect("empty repositories should clear rebase history");
        assert_eq!(
            read(&database_path).expect("rebase history should be readable"),
            RebaseHistory::default()
        );
    }
}
