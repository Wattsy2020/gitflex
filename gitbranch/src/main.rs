use std::io::IsTerminal;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use thiserror::Error;

mod clean;
mod git;
mod rebase;
mod switch;
mod ui;

use git::Repository;

#[derive(Debug, Parser)]
#[command(about = "Interactively operate on local Git branches")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Select and delete local branches
    Clean,
    /// Select a local branch to switch to
    Switch,
    /// Select a local branch to rebase the current branch onto
    Rebase,
}

#[derive(Debug, Error)]
enum Error {
    #[error(transparent)]
    Git(#[from] git::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn run() -> Result<(), Error> {
    let command = Cli::parse().command;
    let repository = Repository::discover(".")?;

    match command {
        Command::Clean => clean::run(&repository)?,
        Command::Switch => switch::run(&repository)?,
        Command::Rebase => rebase::run(&repository)?,
    }

    Ok(())
}

fn main() -> ExitCode {
    run().map_or_else(
        |error| {
            if std::io::stderr().is_terminal() {
                eprintln!("{error}\r");
            } else {
                eprintln!("{error}");
            }
            ExitCode::FAILURE
        },
        |()| ExitCode::SUCCESS,
    )
}
