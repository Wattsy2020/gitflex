use std::io::IsTerminal;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use gitbranch::git::Repository;

#[derive(Debug, Parser)]
#[command(about = "Operate on local Git branches")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Select and delete local branches
    Clean,
    /// Select a local branch to switch to
    Switch {
        /// Branch name, or initial search text when it is not an exact match
        branch: Option<String>,
    },
    /// Select a local branch to rebase the current branch onto
    Rebase {
        /// Branch name, or initial search text when it is not an exact match
        branch: Option<String>,
    },
    /// Select a local branch to merge into the current branch
    Merge {
        /// Branch name, or initial search text when it is not an exact match
        branch: Option<String>,
    },
}

fn run() -> Result<(), gitbranch::Error> {
    let command = Cli::parse().command;
    let repository = Repository::discover(".")?;

    match command {
        Command::Clean => gitbranch::run_clean(&repository)?,
        Command::Switch { branch } => {
            gitbranch::run_switch(&repository.into_head_operation()?, branch.as_deref())?
        }
        Command::Rebase { branch } => gitbranch::run_rebase(
            &repository.into_head_operation()?.into_clean_rebase()?,
            branch.as_deref(),
        )?,
        Command::Merge { branch } => {
            gitbranch::run_merge(&repository.into_head_operation()?, branch.as_deref())?
        }
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

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Command};

    #[test]
    fn single_operations_accept_an_optional_branch_argument() {
        for (operation, expected_branch) in [
            ("switch", "feature/switch"),
            ("rebase", "feature/rebase"),
            ("merge", "feature/merge"),
        ] {
            let command = Cli::try_parse_from(["gitbranch", operation, expected_branch])
                .expect("a branch argument should be accepted")
                .command;
            let branch = match command {
                Command::Switch { branch }
                | Command::Rebase { branch }
                | Command::Merge { branch } => branch,
                Command::Clean => panic!("a single operation should be parsed"),
            };

            assert_eq!(branch.as_deref(), Some(expected_branch));
        }
    }

    #[test]
    fn single_operations_remain_interactive_without_an_argument() {
        for operation in ["switch", "rebase", "merge"] {
            let command = Cli::try_parse_from(["gitbranch", operation])
                .expect("the branch argument should remain optional")
                .command;
            let branch = match command {
                Command::Switch { branch }
                | Command::Rebase { branch }
                | Command::Merge { branch } => branch,
                Command::Clean => panic!("a single operation should be parsed"),
            };

            assert_eq!(branch, None);
        }
    }

    #[test]
    fn clean_does_not_accept_a_branch_argument() {
        assert!(Cli::try_parse_from(["gitbranch", "clean", "feature"]).is_err());
    }
}
