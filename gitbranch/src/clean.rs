use crate::{
    Error,
    git::Repository,
    ui::{
        self,
        Selection::{Cancelled, Selected, Unavailable},
    },
};

pub fn run(repository: &Repository) -> Result<(), Error> {
    let branches = repository.local_branches()?;
    match ui::run_clean_app(branches)? {
        Unavailable => println!("No deletable branches found."),
        Cancelled => println!("Cancelled."),
        Selected(branches) => {
            for branch in branches {
                match repository.delete_branch(&branch) {
                    Ok(()) => println!("Deleted branch {}.", branch.name()),
                    Err(error) => println!("Failed to delete branch {}: {error}", branch.name()),
                }
            }
        }
    }

    Ok(())
}
