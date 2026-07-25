use std::collections::BTreeMap;

use super::history::{History, HistoryStore};

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

#[derive(Debug, Default, Eq, PartialEq)]
pub(super) struct RebaseHistory {
    targets: BTreeMap<String, String>,
}

impl RebaseHistory {
    pub(super) fn target_for(&self, source: &str) -> Option<&str> {
        self.targets.get(source).map(String::as_str)
    }
}

impl History for RebaseHistory {
    const FILE_NAME: &'static str = HISTORY_FILE_NAME;
    type Record = RebaseRecord;
    type ParseError = InvalidRecord;

    fn parse(contents: &str) -> Result<Self, Self::ParseError> {
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

    fn record(mut self, record: Self::Record) -> String {
        self.targets.insert(record.source, record.target);
        self.targets
            .into_iter()
            .map(|(source, target)| format!("{source}\t{target}\n"))
            .collect()
    }
}

pub(super) type RebaseHistoryStore = HistoryStore<RebaseHistory>;

#[derive(Debug, Eq, PartialEq)]
pub(super) struct InvalidRecord {
    line_number: usize,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::{History, InvalidRecord, RebaseHistory, RebaseHistoryStore, RebaseRecord};

    #[test]
    fn parses_and_records_history_in_source_branch_order() {
        let history = RebaseHistory::parse("feature-b\tdevelop\nfeature-a\tmain\n")
            .expect("history should parse");

        assert_eq!(history.target_for("feature-a"), Some("main"));
        assert_eq!(history.target_for("feature-b"), Some("develop"));
        assert_eq!(
            history.record(RebaseRecord::new("feature-a", "release")),
            "feature-a\trelease\nfeature-b\tdevelop\n"
        );
    }

    #[test]
    fn rejects_malformed_history_records() {
        let error = RebaseHistory::parse("feature\tmain\textra\n")
            .expect_err("a record with extra fields should be rejected");

        assert_eq!(error, InvalidRecord { line_number: 1 });
    }

    #[test]
    fn missing_history_is_empty_and_records_are_replaced_per_source() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let store = RebaseHistoryStore::new(directory.path());

        assert_eq!(
            store.load().expect("missing history should be readable"),
            RebaseHistory::default()
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
                .load()
                .expect("feature history should be readable")
                .target_for("feature"),
            Some("release")
        );
        assert_eq!(
            store
                .load()
                .expect("other history should be readable")
                .target_for("other"),
            Some("develop")
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("gitbranch-rebases"))
                .expect("history file should be readable"),
            "feature\trelease\nother\tdevelop\n"
        );
    }
}
