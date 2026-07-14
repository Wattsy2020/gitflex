use crate::{
    Error,
    git::HeadOperationRepository,
    ui::{self, App},
};

pub fn run(repository: &HeadOperationRepository) -> Result<(), Error> {
    let branches = repository.local_branches()?;
    let Some(app) = App::switch(branches) else {
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
