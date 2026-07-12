use crate::{
    Error, git,
    git::{RebaseOutcome, Repository},
    ui,
};

pub fn run(repository: &Repository) -> Result<(), Error> {
    let current_branch = repository
        .current_branch()?
        .ok_or(git::Error::DetachedHead)?;
    let branches = repository.local_branches()?;

    if !branches.iter().any(|branch| branch.is_rebase_target()) {
        println!("No branches available to rebase onto.");
        return Ok(());
    }

    match ui::select_one(branches, ui::SingleOperation::Rebase)? {
        None => println!("Cancelled."),
        Some(branch) => match repository.rebase_onto(&branch)? {
            RebaseOutcome::Completed => {
                println!("Rebased {current_branch} onto {}.", branch.name());
            }
            RebaseOutcome::Conflicted => {
                return Err(git::Error::RebaseConflicts.into());
            }
        },
    }

    Ok(())
}
