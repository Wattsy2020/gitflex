//! Recognises commits that were rewritten before landing on a base branch,
//! in the manner of `git cherry`: a commit counts as applied when a commit on the
//! base has an equivalent stable patch id, even though rebasing gave it a new id.

use std::collections::{HashMap, HashSet};

use git2::{Commit, ErrorCode, Oid, Repository, Revwalk};

pub(super) struct RewrittenCommitChecker<'repo> {
    repository: &'repo Repository,
    patches: HashMap<Oid, CommitPatch>,
}

impl<'repo> RewrittenCommitChecker<'repo> {
    pub(super) fn new(repository: &'repo Repository) -> Self {
        Self {
            repository,
            patches: HashMap::new(),
        }
    }

    /// Whether every non-merge commit added by `branch_tip` has a patch-equivalent
    /// commit on `base_tip` after their merge base.
    ///
    /// Patch equivalence follows stable `git patch-id` semantics, which ignore
    /// whitespace and line numbers
    /// Returns false if there are branch-side merge commits, as its resolution has no unambiguous parent diff.
    pub(super) fn is_rewritten_onto(
        &mut self,
        branch_tip: Oid,
        base_tip: Oid,
    ) -> Result<bool, git2::Error> {
        let merge_base = match self.repository.merge_base(base_tip, branch_tip) {
            Ok(merge_base) => merge_base,
            Err(error) if error.code() == ErrorCode::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };

        let branch_commits = commits_between(self.repository, merge_base, branch_tip)?
            .map(|id| self.repository.find_commit(id?))
            .collect::<Result<Vec<_>, _>>()?;
        let authorships: HashSet<_> = branch_commits.iter().map(Authorship::of).collect();
        let mut missing = HashSet::with_capacity(branch_commits.len());
        for commit in &branch_commits {
            match self.commit_patch(commit)? {
                CommitPatch::Linear(patch_id) => missing.insert(patch_id),
                CommitPatch::Merge => return Ok(false),
            };
        }

        for id in commits_between(self.repository, merge_base, base_tip)? {
            if missing.is_empty() {
                break;
            }
            let commit = self.repository.find_commit(id?)?;

            // Rebasing a commit keeps its author and authored time
            // So to save cost we only diff base commits with the same authorship info
            // otherwise this check can be expensive on a base that has moved far ahead of the branch
            if !authorships.contains(&Authorship::of(&commit)) {
                continue;
            }
            if let CommitPatch::Linear(patch_id) = self.commit_patch(&commit)? {
                missing.remove(&patch_id);
            }
        }

        Ok(missing.is_empty())
    }

    fn commit_patch(&mut self, commit: &Commit<'_>) -> Result<CommitPatch, git2::Error> {
        if let Some(patch) = self.patches.get(&commit.id()) {
            return Ok(*patch);
        }

        let patch = commit_patch(self.repository, commit)?;
        self.patches.insert(commit.id(), patch);
        Ok(patch)
    }
}

#[derive(Clone, Copy)]
enum CommitPatch {
    Linear(Oid),
    Merge,
}

/// The part of a commit's identity that rewriting it preserves
#[derive(Eq, Hash, PartialEq)]
struct Authorship {
    email: Box<[u8]>,
    seconds: i64,
}

impl Authorship {
    fn of(commit: &Commit<'_>) -> Self {
        let author = commit.author();
        Self {
            email: author.email_bytes().into(),
            seconds: author.when().seconds(),
        }
    }
}

/// The commits reachable from `tip` but not from `merge_base`
fn commits_between<'repo>(
    repository: &'repo Repository,
    merge_base: Oid,
    tip: Oid,
) -> Result<Revwalk<'repo>, git2::Error> {
    let mut revwalk = repository.revwalk()?;
    revwalk.push(tip)?;
    revwalk.hide(merge_base)?;
    Ok(revwalk)
}

/// Returns the patch id of a commit
fn commit_patch(repository: &Repository, commit: &Commit<'_>) -> Result<CommitPatch, git2::Error> {
    let parent_tree = match commit.parent_count() {
        0 => None,
        1 => Some(commit.parent(0)?.tree()?),
        _ => return Ok(CommitPatch::Merge),
    };
    let diff = repository.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit.tree()?), None)?;
    diff.patchid(None).map(CommitPatch::Linear)
}
