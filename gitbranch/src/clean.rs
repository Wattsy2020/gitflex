use crate::{
    Error,
    git::Repository,
    ui::{self, App, Mode},
};

pub fn run(repository: &Repository) -> Result<(), Error> {
    let branches = repository.local_branches()?;
    let Some(app) = App::new(branches, Mode::Clean) else {
        println!("No deletable branches found.");
        return Ok(());
    };

    match ui::select_many(app)? {
        None => println!("Cancelled."),
        Some(branches) if branches.is_empty() => println!("No branches selected."),
        Some(branches) => {
            branches
                .iter()
                .for_each(|branch| match repository.delete_branch(branch) {
                    Ok(()) => println!("Deleted branch {}.", branch.name()),
                    Err(error) => println!("Failed to delete branch {}: {error}", branch.name()),
                })
        }
    }

    Ok(())
}
