use std::{
    path::{Path, PathBuf},
    thread,
};

use sqlx::{QueryBuilder, Sqlite};

use super::{
    database::{Result, database_path, with_connection},
    merge::{self, MergeHistory},
    rebase, switch,
    switch::SwitchHistory,
};

const DELETE_SWITCH: &str =
    "DELETE FROM switch_history WHERE branch NOT IN (SELECT branch FROM existing_branches)";
const DELETE_MERGE: &str = "DELETE FROM merge_history WHERE destination NOT IN (SELECT branch FROM existing_branches) OR source NOT IN (SELECT branch FROM existing_branches)";
const DELETE_REBASE: &str = "DELETE FROM rebase_history WHERE source NOT IN (SELECT branch FROM existing_branches) OR target NOT IN (SELECT branch FROM existing_branches)";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryStore {
    database_path: PathBuf,
}

impl HistoryStore {
    pub fn new(common_directory: impl AsRef<Path>) -> Self {
        Self {
            database_path: database_path(common_directory.as_ref()),
        }
    }

    pub fn read_switch(&self) -> Result<SwitchHistory> {
        switch::read(&self.database_path)
    }

    pub fn read_merge(&self) -> Result<MergeHistory> {
        merge::read(&self.database_path)
    }

    pub fn last_rebase_target(&self, source: &str) -> Result<Option<String>> {
        Ok(rebase::read(&self.database_path)?
            .target_for(source)
            .map(str::to_owned))
    }

    pub fn record_switch(&self, branch: impl Into<String>) -> Result<()> {
        switch::write(&self.database_path, branch.into())
    }

    pub fn record_merge(
        &self,
        destination: impl Into<String>,
        source: impl Into<String>,
    ) -> Result<()> {
        merge::write(
            &self.database_path,
            merge::MergeRecord::new(destination, source),
        )
    }

    pub fn record_rebase(
        &self,
        source: impl Into<String>,
        target: impl Into<String>,
    ) -> Result<()> {
        rebase::write(
            &self.database_path,
            rebase::RebaseRecord::new(source, target),
        )
    }

    fn prune_in_background(&self, existing_branches: Vec<String>, delete_sql: &'static str) {
        // this shouldn't happen since the tui will early exit if there are no branches in the repository
        if existing_branches.is_empty() {
            return;
        }

        let path = self.database_path.to_owned();
        let _ = thread::Builder::new()
            .name("gitbranch-history-prune".to_owned())
            .spawn(move || {
                let _ = prune_missing(&path, existing_branches, delete_sql);
            });
    }

    pub fn prune_switch_in_background(&self, existing_branches: Vec<String>) {
        self.prune_in_background(existing_branches, DELETE_SWITCH);
    }

    pub fn prune_merge_in_background(&self, existing_branches: Vec<String>) {
        self.prune_in_background(existing_branches, DELETE_MERGE);
    }

    pub fn prune_rebase_in_background(&self, existing_branches: Vec<String>) {
        self.prune_in_background(existing_branches, DELETE_REBASE);
    }
}

fn prune_missing(
    database_path: &Path,
    existing_branches: Vec<String>,
    delete_sql: &'static str,
) -> Result<()> {
    with_connection(database_path, |connection| {
        Box::pin(async move {
            let mut query = QueryBuilder::<Sqlite>::new("WITH existing_branches(branch) AS (");
            query.push_values(existing_branches, |mut row, branch| {
                row.push_bind(branch);
            });
            query.push(") ").push(delete_sql);
            query.build().execute(connection).await?;
            Ok(())
        })
    })
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{DELETE_MERGE, DELETE_REBASE, DELETE_SWITCH, HistoryStore, prune_missing};

    struct HistoryFixture {
        _directory: TempDir,
        store: HistoryStore,
    }

    impl HistoryFixture {
        fn new() -> Self {
            let directory = TempDir::new().expect("temporary directory should be created");
            let store = HistoryStore::new(directory.path());
            for branch in ["feature", "deleted"] {
                store
                    .record_switch(branch)
                    .expect("switch should be recorded");
            }
            for (destination, source) in [
                ("main", "feature"),
                ("deleted", "feature"),
                ("main", "deleted"),
            ] {
                store
                    .record_merge(destination, source)
                    .expect("merge should be recorded");
            }
            for (source, target) in [
                ("feature", "main"),
                ("deleted", "main"),
                ("other", "deleted"),
            ] {
                store
                    .record_rebase(source, target)
                    .expect("rebase should be recorded");
            }

            Self {
                _directory: directory,
                store,
            }
        }

        fn prune(&self, delete_sql: &'static str) {
            prune_missing(&self.store.database_path, existing_branches(), delete_sql)
                .expect("history should be pruned");
        }

        fn switch_rank(&self, branch: &str) -> Option<usize> {
            self.store
                .read_switch()
                .expect("switch history should be readable")
                .rank(branch)
        }

        fn merge_rank(&self, destination: &str, source: &str) -> Option<usize> {
            self.store
                .read_merge()
                .expect("merge history should be readable")
                .rank(destination, source)
        }

        fn rebase_target(&self, source: &str) -> Option<String> {
            self.store
                .last_rebase_target(source)
                .expect("rebase history should be readable")
        }
    }

    fn existing_branches() -> Vec<String> {
        ["main", "feature", "other"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn pruning_removes_missing_branches_from_only_the_selected_history() {
        let fixture = HistoryFixture::new();

        fixture.prune(DELETE_SWITCH);

        assert_eq!(fixture.switch_rank("feature"), Some(1));
        assert_eq!(fixture.switch_rank("deleted"), None);
        assert_eq!(fixture.merge_rank("main", "deleted"), Some(3));
        assert_eq!(fixture.rebase_target("deleted"), Some("main".to_owned()));

        fixture
            .store
            .record_switch("review")
            .expect("a switch after pruning should be recorded");
        assert_eq!(fixture.switch_rank("review"), Some(3));

        fixture.prune(DELETE_MERGE);

        assert_eq!(fixture.merge_rank("main", "feature"), Some(1));
        assert_eq!(fixture.merge_rank("deleted", "feature"), None);
        assert_eq!(fixture.merge_rank("main", "deleted"), None);
        assert_eq!(fixture.rebase_target("other"), Some("deleted".to_owned()));

        fixture.prune(DELETE_REBASE);

        assert_eq!(fixture.rebase_target("feature"), Some("main".to_owned()));
        assert_eq!(fixture.rebase_target("deleted"), None);
        assert_eq!(fixture.rebase_target("other"), None);
        assert_eq!(fixture.switch_rank("review"), Some(3));
    }

    #[test]
    fn empty_branch_lists_skip_background_pruning() {
        let fixture = HistoryFixture::new();

        fixture.store.prune_switch_in_background(Vec::new());
        fixture.store.prune_merge_in_background(Vec::new());
        fixture.store.prune_rebase_in_background(Vec::new());

        assert_eq!(fixture.switch_rank("deleted"), Some(2));
        assert_eq!(fixture.merge_rank("main", "deleted"), Some(3));
        assert_eq!(fixture.rebase_target("other"), Some("deleted".to_owned()));
    }
}
