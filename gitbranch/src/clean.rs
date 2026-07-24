use crate::{
    Error,
    git::Repository,
    ui::{self, AppImpl},
};

pub fn run(repository: &Repository) -> Result<(), Error> {
    let branches = repository.local_branches()?;
    let Some(app) = AppImpl::clean(branches) else {
        println!("No deletable branches found.");
        return Ok(());
    };

    match ui::select_many(app)? {
        None => println!("Cancelled."),
        Some(branches) => {
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
