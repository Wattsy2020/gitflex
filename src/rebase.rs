use crate::{
    Error,
    git::{self, CleanRebaseRepository, ConflictableCommandOutcome},
    history::HistoryStore,
    local_branch_names,
    ui::{
        self,
        Selection::{Cancelled, Selected, Unavailable},
    },
};

pub fn run(repository: &CleanRebaseRepository, branch: Option<&str>) -> Result<(), Error> {
    let current_branch = repository
        .current_branch()?
        .ok_or(git::Error::DetachedHead)?;

    let history_store = HistoryStore::new(repository.common_directory());
    // ignore errors if we can't find last_rebase_target in the history database
    // the user doesn't know or care that we cache things and failed to read it
    // they just want to complete their operation
    let last_target = history_store
        .last_rebase_target(&current_branch)
        .unwrap_or_default();

    let branches = repository.local_branches()?;
    let branch_names = local_branch_names(&branches);

    match ui::run_rebase_app(branches, last_target, branch, || {
        history_store.prune_rebase_in_background(branch_names)
    })? {
        Unavailable => println!("No branches available to rebase onto."),
        Cancelled => println!("Cancelled."),
        Selected(branch) => {
            let outcome = repository.rebase_onto(&branch)?;
            let _ = history_store.record_rebase(&current_branch, branch.name());
            match outcome {
                ConflictableCommandOutcome::Completed => {
                    println!("Rebased {current_branch} onto {}.", branch.name());
                }
                ConflictableCommandOutcome::Conflicted => {
                    return Err(git::Error::RebaseConflicts.into());
                }
            }
        }
    }

    Ok(())
}
