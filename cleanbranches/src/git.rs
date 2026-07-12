use std::error::Error as StdError;
use std::fmt;
use std::path::Path;

use git2::{BranchType, Repository as GitRepository};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalBranch {
    name: String,
    is_current: bool,
}

impl LocalBranch {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn is_current(&self) -> bool {
        self.is_current
    }
}

pub struct Repository {
    inner: git2::Repository,
}

fn new_repository(repository: GitRepository) -> Repository {
    Repository { inner: repository }
}

impl Repository {
    pub fn discover(path: impl AsRef<Path>) -> Result<Self, Error> {
        git2::Repository::discover(path)
            .map(new_repository)
            .map_err(Error::Discover)
    }

    pub fn local_branches(&self) -> Result<Vec<LocalBranch>, Error> {
        let mut branches = self
            .inner
            .branches(Some(BranchType::Local))
            .map_err(Error::ListBranches)?
            .map(|branch| {
                let (branch, _) = branch.map_err(Error::ListBranches)?;
                branch
                    .name()
                    .map_err(Error::ListBranches)?
                    .ok_or(Error::InvalidBranchName)
                    .map(|name| LocalBranch {
                        name: name.to_string(),
                        is_current: branch.is_head(),
                    })
            })
            .collect::<Result<Vec<_>, Error>>()?;

        branches.sort_unstable_by(|left, right| left.name.cmp(&right.name));
        Ok(branches)
    }

    pub fn delete_branch(&self, branch: &LocalBranch) -> Result<(), Error> {
        let mut branch_to_delete = self
            .inner
            .find_branch(branch.name(), BranchType::Local)
            .map_err(|source| Error::FindBranch {
                name: branch.name.clone(),
                source,
            })?;

        branch_to_delete
            .delete()
            .map_err(|source| Error::DeleteBranch {
                name: branch.name.clone(),
                source,
            })
    }
}

#[derive(Debug)]
pub enum Error {
    Discover(git2::Error),
    ListBranches(git2::Error),
    InvalidBranchName,
    FindBranch { name: String, source: git2::Error },
    DeleteBranch { name: String, source: git2::Error },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discover(source) => write!(formatter, "failed to discover repository: {source}"),
            Self::ListBranches(source) => write!(formatter, "failed to list branches: {source}"),
            Self::InvalidBranchName => write!(formatter, "branch name is not valid UTF-8"),
            Self::FindBranch { name, source } => {
                write!(formatter, "could not find {name}: {source}")
            }
            Self::DeleteBranch { name, source } => {
                write!(formatter, "could not delete {name}: {source}")
            }
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Discover(source)
            | Self::ListBranches(source)
            | Self::FindBranch { source, .. }
            | Self::DeleteBranch { source, .. } => Some(source),
            Self::InvalidBranchName => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use git2::{ErrorCode, Repository as GitRepository, Signature};
    use tempfile::TempDir;

    use super::{Repository, new_repository};

    fn repository_with_branches() -> (TempDir, GitRepository) {
        let directory = TempDir::new().expect("temporary directory should be created");
        let repository =
            GitRepository::init(directory.path()).expect("repository should be created");
        repository
            .set_head("refs/heads/main")
            .expect("HEAD should point to main");

        let tree_id = {
            let mut index = repository.index().expect("index should open");
            index.write_tree().expect("empty tree should be written")
        };
        let tree = repository.find_tree(tree_id).expect("tree should exist");
        let signature = Signature::now("Clean Branches", "cleanbranches@example.com")
            .expect("signature should be valid");
        let commit_id = repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "Initial commit",
                &tree,
                &[],
            )
            .expect("commit should be created");
        drop(tree);

        let commit = repository
            .find_commit(commit_id)
            .expect("commit should exist");
        repository
            .branch("feature", &commit, false)
            .expect("feature branch should be created");
        drop(commit);

        (directory, repository)
    }

    #[test]
    fn discovers_repository_from_nested_directory() {
        let (directory, _) = repository_with_branches();
        let nested = directory.path().join("nested/directory");
        fs::create_dir_all(&nested).expect("nested directory should be created");

        let repository = Repository::discover(nested).expect("repository should be discovered");
        let branches = repository
            .local_branches()
            .expect("branches should be listed");

        assert_eq!(
            branches
                .iter()
                .map(|branch| branch.name())
                .collect::<Vec<_>>(),
            ["feature", "main"]
        );
    }

    #[test]
    fn lists_local_branches_in_name_order_and_marks_current_branch() {
        let (_directory, repository) = repository_with_branches();
        let branches = new_repository(repository)
            .local_branches()
            .expect("branches should be listed");

        assert_eq!(
            branches
                .iter()
                .map(|branch| (branch.name(), branch.is_current()))
                .collect::<Vec<_>>(),
            [("feature", false), ("main", true)]
        );
    }

    #[test]
    fn detached_head_has_no_current_branch() {
        let (_directory, repository) = repository_with_branches();
        let head = repository
            .head()
            .expect("HEAD should exist")
            .target()
            .expect("HEAD should have a target");
        repository
            .set_head_detached(head)
            .expect("HEAD should detach");

        let branches = new_repository(repository)
            .local_branches()
            .expect("branches should be listed");

        assert!(branches.iter().all(|branch| !branch.is_current()));
    }

    #[test]
    fn deletes_non_current_branch() {
        let (_directory, repository) = repository_with_branches();
        let repository = new_repository(repository);
        let feature = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.name() == "feature")
            .expect("feature branch should exist");

        repository
            .delete_branch(&feature)
            .expect("feature branch should be deleted");

        let error = repository
            .inner
            .find_branch("feature", git2::BranchType::Local)
            .err()
            .expect("feature branch should no longer exist");
        assert_eq!(error.code(), ErrorCode::NotFound);
    }

    #[test]
    fn refuses_to_delete_current_branch() {
        let (_directory, repository) = repository_with_branches();
        let repository = new_repository(repository);
        let current = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.is_current())
            .expect("current branch should exist");

        assert!(repository.delete_branch(&current).is_err());
    }
}
