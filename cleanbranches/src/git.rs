use std::collections::HashSet;
use std::path::Path;

use git2::{BranchType, ErrorCode, Repository as GitRepository};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Checkout {
    Available,
    CurrentWorktree,
    OtherWorktree,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalBranch {
    name: String,
    checkout: Checkout,
}

impl LocalBranch {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn checkout(&self) -> Checkout {
        self.checkout
    }

    pub fn is_deletable(&self) -> bool {
        self.checkout == Checkout::Available
    }
}

pub struct Repository {
    inner: git2::Repository,
}

impl Repository {
    fn new(repository: GitRepository) -> Repository {
        Repository { inner: repository }
    }

    pub fn discover(path: impl AsRef<Path>) -> Result<Self, Error> {
        let repository = git2::Repository::discover(path)?;
        Ok(Repository::new(repository))
    }

    pub fn local_branches(&self) -> Result<Vec<LocalBranch>, Error> {
        let checked_out_branches = self.checked_out_branches()?;
        let mut branches = self
            .inner
            .branches(Some(BranchType::Local))?
            .map(|branch| {
                let (branch, _) = branch?;
                branch
                    .name()?
                    .ok_or(Error::InvalidBranchName)
                    .map(|name| LocalBranch {
                        name: name.to_string(),
                        checkout: if branch.is_head() {
                            Checkout::CurrentWorktree
                        } else if checked_out_branches.contains(name) {
                            Checkout::OtherWorktree
                        } else {
                            Checkout::Available
                        },
                    })
            })
            .collect::<Result<Vec<_>, Error>>()?;

        branches.sort_unstable_by(|left, right| left.name.cmp(&right.name));
        Ok(branches)
    }

    pub fn delete_branch(&self, branch: &LocalBranch) -> Result<(), Error> {
        if self.checked_out_branches()?.contains(branch.name()) {
            return Err(Error::BranchCheckedOut(branch.name.clone()));
        }

        let mut branch_to_delete = self.inner.find_branch(branch.name(), BranchType::Local)?;
        branch_to_delete.delete().map_err(Error::from)
    }

    fn checked_out_branches(&self) -> Result<HashSet<String>, Error> {
        let linked_worktree_branches = self
            .inner
            .worktrees()?
            .iter()
            .map(|name| {
                let name = name?.ok_or(Error::InvalidWorktreeName)?;
                let worktree = self.inner.find_worktree(name)?;
                let repository = GitRepository::open_from_worktree(&worktree)?;
                checked_out_branch(&repository)
            })
            .collect::<Result<Vec<_>, Error>>()?;

        // if the command is being run from within a worktree, get the main repository's checked out branch
        let main_worktree_branch = self
            .inner
            .is_worktree()
            .then(|| GitRepository::open(self.inner.commondir()))
            .transpose()?
            .as_ref()
            .map(checked_out_branch)
            .transpose()?
            .flatten();

        Ok(linked_worktree_branches
            .into_iter()
            .flatten()
            .chain(main_worktree_branch)
            // if this isn't a worktree, include the checked out branch
            .chain(checked_out_branch(&self.inner)?)
            .collect())
    }
}

fn checked_out_branch(repository: &GitRepository) -> Result<Option<String>, Error> {
    match repository.head() {
        Ok(head) if head.is_branch() => Ok(Some(head.shorthand()?.to_string())),
        Ok(_) => Ok(None),
        Err(error) if matches!(error.code(), ErrorCode::NotFound | ErrorCode::UnbornBranch) => {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Git(#[from] git2::Error),
    #[error("branch name is not valid UTF-8")]
    InvalidBranchName,
    #[error("worktree name is not valid UTF-8")]
    InvalidWorktreeName,
    #[error("branch {0} is checked out in a worktree")]
    BranchCheckedOut(String),
}

#[cfg(test)]
mod tests {
    use std::fs;

    use git2::{BranchType, ErrorCode, Repository as GitRepository, Signature, WorktreeAddOptions};
    use tempfile::TempDir;

    use super::{Checkout, Error, Repository};

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

    fn add_feature_worktree(repository: &GitRepository, directory: &TempDir) {
        let branch = repository
            .find_branch("feature", BranchType::Local)
            .expect("feature branch should exist");
        let reference = branch.into_reference();
        let mut options = WorktreeAddOptions::new();
        options.reference(Some(&reference));

        repository
            .worktree(
                "feature-worktree",
                &directory.path().join("feature"),
                Some(&options),
            )
            .expect("feature worktree should be created");
    }

    fn initialise_worktree() -> (TempDir, TempDir, Repository) {
        let (_directory, repository) = repository_with_branches();
        let worktree_directory = TempDir::new().expect("temporary directory should be created");
        add_feature_worktree(&repository, &worktree_directory);
        (_directory, worktree_directory, Repository::new(repository))
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
        let branches = Repository::new(repository)
            .local_branches()
            .expect("branches should be listed");

        assert_eq!(
            branches
                .iter()
                .map(|branch| (branch.name(), branch.checkout()))
                .collect::<Vec<_>>(),
            [
                ("feature", Checkout::Available),
                ("main", Checkout::CurrentWorktree)
            ]
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

        let branches = Repository::new(repository)
            .local_branches()
            .expect("branches should be listed");

        assert!(
            branches
                .iter()
                .all(|branch| branch.checkout() == Checkout::Available)
        );
    }

    #[test]
    fn deletes_non_current_branch() {
        let (_directory, repository) = repository_with_branches();
        let repository = Repository::new(repository);
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
        let repository = Repository::new(repository);
        let current = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.checkout() == Checkout::CurrentWorktree)
            .expect("current branch should exist");

        assert!(repository.delete_branch(&current).is_err());
    }

    #[test]
    fn marks_branch_checked_out_in_other_worktree_as_not_deletable() {
        let (_dir1, _dir2, repository) = initialise_worktree();

        let feature = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.name() == "feature")
            .expect("feature branch should exist");

        assert_eq!(feature.checkout(), Checkout::OtherWorktree);
        assert!(!feature.is_deletable());
    }

    #[test]
    fn refuses_to_delete_branch_checked_out_in_other_worktree() {
        let (_dir1, _dir2, repository) = initialise_worktree();

        let feature = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.name() == "feature")
            .expect("feature branch should exist");

        let error = repository
            .delete_branch(&feature)
            .expect_err("checked-out branch should not be deleted");

        assert!(matches!(error, Error::BranchCheckedOut(name) if name == "feature"));
        assert!(
            repository
                .inner
                .find_branch("feature", BranchType::Local)
                .is_ok()
        );
    }

    #[test]
    fn linked_worktree_treats_main_worktree_branch_as_other_worktree() {
        let (_dir1, worktree_directory, _) = initialise_worktree();

        let repository: Repository =
            Repository::discover(worktree_directory.path().join("feature"))
                .expect("linked worktree repository should be discovered");
        let branches = repository
            .local_branches()
            .expect("branches should be listed");

        assert_eq!(
            branches
                .iter()
                .map(|branch| (branch.name(), branch.checkout()))
                .collect::<Vec<_>>(),
            [
                ("feature", Checkout::CurrentWorktree),
                ("main", Checkout::OtherWorktree)
            ]
        );
        assert!(branches.iter().all(|branch| !branch.is_deletable()));
    }
}
