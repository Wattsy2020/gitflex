use std::io;

use crate::{
    git::{CleanBranch, LocalBranch},
    history::{MergeHistory, SwitchHistory},
    ui::{
        Selection::{Cancelled, Selected, Unavailable},
        app::{App, AppImpl},
    },
};

mod app;
mod search;
mod tui;

pub enum Selection<T> {
    Unavailable,
    Cancelled,
    Selected(T),
}

fn run_app<A: App>(app: Option<A>, on_ui_start: impl FnOnce()) -> io::Result<Selection<A::Output>> {
    match app {
        None => Ok(Unavailable),
        Some(app) => {
            on_ui_start();
            match tui::run(app)? {
                None => Ok(Cancelled),
                Some(result) => Ok(Selected(result)),
            }
        }
    }
}

fn run_single_app_with_ui_start<A: App<Output = LocalBranch>>(
    mut branches: Vec<LocalBranch>,
    branch: Option<&str>,
    make_app: impl FnOnce(Vec<LocalBranch>, Option<String>) -> Option<A>,
    on_ui_start: impl FnOnce(),
) -> io::Result<Selection<LocalBranch>> {
    let exact_match = branch.and_then(|name| {
        branches
            .iter()
            .position(|branch| branch.name() == name)
            .map(|position| branches.remove(position))
    });

    match exact_match {
        Some(branch) => Ok(Selected(branch)),
        None => run_app(make_app(branches, branch.map(str::to_owned)), on_ui_start),
    }
}

fn run_single_app<A: App<Output = LocalBranch>>(
    branches: Vec<LocalBranch>,
    branch: Option<&str>,
    make_app: impl FnOnce(Vec<LocalBranch>, Option<String>) -> Option<A>,
) -> io::Result<Selection<LocalBranch>> {
    run_single_app_with_ui_start(branches, branch, make_app, || {})
}

pub fn run_clean_app(branches: Vec<CleanBranch>) -> io::Result<Selection<Vec<LocalBranch>>> {
    run_app(AppImpl::clean(branches), || {})
}

pub fn run_delete_app(
    branches: Vec<LocalBranch>,
    branch: Option<&str>,
) -> io::Result<Selection<LocalBranch>> {
    run_single_app(branches, branch, AppImpl::delete)
}

pub fn run_merge_app(
    branches: Vec<LocalBranch>,
    destination: &str,
    history: MergeHistory,
    branch: Option<&str>,
    on_ui_start: impl FnOnce(),
) -> io::Result<Selection<LocalBranch>> {
    run_single_app_with_ui_start(
        branches,
        branch,
        |branches, initial_search| AppImpl::merge(branches, destination, &history, initial_search),
        on_ui_start,
    )
}

pub fn run_rebase_app(
    branches: Vec<LocalBranch>,
    last_target: Option<String>,
    branch: Option<&str>,
    on_ui_start: impl FnOnce(),
) -> io::Result<Selection<LocalBranch>> {
    run_single_app_with_ui_start(
        branches,
        branch,
        |branches, initial_search| AppImpl::rebase(branches, last_target, initial_search),
        on_ui_start,
    )
}

pub fn run_switch_app(
    branches: Vec<LocalBranch>,
    history: SwitchHistory,
    branch: Option<&str>,
    on_ui_start: impl FnOnce(),
) -> io::Result<Selection<LocalBranch>> {
    run_single_app_with_ui_start(
        branches,
        branch,
        |branches, initial_search| AppImpl::switch(branches, &history, initial_search),
        on_ui_start,
    )
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{Selection, run_switch_app};
    use crate::{
        git::{Checkout, LocalBranch},
        history::SwitchHistory,
    };

    fn branch(name: &str, checkout: Checkout) -> LocalBranch {
        LocalBranch::for_test(name, checkout)
    }

    #[test]
    fn exact_matches_and_unavailable_uis_skip_the_start_callback() {
        let exact_started = Cell::new(false);
        let selection = run_switch_app(
            vec![branch("feature", Checkout::Available)],
            SwitchHistory::default(),
            Some("feature"),
            || exact_started.set(true),
        )
        .expect("an exact branch should be selected");
        assert!(!exact_started.get());
        assert!(matches!(selection, Selection::Selected(branch) if branch.name() == "feature"));

        let unavailable_started = Cell::new(false);
        let selection = run_switch_app(
            vec![branch("main", Checkout::CurrentWorktree)],
            SwitchHistory::default(),
            None,
            || unavailable_started.set(true),
        )
        .expect("an unavailable UI should return cleanly");
        assert!(!unavailable_started.get());
        assert!(matches!(selection, Selection::Unavailable));
    }
}
