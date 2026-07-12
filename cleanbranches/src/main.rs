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
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

mod git;

use git::{Checkout, LocalBranch, Repository};

struct App {
    branches: Vec<LocalBranch>,
    selected: Vec<bool>,
    state: ListState,
}

impl App {
    fn new(branches: Vec<LocalBranch>) -> Self {
        let selected = branches.iter().map(LocalBranch::is_deletable).collect();
        let mut state = ListState::default();
        if !branches.is_empty() {
            state.select(Some(0));
        }
        Self {
            branches,
            selected,
            state,
        }
    }

    fn next(&mut self) {
        if self.branches.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => (i + 1).min(self.branches.len() - 1),
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn prev(&mut self) {
        if self.branches.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(0) => 0,
            Some(i) => i - 1,
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn toggle(&mut self) {
        if let Some(i) = self.state.selected() {
            if self.branches[i].is_deletable() {
                self.selected[i] = !self.selected[i];
            }
        }
    }

    fn branches_to_delete(&self) -> Vec<LocalBranch> {
        self.branches
            .iter()
            .zip(self.selected.iter())
            .filter_map(|(branch, &selected)| selected.then_some(branch.clone()))
            .collect()
    }
}

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
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(3)])
                .split(f.area());

            let items: Vec<ListItem> = app
                .branches
                .iter()
                .zip(app.selected.iter())
                .map(|(branch, &selected)| {
                    let mark = if selected { "[x]" } else { "[ ]" };
                    let label = match branch.checkout() {
                        Checkout::Available => format!("{} {}", mark, branch.name()),
                        Checkout::CurrentWorktree => {
                            format!("{} {} (current)", mark, branch.name())
                        }
                        Checkout::OtherWorktree => {
                            format!("{} {} (other worktree)", mark, branch.name())
                        }
                    };
                    let style = if !branch.is_deletable() {
                        Style::default().fg(Color::DarkGray)
                    } else if selected {
                        Style::default().fg(Color::Red)
                    } else {
                        Style::default()
                    };
                    ListItem::new(Line::from(Span::styled(label, style)))
                })
                .collect();

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title("Branches"))
                .highlight_style(
                    Style::default()
                        .add_modifier(Modifier::BOLD)
                        .bg(Color::DarkGray),
                )
                .highlight_symbol("> ");

            f.render_stateful_widget(list, chunks[0], &mut app.state);

            let help = Paragraph::new(
                "↑/↓ navigate   space toggle   cmd/ctrl+enter delete selected   q/esc quit",
            )
            .block(Block::default().borders(Borders::ALL).title("Help"));
            f.render_widget(help, chunks[1]);
        })?;

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
