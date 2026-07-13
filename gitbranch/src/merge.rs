use crate::{
    Error,
    git::{self, ConflictableCommandOutcome, Repository},
    ui::{self, App, SingleOperation},
};

pub fn run(repository: &Repository) -> Result<(), Error> {
    let current_branch = repository
        .current_branch()?
        .ok_or(git::Error::DetachedHeadForMerge)?;

    let branches = repository.local_branches()?;
    let Some(app) = App::single(branches, SingleOperation::Merge) else {
        println!("No branches available to merge.");
        return Ok(());
    };

    match ui::select_one(app)? {
        None => println!("Cancelled."),
        Some(branch) => match repository.merge_from(&branch)? {
            ConflictableCommandOutcome::Completed => {
                println!("Merged {} into {current_branch}.", branch.name());
            }
            ConflictableCommandOutcome::Conflicted => {
                return Err(git::Error::MergeConflicts.into());
            }
        },
    }

    Ok(())
}
