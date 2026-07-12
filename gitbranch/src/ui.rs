use std::io;

use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::supports_keyboard_enhancement;
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::git::{Checkout, LocalBranch};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SingleOperation {
    Switch,
    Rebase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    Clean,
    Single(SingleOperation),
}

impl Mode {
    fn is_selectable(self, branch: &Branch) -> bool {
        let branch = &branch.branch;
        match self {
            Self::Clean => branch.is_deletable(),
            Self::Single(SingleOperation::Switch) => branch.is_switchable(),
            Self::Single(SingleOperation::Rebase) => branch.is_rebase_target(),
        }
    }

    fn help(self) -> &'static str {
        match self {
            Self::Clean => {
                "↑/↓ navigate   space toggle   cmd/ctrl+enter delete selected   q/esc quit"
            }
            Self::Single(SingleOperation::Switch) => {
                "↑/↓ navigate   enter switch to branch   q/esc quit"
            }
            Self::Single(SingleOperation::Rebase) => {
                "↑/↓ navigate   enter rebase onto branch   q/esc quit"
            }
        }
    }
}

struct Branch {
    branch: LocalBranch,
    selected: bool,
}

impl Branch {
    fn new(branch: LocalBranch, mode: Mode) -> Self {
        Self {
            selected: mode == Mode::Clean && branch.is_deletable(),
            branch,
        }
    }

    fn toggle(&mut self, mode: Mode) {
        if mode.is_selectable(self) {
            self.selected = !self.selected;
        }
    }

    fn branch_text(&self, mode: Mode) -> String {
        let marker = match mode {
            Mode::Clean if self.branch.is_deletable() => {
                if self.selected {
                    "[x] "
                } else {
                    "[ ] "
                }
            }
            Mode::Clean => "[-] ",
            Mode::Single(_) => "",
        };
        let status = match self.branch.checkout() {
            Checkout::Available => "",
            Checkout::CurrentWorktree => " (current)",
            Checkout::OtherWorktree => " (other worktree)",
        };

        format!("{marker}{}{status}", self.branch.name())
    }

    fn color(&self, mode: Mode, highlighted: bool) -> Color {
        if !mode.is_selectable(self) {
            if highlighted {
                Color::Gray
            } else {
                Color::DarkGray
            }
        } else if mode == Mode::Clean && self.selected {
            Color::Red
        } else {
            Color::Gray
        }
    }

    fn render(&self, mode: Mode, highlighted: bool) -> ListItem<'static> {
        ListItem::new(Line::from(Span::styled(
            self.branch_text(mode),
            Style::default().fg(self.color(mode, highlighted)),
        )))
    }
}

pub struct App {
    branches: Vec<Branch>,
    mode: Mode,
    state: ListState,
}

impl App {
    pub fn new(branches: Vec<LocalBranch>, mode: Mode) -> Option<Self> {
        let branches = branches
            .into_iter()
            .map(|branch| Branch::new(branch, mode))
            .collect();
        let mut app = Self {
            branches,
            mode,
            state: ListState::default(),
        };
        app.state.select(Some(app.first_selectable()?));
        Some(app)
    }

    fn position(&self) -> usize {
        self.state
            .selected()
            .expect("a list element is always selected")
    }

    fn first_selectable(&self) -> Option<usize> {
        self.branches
            .iter()
            .position(|branch| self.mode.is_selectable(branch))
    }

    fn next(&mut self) {
        let position = (self.position() + 1).min(self.branches.len() - 1);
        self.state.select(Some(position));
    }

    fn previous(&mut self) {
        let position = self.position().saturating_sub(1);
        self.state.select(Some(position));
    }

    fn toggle(&mut self) {
        let position = self.position();
        self.branches[position].toggle(self.mode);
    }

    fn confirm_many(&self) -> Vec<LocalBranch> {
        self.branches
            .iter()
            .filter(|branch| branch.selected)
            .map(|branch| branch.branch.clone())
            .collect()
    }

    fn confirm_one(&self) -> Option<LocalBranch> {
        self.branches
            .get(self.position())
            .filter(|branch| self.mode.is_selectable(branch))
            .map(|branch| branch.branch.clone())
    }

    fn render_branches(&self) -> List<'static> {
        let items = self
            .branches
            .iter()
            .enumerate()
            .map(|(index, branch)| branch.render(self.mode, self.position() == index))
            .collect::<Vec<_>>();

        List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Branches"))
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .bg(Color::DarkGray),
            )
            .highlight_symbol("> ")
    }

    fn draw(&mut self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(frame.area());

        frame.render_stateful_widget(self.render_branches(), chunks[0], &mut self.state);
        frame.render_widget(
            Paragraph::new(self.mode.help())
                .block(Block::default().borders(Borders::ALL).title("Help")),
            chunks[1],
        );
    }
}

/// Enables enhanced key reporting when supported and restores the terminal mode on drop.
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

fn run<TOutput>(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    get_output: fn(&App) -> Option<TOutput>,
) -> io::Result<Option<TOutput>> {
    let _keyboard_enhancements = KeyboardEnhancementGuard::try_enable();

    loop {
        terminal.draw(|frame| app.draw(frame))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(None),
                KeyCode::Down | KeyCode::Char('j') => app.next(),
                KeyCode::Up | KeyCode::Char('k') => app.previous(),
                KeyCode::Char(' ') if app.mode == Mode::Clean => app.toggle(),
                KeyCode::Enter => match app.mode {
                    Mode::Clean
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::SUPER | KeyModifiers::CONTROL) => {}
                    Mode::Single(_) | Mode::Clean => {
                        if let Some(selection) = get_output(app) {
                            return Ok(Some(selection));
                        }
                    }
                },
                _ => {}
            }
        }
    }
}

pub fn select_many(mut app: App) -> io::Result<Option<Vec<LocalBranch>>> {
    ratatui::run(|terminal| {
        run(terminal, &mut app, |app| {
            // if the user didn't select anything, assume it was a mistake and stay in the TUI
            // they can click esc to exit if they want to
            let result = app.confirm_many();
            if result.is_empty() {
                None
            } else {
                Some(result)
            }
        })
    })
}

pub fn select_one(mut app: App) -> io::Result<Option<LocalBranch>> {
    ratatui::run(|terminal| run(terminal, &mut app, |app| app.confirm_one()))
}

#[cfg(test)]
mod tests {
    use crate::git::{Checkout, LocalBranch};

    use super::{App, Mode, SingleOperation};

    fn branch(name: &str, checkout: Checkout) -> LocalBranch {
        LocalBranch::for_test(name, checkout)
    }

    #[test]
    fn clean_starts_with_every_deletable_branch_selected() {
        let app = App::new(
            vec![
                branch("feature-a", Checkout::Available),
                branch("feature-b", Checkout::Available),
                branch("main", Checkout::CurrentWorktree),
            ],
            Mode::Clean,
        )
        .unwrap();

        let branches = app.confirm_many();
        assert_eq!(
            branches.iter().map(LocalBranch::name).collect::<Vec<_>>(),
            ["feature-a", "feature-b"]
        );
    }

    #[test]
    fn single_selection_returns_only_the_highlighted_branch() {
        let mut app = App::new(
            vec![
                branch("feature-a", Checkout::Available),
                branch("feature-b", Checkout::Available),
                branch("main", Checkout::CurrentWorktree),
            ],
            Mode::Single(SingleOperation::Rebase),
        )
        .unwrap();
        app.next();

        let Some(branch) = app.confirm_one() else {
            panic!("an eligible highlighted branch should be returned");
        };
        assert_eq!(branch.name(), "feature-b");
    }

    #[test]
    fn single_selection_rejects_an_ineligible_highlighted_branch() {
        let mut app = App::new(
            vec![
                branch("feature", Checkout::Available),
                branch("main", Checkout::CurrentWorktree),
            ],
            Mode::Single(SingleOperation::Switch),
        )
        .unwrap();
        app.next();

        assert!(app.confirm_one().is_none());
    }
}
