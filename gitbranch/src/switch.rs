use crate::{
    Error,
    git::HeadOperationRepository,
    history::HistoryStore,
    ui::{
        self,
        Selection::{Cancelled, Selected, Unavailable},
    },
};

pub fn run(repository: &HeadOperationRepository, branch: Option<&str>) -> Result<(), Error> {
    let branches = repository.local_branches()?;
    let history_store = HistoryStore::new(repository.common_directory());
    // History is only a ranking cache; failing to read it must not block switching.
    let history = history_store.read_switch().unwrap_or_default();

    match ui::run_switch_app(branches, history, branch, |existing_branches| {
        history_store.prune_switch_in_background(existing_branches);
    })? {
        Unavailable => println!("No branches available to switch to."),
        Cancelled => println!("Cancelled."),
        Selected(branch) => {
            repository.switch_to(&branch)?;
            let _ = history_store.record_switch(branch.name());
            println!("Switched to branch {}.", branch.name());
        }
    }

    Ok(())
}
