use thiserror::Error;

mod clean;
mod delete;
pub mod git;
pub mod history;
mod merge;
mod rebase;
mod switch;
mod ui;

use git::{CleanRebaseRepository, HeadOperationRepository, Repository};

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Git(#[from] git::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn run_clean(repository: &Repository) -> Result<(), Error> {
    clean::run(repository)
}

pub fn run_delete(repository: &Repository, branch: Option<&str>) -> Result<(), Error> {
    delete::run(repository, branch)
}

pub fn run_switch(repository: &HeadOperationRepository, branch: Option<&str>) -> Result<(), Error> {
    switch::run(repository, branch)
}

pub fn run_rebase(repository: &CleanRebaseRepository, branch: Option<&str>) -> Result<(), Error> {
    rebase::run(repository, branch)
}

pub fn run_merge(repository: &HeadOperationRepository, branch: Option<&str>) -> Result<(), Error> {
    merge::run(repository, branch)
}

/// Map git branches to their names
fn local_branch_names(branches: &[git::LocalBranch]) -> Vec<String> {
    branches
        .iter()
        .map(|branch| branch.name().to_owned())
        .collect()
}
