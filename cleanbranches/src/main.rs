use std::io::{self, Stdout, Write};
use std::process::Command;

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

struct App {
    branches: Vec<String>,
    selected: Vec<bool>,
    current: String,
    state: ListState,
}

impl App {
    fn new(branches: Vec<String>, current: String) -> Self {
        let selected = branches.iter().map(|b| b != &current).collect();
        let mut state = ListState::default();
        if !branches.is_empty() {
            state.select(Some(0));
        }
        Self {
            branches,
            selected,
            current,
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
            if self.branches[i] == self.current {
                return;
            }
            self.selected[i] = !self.selected[i];
        }
    }

    fn branches_to_delete(&self) -> Vec<String> {
        self.branches
            .iter()
            .zip(self.selected.iter())
            .filter_map(|(b, &s)| if s { Some(b.clone()) } else { None })
            .collect()
    }
}

fn list_branches() -> io::Result<(Vec<String>, String)> {
    let out = Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .output()?;
    if !out.status.success() {
        return Err(io::Error::other(format!(
            "git branch failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    let branches: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    let current_out = Command::new("git")
        .args(["branch", "--show-current"])
        .output()?;
    let current = String::from_utf8_lossy(&current_out.stdout)
        .trim()
        .to_string();

    Ok((branches, current))
}

/// Delete branch, returning the message from git
fn delete_branch(name: &str) -> String {
    match Command::new("git").args(["branch", "-D", name]).output() {
        Ok(out) => {
            let msg = if out.status.success() {
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            } else {
                String::from_utf8_lossy(&out.stderr).trim().to_string()
            };
            msg
        }
        Err(e) => format!("failed: {}", e.to_string()),
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
) -> io::Result<Option<Vec<String>>> {
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
                .map(|(b, &s)| {
                    let mark = if s { "[x]" } else { "[ ]" };
                    let is_current = b == &app.current;
                    let label = if is_current {
                        format!("{} {} (current)", mark, b)
                    } else {
                        format!("{} {}", mark, b)
                    };
                    let style = if is_current {
                        Style::default().fg(Color::DarkGray)
                    } else if s {
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
    let (branches, current) = match list_branches() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to list branches: {}", e);
            std::process::exit(1);
        }
    };

    if branches.is_empty() {
        println!("No branches found.");
        return Ok(());
    }

    let mut app = App::new(branches, current);
    install_panic_hook();
    let result = {
        let mut guard = TerminalGuard::new()?;
        run(&mut guard.terminal, &mut app)
    };

    match result? {
        None => println!("Cancelled."),
        Some(to_delete) if to_delete.is_empty() => println!("No branches selected."),
        Some(to_delete) => {
            for name in to_delete {
                let msg = delete_branch(&name);
                println!("{}", msg);
            }
        }
    }

    Ok(())
}
