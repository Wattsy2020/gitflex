use std::io;

use crate::{
    git::{CleanBranch, LocalBranch},
    ui::{
        Selection::{Cancelled, Selected, Unavailable},
        app::{App, AppImpl},
    },
};

mod app;
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

pub fn run_clean_app(branches: Vec<CleanBranch>) -> io::Result<Selection<Vec<LocalBranch>>> {
    run_app(AppImpl::clean(branches))
}

pub fn run_merge_app(branches: Vec<LocalBranch>) -> io::Result<Selection<LocalBranch>> {
    run_app(AppImpl::merge(branches))
}

pub fn run_rebase_app(
    branches: Vec<LocalBranch>,
    last_target: Option<String>,
) -> io::Result<Selection<LocalBranch>> {
    run_app(AppImpl::rebase(branches, last_target))
}

pub fn run_switch_app(branches: Vec<LocalBranch>) -> io::Result<Selection<LocalBranch>> {
    run_app(AppImpl::switch(branches))
}
