use crate::{
    Error,
    git::{self, ConflictableCommandOutcome, HeadOperationRepository},
    ui::{
        self,
        Selection::{Cancelled, Selected, Unavailable},
    },
};

pub fn run(repository: &HeadOperationRepository) -> Result<(), Error> {
    let current_branch = repository
        .current_branch()?
        .ok_or(git::Error::DetachedHeadForMerge)?;

    let branches = repository.local_branches()?;

    match ui::run_merge_app(branches)? {
        Unavailable => println!("No branches available to merge."),
        Cancelled => println!("Cancelled."),
        Selected(branch) => match repository.merge_from(&branch)? {
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
