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
}

fn run() -> Result<(), gitbranch::Error> {
    let command = Cli::parse().command;
    let repository = Repository::discover(".")?;

    match command {
        Command::Clean => gitbranch::run_clean(&repository)?,
        Command::Switch => gitbranch::run_switch(&repository)?,
        Command::Rebase => gitbranch::run_rebase(&repository)?,
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
