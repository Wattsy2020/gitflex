use std::collections::HashSet;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};

use git2::Worktree;
use git2::build::CheckoutBuilder;
use git2::{
    Branch, BranchType, ErrorCode, ObjectType, Repository as GitRepository, RepositoryState,
    StatusOptions,
};
use thiserror::Error;

use crate::git::Error::DeletedWorktree;
use crate::git::cherry::is_rewritten_onto;

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CleanBranch {
    branch: LocalBranch,
    is_trunk: bool,
    is_merged: bool,
    is_authored_by_other: bool,
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

    pub fn is_merge_source(&self) -> bool {
        self.checkout != Checkout::CurrentWorktree
    }
}

impl CleanBranch {
    fn new(
        branch: LocalBranch,
        is_trunk: bool,
        is_merged: bool,
        is_authored_by_other: bool,
    ) -> Self {
        Self {
            branch,
            is_trunk,
            is_merged,
            is_authored_by_other,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        branch: LocalBranch,
        is_trunk: bool,
        is_merged: bool,
        is_authored_by_other: bool,
    ) -> Self {
        Self::new(branch, is_trunk, is_merged, is_authored_by_other)
    }

    pub fn name(&self) -> &str {
        self.branch.name()
    }

    pub fn branch(&self) -> &LocalBranch {
        &self.branch
    }

    pub fn is_trunk(&self) -> bool {
        self.is_trunk
    }

    pub fn is_merged(&self) -> bool {
        self.is_merged
    }

    pub fn is_authored_by_other(&self) -> bool {
        self.is_authored_by_other
    }

    pub fn into_branch(self) -> LocalBranch {
        self.branch
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictableCommandOutcome {
    Completed,
    Conflicted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InProgressOperation {
    Merge,
    Revert,
    CherryPick,
    Bisect,
    Rebase,
    MailboxApply,
    MailboxApplyOrRebase,
}

impl InProgressOperation {
    fn from_repository_state(state: RepositoryState) -> Option<Self> {
        match state {
            RepositoryState::Clean => None,
            RepositoryState::Merge => Some(Self::Merge),
            RepositoryState::Revert | RepositoryState::RevertSequence => Some(Self::Revert),
            RepositoryState::CherryPick | RepositoryState::CherryPickSequence => {
                Some(Self::CherryPick)
            }
            RepositoryState::Bisect => Some(Self::Bisect),
            RepositoryState::Rebase
            | RepositoryState::RebaseInteractive
            | RepositoryState::RebaseMerge => Some(Self::Rebase),
            RepositoryState::ApplyMailbox => Some(Self::MailboxApply),
            RepositoryState::ApplyMailboxOrRebase => Some(Self::MailboxApplyOrRebase),
        }
    }
}

impl fmt::Display for InProgressOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let operation = match self {
            Self::Merge => "merge",
            Self::Revert => "revert",
            Self::CherryPick => "cherry-pick",
            Self::Bisect => "bisect",
            Self::Rebase => "rebase",
            Self::MailboxApply => "mailbox apply",
            Self::MailboxApplyOrRebase => "mailbox apply or rebase",
        };
        formatter.write_str(operation)
    }
}

pub struct Repository {
    inner: git2::Repository,
    worktree: PathBuf,
}

pub struct HeadOperationRepository {
    repository: Repository,
}

pub struct CleanRebaseRepository {
    repository: Repository,
}

impl Repository {
    fn new(repository: GitRepository) -> Repository {
        let worktree = repository
            .workdir()
            .expect("repository should have been validated as non-bare")
            .to_owned();
        Repository {
            inner: repository,
            worktree,
        }
    }

    pub fn discover(path: impl AsRef<Path>) -> Result<Self, Error> {
        let repository = git2::Repository::discover(path)?;
        if repository.is_bare() {
            return Err(Error::BareRepository);
        }
        Ok(Repository::new(repository))
    }

    pub(crate) fn common_directory(&self) -> &Path {
        self.inner.commondir()
    }

    pub fn into_head_operation(self) -> Result<HeadOperationRepository, Error> {
        if let Some(operation) = InProgressOperation::from_repository_state(self.inner.state()) {
            return Err(Error::OperationInProgress(operation));
        }
        Ok(HeadOperationRepository { repository: self })
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

    pub fn clean_branches(&self) -> Result<Vec<CleanBranch>, Error> {
        let branches = self.local_branches()?;
        let base_name = ["main", "master"]
            .into_iter()
            .find(|candidate| branches.iter().any(|branch| branch.name() == *candidate));
        let base_tip = base_name
            .map(|name| self.inner.refname_to_id(&format!("refs/heads/{name}")))
            .transpose()?;

        // ignore any errors when retrieving user email
        // if the user hasn't configured their email then we will ignore that rule, and continue the UI flow
        let user_email = configured_user_email(&self.inner).ok().flatten();

        branches
            .into_iter()
            .map(|branch| {
                let is_trunk = base_name == Some(branch.name());
                let tip = self
                    .find_branch(&branch)?
                    .into_reference()
                    .peel_to_commit()?;
                let is_merged = match base_tip {
                    Some(base_tip) => {
                        tip.id() == base_tip
                            || self.inner.graph_descendant_of(base_tip, tip.id())?
                            || is_rewritten_onto(&self.inner, tip.id(), base_tip)?
                    }
                    None => false,
                };
                let is_authored_by_other = user_email.as_deref().is_some_and(|user_email| {
                    tip.author()
                        .email()
                        .is_ok_and(|author_email| author_email != user_email)
                });

                Ok(CleanBranch::new(
                    branch,
                    is_trunk,
                    is_merged,
                    is_authored_by_other,
                ))
            })
            .collect()
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
        self.get_non_checkedout_branch(branch)?;
        let output = Command::new("git")
            .args(["branch", "-D", "--", branch.name()])
            .current_dir(&self.worktree)
            .output()?;
        output
            .status
            .success()
            .then_some(())
            .ok_or(Error::DeleteBranchFailed {
                status: output.status,
                message: command_message(&output.stdout, &output.stderr),
            })
    }

    fn checked_out_branches(&self) -> Result<HashSet<String>, Error> {
        let linked_worktree_branches = self
            .inner
            .worktrees()?
            .iter()
            .map(|name| {
                let name = name?.ok_or(Error::InvalidWorktreeName)?;
                // ignore if we failed to open the worktree
                // we just use worktrees to show additional information to the user,
                // the tool can proceed if one worktree is in a weird state by not showing that information to the user
                Ok(self
                    .find_worktree(name)
                    .ok()
                    .and_then(|worktree| GitRepository::open_from_worktree(&worktree).ok())
                    .and_then(|repository| checked_out_branch(&repository).ok())
                    .flatten())
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

    fn find_worktree(&self, name: &str) -> Result<Worktree, Error> {
        let worktree = self.inner.find_worktree(name)?;

        // automatically prune invalid worktrees whose directories were deleted
        // git does this so its ok
        if worktree.is_prunable(None)? {
            worktree.prune(None)?;
            return Err(DeletedWorktree);
        }

        Ok(worktree)
    }

    fn run_git_operation<'a, 'b>(
        &'a self,
        branch: &LocalBranch,
        arguments: impl IntoIterator<Item = &'b str>,
    ) -> Result<ConflictableCommandOutcome, Error> {
        let other_branch = self.find_branch(branch)?.get().name()?.to_string();
        let mut args: Vec<&str> = arguments.into_iter().collect();
        args.push(&other_branch);
        let command = args[0].to_string();
        let had_conflicts = has_conflicts(&self.worktree)?;

        let output = Command::new("git")
            .args(args)
            .current_dir(&self.worktree)
            .output()?;

        if output.status.success() {
            Ok(ConflictableCommandOutcome::Completed)
        } else if !had_conflicts && has_conflicts(&self.worktree)? {
            Ok(ConflictableCommandOutcome::Conflicted)
        } else {
            Err(Error::CommandFailed {
                command,
                status: output.status,
                message: command_message(&output.stdout, &output.stderr),
            })
        }
    }
}

impl HeadOperationRepository {
    pub(crate) fn common_directory(&self) -> &Path {
        self.repository.common_directory()
    }

    pub fn local_branches(&self) -> Result<Vec<LocalBranch>, Error> {
        self.repository.local_branches()
    }

    pub fn current_branch(&self) -> Result<Option<String>, Error> {
        self.repository.current_branch()
    }

    pub fn into_clean_rebase(self) -> Result<CleanRebaseRepository, Error> {
        if has_tracked_changes(&self.repository.inner)? {
            return Err(Error::TrackedChangesForRebase);
        }
        Ok(CleanRebaseRepository {
            repository: self.repository,
        })
    }

    pub fn switch_to(&self, branch: &LocalBranch) -> Result<(), Error> {
        let branch = self.repository.get_non_checkedout_branch(branch)?;
        let reference = branch.get();
        let reference_name = reference.name()?;
        let target = reference.peel(ObjectType::Commit)?;
        let mut checkout = CheckoutBuilder::new();
        checkout.safe();

        self.repository
            .inner
            .checkout_tree(&target, Some(&mut checkout))?;
        self.repository.inner.set_head(reference_name)?;
        Ok(())
    }

    pub fn merge_from(&self, branch: &LocalBranch) -> Result<ConflictableCommandOutcome, Error> {
        let current_branch = self.current_branch()?.ok_or(Error::DetachedHeadForMerge)?;
        if current_branch == branch.name() {
            return Err(Error::CurrentBranchAsMergeSource);
        }
        let outcome = self
            .repository
            .run_git_operation(branch, ["merge", "--no-edit"])?;
        Ok(outcome)
    }
}

impl CleanRebaseRepository {
    pub(crate) fn common_directory(&self) -> &Path {
        self.repository.common_directory()
    }

    pub fn local_branches(&self) -> Result<Vec<LocalBranch>, Error> {
        self.repository.local_branches()
    }

    pub fn current_branch(&self) -> Result<Option<String>, Error> {
        self.repository.current_branch()
    }

    pub fn rebase_onto(&self, branch: &LocalBranch) -> Result<ConflictableCommandOutcome, Error> {
        let current_branch = self.current_branch()?.ok_or(Error::DetachedHead)?;
        if current_branch == branch.name() {
            return Err(Error::CurrentBranchAsRebaseTarget);
        }
        let outcome = self.repository.run_git_operation(branch, ["rebase"])?;
        Ok(outcome)
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

fn configured_user_email(repository: &GitRepository) -> Result<Option<String>, Error> {
    match repository.config()?.get_string("user.email") {
        Ok(email) => Ok((!email.is_empty()).then_some(email)),
        Err(error) if error.code() == ErrorCode::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn has_conflicts(worktree: &Path) -> Result<bool, Error> {
    Ok(GitRepository::open(worktree)?.index()?.has_conflicts())
}

fn has_tracked_changes(repository: &GitRepository) -> Result<bool, Error> {
    let mut options = StatusOptions::new();
    options.include_untracked(false).include_ignored(false);
    Ok(!repository.statuses(Some(&mut options))?.is_empty())
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
    #[error("deleted worktree")]
    DeletedWorktree,
    #[error("branch {0} is checked out in a worktree")]
    BranchCheckedOut(String),
    #[error("git branch deletion failed with {status}: {message}")]
    DeleteBranchFailed { status: ExitStatus, message: String },
    #[error("cannot rebase while HEAD is detached")]
    DetachedHead,
    #[error("the current branch cannot be its own rebase target")]
    CurrentBranchAsRebaseTarget,
    #[error("cannot operate in a bare repository")]
    BareRepository,
    #[error("cannot perform this operation while a Git {0} operation is in progress")]
    OperationInProgress(InProgressOperation),
    #[error("cannot rebase with staged or unstaged tracked changes; commit or stash them first")]
    TrackedChangesForRebase,
    #[error("git {command} failed with {status}: {message}")]
    CommandFailed {
        command: String,
        status: ExitStatus,
        message: String,
    },
    #[error(
        "rebase stopped due to conflicts; resolve them and run `git rebase --continue`, or run `git rebase --abort`"
    )]
    RebaseConflicts,
    #[error("cannot merge while HEAD is detached")]
    DetachedHeadForMerge,
    #[error("the current branch cannot be merged into itself")]
    CurrentBranchAsMergeSource,
    #[error(
        "merge stopped due to conflicts; resolve them and run `git merge --continue`, or run `git merge --abort`"
    )]
    MergeConflicts,
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use git2::build::CheckoutBuilder;
    use git2::{
        BranchType, ErrorCode, Oid, Repository as GitRepository, RepositoryState, Signature,
        WorktreeAddOptions,
    };
    use tempfile::TempDir;

    use super::{
        Checkout, CleanBranch, ConflictableCommandOutcome, Error, InProgressOperation, LocalBranch,
        Repository,
    };

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
                .set_str("user.email", "gitflex@example.com")
                .expect("user email should be configured");
        }

        let tree_id = {
            let mut index = repository.index().expect("index should open");
            index.write_tree().expect("empty tree should be written")
        };
        let tree = repository.find_tree(tree_id).expect("tree should exist");
        let signature =
            Signature::now("Git Branch", "gitflex@example.com").expect("signature should be valid");
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

    fn repository_with_tracked_file() -> (TempDir, GitRepository) {
        let (directory, repository) = repository_with_branches();
        let initial = repository
            .head()
            .expect("HEAD should exist")
            .target()
            .expect("HEAD should have a target");
        commit_file(
            &repository,
            "refs/heads/main",
            initial,
            "tracked.txt",
            "original\n",
        );
        force_checkout_head(&repository);
        (directory, repository)
    }

    fn commit_file(
        repository: &GitRepository,
        reference: &str,
        parent_id: Oid,
        path: &str,
        contents: &str,
    ) -> Oid {
        let signature = repository.signature().expect("signature should exist");
        commit_file_as(repository, reference, parent_id, path, contents, &signature)
    }

    fn commit_file_as(
        repository: &GitRepository,
        reference: &str,
        parent_id: Oid,
        path: &str,
        contents: &str,
        author: &Signature<'_>,
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
        let committer = repository.signature().expect("signature should exist");

        repository
            .commit(
                Some(reference),
                author,
                &committer,
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

    fn clean_branch<'a>(branches: &'a [CleanBranch], name: &str) -> &'a CleanBranch {
        branches
            .iter()
            .find(|branch| branch.name() == name)
            .unwrap_or_else(|| panic!("clean branch {name} should exist"))
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
    fn refuses_to_discover_bare_repository() {
        let directory = TempDir::new().expect("temporary directory should be created");
        GitRepository::init_bare(directory.path()).expect("bare repository should be created");

        let error = Repository::discover(directory.path())
            .err()
            .expect("bare repository should be rejected");

        assert!(matches!(error, Error::BareRepository));
    }

    #[test]
    fn categorises_every_in_progress_repository_state() {
        let states = [
            (RepositoryState::Clean, None),
            (RepositoryState::Merge, Some(InProgressOperation::Merge)),
            (RepositoryState::Revert, Some(InProgressOperation::Revert)),
            (
                RepositoryState::RevertSequence,
                Some(InProgressOperation::Revert),
            ),
            (
                RepositoryState::CherryPick,
                Some(InProgressOperation::CherryPick),
            ),
            (
                RepositoryState::CherryPickSequence,
                Some(InProgressOperation::CherryPick),
            ),
            (RepositoryState::Bisect, Some(InProgressOperation::Bisect)),
            (RepositoryState::Rebase, Some(InProgressOperation::Rebase)),
            (
                RepositoryState::RebaseInteractive,
                Some(InProgressOperation::Rebase),
            ),
            (
                RepositoryState::RebaseMerge,
                Some(InProgressOperation::Rebase),
            ),
            (
                RepositoryState::ApplyMailbox,
                Some(InProgressOperation::MailboxApply),
            ),
            (
                RepositoryState::ApplyMailboxOrRebase,
                Some(InProgressOperation::MailboxApplyOrRebase),
            ),
        ];

        assert!(states.into_iter().all(|(state, expected)| {
            InProgressOperation::from_repository_state(state) == expected
        }));
    }

    #[test]
    fn tracked_unstaged_changes_allow_head_operations_but_prevent_rebase() {
        let (directory, repository) = repository_with_tracked_file();
        fs::write(directory.path().join("tracked.txt"), "modified\n")
            .expect("tracked file should be modified");

        let error = Repository::new(repository)
            .into_head_operation()
            .expect("tracked changes should allow switch and merge")
            .into_clean_rebase()
            .err()
            .expect("unstaged changes should prevent rebase");

        assert!(matches!(error, Error::TrackedChangesForRebase));
    }

    #[test]
    fn tracked_staged_changes_allow_head_operations_but_prevent_rebase() {
        let (directory, repository) = repository_with_tracked_file();
        fs::write(directory.path().join("tracked.txt"), "modified\n")
            .expect("tracked file should be modified");
        {
            let mut index = repository.index().expect("index should open");
            index
                .add_path(Path::new("tracked.txt"))
                .expect("tracked file should be staged");
            index.write().expect("index should be written");
        }

        let error = Repository::new(repository)
            .into_head_operation()
            .expect("tracked changes should allow switch and merge")
            .into_clean_rebase()
            .err()
            .expect("staged changes should prevent rebase");

        assert!(matches!(error, Error::TrackedChangesForRebase));
    }

    #[test]
    fn untracked_and_ignored_files_allow_head_operations() {
        let (directory, repository) = repository_with_branches();
        repository
            .add_ignore_rule("/ignored.txt")
            .expect("ignore rule should be added");
        fs::write(directory.path().join("untracked.txt"), "untracked\n")
            .expect("untracked file should be written");
        fs::write(directory.path().join("ignored.txt"), "ignored\n")
            .expect("ignored file should be written");

        Repository::new(repository)
            .into_head_operation()
            .expect("untracked and ignored files should allow switch and merge")
            .into_clean_rebase()
            .expect("untracked and ignored files should allow rebase");
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
    fn clean_classifies_merged_foreign_owned_and_unmerged_branches() {
        let (_directory, repository) = repository_with_branches();
        let initial = repository
            .head()
            .expect("HEAD should exist")
            .target()
            .expect("HEAD should have a target");
        let initial_commit = repository
            .find_commit(initial)
            .expect("initial commit should exist");
        repository
            .branch("merged", &initial_commit, false)
            .expect("merged branch should be created");
        repository
            .branch("foreign", &initial_commit, false)
            .expect("foreign branch should be created");
        drop(initial_commit);

        commit_file(
            &repository,
            "refs/heads/feature",
            initial,
            "feature.txt",
            "feature\n",
        );
        let foreign_author = Signature::now("Reviewer", "reviewer@example.com")
            .expect("foreign signature should be valid");
        commit_file_as(
            &repository,
            "refs/heads/foreign",
            initial,
            "foreign.txt",
            "foreign\n",
            &foreign_author,
        );

        let branches = Repository::new(repository)
            .clean_branches()
            .expect("clean branches should be classified");

        let feature = clean_branch(&branches, "feature");
        assert!(!feature.is_trunk());
        assert!(!feature.is_merged());
        assert!(!feature.is_authored_by_other());

        let foreign = clean_branch(&branches, "foreign");
        assert!(!foreign.is_trunk());
        assert!(!foreign.is_merged());
        assert!(foreign.is_authored_by_other());

        let merged = clean_branch(&branches, "merged");
        assert!(!merged.is_trunk());
        assert!(merged.is_merged());
        assert!(!merged.is_authored_by_other());

        let main = clean_branch(&branches, "main");
        assert!(main.is_trunk());
        assert!(main.is_merged());
        assert!(!main.is_authored_by_other());
    }

    #[test]
    fn clean_branch_metadata_preserves_checkout_status() {
        let (_main_directory, _worktree_directory, repository) = initialise_worktree();
        let branches = repository
            .clean_branches()
            .expect("clean branches should be classified");

        assert_eq!(
            clean_branch(&branches, "feature").branch().checkout(),
            Checkout::OtherWorktree
        );
        assert_eq!(
            clean_branch(&branches, "main").branch().checkout(),
            Checkout::CurrentWorktree
        );
    }

    #[test]
    fn clean_prefers_main_over_master_and_uses_master_as_a_fallback() {
        let (_directory, repository) = repository_with_branches();
        let initial = repository
            .head()
            .expect("HEAD should exist")
            .target()
            .expect("HEAD should have a target");
        let initial_commit = repository
            .find_commit(initial)
            .expect("initial commit should exist");
        repository
            .branch("master", &initial_commit, false)
            .expect("master branch should be created");
        drop(initial_commit);
        commit_file(
            &repository,
            "refs/heads/master",
            initial,
            "master.txt",
            "master\n",
        );
        repository
            .set_head_detached(initial)
            .expect("HEAD should detach");
        let branches = Repository::new(repository)
            .clean_branches()
            .expect("clean branches should be classified");

        assert!(clean_branch(&branches, "main").is_trunk());
        let master = clean_branch(&branches, "master");
        assert!(!master.is_trunk());
        assert!(!master.is_merged());

        let (_directory, repository) = repository_with_branches();
        let initial = repository
            .head()
            .expect("HEAD should exist")
            .target()
            .expect("HEAD should have a target");
        repository
            .find_branch("main", BranchType::Local)
            .expect("main branch should exist")
            .rename("master", false)
            .expect("main should be renamed to master");
        repository
            .set_head_detached(initial)
            .expect("HEAD should detach");
        let branches = Repository::new(repository)
            .clean_branches()
            .expect("clean branches should be classified");

        assert!(clean_branch(&branches, "feature").is_merged());
        assert!(clean_branch(&branches, "master").is_trunk());
    }

    #[test]
    fn clean_uses_available_rules_when_base_or_user_email_is_missing() {
        let (_directory, repository) = repository_with_branches();
        let initial = repository
            .head()
            .expect("HEAD should exist")
            .target()
            .expect("HEAD should have a target");
        repository
            .find_branch("main", BranchType::Local)
            .expect("main branch should exist")
            .rename("trunk", false)
            .expect("main should be renamed to trunk");
        repository
            .set_head_detached(initial)
            .expect("HEAD should detach");
        let branches = Repository::new(repository)
            .clean_branches()
            .expect("clean branches should be classified");

        let feature = clean_branch(&branches, "feature");
        assert!(!feature.is_trunk());
        assert!(!feature.is_merged());

        let (_directory, repository) = repository_with_branches();
        let initial = repository
            .head()
            .expect("HEAD should exist")
            .target()
            .expect("HEAD should have a target");
        let initial_commit = repository
            .find_commit(initial)
            .expect("initial commit should exist");
        repository
            .branch("foreign", &initial_commit, false)
            .expect("foreign branch should be created");
        drop(initial_commit);
        let foreign_author = Signature::now("Reviewer", "reviewer@example.com")
            .expect("foreign signature should be valid");
        commit_file_as(
            &repository,
            "refs/heads/foreign",
            initial,
            "foreign.txt",
            "foreign\n",
            &foreign_author,
        );
        repository
            .config()
            .expect("config should open")
            .set_str("user.email", "")
            .expect("user email should be cleared");
        let branches = Repository::new(repository)
            .clean_branches()
            .expect("clean branches should be classified");

        assert!(clean_branch(&branches, "feature").is_merged());
        assert!(!clean_branch(&branches, "foreign").is_authored_by_other());
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
    fn deletes_branch_with_tracked_worktree_changes() {
        let (directory, repository) = repository_with_tracked_file();
        fs::write(directory.path().join("tracked.txt"), "modified\n")
            .expect("tracked file should be modified");
        let repository = Repository::new(repository);
        let feature = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.name() == "feature")
            .expect("feature branch should exist");

        repository
            .delete_branch(&feature)
            .expect("dirty worktree should not prevent branch deletion");

        assert_eq!(
            fs::read_to_string(directory.path().join("tracked.txt"))
                .expect("tracked file should remain"),
            "modified\n"
        );
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
    fn linked_worktree_rebases_onto_a_branch_from_the_main_worktree() {
        let (_main_directory, worktree_directory, _) = initialise_worktree();
        let repository = Repository::discover(worktree_directory.path().join("feature"))
            .expect("linked worktree repository should be discovered");
        let main = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.name() == "main")
            .expect("main branch should exist");
        let repository = repository
            .into_head_operation()
            .expect("repository should allow HEAD operations")
            .into_clean_rebase()
            .expect("repository should allow rebase");

        let outcome = repository
            .rebase_onto(&main)
            .expect("up-to-date rebase should succeed");

        assert_eq!(outcome, ConflictableCommandOutcome::Completed);
    }

    #[test]
    fn linked_worktree_merges_a_branch_from_the_main_worktree() {
        let (_main_directory, worktree_directory, _) = initialise_worktree();
        let repository = Repository::discover(worktree_directory.path().join("feature"))
            .expect("linked worktree repository should be discovered");
        let main = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.name() == "main")
            .expect("main branch should exist");
        let repository = repository
            .into_head_operation()
            .expect("repository should allow HEAD operations");

        let outcome = repository
            .merge_from(&main)
            .expect("up-to-date merge should succeed");

        assert_eq!(outcome, ConflictableCommandOutcome::Completed);
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
        let repository = repository
            .into_head_operation()
            .expect("repository should allow HEAD operations");

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
        let (_main_directory, _worktree_directory, repository) = initialise_worktree();
        let feature = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.name() == "feature")
            .expect("feature branch should exist");
        let repository = repository
            .into_head_operation()
            .expect("repository should allow HEAD operations");

        let error = repository
            .switch_to(&feature)
            .expect_err("checked-out branch should not be switched to");

        assert!(matches!(error, Error::BranchCheckedOut(name) if name == "feature"));
    }

    #[test]
    fn linked_worktree_switches_to_an_available_branch() {
        let (_main_directory, repository) = repository_with_branches();
        let head = repository
            .head()
            .expect("HEAD should exist")
            .peel_to_commit()
            .expect("HEAD should point to a commit");
        repository
            .branch("review", &head, false)
            .expect("review branch should be created");
        drop(head);
        let worktree_directory = TempDir::new().expect("temporary directory should be created");
        add_feature_worktree(&repository, &worktree_directory);
        drop(repository);

        let repository = Repository::discover(worktree_directory.path().join("feature"))
            .expect("linked worktree repository should be discovered");
        let review = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.name() == "review")
            .expect("review branch should exist");
        let repository = repository
            .into_head_operation()
            .expect("repository should allow HEAD operations");

        repository
            .switch_to(&review)
            .expect("review branch should be switched");

        assert_eq!(
            repository
                .current_branch()
                .expect("current branch should be readable")
                .as_deref(),
            Some("review")
        );
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
        let repository = repository
            .into_head_operation()
            .expect("untracked files should allow HEAD operations");

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
    fn fast_forward_merges_source_into_current_branch() {
        let (directory, repository) = repository_with_branches();
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
        let repository = Repository::new(repository);
        let feature = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.name() == "feature")
            .expect("feature branch should exist");
        let repository = repository
            .into_head_operation()
            .expect("repository should allow HEAD operations");

        let outcome = repository
            .merge_from(&feature)
            .expect("merge should succeed");

        assert_eq!(outcome, ConflictableCommandOutcome::Completed);
        assert_eq!(
            repository
                .repository
                .inner
                .head()
                .expect("HEAD should exist")
                .target(),
            Some(feature_tip)
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("feature.txt"))
                .expect("merged file should be checked out"),
            "feature\n"
        );
    }

    #[test]
    fn refuses_to_merge_current_branch_into_itself() {
        let (_directory, repository) = repository_with_branches();
        let repository = Repository::new(repository);
        let main = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.name() == "main")
            .expect("main branch should exist");
        let repository = repository
            .into_head_operation()
            .expect("repository should allow HEAD operations");

        let error = repository
            .merge_from(&main)
            .expect_err("current branch should not be merged into itself");

        assert!(matches!(error, Error::CurrentBranchAsMergeSource));
    }

    #[test]
    fn refuses_to_merge_a_missing_branch() {
        let (_directory, repository) = repository_with_branches();
        let repository = Repository::new(repository)
            .into_head_operation()
            .expect("repository should allow HEAD operations");
        let missing = LocalBranch::new("missing", Checkout::Available);

        repository
            .merge_from(&missing)
            .expect_err("missing merge source should fail");
    }

    #[test]
    fn refuses_to_merge_with_detached_head() {
        let (_directory, repository) = repository_with_branches();
        let head = repository
            .head()
            .expect("HEAD should exist")
            .target()
            .expect("HEAD should have a target");
        repository
            .set_head_detached(head)
            .expect("HEAD should detach");
        let repository = Repository::new(repository);
        let feature = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.name() == "feature")
            .expect("feature branch should exist");
        let repository = repository
            .into_head_operation()
            .expect("repository should allow HEAD operations");

        let error = repository
            .merge_from(&feature)
            .expect_err("merge should require a current branch");

        assert!(matches!(error, Error::DetachedHeadForMerge));
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
        let repository = repository
            .into_head_operation()
            .expect("repository should allow HEAD operations")
            .into_clean_rebase()
            .expect("repository should allow rebase");

        let outcome = repository
            .rebase_onto(&feature)
            .expect("rebase should succeed");

        let new_main_tip = repository
            .repository
            .inner
            .head()
            .expect("HEAD should exist")
            .target()
            .expect("HEAD should have a target");
        let new_main = repository
            .repository
            .inner
            .find_commit(new_main_tip)
            .expect("rebased commit should exist");
        assert_eq!(outcome, ConflictableCommandOutcome::Completed);
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
    fn refuses_to_rebase_current_branch_onto_itself() {
        let (_directory, repository) = repository_with_branches();
        let repository = Repository::new(repository);
        let main = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.name() == "main")
            .expect("main branch should exist");
        let repository = repository
            .into_head_operation()
            .expect("repository should allow HEAD operations")
            .into_clean_rebase()
            .expect("repository should allow rebase");

        let error = repository
            .rebase_onto(&main)
            .expect_err("a branch should not be rebased onto itself");

        assert!(matches!(error, Error::CurrentBranchAsRebaseTarget));
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
        {
            let initial = repository
                .find_commit(initial)
                .expect("initial commit should exist");
            repository
                .branch("unrelated", &initial, false)
                .expect("unrelated branch should be created");
        }
        force_checkout_head(&repository);
        let repository = Repository::new(repository);
        let feature = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.name() == "feature")
            .expect("feature branch should exist");
        let repository = repository
            .into_head_operation()
            .expect("repository should allow HEAD operations")
            .into_clean_rebase()
            .expect("repository should allow rebase");

        let outcome = repository
            .rebase_onto(&feature)
            .expect("conflict should be returned as an outcome");

        assert_eq!(outcome, ConflictableCommandOutcome::Conflicted);
        assert!(matches!(
            repository.repository.inner.state(),
            RepositoryState::RebaseMerge | RepositoryState::RebaseInteractive
        ));
        assert!(
            GitRepository::open(directory.path())
                .expect("repository should reopen")
                .index()
                .expect("index should open")
                .has_conflicts()
        );
        let repository = Repository::discover(directory.path())
            .expect("active rebase should still allow branch deletion");
        let unrelated = repository
            .local_branches()
            .expect("branches should be listed")
            .into_iter()
            .find(|branch| branch.name() == "unrelated")
            .expect("unrelated branch should exist");
        repository
            .delete_branch(&unrelated)
            .expect("active rebase should allow unrelated branch deletion");

        let error = repository
            .into_head_operation()
            .err()
            .expect("active rebase should prevent HEAD operations");
        assert!(matches!(
            error,
            Error::OperationInProgress(InProgressOperation::Rebase)
        ));
    }
}
