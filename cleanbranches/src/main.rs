use std::io;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::supports_keyboard_enhancement;

mod git;
mod ui;

use git::{LocalBranch, Repository};

use crate::ui::App;

/// Constructing it enables keyboard enhancement if possible, upon drop it cleans up by removing keyboard enhancement
struct KeyboardEnhancementGuard;

impl KeyboardEnhancementGuard {
    fn try_enable() -> Option<Self> {
        supports_keyboard_enhancement()
            .unwrap_or(false)
            .then(|| {
                execute!(
                    io::stdout(),
                    PushKeyboardEnhancementFlags(
                        KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                            | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES,
                    )
                )
            })
            .and_then(Result::ok)
            .map(|()| Self)
    }
}

impl Drop for KeyboardEnhancementGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
}

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<Option<Vec<LocalBranch>>> {
    let _keyboard_enhancements = KeyboardEnhancementGuard::try_enable();

    loop {
        terminal.draw(|f| app.draw(f))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(None),
                KeyCode::Down | KeyCode::Char('j') => app.next(),
                KeyCode::Up | KeyCode::Char('k') => app.prev(),
                KeyCode::Char(' ') => app.toggle(),
                KeyCode::Enter
                    if key
                        .modifiers
                        .intersects(KeyModifiers::SUPER | KeyModifiers::CONTROL) =>
                {
                    return Ok(Some(app.branches_to_delete()));
                }
                _ => {}
            }
        }
    }
}

fn main() -> io::Result<()> {
    let repository = match Repository::discover(".") {
        Ok(repository) => repository,
        Err(e) => {
            eprintln!("Failed to find git repository: {e}");
            std::process::exit(1);
        }
    };
    let branches = match repository.local_branches() {
        Ok(branches) => branches,
        Err(e) => {
            eprintln!("Failed to list branches: {e}");
            std::process::exit(1);
        }
    };

    if branches.is_empty() {
        println!("No branches found.");
        return Ok(());
    }

    let mut app = App::new(branches);
    let result = ratatui::run(|terminal| run(terminal, &mut app));

    match result? {
        None => println!("Cancelled."),
        Some(to_delete) if to_delete.is_empty() => println!("No branches selected."),
        Some(to_delete) => {
            to_delete
                .iter()
                .for_each(|branch| match repository.delete_branch(branch) {
                    Ok(()) => println!("Deleted branch {}.", branch.name()),
                    Err(error) => println!("Failed to delete branch {}: {error}", branch.name()),
                });
        }
    }

    Ok(())
}
