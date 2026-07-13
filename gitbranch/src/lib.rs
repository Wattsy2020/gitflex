use thiserror::Error;

mod clean;
pub mod git;
mod merge;
mod rebase;
mod switch;
mod ui;

use git::Repository;

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

pub fn run_switch(repository: &Repository) -> Result<(), Error> {
    switch::run(repository)
}

pub fn run_rebase(repository: &Repository) -> Result<(), Error> {
    rebase::run(repository)
}

pub fn run_merge(repository: &Repository) -> Result<(), Error> {
    merge::run(repository)
}
