use std::collections::HashMap;

use super::history::{History, HistoryStore};

const HISTORY_FILE_NAME: &str = "gitbranch-merges";

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
                .map(|(rank, merge)| (merge, rank))
                .collect(),
        }
    }
}

impl History for MergeHistory {
    const FILE_NAME: &'static str = HISTORY_FILE_NAME;
    type Record = MergeRecord;
    type ParseError = InvalidRecord;

    fn parse(contents: &str) -> Result<Self, Self::ParseError> {
        let rankings = contents.lines().enumerate().try_fold(
            HashMap::new(),
            |mut rankings, (index, line)| {
                let line_number = index + 1;
                let mut fields = line.split('\t');
                let record = match (fields.next(), fields.next(), fields.next(), fields.next()) {
                    (Some(destination), Some(source), Some(rank), None)
                        if !destination.is_empty() && !source.is_empty() =>
                    {
                        let rank = rank
                            .parse::<usize>()
                            .ok()
                            .filter(|rank| *rank < usize::MAX)
                            .ok_or(InvalidRecord { line_number })?;
                        ((destination.to_owned(), source.to_owned()), rank)
                    }
                    _ => return Err(InvalidRecord { line_number }),
                };

                match rankings.insert(record.0, record.1) {
                    Some(_) => Err(InvalidRecord { line_number }),
                    None => Ok(rankings),
                }
            },
        )?;

        Ok(Self { rankings })
    }

    fn record(mut self, record: Self::Record) -> String {
        let next_rank = self.rankings.values().max().map_or(0, |rank| rank + 1);
        self.rankings
            .insert((record.destination, record.source), next_rank);

        let mut rankings = self.rankings.into_iter().collect::<Vec<_>>();
        rankings.sort_unstable_by(
            |((left_destination, left_source), _), ((right_destination, right_source), _)| {
                left_destination
                    .cmp(right_destination)
                    .then_with(|| left_source.cmp(right_source))
            },
        );
        rankings
            .into_iter()
            .map(|((destination, source), rank)| format!("{destination}\t{source}\t{rank}\n"))
            .collect()
    }
}

pub(super) type MergeHistoryStore = HistoryStore<MergeHistory>;

#[derive(Debug, Eq, PartialEq)]
pub(super) struct InvalidRecord {
    line_number: usize,
}

#[cfg(test)]
mod tests {
    use std::{fs, io};

    use tempfile::TempDir;

    use super::{
        HISTORY_FILE_NAME, History, InvalidRecord, MergeHistory, MergeHistoryStore, MergeRecord,
    };

    #[test]
    fn parses_destination_scoped_rankings() {
        let history = MergeHistory::parse("main\tfeature\t4\nrelease\tfeature\t2\n")
            .expect("history should parse");

        assert_eq!(history.rank("main", "feature"), Some(4));
        assert_eq!(history.rank("release", "feature"), Some(2));
        assert_eq!(history.rank("main", "unknown"), None);
        assert_eq!(history.rank("unknown", "feature"), None);
    }

    #[test]
    fn rejects_malformed_and_duplicate_records() {
        [
            "\tfeature\t0\n",
            "main\t\t0\n",
            "main\tfeature\n",
            "main\tfeature\trecent\n",
            "main\tfeature\t0\textra\n",
        ]
        .into_iter()
        .for_each(|contents| {
            assert_eq!(
                MergeHistory::parse(contents).expect_err("malformed history should be rejected"),
                InvalidRecord { line_number: 1 }
            );
        });
        assert_eq!(
            MergeHistory::parse("main\tfeature\t1\nmain\tfeature\t2\n")
                .expect_err("duplicate merge should be rejected"),
            InvalidRecord { line_number: 2 }
        );
        assert_eq!(
            MergeHistory::parse(&format!("main\tfeature\t{}\n", usize::MAX))
                .expect_err("maximum rank should be rejected"),
            InvalidRecord { line_number: 1 }
        );
    }

    #[test]
    fn recording_assigns_global_recency_and_serializes_by_branch_names() {
        let history =
            MergeHistory::parse("release\tfeature\t1\nmain\treview\t4\nmain\tfeature\t2\n")
                .expect("history should parse");

        assert_eq!(
            history.record(MergeRecord::new("main", "feature")),
            "main\tfeature\t5\nmain\treview\t4\nrelease\tfeature\t1\n"
        );
    }

    #[test]
    fn missing_history_is_empty_and_destinations_are_independent() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let store = MergeHistoryStore::new(directory.path());

        assert_eq!(
            store.load().expect("missing history should be readable"),
            MergeHistory::default()
        );

        store
            .record(MergeRecord::new("main", "feature"))
            .expect("first merge should be recorded");
        store
            .record(MergeRecord::new("release", "feature"))
            .expect("second destination should be recorded");
        store
            .record(MergeRecord::new("main", "feature"))
            .expect("repeated merge should be recorded");

        let history = store.load().expect("history should be readable");
        assert_eq!(history.rank("main", "feature"), Some(2));
        assert_eq!(history.rank("release", "feature"), Some(1));
        assert_eq!(
            fs::read_to_string(directory.path().join(HISTORY_FILE_NAME))
                .expect("history file should be readable"),
            "main\tfeature\t2\nrelease\tfeature\t1\n"
        );
    }

    #[test]
    fn malformed_history_is_deleted_and_locked_history_is_unchanged() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let history_path = directory.path().join(HISTORY_FILE_NAME);
        let lock_path = directory.path().join(format!("{HISTORY_FILE_NAME}.lock"));
        let store = MergeHistoryStore::new(directory.path());

        fs::write(&history_path, "main\tfeature\n").expect("malformed history should be written");
        assert_eq!(
            store.load().expect("malformed history should be ignored"),
            MergeHistory::default()
        );
        assert!(!history_path.exists());

        fs::write(&history_path, "main\tfeature\t0\n").expect("valid history should be written");
        fs::write(&lock_path, "locked\n").expect("history lock should be written");
        let error = store
            .record(MergeRecord::new("main", "review"))
            .expect_err("existing lock should prevent recording");

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read_to_string(history_path).expect("history should remain unchanged"),
            "main\tfeature\t0\n"
        );
    }
}
