use std::collections::HashMap;

use super::history::{History, HistoryStore};

const HISTORY_FILE_NAME: &str = "gitbranch-switches";

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
                .map(|(rank, branch)| (branch, rank))
                .collect(),
        }
    }
}

impl History for SwitchHistory {
    const FILE_NAME: &'static str = HISTORY_FILE_NAME;
    type Record = String;
    type ParseError = InvalidRecord;

    fn parse(contents: &str) -> Result<Self, Self::ParseError> {
        let rankings = contents.lines().enumerate().try_fold(
            HashMap::new(),
            |mut rankings, (index, line)| {
                let line_number = index + 1;
                let (branch, rank) = line
                    .split_once('\t')
                    .filter(|(branch, rank)| !branch.is_empty() && !rank.contains('\t'))
                    .ok_or(InvalidRecord { line_number })?;
                let rank = rank
                    .parse::<usize>()
                    .ok()
                    .filter(|rank| *rank < usize::MAX)
                    .ok_or(InvalidRecord { line_number })?;

                if rankings.insert(branch.to_owned(), rank).is_some() {
                    Err(InvalidRecord { line_number })
                } else {
                    Ok(rankings)
                }
            },
        )?;

        Ok(Self { rankings })
    }

    fn record(mut self, branch: Self::Record) -> String {
        let next_rank = self.rankings.values().max().map_or(0, |rank| rank + 1);
        self.rankings.insert(branch, next_rank);

        let mut rankings = self.rankings.into_iter().collect::<Vec<_>>();
        rankings.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
        rankings
            .into_iter()
            .map(|(branch, rank)| format!("{branch}\t{rank}\n"))
            .collect()
    }
}

pub(super) type SwitchHistoryStore = HistoryStore<SwitchHistory>;

#[derive(Debug, Eq, PartialEq)]
pub(super) struct InvalidRecord {
    line_number: usize,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::{HISTORY_FILE_NAME, History, InvalidRecord, SwitchHistory, SwitchHistoryStore};

    #[test]
    fn parses_history_into_branch_rankings() {
        let history =
            SwitchHistory::parse("feature-b\t4\nfeature-a\t2\n").expect("history should parse");

        assert_eq!(history.rank("feature-b"), Some(4));
        assert_eq!(history.rank("feature-a"), Some(2));
        assert_eq!(history.rank("unknown"), None);
    }

    #[test]
    fn rejects_empty_and_duplicate_records() {
        assert_eq!(
            SwitchHistory::parse("\t0\n").expect_err("empty branch should be rejected"),
            InvalidRecord { line_number: 1 }
        );
        assert_eq!(
            SwitchHistory::parse("feature\t1\nfeature\t2\n")
                .expect_err("duplicate branch should be rejected"),
            InvalidRecord { line_number: 2 }
        );
        assert_eq!(
            SwitchHistory::parse("feature\n").expect_err("missing rank should be rejected"),
            InvalidRecord { line_number: 1 }
        );
        assert_eq!(
            SwitchHistory::parse("feature\trecent\n")
                .expect_err("non-numeric rank should be rejected"),
            InvalidRecord { line_number: 1 }
        );
        assert_eq!(
            SwitchHistory::parse(&format!("feature\t{}\n", usize::MAX))
                .expect_err("maximum rank should be rejected"),
            InvalidRecord { line_number: 1 }
        );
    }

    #[test]
    fn recording_assigns_the_next_rank_and_serializes_by_branch_name() {
        let history =
            SwitchHistory::parse("feature-b\t5\nfeature-a\t2\n").expect("history should parse");
        assert_eq!(
            history.record("feature-a".to_owned()),
            "feature-a\t6\nfeature-b\t5\n"
        );

        let history =
            SwitchHistory::parse("feature-b\t5\nfeature-a\t2\n").expect("history should parse");
        assert_eq!(
            history.record("feature-c".to_owned()),
            "feature-a\t2\nfeature-b\t5\nfeature-c\t6\n"
        );
    }

    #[test]
    fn missing_history_is_empty_and_repeated_branches_receive_the_next_rank() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let store = SwitchHistoryStore::new(directory.path());

        assert_eq!(
            store.load().expect("missing history should be readable"),
            SwitchHistory::default()
        );

        store
            .record("feature".to_owned())
            .expect("first switch should be recorded");
        store
            .record("review".to_owned())
            .expect("second switch should be recorded");
        store
            .record("feature".to_owned())
            .expect("repeated switch should be recorded");

        let history = store.load().expect("history should be readable");
        assert_eq!(history.rank("feature"), Some(2));
        assert_eq!(history.rank("review"), Some(1));
        assert_eq!(
            fs::read_to_string(directory.path().join(HISTORY_FILE_NAME))
                .expect("history file should be readable"),
            "feature\t2\nreview\t1\n"
        );
    }
}
