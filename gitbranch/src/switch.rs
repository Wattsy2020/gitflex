use crate::{
    Error,
    git::HeadOperationRepository,
    history::HistoryStore,
    local_branch_names,
    ui::{
        self,
        Selection::{Cancelled, Selected, Unavailable},
    },
};

pub fn run(repository: &HeadOperationRepository, branch: Option<&str>) -> Result<(), Error> {
    let history_store = HistoryStore::new(repository.common_directory());
    // History is only a ranking cache; failing to read it must not block switching.
    let history = history_store.read_switch().unwrap_or_default();

    let branches = repository.local_branches()?;
    let branch_names = local_branch_names(&branches);

    match ui::run_switch_app(branches, history, branch, || {
        history_store.prune_switch_in_background(branch_names);
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
