//! Recognises commits that were rewritten before landing on a base branch,
//! in the manner of `git cherry`: a commit counts as applied when a commit on the
//! base carries an identical patch, even though rebasing gave it a new id.

use std::collections::HashSet;

use git2::{Commit, ErrorCode, Oid, Repository, Revwalk};

/// Whether every commit `tip_branch` adds on top of its merge base with `base_branch`
/// has a commit on `base_branch` with an identical patch.
pub(super) fn is_rewritten_onto(
    repository: &Repository,
    tip_branch: Oid,
    base_branch: Oid,
) -> Result<bool, git2::Error> {
    let merge_base = match repository.merge_base(base_branch, tip_branch) {
        Ok(merge_base) => merge_base,
        Err(error) if error.code() == ErrorCode::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };

    let branch_commits = commits_between(repository, merge_base, tip_branch)?
        .map(|id| repository.find_commit(id?))
        .collect::<Result<Vec<_>, _>>()?;
    let authorships: HashSet<_> = branch_commits.iter().map(Authorship::of).collect();
    let mut missing = branch_commits
        .iter()
        .map(|commit| patch_id(repository, commit))
        // filter out merge commits
        .filter_map(Result::transpose)
        .collect::<Result<HashSet<_>, _>>()?;

    for id in commits_between(repository, merge_base, base_branch)? {
        if missing.is_empty() {
            break;
        }
        let commit = repository.find_commit(id?)?;

        // Rewriting a commit keeps its author and authored time
        // So to save cost we only diff base commits with the same authorship info
        // otherwise this check can be expensive on a base that has moved far ahead of the branch
        if !authorships.contains(&Authorship::of(&commit)) {
            continue;
        }
        if let Some(patch_id) = patch_id(repository, &commit)? {
            missing.remove(&patch_id);
        }
    }

    Ok(missing.is_empty())
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

/// The patch id of a commit, or `None` for merge commits
fn patch_id(repository: &Repository, commit: &Commit<'_>) -> Result<Option<Oid>, git2::Error> {
    let parent_tree = match commit.parent_count() {
        0 => None,
        1 => Some(commit.parent(0)?.tree()?),
        _ => return Ok(None), // merge commit has multiple parents
    };
    let diff = repository.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit.tree()?), None)?;
    diff.patchid(None).map(Some)
}
