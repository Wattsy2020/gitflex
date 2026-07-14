use std::io::IsTerminal;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use gitbranch::git::Repository;

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
    /// Select a local branch to merge into the current branch
    Merge,
}

fn run() -> Result<(), gitbranch::Error> {
    let command = Cli::parse().command;
    let repository = Repository::discover(".")?;

    match command {
        Command::Clean => gitbranch::run_clean(&repository)?,
        Command::Switch => gitbranch::run_switch(&repository.into_head_operation()?)?,
        Command::Rebase => gitbranch::run_rebase(&repository.into_head_operation()?)?,
        Command::Merge => gitbranch::run_merge(&repository.into_head_operation()?)?,
    }

    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if std::io::stderr().is_terminal() {
                eprintln!("{error}\r");
            } else {
                eprintln!("{error}");
            }
            ExitCode::FAILURE
        }
    }
}
