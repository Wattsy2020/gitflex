use crate::{
    Error,
    git::{self, ConflictableCommandOutcome, HeadOperationRepository},
    ui::{self, App},
};

pub fn run(repository: &HeadOperationRepository) -> Result<(), Error> {
    let current_branch = repository
        .current_branch()?
        .ok_or(git::Error::DetachedHeadForMerge)?;

    let branches = repository.local_branches()?;
    let Some(app) = App::merge(branches) else {
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
