use crate::{
    Error,
    git::{self, CleanRebaseRepository, ConflictableCommandOutcome},
    ui::{
        self,
        Selection::{Cancelled, Selected, Unavailable},
    },
};

pub fn run(repository: &CleanRebaseRepository, branch: Option<&str>) -> Result<(), Error> {
    let current_branch = repository
        .current_branch()?
        .ok_or(git::Error::DetachedHead)?;

    // ignore errors if we can't find last_rebase_target in the history database
    // the user doesn't know or care that we cache things and failed to read it
    // they just want to complete their operation
    let last_target = repository.last_rebase_target().unwrap_or_default();
    let branches = repository.local_branches()?;

    match ui::run_rebase_app(branches, last_target, branch)? {
        Unavailable => println!("No branches available to rebase onto."),
        Cancelled => println!("Cancelled."),
        Selected(branch) => match repository.rebase_onto(&branch)? {
            ConflictableCommandOutcome::Completed => {
                println!("Rebased {current_branch} onto {}.", branch.name());
            }
            ConflictableCommandOutcome::Conflicted => {
                return Err(git::Error::RebaseConflicts.into());
            }
        },
    }

    Ok(())
}
