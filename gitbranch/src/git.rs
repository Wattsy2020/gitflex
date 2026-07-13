use std::collections::HashSet;
use std::path::Path;
use std::process::{Command, ExitStatus};

use git2::build::CheckoutBuilder;
use git2::{Branch, BranchType, ErrorCode, ObjectType, Repository as GitRepository};
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
    fn new(name: impl Into<String>, checkout: Checkout) -> Self {
        Self {
            name: name.into(),
            checkout,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(name: impl Into<String>, checkout: Checkout) -> Self {
        Self::new(name, checkout)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn checkout(&self) -> Checkout {
        self.checkout
    }

    pub fn is_deletable(&self) -> bool {
        self.checkout == Checkout::Available
    }

    pub fn is_switchable(&self) -> bool {
        self.checkout == Checkout::Available
    }

    pub fn is_rebase_target(&self) -> bool {
        self.checkout != Checkout::CurrentWorktree
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RebaseOutcome {
    Completed,
    Conflicted,
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
                branch.name()?.ok_or(Error::InvalidBranchName).map(|name| {
                    let checkout = if branch.is_head() {
                        Checkout::CurrentWorktree
                    } else if checked_out_branches.contains(name) {
                        Checkout::OtherWorktree
                    } else {
                        Checkout::Available
                    };
                    LocalBranch::new(name, checkout)
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;

        branches.sort_unstable_by(|left, right| left.name.cmp(&right.name));
        Ok(branches)
    }

    pub fn current_branch(&self) -> Result<Option<String>, Error> {
        checked_out_branch(&self.inner)
    }

    fn find_branch(&self, branch: &LocalBranch) -> Result<Branch<'_>, Error> {
        self.inner
            .find_branch(branch.name(), BranchType::Local)
            .map_err(Error::from)
    }

    fn get_non_checkedout_branch(&self, branch: &LocalBranch) -> Result<Branch<'_>, Error> {
        if self.checked_out_branches()?.contains(branch.name()) {
            return Err(Error::BranchCheckedOut(branch.name.clone()));
        }
        self.find_branch(branch)
    }

    pub fn delete_branch(&self, branch: &LocalBranch) -> Result<(), Error> {
        let mut branch_to_delete = self.get_non_checkedout_branch(branch)?;
        branch_to_delete.delete().map_err(Error::from)
    }

    pub fn switch_to(&self, branch: &LocalBranch) -> Result<(), Error> {
        let branch = self.get_non_checkedout_branch(branch)?;
        let reference = branch.get();
        let reference_name = reference.name()?;
        let target = reference.peel(ObjectType::Commit)?;
        let mut checkout = CheckoutBuilder::new();
        checkout.safe();

        self.inner.checkout_tree(&target, Some(&mut checkout))?;
        self.inner.set_head(reference_name)?;
        Ok(())
    }

    pub fn rebase_onto(&self, branch: &LocalBranch) -> Result<RebaseOutcome, Error> {
        if self.current_branch()?.as_deref() == Some(branch.name()) {
            return Err(Error::CurrentBranchAsRebaseTarget);
        }

        let branch = self.find_branch(branch)?;
        let target = branch.get().name()?.to_string();
        let worktree = self.inner.workdir().ok_or(Error::BareRepository)?;
        let had_conflicts = GitRepository::open(worktree)?.index()?.has_conflicts();
        let output = Command::new("git")
            .args(["rebase", &target])
            .current_dir(worktree)
            .output()?;

        if output.status.success() {
            Ok(RebaseOutcome::Completed)
        } else if !had_conflicts && GitRepository::open(worktree)?.index()?.has_conflicts() {
            Ok(RebaseOutcome::Conflicted)
        } else {
            Err(Error::RebaseFailed {
                status: output.status,
                message: command_message(&output.stdout, &output.stderr),
            })
        }
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

        // `Repository::worktrees` only returns linked worktrees, so include the main worktree explicitly
        let main_repository = GitRepository::open(self.inner.commondir())?;

        Ok(linked_worktree_branches
            .into_iter()
            .flatten()
            .chain(checked_out_branch(&main_repository)?)
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

fn command_message(stdout: &[u8], stderr: &[u8]) -> String {
    [stderr, stdout]
        .into_iter()
        .map(String::from_utf8_lossy)
        .map(|output| output.trim().to_string())
        .find(|output| !output.is_empty())
        .unwrap_or_else(|| "git did not report an error".to_string())
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Git(#[from] git2::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("branch name is not valid UTF-8")]
    InvalidBranchName,
    #[error("worktree name is not valid UTF-8")]
    InvalidWorktreeName,
    #[error("branch {0} is checked out in a worktree")]
    BranchCheckedOut(String),
    #[error("cannot rebase while HEAD is detached")]
    DetachedHead,
    #[error("the current branch cannot be its own rebase target")]
    CurrentBranchAsRebaseTarget,
    #[error("cannot rebase a bare repository")]
    BareRepository,
    #[error("git rebase failed with {status}: {message}")]
    RebaseFailed { status: ExitStatus, message: String },
    #[error(
        "rebase stopped due to conflicts; resolve them and run `git rebase --continue`, or run `git rebase --abort`"
    )]
    RebaseConflicts,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use git2::build::CheckoutBuilder;
    use git2::{
        BranchType, ErrorCode, Oid, Repository as GitRepository, RepositoryState, Signature,
        WorktreeAddOptions,
    };
    use tempfile::TempDir;

    use super::{Checkout, Error, RebaseOutcome, Repository};

    fn repository_with_branches() -> (TempDir, GitRepository) {
        let directory = TempDir::new().expect("temporary directory should be created");
        let repository =
            GitRepository::init(directory.path()).expect("repository should be created");
        repository
            .set_head("refs/heads/main")
            .expect("HEAD should point to main");
        {
            let mut config = repository.config().expect("config should open");
            config
                .set_str("user.name", "Git Branch Tests")
                .expect("user name should be configured");
            config
                .set_str("user.email", "gitbranch@example.com")
                .expect("user email should be configured");
        }

        let tree_id = {
            let mut index = repository.index().expect("index should open");
            index.write_tree().expect("empty tree should be written")
        };
        let tree = repository.find_tree(tree_id).expect("tree should exist");
        let signature = Signature::now("Git Branch", "gitbranch@example.com")
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

    fn commit_file(
        repository: &GitRepository,
        reference: &str,
        parent_id: Oid,
        path: &str,
        contents: &str,
    ) -> Oid {
        let parent = repository
            .find_commit(parent_id)
            .expect("parent commit should exist");
        let parent_tree = parent.tree().expect("parent tree should exist");
        let blob = repository
            .blob(contents.as_bytes())
            .expect("blob should be created");
        let mut tree = repository
            .treebuilder(Some(&parent_tree))
            .expect("tree builder should be created");
        tree.insert(path, blob, 0o100644)
            .expect("file should be added to tree");
        let tree_id = tree.write().expect("tree should be written");
        let tree = repository.find_tree(tree_id).expect("tree should exist");
        let signature = repository.signature().expect("signature should exist");

        repository
            .commit(
                Some(reference),
                &signature,
                &signature,
                &format!("Update {path}"),
                &tree,
                &[&parent],
            )
            .expect("commit should be created")
    }

    fn force_checkout_head(repository: &GitRepository) {
        let mut checkout = CheckoutBuilder::new();
        checkout.force();
        repository
            .checkout_head(Some(&mut checkout))
            .expect("HEAD should be checked out");
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
        let (directory, repository) = repository_with_branches();
        let worktree_directory = TempDir::new().expect("temporary directory should be created");
        add_feature_worktree(&repository, &worktree_directory);
        (directory, worktree_directory, Repository::new(repository))
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

    #[test]
    fn switches_to_available_branch() {
        let (_directory, repository) = repository_with_branches();
        let repository = Repository::new(repository);
        let feature = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.name() == "feature")
            .expect("feature branch should exist");

        repository
            .switch_to(&feature)
            .expect("branch should be switched");

        assert_eq!(
            repository
                .current_branch()
                .expect("current branch should be read")
                .as_deref(),
            Some("feature")
        );
    }

    #[test]
    fn refuses_to_switch_to_branch_in_other_worktree() {
        let (_dir1, _dir2, repository) = initialise_worktree();
        let feature = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.name() == "feature")
            .expect("feature branch should exist");

        let error = repository
            .switch_to(&feature)
            .expect_err("checked-out branch should not be switched to");

        assert!(matches!(error, Error::BranchCheckedOut(name) if name == "feature"));
    }

    #[test]
    fn refuses_to_overwrite_working_tree_changes_when_switching() {
        let (directory, repository) = repository_with_branches();
        let initial = repository
            .head()
            .expect("HEAD should exist")
            .target()
            .expect("HEAD should have a target");
        commit_file(
            &repository,
            "refs/heads/feature",
            initial,
            "shared.txt",
            "committed\n",
        );
        fs::write(directory.path().join("shared.txt"), "working tree\n")
            .expect("working-tree file should be written");
        let repository = Repository::new(repository);
        let feature = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.name() == "feature")
            .expect("feature branch should exist");

        repository
            .switch_to(&feature)
            .expect_err("switch should not overwrite an untracked file");

        assert_eq!(
            repository
                .current_branch()
                .expect("current branch should be read")
                .as_deref(),
            Some("main")
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("shared.txt"))
                .expect("working-tree file should remain"),
            "working tree\n"
        );
    }

    #[test]
    fn rebases_current_branch_onto_target() {
        let (_directory, repository) = repository_with_branches();
        let initial = repository
            .head()
            .expect("HEAD should exist")
            .target()
            .expect("HEAD should have a target");
        let feature_tip = commit_file(
            &repository,
            "refs/heads/feature",
            initial,
            "feature.txt",
            "feature\n",
        );
        let old_main_tip = commit_file(
            &repository,
            "refs/heads/main",
            initial,
            "main.txt",
            "main\n",
        );
        force_checkout_head(&repository);
        let repository = Repository::new(repository);
        let feature = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.name() == "feature")
            .expect("feature branch should exist");

        let outcome = repository
            .rebase_onto(&feature)
            .expect("rebase should succeed");

        let new_main_tip = repository
            .inner
            .head()
            .expect("HEAD should exist")
            .target()
            .expect("HEAD should have a target");
        let new_main = repository
            .inner
            .find_commit(new_main_tip)
            .expect("rebased commit should exist");
        assert_eq!(outcome, RebaseOutcome::Completed);
        assert_ne!(new_main_tip, old_main_tip);
        assert_eq!(new_main.parent_id(0), Ok(feature_tip));
        assert!(
            new_main
                .tree()
                .expect("tree should exist")
                .get_name("main.txt")
                .is_some()
        );
    }

    #[test]
    fn leaves_conflicted_rebase_in_progress() {
        let (directory, repository) = repository_with_branches();
        let initial = repository
            .head()
            .expect("HEAD should exist")
            .target()
            .expect("HEAD should have a target");
        commit_file(
            &repository,
            "refs/heads/feature",
            initial,
            "shared.txt",
            "feature\n",
        );
        commit_file(
            &repository,
            "refs/heads/main",
            initial,
            "shared.txt",
            "main\n",
        );
        force_checkout_head(&repository);
        let repository = Repository::new(repository);
        let feature = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.name() == "feature")
            .expect("feature branch should exist");

        let outcome = repository
            .rebase_onto(&feature)
            .expect("conflict should be returned as an outcome");

        assert_eq!(outcome, RebaseOutcome::Conflicted);
        assert!(matches!(
            repository.inner.state(),
            RepositoryState::RebaseMerge | RepositoryState::RebaseInteractive
        ));
        assert!(
            GitRepository::open(directory.path())
                .expect("repository should reopen")
                .index()
                .expect("index should open")
                .has_conflicts()
        );
    }
}
