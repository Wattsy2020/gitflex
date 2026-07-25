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
    // History is only a ranking cache; failing to read it must not block switching.
    let history = repository.switch_history().unwrap_or_default();

    match ui::run_switch_app(branches, history)? {
        Unavailable => println!("No branches available to switch to."),
        Cancelled => println!("Cancelled."),
        Selected(branch) => {
            repository.switch_to(&branch)?;
            println!("Switched to branch {}.", branch.name());
        }
    }

    Ok(())
}
