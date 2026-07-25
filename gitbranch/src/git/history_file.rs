use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct HistoryFile {
    history_path: PathBuf,
    lock_path: PathBuf,
}

impl HistoryFile {
    pub fn new(common_directory: &Path, file_name: &str) -> Self {
        Self {
            history_path: common_directory.join(file_name),
            lock_path: common_directory.join(format!("{file_name}.lock")),
        }
    }

    pub fn read(&self) -> io::Result<Option<Vec<u8>>> {
        match fs::read(&self.history_path) {
            Ok(contents) => Ok(Some(contents)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn lock(&self) -> io::Result<HistoryFileLock<'_>> {
        Ok(HistoryFileLock {
            destination: &self.history_path,
            lock: LockFile::create(&self.lock_path)?,
        })
    }

    pub fn remove(&self) -> io::Result<()> {
        match fs::remove_file(&self.history_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

pub struct HistoryFileLock<'a> {
    destination: &'a Path,
    lock: LockFile,
}

impl HistoryFileLock<'_> {
    pub fn commit(mut self, contents: &[u8]) -> io::Result<()> {
        self.lock.write_all(contents)?;
        self.lock.commit(self.destination)
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
