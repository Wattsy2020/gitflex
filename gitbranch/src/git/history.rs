use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    marker::PhantomData,
    path::{Path, PathBuf},
};

pub(super) trait History: Default {
    const FILE_NAME: &'static str;
    type Record;
    type ParseError;

    fn parse(contents: &str) -> Result<Self, Self::ParseError>;
    fn record(self, record: Self::Record) -> String;
}

#[derive(Debug)]
pub(super) struct HistoryStore<H> {
    history_path: PathBuf,
    lock_path: PathBuf,
    history: PhantomData<fn() -> H>,
}

impl<H: History> HistoryStore<H> {
    pub(super) fn new(common_directory: &Path) -> Self {
        Self {
            history_path: common_directory.join(H::FILE_NAME),
            lock_path: common_directory.join(format!("{}.lock", H::FILE_NAME)),
            history: PhantomData,
        }
    }

    pub(super) fn load(&self) -> io::Result<H> {
        let contents = match fs::read(&self.history_path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(H::default()),
            Err(error) => return Err(error),
        };

        match std::str::from_utf8(&contents)
            .ok()
            .and_then(|contents| H::parse(contents).ok())
        {
            Some(history) => Ok(history),
            None => {
                remove_if_present(&self.history_path)?;
                Ok(H::default())
            }
        }
    }

    pub(super) fn record(&self, record: H::Record) -> io::Result<()> {
        let mut lock = LockFile::create(&self.lock_path)?;
        let contents = self.load()?.record(record);
        lock.write_all(contents.as_bytes())?;
        lock.commit(&self.history_path)
    }
}

/// A file-based lock that atomically writes to a path by writing to the lock file first.
struct LockFile {
    path: PathBuf,
    file: Option<File>,
}

impl LockFile {
    /// Attempts to create the lock, failing if it already exists.
    fn create(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        Ok(Self {
            path: path.to_owned(),
            file: Some(file),
        })
    }

    fn write_all(&mut self, contents: &[u8]) -> io::Result<()> {
        self.file
            .as_mut()
            .expect("an uncommitted lock always has an open file")
            .write_all(contents)
    }

    /// Commits the lock contents by renaming the lock over the destination.
    fn commit(mut self, destination: &Path) -> io::Result<()> {
        self.file
            .take()
            .expect("an uncommitted lock always has an open file")
            .sync_all()?;
        fs::rename(&self.path, destination)
    }
}

impl Drop for LockFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, io};

    use tempfile::TempDir;

    use super::{History, HistoryStore};

    #[derive(Debug, Default, Eq, PartialEq)]
    struct TestHistory {
        records: Vec<String>,
    }

    impl History for TestHistory {
        const FILE_NAME: &'static str = "test-history";
        type Record = String;
        type ParseError = ();

        fn parse(contents: &str) -> Result<Self, Self::ParseError> {
            if contents == "invalid\n" {
                Err(())
            } else {
                Ok(Self {
                    records: contents.lines().map(str::to_owned).collect(),
                })
            }
        }

        fn record(mut self, record: Self::Record) -> String {
            self.records.push(record);
            self.records
                .into_iter()
                .map(|record| format!("{record}\n"))
                .collect()
        }
    }

    #[test]
    fn missing_history_is_empty_and_records_are_updated_atomically() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let store = HistoryStore::<TestHistory>::new(directory.path());

        assert_eq!(
            store.load().expect("missing history should be readable"),
            TestHistory::default()
        );

        store
            .record("first".to_owned())
            .expect("first record should be written");
        store
            .record("second".to_owned())
            .expect("second record should be written");

        assert_eq!(
            fs::read_to_string(directory.path().join("test-history"))
                .expect("history should be readable"),
            "first\nsecond\n"
        );
        assert!(!directory.path().join("test-history.lock").exists());
    }

    #[test]
    fn malformed_and_non_utf8_history_are_deleted_and_treated_as_empty() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let history_path = directory.path().join("test-history");
        let store = HistoryStore::<TestHistory>::new(directory.path());

        fs::write(&history_path, "invalid\n").expect("malformed history should be written");
        assert_eq!(
            store.load().expect("malformed history should be ignored"),
            TestHistory::default()
        );
        assert!(!history_path.exists());

        fs::write(&history_path, [0xff]).expect("non-UTF-8 history should be written");
        assert_eq!(
            store.load().expect("non-UTF-8 history should be ignored"),
            TestHistory::default()
        );
        assert!(!history_path.exists());
    }

    #[test]
    fn record_locks_before_loading_history() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let history_path = directory.path().join("test-history");
        fs::write(&history_path, "invalid\n").expect("malformed history should be written");
        fs::write(directory.path().join("test-history.lock"), "locked\n")
            .expect("history lock should be written");
        let store = HistoryStore::<TestHistory>::new(directory.path());

        let error = store
            .record("record".to_owned())
            .expect_err("existing lock should prevent recording");

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read_to_string(history_path).expect("history should remain untouched"),
            "invalid\n"
        );
    }
}
