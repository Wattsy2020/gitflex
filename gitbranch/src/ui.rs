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

pub struct CleanMode;

pub struct SingleMode {
    operation: SingleOperation,
}

trait Mode {
    type Output;

    fn is_selectable(&self, branch: &LocalBranch) -> bool;
    fn initially_selected(&self, branch: &LocalBranch) -> bool;
    fn marker(&self, branch: &Branch) -> &'static str;
    fn is_selected(&self, branch: &Branch) -> bool;
    fn toggle(&self, branch: &mut Branch);
    fn confirms(&self, modifiers: KeyModifiers) -> bool;
    fn output(&self, branches: &[Branch], position: usize) -> Option<Self::Output>;
    fn help(&self) -> &'static str;
}

impl Mode for CleanMode {
    type Output = Vec<LocalBranch>;

    fn is_selectable(&self, branch: &LocalBranch) -> bool {
        branch.is_deletable()
    }

    fn initially_selected(&self, branch: &LocalBranch) -> bool {
        branch.is_deletable()
    }

    fn marker(&self, branch: &Branch) -> &'static str {
        if !branch.branch.is_deletable() {
            "[-] "
        } else if branch.selected {
            "[x] "
        } else {
            "[ ] "
        }
    }

    fn is_selected(&self, branch: &Branch) -> bool {
        branch.selected
    }

    fn toggle(&self, branch: &mut Branch) {
        if self.is_selectable(&branch.branch) {
            branch.selected = !branch.selected;
        }
    }

    fn confirms(&self, modifiers: KeyModifiers) -> bool {
        modifiers.intersects(KeyModifiers::SUPER | KeyModifiers::CONTROL)
    }

    fn output(&self, branches: &[Branch], _position: usize) -> Option<Self::Output> {
        let branches = branches
            .iter()
            .filter(|branch| branch.selected)
            .map(|branch| branch.branch.clone())
            .collect::<Vec<_>>();

        // An empty selection is assumed to be accidental; Escape remains available to cancel.
        (!branches.is_empty()).then_some(branches)
    }

    fn help(&self) -> &'static str {
        "↑/↓ navigate   space toggle   cmd/ctrl+enter delete selected   q/esc quit"
    }
}

impl Mode for SingleMode {
    type Output = LocalBranch;

    fn is_selectable(&self, branch: &LocalBranch) -> bool {
        match self.operation {
            SingleOperation::Switch => branch.is_switchable(),
            SingleOperation::Rebase => branch.is_rebase_target(),
        }
    }

    fn initially_selected(&self, _branch: &LocalBranch) -> bool {
        false
    }

    fn marker(&self, _branch: &Branch) -> &'static str {
        ""
    }

    fn is_selected(&self, _branch: &Branch) -> bool {
        false
    }

    fn toggle(&self, _branch: &mut Branch) {}

    fn confirms(&self, _modifiers: KeyModifiers) -> bool {
        true
    }

    fn output(&self, branches: &[Branch], position: usize) -> Option<Self::Output> {
        branches
            .get(position)
            .filter(|branch| self.is_selectable(&branch.branch))
            .map(|branch| branch.branch.clone())
    }

    fn help(&self) -> &'static str {
        match self.operation {
            SingleOperation::Switch => "↑/↓ navigate   enter switch to branch   q/esc quit",
            SingleOperation::Rebase => "↑/↓ navigate   enter rebase onto branch   q/esc quit",
        }
    }
}

struct Branch {
    branch: LocalBranch,
    selected: bool,
}

impl Branch {
    fn new<M: Mode>(branch: LocalBranch, mode: &M) -> Self {
        Self {
            selected: mode.initially_selected(&branch),
            branch,
        }
    }

    fn branch_text<M: Mode>(&self, mode: &M) -> String {
        let marker = mode.marker(self);
        let status = match self.branch.checkout() {
            Checkout::Available => "",
            Checkout::CurrentWorktree => " (current)",
            Checkout::OtherWorktree => " (other worktree)",
        };

        format!("{marker}{}{status}", self.branch.name())
    }

    fn color<M: Mode>(&self, mode: &M, highlighted: bool) -> Color {
        if !mode.is_selectable(&self.branch) {
            if highlighted {
                Color::Gray
            } else {
                Color::DarkGray
            }
        } else if mode.is_selected(self) {
            Color::Red
        } else {
            Color::Gray
        }
    }

    fn render<M: Mode>(&self, mode: &M, highlighted: bool) -> ListItem<'static> {
        ListItem::new(Line::from(Span::styled(
            self.branch_text(mode),
            Style::default().fg(self.color(mode, highlighted)),
        )))
    }
}

pub struct App<M> {
    branches: Vec<Branch>,
    mode: M,
    state: ListState,
}

impl App<CleanMode> {
    pub fn clean(branches: Vec<LocalBranch>) -> Option<Self> {
        Self::new(branches, CleanMode)
    }
}

impl App<SingleMode> {
    pub fn single(branches: Vec<LocalBranch>, operation: SingleOperation) -> Option<Self> {
        Self::new(branches, SingleMode { operation })
    }
}

impl<M> App<M> {
    fn new(branches: Vec<LocalBranch>, mode: M) -> Option<Self>
    where
        M: Mode,
    {
        let branches = branches
            .into_iter()
            .map(|branch| Branch::new(branch, &mode))
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

    fn first_selectable(&self) -> Option<usize>
    where
        M: Mode,
    {
        self.branches
            .iter()
            .position(|branch| self.mode.is_selectable(&branch.branch))
    }

    fn next(&mut self) {
        let position = (self.position() + 1).min(self.branches.len() - 1);
        self.state.select(Some(position));
    }

    fn previous(&mut self) {
        let position = self.position().saturating_sub(1);
        self.state.select(Some(position));
    }

    fn toggle(&mut self)
    where
        M: Mode,
    {
        let position = self.position();
        self.mode.toggle(&mut self.branches[position]);
    }

    fn output(&self) -> Option<M::Output>
    where
        M: Mode,
    {
        self.mode.output(&self.branches, self.position())
    }

    fn render_branches(&self) -> List<'static>
    where
        M: Mode,
    {
        let items = self
            .branches
            .iter()
            .enumerate()
            .map(|(index, branch)| branch.render(&self.mode, self.position() == index))
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

    fn draw(&mut self, frame: &mut Frame)
    where
        M: Mode,
    {
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

fn run<M: Mode>(terminal: &mut DefaultTerminal, app: &mut App<M>) -> io::Result<Option<M::Output>> {
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
                KeyCode::Char(' ') => app.toggle(),
                KeyCode::Enter if app.mode.confirms(key.modifiers) => {
                    if let Some(selection) = app.output() {
                        return Ok(Some(selection));
                    }
                }
                _ => {}
            }
        }
    }
}

pub fn select_many(mut app: App<CleanMode>) -> io::Result<Option<Vec<LocalBranch>>> {
    ratatui::run(|terminal| run(terminal, &mut app))
}

pub fn select_one(mut app: App<SingleMode>) -> io::Result<Option<LocalBranch>> {
    ratatui::run(|terminal| run(terminal, &mut app))
}

#[cfg(test)]
mod tests {
    use crate::git::{Checkout, LocalBranch};

    use super::{App, SingleOperation};

    fn branch(name: &str, checkout: Checkout) -> LocalBranch {
        LocalBranch::for_test(name, checkout)
    }

    #[test]
    fn clean_starts_with_every_deletable_branch_selected() {
        let app = App::clean(vec![
            branch("feature-a", Checkout::Available),
            branch("feature-b", Checkout::Available),
            branch("main", Checkout::CurrentWorktree),
        ])
        .unwrap();

        let branches = app.output().unwrap();
        assert_eq!(
            branches.iter().map(LocalBranch::name).collect::<Vec<_>>(),
            ["feature-a", "feature-b"]
        );
    }

    #[test]
    fn single_selection_returns_only_the_highlighted_branch() {
        let mut app = App::single(
            vec![
                branch("feature-a", Checkout::Available),
                branch("feature-b", Checkout::Available),
                branch("main", Checkout::CurrentWorktree),
            ],
            SingleOperation::Rebase,
        )
        .unwrap();
        app.next();

        let Some(branch) = app.output() else {
            panic!("an eligible highlighted branch should be returned");
        };
        assert_eq!(branch.name(), "feature-b");
    }

    #[test]
    fn single_selection_rejects_an_ineligible_highlighted_branch() {
        let mut app = App::single(
            vec![
                branch("feature", Checkout::Available),
                branch("main", Checkout::CurrentWorktree),
            ],
            SingleOperation::Switch,
        )
        .unwrap();
        app.next();

        assert!(app.output().is_none());
    }
}
