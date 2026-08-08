use std::{collections::HashMap, path::Path};

use super::{self as history, Result};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SwitchHistory {
    rankings: HashMap<String, usize>,
}

impl SwitchHistory {
    pub fn rank(&self, branch: &str) -> Option<usize> {
        self.rankings.get(branch).copied()
    }

    #[cfg(test)]
    pub(crate) fn for_test(branches: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let branches = branches.into_iter().map(Into::into).collect::<Vec<_>>();
        Self {
            rankings: branches
                .into_iter()
                .rev()
                .enumerate()
                .map(|(rank, branch)| (branch, rank + 1))
                .collect(),
        }
    }
}

pub(super) fn read(database_path: &Path) -> Result<SwitchHistory> {
    history::with_connection(database_path, |connection| {
        Box::pin(async move {
            let rankings =
                sqlx::query_as::<_, (String, i64)>("SELECT branch, rank FROM switch_history")
                    .fetch_all(connection)
                    .await?
                    .into_iter()
                    .map(|(branch, rank)| Ok((branch, history::rank_from_database(rank)?)))
                    .collect::<Result<_>>()?;

            Ok(SwitchHistory { rankings })
        })
    })
}

pub(super) fn write(database_path: &Path, branch: String) -> Result<()> {
    history::with_connection(database_path, |connection| {
        Box::pin(async move {
            sqlx::query(
                r#"
INSERT INTO switch_history (branch)
VALUES (?)
ON CONFLICT (branch) DO UPDATE SET rank = excluded.rank
"#,
            )
            .bind(branch)
            .execute(connection)
            .await?;
            Ok(())
        })
    })
}

pub(super) fn prune(database_path: &Path, existing_branches: Vec<String>) -> Result<()> {
    history::prune(database_path, existing_branches, history::Table::Switch)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::{Arc, Barrier},
    };

    use tempfile::TempDir;

    use super::{SwitchHistory, prune, read, write};
    use crate::history::{
        database_path,
        merge::{self, MergeRecord},
        rebase::{self, RebaseRecord},
    };

    #[test]
    fn missing_history_is_empty_and_repeated_branches_receive_the_next_rank() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let database_path = database_path(directory.path());

        assert_eq!(
            read(&database_path).expect("missing history should be readable"),
            SwitchHistory::default()
        );

        write(&database_path, "feature".to_owned()).expect("first switch should be recorded");
        write(&database_path, "review".to_owned()).expect("second switch should be recorded");
        write(&database_path, "feature".to_owned()).expect("repeated switch should be recorded");

        let history = read(&database_path).expect("history should be readable");
        assert_eq!(history.rank("feature"), Some(3));
        assert_eq!(history.rank("review"), Some(2));
        assert!(database_path.exists());
        assert!(!directory.path().join("gitbranch-switches").exists());
        assert!(!directory.path().join("gitbranch-switches.lock").exists());
    }

    #[test]
    fn legacy_history_is_ignored_and_left_untouched() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let legacy_path = directory.path().join("gitbranch-switches");
        fs::write(&legacy_path, "feature\t99\n").expect("legacy history should be written");

        assert_eq!(
            read(&database_path(directory.path())).expect("history should be readable"),
            SwitchHistory::default()
        );
        assert_eq!(
            fs::read_to_string(legacy_path).expect("legacy history should remain readable"),
            "feature\t99\n"
        );
    }

    #[test]
    fn pruning_removes_missing_branches_and_only_changes_switch_history() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let database_path = database_path(directory.path());
        write(&database_path, "feature".to_owned()).expect("existing switch should be recorded");
        write(&database_path, "deleted".to_owned()).expect("stale switch should be recorded");
        merge::write(&database_path, MergeRecord::new("main", "deleted"))
            .expect("merge should be recorded");
        rebase::write(&database_path, RebaseRecord::new("main", "deleted"))
            .expect("rebase should be recorded");

        prune(
            &database_path,
            vec!["main".to_owned(), "feature".to_owned()],
        )
        .expect("switch history should be pruned");

        let switch_history = read(&database_path).expect("switch history should be readable");
        assert_eq!(switch_history.rank("feature"), Some(1));
        assert_eq!(switch_history.rank("deleted"), None);
        write(&database_path, "review".to_owned())
            .expect("a switch after pruning should be recorded");
        assert_eq!(
            read(&database_path)
                .expect("updated switch history should be readable")
                .rank("review"),
            Some(3)
        );
        assert_eq!(
            merge::read(&database_path)
                .expect("merge history should be readable")
                .rank("main", "deleted"),
            Some(1)
        );
        assert_eq!(
            rebase::read(&database_path)
                .expect("rebase history should be readable")
                .target_for("main"),
            Some("deleted")
        );

        prune(&database_path, Vec::new()).expect("empty repositories should clear switch history");
        assert_eq!(
            read(&database_path).expect("switch history should be readable"),
            SwitchHistory::default()
        );
    }

    #[test]
    fn concurrent_writers_preserve_every_branch() {
        const WRITER_COUNT: usize = 8;

        let directory = TempDir::new().expect("temporary directory should be created");
        let database_path = Arc::new(database_path(directory.path()));
        read(&database_path).expect("database schema should be created");
        let barrier = Arc::new(Barrier::new(WRITER_COUNT));
        let writers = (0..WRITER_COUNT)
            .map(|index| {
                let database_path = Arc::clone(&database_path);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    write(&database_path, format!("branch-{index}"))
                })
            })
            .collect::<Vec<_>>();

        for writer in writers {
            writer
                .join()
                .expect("history writer should not panic")
                .expect("history writer should succeed");
        }

        let history = read(&database_path).expect("history should be readable");
        let mut ranks = (0..WRITER_COUNT)
            .map(|index| {
                history
                    .rank(&format!("branch-{index}"))
                    .expect("concurrent branch should be recorded")
            })
            .collect::<Vec<_>>();
        ranks.sort_unstable();
        assert_eq!(ranks, (1..=WRITER_COUNT).collect::<Vec<_>>());
    }
}
