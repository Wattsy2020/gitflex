use crate::{
    Error,
    git::{self, ConflictableCommandOutcome, HeadOperationRepository},
    ui::{self, App},
};

pub fn run(repository: &HeadOperationRepository) -> Result<(), Error> {
    let current_branch = repository
        .current_branch()?
        .ok_or(git::Error::DetachedHead)?;

    let last_target = repository.last_rebase_target()?;
    let branches = repository.local_branches()?;
    let Some(app) = App::rebase(branches, last_target) else {
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
