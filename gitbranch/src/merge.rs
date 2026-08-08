use crate::{
    Error,
    git::{self, ConflictableCommandOutcome, HeadOperationRepository},
    history::HistoryStore,
    ui::{
        self,
        Selection::{Cancelled, Selected, Unavailable},
    },
};

pub fn run(repository: &HeadOperationRepository, branch: Option<&str>) -> Result<(), Error> {
    let current_branch = repository
        .current_branch()?
        .ok_or(git::Error::DetachedHeadForMerge)?;

    let history_store = HistoryStore::new(repository.common_directory());
    // History is only a ranking cache; failing to read it must not block merging.
    let history = history_store.read_merge().unwrap_or_default();
    let branches = repository.local_branches()?;

    match ui::run_merge_app(
        branches,
        &current_branch,
        history,
        branch,
        |existing_branches| history_store.prune_merge_in_background(existing_branches),
    )? {
        Unavailable => println!("No branches available to merge."),
        Cancelled => println!("Cancelled."),
        Selected(branch) => {
            let outcome = repository.merge_from(&branch)?;
            let _ = history_store.record_merge(&current_branch, branch.name());
            match outcome {
                ConflictableCommandOutcome::Completed => {
                    println!("Merged {} into {current_branch}.", branch.name());
                }
                ConflictableCommandOutcome::Conflicted => {
                    return Err(git::Error::MergeConflicts.into());
                }
            }
        }
    }

    Ok(())
}
