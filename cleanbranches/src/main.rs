use std::io::{self, Stdout, Write};

use crossterm::cursor::Show;
use crossterm::event::{
    self, Event, KeyCode, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    supports_keyboard_enhancement,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

mod git;
mod ui;

use git::{LocalBranch, Repository};

use crate::ui::App;

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    pushed_keyboard_flags: bool,
}

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(e) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(e);
        }
        let pushed_keyboard_flags = if supports_keyboard_enhancement().unwrap_or(false) {
            execute!(
                stdout,
                PushKeyboardEnhancementFlags(
                    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                        | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES,
                )
            )
            .is_ok()
        } else {
            false
        };
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        Ok(Self {
            terminal,
            pushed_keyboard_flags,
        })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.pushed_keyboard_flags {
            let _ = execute!(self.terminal.backend_mut(), PopKeyboardEnhancementFlags);
        }
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen, Show);
        let _ = io::stdout().flush();
    }
}

fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
        let _ = io::stdout().flush();
        original(info);
    }));
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
) -> io::Result<Option<Vec<LocalBranch>>> {
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
    install_panic_hook();
    let result = {
        let mut guard = TerminalGuard::new()?;
        run(&mut guard.terminal, &mut app)
    };

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
