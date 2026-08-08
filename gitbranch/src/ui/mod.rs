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

fn run_app<A: App>(app: Option<A>) -> io::Result<Selection<A::Output>> {
    match app {
        None => Ok(Unavailable),
        Some(app) => match tui::run(app)? {
            None => Ok(Cancelled),
            Some(result) => Ok(Selected(result)),
        },
    }
}

fn run_single_app<A: App<Output = LocalBranch>>(
    mut branches: Vec<LocalBranch>,
    branch: Option<&str>,
    make_app: impl FnOnce(Vec<LocalBranch>, Option<String>) -> Option<A>,
) -> io::Result<Selection<LocalBranch>> {
    let exact_match = branch.and_then(|name| {
        branches
            .iter()
            .position(|branch| branch.name() == name)
            .map(|position| branches.remove(position))
    });

    match exact_match {
        Some(branch) => Ok(Selected(branch)),
        None => run_app(make_app(branches, branch.map(str::to_owned))),
    }
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
) -> io::Result<Selection<LocalBranch>> {
    run_single_app(branches, branch, |branches, initial_search| {
        AppImpl::merge(branches, destination, &history, initial_search)
    })
}

pub fn run_rebase_app(
    branches: Vec<LocalBranch>,
    last_target: Option<String>,
    branch: Option<&str>,
) -> io::Result<Selection<LocalBranch>> {
    run_single_app(branches, branch, |branches, initial_search| {
        AppImpl::rebase(branches, last_target, initial_search)
    })
}

pub fn run_switch_app(
    branches: Vec<LocalBranch>,
    history: SwitchHistory,
    branch: Option<&str>,
) -> io::Result<Selection<LocalBranch>> {
    run_single_app(branches, branch, |branches, initial_search| {
        AppImpl::switch(branches, &history, initial_search)
    })
}
