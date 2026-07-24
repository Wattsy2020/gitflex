use crate::{
    Error,
    git::HeadOperationRepository,
    ui::{
        self,
        Selection::{Cancelled, Selected, Unavailable},
    },
};

pub fn run(repository: &HeadOperationRepository) -> Result<(), Error> {
    let branches = repository.local_branches()?;

    match ui::run_switch_app(branches)? {
        Unavailable => println!("No branches available to switch to."),
        Cancelled => println!("Cancelled."),
        Selected(branch) => {
            repository.switch_to(&branch)?;
            println!("Switched to branch {}.", branch.name());
        }
    }

    Ok(())
}
