use crate::{
    Error,
    git::Repository,
    ui::{
        self,
        Selection::{Cancelled, Selected, Unavailable},
    },
};

pub fn run(repository: &Repository, branch: Option<&str>) -> Result<(), Error> {
    let branches = repository.local_branches()?;

    match ui::run_delete_app(branches, branch)? {
        Unavailable => println!("No branches available to delete."),
        Cancelled => println!("Cancelled."),
        Selected(branch) => {
            repository.delete_branch(&branch)?;
            println!("Deleted branch {}.", branch.name());
        }
    }

    Ok(())
}
