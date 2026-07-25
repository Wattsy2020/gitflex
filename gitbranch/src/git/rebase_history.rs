use std::{collections::BTreeMap, io, path::Path};

use super::history_file::HistoryFile;

const HISTORY_FILE_NAME: &str = "gitbranch-rebases";

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

#[derive(Debug)]
pub struct RebaseHistoryStore {
    file: HistoryFile,
}

impl RebaseHistoryStore {
    pub fn new(common_directory: &Path) -> Self {
        Self {
            file: HistoryFile::new(common_directory, HISTORY_FILE_NAME),
        }
    }

    pub fn target_for(&self, source: &str) -> io::Result<Option<String>> {
        Ok(self.load()?.target_for(source).map(str::to_owned))
    }

    pub fn record(&self, record: RebaseRecord) -> io::Result<()> {
        let lock = self.file.lock()?;
        let mut history = self.load()?;
        history.record(record);
        lock.commit(history.serialize().as_bytes())
    }

    fn load(&self) -> io::Result<RebaseHistory> {
        match self.file.read()? {
            Some(contents) => match std::str::from_utf8(&contents)
                .ok()
                .and_then(|contents| RebaseHistory::parse(contents).ok())
            {
                Some(history) => Ok(history),
                None => {
                    self.file.remove()?;
                    Ok(RebaseHistory::default())
                }
            },
            None => Ok(RebaseHistory::default()),
        }
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
struct RebaseHistory {
    targets: BTreeMap<String, String>,
}

impl RebaseHistory {
    fn parse(contents: &str) -> Result<Self, InvalidRecord> {
        let targets = contents
            .lines()
            .enumerate()
            .map(|(index, line)| {
                let line_number = index + 1;
                line.split_once('\t')
                    .filter(|(source, target)| {
                        !source.is_empty() && !target.is_empty() && !target.contains('\t')
                    })
                    .ok_or(InvalidRecord { line_number })
                    .map(|(source, target)| (source.to_owned(), target.to_owned()))
            })
            .collect::<Result<BTreeMap<_, _>, InvalidRecord>>()?;

        Ok(Self { targets })
    }

    fn target_for(&self, source: &str) -> Option<&str> {
        self.targets.get(source).map(String::as_str)
    }

    fn record(&mut self, record: RebaseRecord) {
        self.targets.insert(record.source, record.target);
    }

    fn serialize(&self) -> String {
        self.targets
            .iter()
            .map(|(source, target)| format!("{source}\t{target}\n"))
            .collect()
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

    use super::{InvalidRecord, RebaseHistory, RebaseHistoryStore, RebaseRecord};

    #[test]
    fn parses_and_serializes_history_in_source_branch_order() {
        let history = RebaseHistory::parse("feature-b\tdevelop\nfeature-a\tmain\n")
            .expect("history should parse");

        assert_eq!(history.target_for("feature-a"), Some("main"));
        assert_eq!(history.target_for("feature-b"), Some("develop"));
        assert_eq!(history.serialize(), "feature-a\tmain\nfeature-b\tdevelop\n");
    }

    #[test]
    fn rejects_malformed_history_records() {
        let error = RebaseHistory::parse("feature\tmain\textra\n")
            .expect_err("a record with extra fields should be rejected");

        assert_eq!(error, InvalidRecord { line_number: 1 });
    }

    #[test]
    fn malformed_history_is_deleted_and_treated_as_empty() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let history_path = directory.path().join("gitbranch-rebases");
        fs::write(&history_path, "a format from another version\n")
            .expect("malformed history should be written");
        let store = RebaseHistoryStore::new(directory.path());

        assert_eq!(
            store
                .target_for("feature")
                .expect("malformed history should be ignored"),
            None
        );
        assert!(!history_path.exists());

        store
            .record(RebaseRecord::new("feature", "main"))
            .expect("a later rebase target should recreate history");
        assert_eq!(
            fs::read_to_string(history_path).expect("recreated history should be readable"),
            "feature\tmain\n"
        );
    }

    #[test]
    fn non_utf8_history_is_deleted_and_treated_as_empty() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let history_path = directory.path().join("gitbranch-rebases");
        fs::write(&history_path, [0xff]).expect("non-UTF-8 history should be written");
        let store = RebaseHistoryStore::new(directory.path());

        assert_eq!(
            store
                .target_for("feature")
                .expect("non-UTF-8 history should be ignored"),
            None
        );
        assert!(!history_path.exists());
    }

    #[test]
    fn missing_history_is_empty_and_records_are_replaced_per_source() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let store = RebaseHistoryStore::new(directory.path());

        assert_eq!(
            store
                .target_for("feature")
                .expect("missing history should be readable"),
            None
        );

        store
            .record(RebaseRecord::new("feature", "main"))
            .expect("first target should be recorded");
        store
            .record(RebaseRecord::new("other", "develop"))
            .expect("another source target should be recorded");
        store
            .record(RebaseRecord::new("feature", "release"))
            .expect("the feature target should be replaced");

        assert_eq!(
            store
                .target_for("feature")
                .expect("feature history should be readable")
                .as_deref(),
            Some("release")
        );
        assert_eq!(
            store
                .target_for("other")
                .expect("other history should be readable")
                .as_deref(),
            Some("develop")
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("gitbranch-rebases"))
                .expect("history file should be readable"),
            "feature\trelease\nother\tdevelop\n"
        );
    }
}
