use thiserror::Error;

mod clean;
pub mod git;
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

pub fn run_switch(repository: &HeadOperationRepository) -> Result<(), Error> {
    switch::run(repository)
}

pub fn run_rebase(repository: &CleanRebaseRepository) -> Result<(), Error> {
    rebase::run(repository)
}

pub fn run_merge(repository: &HeadOperationRepository) -> Result<(), Error> {
    merge::run(repository)
}
