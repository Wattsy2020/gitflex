use crate::{
    Error,
    git::HeadOperationRepository,
    ui::{self, App, SingleOperation},
};

pub fn run(repository: &HeadOperationRepository) -> Result<(), Error> {
    let branches = repository.local_branches()?;
    let Some(app) = App::single(branches, SingleOperation::Switch) else {
        println!("No branches available to switch to.");
        return Ok(());
    };

    match ui::select_one(app)? {
        None => println!("Cancelled."),
        Some(branch) => {
            repository.switch_to(&branch)?;
            println!("Switched to branch {}.", branch.name());
        }
    }

    Ok(())
}
