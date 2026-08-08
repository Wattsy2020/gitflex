use std::io;

use crate::{
    git::{CleanBranch, LocalBranch, MergeHistory, SwitchHistory},
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

fn run_app_with<A: App>(
    app: Option<A>,
    on_ui_start: impl FnOnce(),
    run: impl FnOnce(A) -> io::Result<Option<A::Output>>,
) -> io::Result<Selection<A::Output>> {
    match app {
        None => Ok(Unavailable),
        Some(app) => {
            on_ui_start();
            match run(app)? {
                None => Ok(Cancelled),
                Some(result) => Ok(Selected(result)),
            }
        }
    }
}

fn run_app<A: App>(app: Option<A>) -> io::Result<Selection<A::Output>> {
    run_app_with(app, || {}, tui::run)
}

fn run_app_with_ui_start<A: App>(
    app: Option<A>,
    on_ui_start: impl FnOnce(),
) -> io::Result<Selection<A::Output>> {
    run_app_with(app, on_ui_start, tui::run)
}

fn run_single_app_with_ui_start<A: App<Output = LocalBranch>, StartData>(
    mut branches: Vec<LocalBranch>,
    branch: Option<&str>,
    make_app: impl FnOnce(Vec<LocalBranch>, Option<String>) -> Option<A>,
    prepare_ui_start: impl FnOnce(&[LocalBranch]) -> StartData,
    on_ui_start: impl FnOnce(StartData),
) -> io::Result<Selection<LocalBranch>> {
    let exact_match = branch.and_then(|name| {
        branches
            .iter()
            .position(|branch| branch.name() == name)
            .map(|position| branches.remove(position))
    });

    match exact_match {
        Some(branch) => Ok(Selected(branch)),
        None => {
            let start_data = prepare_ui_start(&branches);
            run_app_with_ui_start(make_app(branches, branch.map(str::to_owned)), || {
                on_ui_start(start_data)
            })
        }
    }
}

fn run_single_app<A: App<Output = LocalBranch>>(
    branches: Vec<LocalBranch>,
    branch: Option<&str>,
    make_app: impl FnOnce(Vec<LocalBranch>, Option<String>) -> Option<A>,
) -> io::Result<Selection<LocalBranch>> {
    run_single_app_with_ui_start(branches, branch, make_app, |_| (), |()| {})
}

pub fn run_clean_app(branches: Vec<CleanBranch>) -> io::Result<Selection<Vec<LocalBranch>>> {
    run_app(AppImpl::clean(branches))
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
    on_ui_start: impl FnOnce(Vec<String>),
) -> io::Result<Selection<LocalBranch>> {
    run_single_app_with_ui_start(
        branches,
        branch,
        |branches, initial_search| AppImpl::merge(branches, destination, &history, initial_search),
        local_branch_names,
        on_ui_start,
    )
}

pub fn run_rebase_app(
    branches: Vec<LocalBranch>,
    last_target: Option<String>,
    branch: Option<&str>,
    on_ui_start: impl FnOnce(Vec<String>),
) -> io::Result<Selection<LocalBranch>> {
    run_single_app_with_ui_start(
        branches,
        branch,
        |branches, initial_search| AppImpl::rebase(branches, last_target, initial_search),
        local_branch_names,
        on_ui_start,
    )
}

pub fn run_switch_app(
    branches: Vec<LocalBranch>,
    history: SwitchHistory,
    branch: Option<&str>,
    on_ui_start: impl FnOnce(Vec<String>),
) -> io::Result<Selection<LocalBranch>> {
    run_single_app_with_ui_start(
        branches,
        branch,
        |branches, initial_search| AppImpl::switch(branches, &history, initial_search),
        local_branch_names,
        on_ui_start,
    )
}

fn local_branch_names(branches: &[LocalBranch]) -> Vec<String> {
    branches
        .iter()
        .map(|branch| branch.name().to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{Selection, run_app_with, run_switch_app};
    use crate::git::{Checkout, LocalBranch, SwitchHistory};
    use crate::ui::app::AppImpl;

    fn branch(name: &str, checkout: Checkout) -> LocalBranch {
        LocalBranch::for_test(name, checkout)
    }

    #[test]
    fn starting_an_available_ui_runs_its_start_callback() {
        let started = Cell::new(false);
        let app = AppImpl::switch(
            vec![branch("feature", Checkout::Available)],
            &SwitchHistory::default(),
            None,
        );

        let selection = run_app_with(app, || started.set(true), |_| Ok(None))
            .expect("the simulated UI should run");

        assert!(started.get());
        assert!(matches!(selection, Selection::Cancelled));
    }

    #[test]
    fn exact_matches_and_unavailable_uis_skip_the_start_callback() {
        let exact_started = Cell::new(false);
        let selection = run_switch_app(
            vec![branch("feature", Checkout::Available)],
            SwitchHistory::default(),
            Some("feature"),
            |_| exact_started.set(true),
        )
        .expect("an exact branch should be selected");
        assert!(!exact_started.get());
        assert!(matches!(selection, Selection::Selected(branch) if branch.name() == "feature"));

        let unavailable_started = Cell::new(false);
        let selection = run_switch_app(
            vec![branch("main", Checkout::CurrentWorktree)],
            SwitchHistory::default(),
            None,
            |_| unavailable_started.set(true),
        )
        .expect("an unavailable UI should return cleanly");
        assert!(!unavailable_started.get());
        assert!(matches!(selection, Selection::Unavailable));
    }
}
