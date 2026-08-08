use std::{collections::BTreeMap, path::Path};

use super::{self as history, Result};

#[derive(Debug, Eq, PartialEq)]
pub(super) struct RebaseRecord {
    source: String,
    target: String,
}

impl RebaseRecord {
    pub(super) fn new(source: impl Into<String>, target: impl Into<String>) -> Self {
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

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{RebaseHistory, RebaseRecord, read, write};
    use crate::history::database_path;

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
}
