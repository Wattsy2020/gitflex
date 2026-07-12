use crate::{Error, git::Repository, ui};

pub fn run(repository: &Repository) -> Result<(), Error> {
    let branches = repository.local_branches()?;

    if !branches.iter().any(|branch| branch.is_switchable()) {
        println!("No branches available to switch to.");
        return Ok(());
    }

    match ui::select_one(branches, ui::SingleOperation::Switch)? {
        None => println!("Cancelled."),
        Some(branch) => {
            repository.switch_to(&branch)?;
            println!("Switched to branch {}.", branch.name());
        }
    }

    Ok(())
}
