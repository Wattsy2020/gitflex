/// Tests how the git.rs module interoperates with the git CLI
/// The git CLI sometimes has differing behaviour to libgit2,
/// such as writing a .git/rebase-merge/git-rebase-todo for conflicting rebases
///
/// Tests should set up the repository with the git CLI,
/// perform an operation with the git module,
/// then validate the results with the git CLI
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use gitbranch::git::{Checkout, ConflictableCommandOutcome, Error, LocalBranch, Repository};
use tempfile::TempDir;

struct TestRepository {
    directory: TempDir,
    path: PathBuf,
    home: PathBuf,
}

impl TestRepository {
    fn new() -> Self {
        let directory = TempDir::new().expect("temporary directory should be created");
        let path = directory.path().join("repository");
        let home = directory.path().join("home");
        let hooks = directory.path().join("hooks");
        fs::create_dir_all(&home).expect("temporary home should be created");
        fs::create_dir_all(&hooks).expect("empty hooks directory should be created");

        let repository = Self {
            directory,
            path,
            home,
        };
        let path = repository
            .path
            .to_str()
            .expect("path should be valid UTF-8");
        repository.git_success_at(
            repository.directory.path(),
            &["init", "--initial-branch=main", path],
        );
        repository.git_success(&["config", "user.name", "Git Branch Tests"]);
        repository.git_success(&["config", "user.email", "gitbranch@example.com"]);
        repository.git_success(&["config", "commit.gpgSign", "false"]);
        repository.git_success(&["config", "core.autocrlf", "false"]);
        repository.git_success(&[
            "config",
            "core.hooksPath",
            hooks.to_str().expect("path should be valid UTF-8"),
        ]);
        repository.git_success(&["commit", "--allow-empty", "-m", "Initial commit"]);

        repository
    }

    fn command_at(&self, path: &Path) -> Command {
        let mut command = Command::new("git");
        command
            .current_dir(path)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", &self.home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GIT_CONFIG_GLOBAL")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_OBJECT_DIRECTORY")
            .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES");
        command
    }

    fn git_at(&self, path: &Path, arguments: &[&str]) -> Output {
        self.command_at(path)
            .args(arguments)
            .output()
            .expect("git should be available on PATH")
    }

    fn git(&self, arguments: &[&str]) -> Output {
        self.git_at(&self.path, arguments)
    }

    fn gitbranch(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_gitbranch"))
            .current_dir(&self.path)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", &self.home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GIT_CONFIG_GLOBAL")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_OBJECT_DIRECTORY")
            .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
            .args(arguments)
            .output()
            .expect("gitbranch should be executable")
    }

    fn git_success_at(&self, path: &Path, arguments: &[&str]) -> Output {
        let output = self.git_at(path, arguments);
        assert_success(arguments, &output);
        output
    }

    fn git_success(&self, arguments: &[&str]) -> Output {
        self.git_success_at(&self.path, arguments)
    }

    fn git_stdout_at(&self, path: &Path, arguments: &[&str]) -> String {
        let output = self.git_success_at(path, arguments);
        String::from_utf8(output.stdout)
            .expect("git output should be valid UTF-8")
            .trim()
            .to_string()
    }

    fn git_stdout(&self, arguments: &[&str]) -> String {
        self.git_stdout_at(&self.path, arguments)
    }

    fn create_branch(&self, name: &str) {
        self.git_success(&["branch", name]);
    }

    fn switch_to(&self, name: &str) {
        self.git_success(&["switch", name]);
    }

    fn commit_file(&self, path: &str, contents: &str, message: &str) {
        fs::write(self.path.join(path), contents).expect("file should be written");
        self.git_success(&["add", "--", path]);
        self.git_success(&["commit", "-m", message]);
    }

    fn commit_file_as(
        &self,
        path: &str,
        contents: &str,
        message: &str,
        author_name: &str,
        author_email: &str,
    ) {
        fs::write(self.path.join(path), contents).expect("file should be written");
        self.git_success(&["add", "--", path]);
        let output = self
            .command_at(&self.path)
            .env("GIT_AUTHOR_NAME", author_name)
            .env("GIT_AUTHOR_EMAIL", author_email)
            .args(["commit", "-m", message])
            .output()
            .expect("git should be available on PATH");
        assert_success(&["commit", "-m", message], &output);
    }

    fn discover(&self) -> Repository {
        Repository::discover(&self.path).expect("repository should be discovered")
    }

    fn worktree_path(&self) -> PathBuf {
        self.directory.path().join("feature-worktree")
    }
}

fn assert_success(arguments: &[&str], output: &Output) {
    assert!(
        output.status.success(),
        "git {} failed with {}\nstdout:\n{}\nstderr:\n{}",
        arguments.join(" "),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn assert_gitbranch_success(arguments: &[&str], output: &Output) {
    assert!(
        output.status.success(),
        "gitbranch {} failed with {}\nstdout:\n{}\nstderr:\n{}",
        arguments.join(" "),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn branch(repository: &Repository, name: &str) -> LocalBranch {
    repository
        .local_branches()
        .expect("branches should be listed")
        .into_iter()
        .find(|branch| branch.name() == name)
        .unwrap_or_else(|| panic!("branch {name} should exist"))
}

#[test]
fn deletes_branch_created_by_git_cli() {
    let test_repository = TestRepository::new();
    test_repository.create_branch("feature");
    let repository = test_repository.discover();

    repository
        .delete_branch(&branch(&repository, "feature"))
        .expect("feature branch should be deleted");

    let output = test_repository.git(&["show-ref", "--verify", "--quiet", "refs/heads/feature"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        test_repository.git_stdout(&["branch", "--show-current"]),
        "main"
    );
}

#[test]
fn command_line_branch_deletes_without_opening_the_ui() {
    let test_repository = TestRepository::new();
    test_repository.create_branch("feature");

    let output = test_repository.gitbranch(&["delete", "feature"]);

    assert_gitbranch_success(&["delete", "feature"], &output);
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout should be valid UTF-8"),
        "Deleted branch feature.\n"
    );
    assert!(output.stderr.is_empty());
    let branch_output =
        test_repository.git(&["show-ref", "--verify", "--quiet", "refs/heads/feature"]);
    assert_eq!(branch_output.status.code(), Some(1));
}

#[test]
fn describes_clean_branches_created_by_git_cli() {
    let test_repository = TestRepository::new();
    test_repository.create_branch("merged");

    test_repository.create_branch("own-feature");
    test_repository.switch_to("own-feature");
    test_repository.commit_file("own.txt", "own\n", "Add own change");

    test_repository.switch_to("main");
    test_repository.create_branch("review");
    test_repository.switch_to("review");
    test_repository.commit_file_as(
        "review.txt",
        "review\n",
        "Add reviewed change",
        "Reviewer",
        "reviewer@example.com",
    );
    test_repository.switch_to("main");

    let branches = test_repository
        .discover()
        .clean_branches()
        .expect("clean branches should be described");
    let branch = |name: &str| {
        branches
            .iter()
            .find(|branch| branch.name() == name)
            .unwrap_or_else(|| panic!("branch {name} should exist"))
    };

    assert!(branch("main").is_trunk());
    assert!(branch("main").is_merged());
    assert!(!branch("merged").is_trunk());
    assert!(branch("merged").is_merged());
    assert!(!branch("own-feature").is_merged());
    assert!(!branch("own-feature").is_authored_by_other());
    assert!(!branch("review").is_merged());
    assert!(branch("review").is_authored_by_other());
}

#[test]
fn deletes_branch_with_duplicate_github_pr_metadata() {
    // Tests a problem where the branch has duplicate identical branch.<name>.github-pr-owner-number entries metadata,
    // associated with VS Code’s GitHub Pull Requests extension (related extension issue (https://github.com/microsoft/vscode-pull-request-github/issues/6134))
    const BRANCH: &str = "github-pr-branch";
    const CONFIG_KEY: &str = "branch.github-pr-branch.github-pr-owner-number";
    const CONFIG_VALUE: &str = "owner#repository#123";

    let test_repository = TestRepository::new();
    test_repository.create_branch(BRANCH);
    test_repository.git_success(&["config", "--add", CONFIG_KEY, CONFIG_VALUE]);
    test_repository.git_success(&["config", "--add", CONFIG_KEY, CONFIG_VALUE]);
    assert_eq!(
        test_repository.git_stdout(&["config", "--get-all", CONFIG_KEY]),
        format!("{CONFIG_VALUE}\n{CONFIG_VALUE}")
    );
    let repository = test_repository.discover();

    repository
        .delete_branch(&branch(&repository, BRANCH))
        .expect("branch with duplicate PR metadata should be deleted");

    let branch_output = test_repository.git(&[
        "show-ref",
        "--verify",
        "--quiet",
        "refs/heads/github-pr-branch",
    ]);
    assert_eq!(branch_output.status.code(), Some(1));
    let config_output = test_repository.git(&["config", "--get-all", CONFIG_KEY]);
    assert_eq!(config_output.status.code(), Some(1));
}

#[test]
fn switches_to_branch_created_by_git_cli() {
    let test_repository = TestRepository::new();
    test_repository.create_branch("feature");
    test_repository.switch_to("feature");
    test_repository.commit_file("feature.txt", "feature\n", "Add feature");
    let feature_tip = test_repository.git_stdout(&["rev-parse", "HEAD"]);
    test_repository.switch_to("main");
    let repository = test_repository.discover();
    let feature = branch(&repository, "feature");
    let repository = repository
        .into_head_operation()
        .expect("repository should allow HEAD operations");

    repository
        .switch_to(&feature)
        .expect("switch should succeed");

    assert_eq!(
        test_repository.git_stdout(&["branch", "--show-current"]),
        "feature"
    );
    assert_eq!(
        test_repository.git_stdout(&["rev-parse", "HEAD"]),
        feature_tip
    );
    assert_eq!(
        test_repository.git_stdout(&["show", "HEAD:feature.txt"]),
        "feature"
    );
    assert!(
        test_repository
            .git_stdout(&["status", "--porcelain"])
            .is_empty()
    );
    assert_eq!(
        fs::read_to_string(test_repository.path.join(".git/gitbranch-switches"))
            .expect("switch history should be readable"),
        "feature\t0\n"
    );
}

#[test]
fn command_line_branch_switches_without_opening_the_ui_and_records_history() {
    let test_repository = TestRepository::new();
    test_repository.create_branch("feature");

    let output = test_repository.gitbranch(&["switch", "feature"]);

    assert_gitbranch_success(&["switch", "feature"], &output);
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout should be valid UTF-8"),
        "Switched to branch feature.\n"
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        test_repository.git_stdout(&["branch", "--show-current"]),
        "feature"
    );
    assert_eq!(
        fs::read_to_string(test_repository.path.join(".git/gitbranch-switches"))
            .expect("switch history should be readable"),
        "feature\t0\n"
    );
}

#[test]
fn switches_with_compatible_unstaged_tracked_changes() {
    let test_repository = TestRepository::new();
    test_repository.commit_file("local.txt", "base\n", "Add local file");
    test_repository.create_branch("feature");
    test_repository.switch_to("feature");
    test_repository.commit_file("feature.txt", "feature\n", "Add feature file");
    test_repository.switch_to("main");
    fs::write(test_repository.path.join("local.txt"), "modified\n")
        .expect("tracked file should be modified");

    let repository = test_repository.discover();
    let feature = branch(&repository, "feature");
    let repository = repository
        .into_head_operation()
        .expect("tracked changes should allow switching");

    repository
        .switch_to(&feature)
        .expect("compatible tracked changes should be preserved");

    assert_eq!(
        test_repository.git_stdout(&["branch", "--show-current"]),
        "feature"
    );
    assert_eq!(
        fs::read_to_string(test_repository.path.join("local.txt"))
            .expect("tracked file should remain"),
        "modified\n"
    );
    assert_eq!(
        test_repository.git_stdout(&["status", "--porcelain"]),
        "M local.txt"
    );
}

#[test]
fn conflicting_tracked_changes_prevent_switch_without_data_loss() {
    let test_repository = TestRepository::new();
    test_repository.commit_file("shared.txt", "base\n", "Add shared file");
    test_repository.create_branch("feature");
    test_repository.switch_to("feature");
    test_repository.commit_file("shared.txt", "feature\n", "Update shared file");
    test_repository.switch_to("main");
    fs::write(test_repository.path.join("shared.txt"), "local\n")
        .expect("tracked file should be modified");

    let repository = test_repository.discover();
    let feature = branch(&repository, "feature");
    let repository = repository
        .into_head_operation()
        .expect("tracked changes should allow branch selection");

    let error = repository
        .switch_to(&feature)
        .expect_err("conflicting tracked changes should prevent switching");

    assert!(!error.to_string().is_empty());
    assert_eq!(
        test_repository.git_stdout(&["branch", "--show-current"]),
        "main"
    );
    assert_eq!(
        fs::read_to_string(test_repository.path.join("shared.txt"))
            .expect("tracked file should remain"),
        "local\n"
    );
}

#[test]
fn merges_diverged_branch_into_state_validated_by_git_cli() {
    let test_repository = TestRepository::new();
    test_repository.create_branch("feature");
    test_repository.switch_to("feature");
    test_repository.commit_file("feature.txt", "feature\n", "Add feature");
    let feature_tip = test_repository.git_stdout(&["rev-parse", "HEAD"]);
    test_repository.switch_to("main");
    test_repository.commit_file("main.txt", "main\n", "Add main change");
    let old_main_tip = test_repository.git_stdout(&["rev-parse", "HEAD"]);
    let repository = test_repository.discover();
    let feature = branch(&repository, "feature");
    let repository = repository
        .into_head_operation()
        .expect("repository should allow HEAD operations");

    let outcome = repository
        .merge_from(&feature)
        .expect("merge should succeed");

    assert_eq!(outcome, ConflictableCommandOutcome::Completed);
    assert_eq!(
        test_repository.git_stdout(&["branch", "--show-current"]),
        "main"
    );
    assert_eq!(
        test_repository.git_stdout(&["rev-parse", "main^1"]),
        old_main_tip
    );
    assert_eq!(
        test_repository.git_stdout(&["rev-parse", "main^2"]),
        feature_tip
    );
    assert_eq!(
        test_repository.git_stdout(&["show", "main:main.txt"]),
        "main"
    );
    assert_eq!(
        test_repository.git_stdout(&["show", "main:feature.txt"]),
        "feature"
    );
    assert!(
        test_repository
            .git_stdout(&["status", "--porcelain"])
            .is_empty()
    );
    assert_eq!(
        fs::read_to_string(test_repository.path.join(".git/gitbranch-merges"))
            .expect("merge history should be readable"),
        "main\tfeature\t0\n"
    );
}

#[test]
fn command_line_branch_merges_without_opening_the_ui_and_records_history() {
    let test_repository = TestRepository::new();
    test_repository.create_branch("feature");
    test_repository.switch_to("feature");
    test_repository.commit_file("feature.txt", "feature\n", "Add feature");
    test_repository.switch_to("main");
    test_repository.commit_file("main.txt", "main\n", "Add main change");

    let output = test_repository.gitbranch(&["merge", "feature"]);

    assert_gitbranch_success(&["merge", "feature"], &output);
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout should be valid UTF-8"),
        "Merged feature into main.\n"
    );
    assert!(output.stderr.is_empty());
    assert_success(
        &["merge-base", "--is-ancestor", "feature", "main"],
        &test_repository.git(&["merge-base", "--is-ancestor", "feature", "main"]),
    );
    assert_eq!(
        fs::read_to_string(test_repository.path.join(".git/gitbranch-merges"))
            .expect("merge history should be readable"),
        "main\tfeature\t0\n"
    );
}

#[test]
fn invalid_exact_command_line_branch_uses_existing_validation() {
    let test_repository = TestRepository::new();
    let original_tip = test_repository.git_stdout(&["rev-parse", "HEAD"]);

    let output = test_repository.gitbranch(&["merge", "main"]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).expect("stderr should be valid UTF-8"),
        "the current branch cannot be merged into itself\n"
    );
    assert_eq!(
        test_repository.git_stdout(&["rev-parse", "HEAD"]),
        original_tip
    );
    assert!(!test_repository.path.join(".git/gitbranch-merges").exists());
}

#[test]
fn merges_with_compatible_staged_tracked_changes() {
    let test_repository = TestRepository::new();
    test_repository.commit_file("local.txt", "base\n", "Add local file");
    test_repository.create_branch("feature");
    test_repository.switch_to("feature");
    test_repository.commit_file("feature.txt", "feature\n", "Add feature file");
    test_repository.switch_to("main");
    fs::write(test_repository.path.join("local.txt"), "modified\n")
        .expect("tracked file should be modified");
    test_repository.git_success(&["add", "--", "local.txt"]);

    let repository = test_repository.discover();
    let feature = branch(&repository, "feature");
    let repository = repository
        .into_head_operation()
        .expect("tracked changes should allow merging");

    let outcome = repository
        .merge_from(&feature)
        .expect("compatible tracked changes should be preserved");

    assert_eq!(outcome, ConflictableCommandOutcome::Completed);
    assert_eq!(
        fs::read_to_string(test_repository.path.join("local.txt"))
            .expect("tracked file should remain"),
        "modified\n"
    );
    assert_eq!(
        test_repository.git_stdout(&["show", "HEAD:feature.txt"]),
        "feature"
    );
    assert_eq!(
        test_repository.git_stdout(&["status", "--porcelain"]),
        "M  local.txt"
    );
}

#[test]
fn conflicting_tracked_changes_prevent_merge_without_data_loss() {
    let test_repository = TestRepository::new();
    test_repository.commit_file("shared.txt", "base\n", "Add shared file");
    test_repository.create_branch("feature");
    test_repository.switch_to("feature");
    test_repository.commit_file("shared.txt", "feature\n", "Update shared file");
    test_repository.switch_to("main");
    fs::write(test_repository.path.join("shared.txt"), "local\n")
        .expect("tracked file should be modified");

    let repository = test_repository.discover();
    let feature = branch(&repository, "feature");
    let repository = repository
        .into_head_operation()
        .expect("tracked changes should allow branch selection");

    let error = repository
        .merge_from(&feature)
        .expect_err("conflicting tracked changes should prevent merging");

    assert!(matches!(
        error,
        Error::CommandFailed {
            command,
            message,
            ..
        } if command == "merge" && !message.is_empty()
    ));
    assert_eq!(
        test_repository.git_stdout(&["branch", "--show-current"]),
        "main"
    );
    assert_eq!(
        fs::read_to_string(test_repository.path.join("shared.txt"))
            .expect("tracked file should remain"),
        "local\n"
    );
    assert!(
        !test_repository
            .git(&["rev-parse", "--verify", "MERGE_HEAD"])
            .status
            .success()
    );
}

#[test]
fn conflicted_merge_can_be_continued_by_git_cli() {
    let test_repository = TestRepository::new();
    test_repository.commit_file("shared.txt", "base\n", "Add shared file");
    test_repository.create_branch("feature");
    test_repository.switch_to("feature");
    test_repository.commit_file("shared.txt", "feature\n", "Update shared file on feature");
    test_repository.switch_to("main");
    test_repository.commit_file("shared.txt", "main\n", "Update shared file on main");
    let repository = test_repository.discover();
    let feature = branch(&repository, "feature");
    let repository = repository
        .into_head_operation()
        .expect("repository should allow HEAD operations");

    let outcome = repository
        .merge_from(&feature)
        .expect("conflict should be returned as an outcome");
    assert_eq!(outcome, ConflictableCommandOutcome::Conflicted);
    assert!(
        !test_repository
            .git_stdout(&["ls-files", "--unmerged", "--", "shared.txt"])
            .is_empty()
    );
    test_repository.git_success(&["rev-parse", "--verify", "MERGE_HEAD"]);

    fs::write(test_repository.path.join("shared.txt"), "resolved\n")
        .expect("conflict resolution should be written");
    test_repository.git_success(&["add", "--", "shared.txt"]);
    let output = test_repository
        .command_at(&test_repository.path)
        .env("GIT_EDITOR", "true")
        .args(["merge", "--continue"])
        .output()
        .expect("git should be available on PATH");
    assert_success(&["merge", "--continue"], &output);

    assert_eq!(
        test_repository.git_stdout(&["branch", "--show-current"]),
        "main"
    );
    assert_success(
        &["merge-base", "--is-ancestor", "feature", "main"],
        &test_repository.git(&["merge-base", "--is-ancestor", "feature", "main"]),
    );
    assert_eq!(
        test_repository.git_stdout(&["show", "main:shared.txt"]),
        "resolved"
    );
    assert!(
        test_repository
            .git_stdout(&["status", "--porcelain"])
            .is_empty()
    );
}

#[test]
fn rebases_diverged_branch_into_state_validated_by_git_cli() {
    let test_repository = TestRepository::new();
    test_repository.create_branch("feature");
    test_repository.switch_to("feature");
    test_repository.commit_file("feature.txt", "feature\n", "Add feature");
    let feature_tip = test_repository.git_stdout(&["rev-parse", "HEAD"]);
    test_repository.switch_to("main");
    test_repository.commit_file("main.txt", "main\n", "Add main change");
    let old_main_tip = test_repository.git_stdout(&["rev-parse", "HEAD"]);
    let repository = test_repository.discover();
    let feature = branch(&repository, "feature");
    let repository = repository
        .into_head_operation()
        .expect("repository should allow HEAD operations")
        .into_clean_rebase()
        .expect("repository should allow rebase");

    let outcome = repository
        .rebase_onto(&feature)
        .expect("rebase should succeed");

    assert_eq!(outcome, ConflictableCommandOutcome::Completed);
    assert_eq!(
        test_repository.git_stdout(&["branch", "--show-current"]),
        "main"
    );
    assert_success(
        &["merge-base", "--is-ancestor", "feature", "main"],
        &test_repository.git(&["merge-base", "--is-ancestor", "feature", "main"]),
    );
    assert_eq!(
        test_repository.git_stdout(&["rev-parse", "main^"]),
        feature_tip
    );
    assert_ne!(
        test_repository.git_stdout(&["rev-parse", "main"]),
        old_main_tip
    );
    assert_eq!(
        test_repository.git_stdout(&["rev-list", "--count", "feature..main"]),
        "1"
    );
    assert_eq!(
        test_repository.git_stdout(&["show", "main:main.txt"]),
        "main"
    );
    assert_eq!(
        test_repository.git_stdout(&["show", "main:feature.txt"]),
        "feature"
    );
    assert!(
        test_repository
            .git_stdout(&["status", "--porcelain"])
            .is_empty()
    );
}

#[test]
fn command_line_branch_rebases_without_opening_the_ui_and_records_history() {
    let test_repository = TestRepository::new();
    test_repository.create_branch("feature");
    test_repository.switch_to("feature");
    test_repository.commit_file("feature.txt", "feature\n", "Add feature");
    test_repository.switch_to("main");
    test_repository.commit_file("main.txt", "main\n", "Add main change");

    let output = test_repository.gitbranch(&["rebase", "feature"]);

    assert_gitbranch_success(&["rebase", "feature"], &output);
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout should be valid UTF-8"),
        "Rebased main onto feature.\n"
    );
    assert!(output.stderr.is_empty());
    assert_success(
        &["merge-base", "--is-ancestor", "feature", "main"],
        &test_repository.git(&["merge-base", "--is-ancestor", "feature", "main"]),
    );
    assert_eq!(
        fs::read_to_string(test_repository.path.join(".git/gitbranch-rebases"))
            .expect("rebase history should be readable"),
        "main\tfeature\n"
    );
}

#[test]
fn conflicted_rebase_can_be_continued_by_git_cli() {
    // set up the conflict
    let test_repository = TestRepository::new();
    test_repository.commit_file("shared.txt", "base\n", "Add shared file");
    test_repository.create_branch("feature");
    test_repository.switch_to("feature");
    test_repository.commit_file("shared.txt", "feature\n", "Update shared file on feature");
    test_repository.switch_to("main");
    test_repository.commit_file("shared.txt", "main\n", "Update shared file on main");
    test_repository.commit_file("after.txt", "after\n", "Add change after conflict");
    let repository = test_repository.discover();
    let feature = branch(&repository, "feature");
    let repository = repository
        .into_head_operation()
        .expect("repository should allow HEAD operations")
        .into_clean_rebase()
        .expect("repository should allow rebase");

    // try to rebase
    let outcome = repository
        .rebase_onto(&feature)
        .expect("conflict should be returned as an outcome");
    assert_eq!(outcome, ConflictableCommandOutcome::Conflicted);
    assert!(
        !test_repository
            .git_stdout(&["ls-files", "--unmerged", "--", "shared.txt"])
            .is_empty()
    );

    // fix the conflict and continue rebasing
    fs::write(test_repository.path.join("shared.txt"), "resolved\n")
        .expect("conflict resolution should be written");
    test_repository.git_success(&["add", "--", "shared.txt"]);
    let output = test_repository
        .command_at(&test_repository.path)
        .env("GIT_EDITOR", "true")
        .args(["rebase", "--continue"])
        .output()
        .expect("git should be available on PATH");
    assert_success(&["rebase", "--continue"], &output);

    // check the rebase worked
    assert_eq!(
        test_repository.git_stdout(&["branch", "--show-current"]),
        "main"
    );
    assert_success(
        &["merge-base", "--is-ancestor", "feature", "main"],
        &test_repository.git(&["merge-base", "--is-ancestor", "feature", "main"]),
    );
    assert_eq!(
        test_repository.git_stdout(&["show", "main:shared.txt"]),
        "resolved"
    );
    assert_eq!(
        test_repository.git_stdout(&["show", "main:after.txt"]),
        "after"
    );
    assert!(
        test_repository
            .git_stdout(&["status", "--porcelain"])
            .is_empty()
    );
}

#[test]
fn protects_branch_checked_out_in_git_cli_worktree() {
    let test_repository = TestRepository::new();
    test_repository.create_branch("feature");
    let worktree_path = test_repository.worktree_path();
    test_repository.git_success(&[
        "worktree",
        "add",
        worktree_path
            .to_str()
            .expect("worktree path should be valid UTF-8"),
        "feature",
    ]);
    let repository = test_repository.discover();
    let feature = branch(&repository, "feature");

    assert_eq!(feature.checkout(), Checkout::OtherWorktree);
    assert!(
        matches!(repository.delete_branch(&feature), Err(Error::BranchCheckedOut(name)) if name == "feature")
    );
    let repository = repository
        .into_head_operation()
        .expect("repository should allow HEAD operations");
    assert!(
        matches!(repository.switch_to(&feature), Err(Error::BranchCheckedOut(name)) if name == "feature")
    );

    test_repository.git_success(&["show-ref", "--verify", "--quiet", "refs/heads/feature"]);
    let worktrees = test_repository.git_stdout(&["worktree", "list", "--porcelain"]);
    let canonical_worktree_path = fs::canonicalize(&worktree_path)
        .expect("worktree path should have a canonical representation");
    assert!(worktrees.contains(&format!("worktree {}", canonical_worktree_path.display())));
    assert!(worktrees.contains("branch refs/heads/feature"));
    assert_eq!(
        test_repository.git_stdout(&["branch", "--show-current"]),
        "main"
    );
    assert_eq!(
        test_repository.git_stdout_at(&worktree_path, &["branch", "--show-current"]),
        "feature"
    );
}

#[test]
fn prunes_deleted_git_cli_worktree() {
    let test_repository = TestRepository::new();
    test_repository.create_branch("feature");
    let worktree_path = test_repository.worktree_path();
    test_repository.git_success(&[
        "worktree",
        "add",
        worktree_path
            .to_str()
            .expect("worktree path should be valid UTF-8"),
        "feature",
    ]);
    let canonical_worktree_path = fs::canonicalize(&worktree_path).unwrap();
    fs::remove_dir_all(&worktree_path).expect("worktree should be deleted");
    let repository = test_repository.discover();

    let feature = branch(&repository, "feature");

    assert_eq!(feature.checkout(), Checkout::Available);
    repository
        .delete_branch(&feature)
        .expect("feature branch should be deleted");
    let branch_output =
        test_repository.git(&["show-ref", "--verify", "--quiet", "refs/heads/feature"]);
    assert_eq!(branch_output.status.code(), Some(1));
    let worktrees = test_repository.git_stdout(&["worktree", "list", "--porcelain"]);
    assert!(!worktrees.contains(&format!("worktree {}", canonical_worktree_path.display())));
}
