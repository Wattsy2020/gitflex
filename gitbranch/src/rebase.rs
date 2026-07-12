use crate::{
    Error,
    git::{self, RebaseOutcome, Repository},
    ui::{self, App, Mode, SingleOperation},
};

pub fn run(repository: &Repository) -> Result<(), Error> {
    let current_branch = repository
        .current_branch()?
        .ok_or(git::Error::DetachedHead)?;

    let branches = repository.local_branches()?;
    let Some(app) = App::new(branches, Mode::Single(SingleOperation::Rebase)) else {
        println!("No branches available to rebase onto.");
        return Ok(());
    };

    match ui::select_one(app)? {
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
