use std::{collections::HashSet, io, path::Path};

use super::history_file::HistoryFile;

const HISTORY_FILE_NAME: &str = "gitbranch-switches";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SwitchHistory {
    branches: Vec<String>,
}

impl SwitchHistory {
    fn parse(contents: &str) -> Result<Self, InvalidRecord> {
        let mut seen = HashSet::new();
        let branches = contents
            .lines()
            .enumerate()
            .map(|(index, branch)| {
                if branch.is_empty() || !seen.insert(branch) {
                    Err(InvalidRecord {
                        line_number: index + 1,
                    })
                } else {
                    Ok(branch.to_owned())
                }
            })
            .collect::<Result<_, _>>()?;

        Ok(Self { branches })
    }

    pub fn rank(&self, branch: &str) -> Option<usize> {
        self.branches
            .iter()
            .position(|candidate| candidate == branch)
    }

    #[cfg(test)]
    pub(crate) fn for_test(branches: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            branches: branches.into_iter().map(Into::into).collect(),
        }
    }

    fn record(&mut self, branch: &str) {
        self.branches.retain(|candidate| candidate != branch);
        self.branches.insert(0, branch.to_owned());
    }

    fn serialize(&self) -> String {
        self.branches
            .iter()
            .map(|branch| format!("{branch}\n"))
            .collect()
    }
}

#[derive(Debug)]
pub struct SwitchHistoryStore {
    file: HistoryFile,
}

impl SwitchHistoryStore {
    pub fn new(common_directory: &Path) -> Self {
        Self {
            file: HistoryFile::new(common_directory, HISTORY_FILE_NAME),
        }
    }

    pub fn load(&self) -> io::Result<SwitchHistory> {
        let Some(contents) = self.file.read()? else {
            return Ok(SwitchHistory::default());
        };

        match std::str::from_utf8(&contents)
            .ok()
            .and_then(|contents| SwitchHistory::parse(contents).ok())
        {
            Some(history) => Ok(history),
            None => {
                self.file.remove()?;
                Ok(SwitchHistory::default())
            }
        }
    }

    pub fn record(&self, branch: &str) -> io::Result<()> {
        let lock = self.file.lock()?;
        let mut history = self.load()?;
        history.record(branch);
        lock.commit(history.serialize().as_bytes())
    }
}

#[derive(Debug, Eq, PartialEq)]
struct InvalidRecord {
    line_number: usize,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::{HISTORY_FILE_NAME, InvalidRecord, SwitchHistory, SwitchHistoryStore};

    #[test]
    fn parses_and_serializes_history_in_most_recent_order() {
        let history = SwitchHistory::parse("feature-b\nfeature-a\n").expect("history should parse");

        assert_eq!(history.rank("feature-b"), Some(0));
        assert_eq!(history.rank("feature-a"), Some(1));
        assert_eq!(history.rank("unknown"), None);
        assert_eq!(history.serialize(), "feature-b\nfeature-a\n");
    }

    #[test]
    fn rejects_empty_and_duplicate_records() {
        assert_eq!(
            SwitchHistory::parse("feature\n\n").expect_err("empty record should be rejected"),
            InvalidRecord { line_number: 2 }
        );
        assert_eq!(
            SwitchHistory::parse("feature\nfeature\n")
                .expect_err("duplicate record should be rejected"),
            InvalidRecord { line_number: 2 }
        );
    }

    #[test]
    fn malformed_history_is_deleted_and_treated_as_empty() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let history_path = directory.path().join(HISTORY_FILE_NAME);
        fs::write(&history_path, "feature\nfeature\n")
            .expect("malformed history should be written");
        let store = SwitchHistoryStore::new(directory.path());

        assert_eq!(
            store.load().expect("malformed history should be ignored"),
            SwitchHistory::default()
        );
        assert!(!history_path.exists());

        store
            .record("feature")
            .expect("a later switch should recreate history");
        assert_eq!(
            fs::read_to_string(history_path).expect("recreated history should be readable"),
            "feature\n"
        );
    }

    #[test]
    fn non_utf8_history_is_deleted_and_treated_as_empty() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let history_path = directory.path().join(HISTORY_FILE_NAME);
        fs::write(&history_path, [0xff]).expect("non-UTF-8 history should be written");
        let store = SwitchHistoryStore::new(directory.path());

        assert_eq!(
            store.load().expect("non-UTF-8 history should be ignored"),
            SwitchHistory::default()
        );
        assert!(!history_path.exists());
    }

    #[test]
    fn missing_history_is_empty_and_repeated_branches_move_to_the_front() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let store = SwitchHistoryStore::new(directory.path());

        assert_eq!(
            store.load().expect("missing history should be readable"),
            SwitchHistory::default()
        );

        store
            .record("feature")
            .expect("first switch should be recorded");
        store
            .record("review")
            .expect("second switch should be recorded");
        store
            .record("feature")
            .expect("repeated switch should be recorded");

        assert_eq!(
            store.load().expect("history should be readable"),
            SwitchHistory {
                branches: vec!["feature".to_owned(), "review".to_owned()]
            }
        );
        assert_eq!(
            fs::read_to_string(directory.path().join(HISTORY_FILE_NAME))
                .expect("history file should be readable"),
            "feature\nreview\n"
        );
    }
}
