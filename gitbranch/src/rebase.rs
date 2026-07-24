use crate::{
    Error,
    git::{self, ConflictableCommandOutcome, HeadOperationRepository},
    ui::{self, AppImpl},
};

pub fn run(repository: &HeadOperationRepository) -> Result<(), Error> {
    let current_branch = repository
        .current_branch()?
        .ok_or(git::Error::DetachedHead)?;

    // ignore errors if we can't find last_rebase_target from file
    // the user doesn't know or care that we cache things in a file and failed to read from it
    // they just want to complete their operation
    let last_target = repository.last_rebase_target().ok().flatten();

    let branches = repository.local_branches()?;
    let Some(app) = AppImpl::rebase(branches, last_target) else {
        println!("No branches available to rebase onto.");
        return Ok(());
    };

    match ui::select_one(app)? {
        None => println!("Cancelled."),
        Some(branch) => match repository.rebase_onto(&branch)? {
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
