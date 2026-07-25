use std::num::NonZeroUsize;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::git::{Checkout, LocalBranch};

const SELECTED_COLOUR: Color = Color::Red;
const SELECTABLE_COLOUR: Color = Color::Black;
const UNSELECTABLE_COLOUR: Color = Color::Gray;
const HIGHLIGHTED_COLOUR: Color = Color::White;
const BACKGROUND_COLOUR: Color = Color::Black;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Confirmation {
    Plain,
    Modified,
}

/// Instructions the UI sends to the App
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Next,
    Previous,
    Toggle,
    Confirm(Confirmation),
    Cancel,
}

/// Tells the UI the result of an App operation
#[derive(Debug, Eq, PartialEq)]
pub enum Transition<T> {
    Continue,
    Complete(T),
    Cancel,
}

/// Define modes: the type of operation an app can do
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SingleOperation {
    Switch,
    Rebase { last_target: Option<String> },
    Merge,
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
    fn annotation(&self, branch: &LocalBranch) -> Option<&'static str>;
    fn is_selected(&self, branch: &Branch) -> bool;
    fn toggle(&self, branch: &mut Branch);
    fn confirms(&self, confirmation: Confirmation) -> bool;
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

    fn annotation(&self, _branch: &LocalBranch) -> Option<&'static str> {
        None
    }

    fn is_selected(&self, branch: &Branch) -> bool {
        branch.selected
    }

    fn toggle(&self, branch: &mut Branch) {
        if self.is_selectable(&branch.branch) {
            branch.selected = !branch.selected;
        }
    }

    fn confirms(&self, confirmation: Confirmation) -> bool {
        confirmation == Confirmation::Modified
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
        match &self.operation {
            SingleOperation::Switch => branch.is_switchable(),
            SingleOperation::Rebase { .. } => branch.is_rebase_target(),
            SingleOperation::Merge => branch.is_merge_source(),
        }
    }

    fn initially_selected(&self, _branch: &LocalBranch) -> bool {
        false
    }

    fn marker(&self, _branch: &Branch) -> &'static str {
        ""
    }

    fn annotation(&self, branch: &LocalBranch) -> Option<&'static str> {
        match &self.operation {
            SingleOperation::Rebase {
                last_target: Some(last_target),
            } if last_target == branch.name() => Some(" (last rebased onto)"),
            _ => None,
        }
    }

    fn is_selected(&self, _branch: &Branch) -> bool {
        false
    }

    fn toggle(&self, _branch: &mut Branch) {}

    fn confirms(&self, _confirmation: Confirmation) -> bool {
        true
    }

    fn output(&self, branches: &[Branch], position: usize) -> Option<Self::Output> {
        branches
            .get(position)
            .filter(|branch| self.is_selectable(&branch.branch))
            .map(|branch| branch.branch.clone())
    }

    fn help(&self) -> &'static str {
        match &self.operation {
            SingleOperation::Switch => "↑/↓ navigate   enter switch to branch   q/esc quit",
            SingleOperation::Rebase { .. } => {
                "↑/↓ navigate   enter rebase onto branch   q/esc quit"
            }
            SingleOperation::Merge => "↑/↓ navigate   enter merge branch   q/esc quit",
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
        let name = self.branch.name();
        let annotation = mode.annotation(&self.branch).unwrap_or_default();
        let status = match self.branch.checkout() {
            Checkout::Available => "",
            Checkout::CurrentWorktree => " (current)",
            Checkout::OtherWorktree => " (other worktree)",
        };

        format!("{marker}{name}{status}{annotation}")
    }

    fn colour<M: Mode>(&self, mode: &M, highlighted: bool) -> Color {
        if mode.is_selected(self) {
            SELECTED_COLOUR
        } else if highlighted {
            HIGHLIGHTED_COLOUR
        } else if mode.is_selectable(&self.branch) {
            SELECTABLE_COLOUR
        } else {
            UNSELECTABLE_COLOUR
        }
    }

    fn render<M: Mode>(&self, mode: &M, highlighted: bool) -> ListItem<'static> {
        ListItem::new(Line::from(Span::styled(
            self.branch_text(mode),
            Style::default().fg(self.colour(mode, highlighted)),
        )))
    }
}

pub struct AppImpl<M> {
    branches: Vec<Branch>,
    mode: M,
    selectable_count: NonZeroUsize,
    state: ListState,
}

impl AppImpl<CleanMode> {
    pub fn clean(branches: Vec<LocalBranch>) -> Option<Self> {
        Self::new(branches, CleanMode)
    }
}

impl AppImpl<SingleMode> {
    pub fn switch(branches: Vec<LocalBranch>) -> Option<Self> {
        Self::new(
            branches,
            SingleMode {
                operation: SingleOperation::Switch,
            },
        )
    }

    pub fn rebase(mut branches: Vec<LocalBranch>, last_target: Option<String>) -> Option<Self> {
        if let Some(last_target) = last_target.as_deref() {
            branches.sort_by(|left, right| {
                let left_was_last = last_target == left.name();
                let right_was_last = last_target == right.name();
                right_was_last
                    .cmp(&left_was_last)
                    .then_with(|| left.name().cmp(right.name()))
            });
        }

        Self::new(
            branches,
            SingleMode {
                operation: SingleOperation::Rebase { last_target },
            },
        )
    }

    pub fn merge(branches: Vec<LocalBranch>) -> Option<Self> {
        Self::new(
            branches,
            SingleMode {
                operation: SingleOperation::Merge,
            },
        )
    }
}

impl<M> AppImpl<M> {
    fn new(mut branches: Vec<LocalBranch>, mode: M) -> Option<Self>
    where
        M: Mode,
    {
        branches.sort_by_key(|branch| !mode.is_selectable(branch));
        let selectable_count = NonZeroUsize::new(
            branches
                .iter()
                .take_while(|branch| mode.is_selectable(branch))
                .count(),
        )?;
        let branches = branches
            .into_iter()
            .map(|branch| Branch::new(branch, &mode))
            .collect();
        let mut app = Self {
            branches,
            mode,
            selectable_count,
            state: ListState::default(),
        };
        app.state.select(Some(0));
        Some(app)
    }

    fn position(&self) -> usize {
        self.state
            .selected()
            .expect("a list element is always selected")
    }

    fn next(&mut self) {
        let new_pos = (self.position() + 1) % self.selectable_count.get();
        self.state.select(Some(new_pos));
    }

    fn previous(&mut self) {
        let new_pos = self
            .position()
            .checked_sub(1)
            .unwrap_or(self.selectable_count.get() - 1);
        self.state.select(Some(new_pos));
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

    fn update(&mut self, action: Action) -> Transition<M::Output>
    where
        M: Mode,
    {
        match action {
            Action::Next => self.next(),
            Action::Previous => self.previous(),
            Action::Toggle => self.toggle(),
            Action::Confirm(confirmation) if self.mode.confirms(confirmation) => {
                return self
                    .output()
                    .map_or(Transition::Continue, Transition::Complete);
            }
            Action::Cancel => return Transition::Cancel,
            Action::Confirm(_) => {}
        }

        Transition::Continue
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
                    .bg(BACKGROUND_COLOUR),
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

pub trait App {
    type Output;
    fn update(&mut self, action: Action) -> Transition<Self::Output>;
    fn draw(&mut self, frame: &mut Frame);
}

impl App for AppImpl<CleanMode> {
    type Output = Vec<LocalBranch>;
    fn update(&mut self, action: Action) -> Transition<Self::Output> {
        self.update(action)
    }
    fn draw(&mut self, frame: &mut Frame) {
        self.draw(frame);
    }
}

impl App for AppImpl<SingleMode> {
    type Output = LocalBranch;
    fn update(&mut self, action: Action) -> Transition<Self::Output> {
        self.update(action)
    }
    fn draw(&mut self, frame: &mut Frame) {
        self.draw(frame);
    }
}

#[cfg(test)]
mod tests {
    use crate::git::{Checkout, LocalBranch};

    use super::{Action, AppImpl, Confirmation, Transition};

    fn branch(name: &str, checkout: Checkout) -> LocalBranch {
        LocalBranch::for_test(name, checkout)
    }

    fn branch_names<M>(app: &AppImpl<M>) -> Vec<&str> {
        app.branches
            .iter()
            .map(|branch| branch.branch.name())
            .collect()
    }

    #[test]
    fn clean_starts_with_every_deletable_branch_selected() {
        let mut app = AppImpl::clean(vec![
            branch("feature-a", Checkout::Available),
            branch("feature-b", Checkout::Available),
            branch("main", Checkout::CurrentWorktree),
        ])
        .unwrap();

        let Transition::Complete(branches) = app.update(Action::Confirm(Confirmation::Modified))
        else {
            panic!("a modified confirmation should complete clean selection");
        };
        assert_eq!(
            branches.iter().map(LocalBranch::name).collect::<Vec<_>>(),
            ["feature-a", "feature-b"]
        );
    }

    #[test]
    fn clean_requires_modified_confirmation_and_toggle_changes_selection() {
        let mut app = AppImpl::clean(vec![
            branch("feature-a", Checkout::Available),
            branch("feature-b", Checkout::Available),
        ])
        .unwrap();

        assert_eq!(
            app.update(Action::Confirm(Confirmation::Plain)),
            Transition::Continue
        );
        assert_eq!(app.update(Action::Toggle), Transition::Continue);

        let Transition::Complete(branches) = app.update(Action::Confirm(Confirmation::Modified))
        else {
            panic!("a modified confirmation should complete clean selection");
        };
        assert_eq!(
            branches.iter().map(LocalBranch::name).collect::<Vec<_>>(),
            ["feature-b"]
        );
    }

    #[test]
    fn single_selection_navigates_and_confirms_the_highlighted_branch() {
        let mut app = AppImpl::rebase(
            vec![
                branch("feature-a", Checkout::Available),
                branch("feature-b", Checkout::Available),
                branch("main", Checkout::CurrentWorktree),
            ],
            None,
        )
        .unwrap();
        assert_eq!(app.update(Action::Next), Transition::Continue);

        let Transition::Complete(branch) = app.update(Action::Confirm(Confirmation::Plain)) else {
            panic!("a valid single selection should complete");
        };
        assert_eq!(branch.name(), "feature-b");
    }

    #[test]
    fn commands_put_unselectable_branches_last() {
        let branches = || {
            vec![
                branch("develop", Checkout::Available),
                branch("feature", Checkout::OtherWorktree),
                branch("main", Checkout::CurrentWorktree),
                branch("release", Checkout::Available),
            ]
        };

        let clean = AppImpl::clean(branches()).unwrap();
        assert_eq!(
            branch_names(&clean),
            ["develop", "release", "feature", "main"]
        );

        let switch = AppImpl::switch(branches()).unwrap();
        assert_eq!(
            branch_names(&switch),
            ["develop", "release", "feature", "main"]
        );

        let rebase = AppImpl::rebase(branches(), None).unwrap();
        assert_eq!(
            branch_names(&rebase),
            ["develop", "feature", "release", "main"]
        );

        let merge = AppImpl::merge(branches()).unwrap();
        assert_eq!(
            branch_names(&merge),
            ["develop", "feature", "release", "main"]
        );
    }

    #[test]
    fn navigation_wraps_without_highlighting_unselectable_branches() {
        let mut app = AppImpl::switch(vec![
            branch("develop", Checkout::Available),
            branch("feature", Checkout::Available),
            branch("main", Checkout::CurrentWorktree),
        ])
        .unwrap();

        assert_eq!(app.position(), 0);
        assert_eq!(app.update(Action::Next), Transition::Continue);
        assert_eq!(app.position(), 1);
        assert_eq!(app.update(Action::Next), Transition::Continue);
        assert_eq!(app.position(), 0);

        assert_eq!(app.update(Action::Previous), Transition::Continue);
        assert_eq!(app.position(), 1);
        assert_eq!(app.update(Action::Previous), Transition::Continue);
        assert_eq!(app.position(), 0);
    }

    #[test]
    fn merge_selects_a_branch_checked_out_in_another_worktree() {
        let mut app = AppImpl::merge(vec![
            branch("main", Checkout::CurrentWorktree),
            branch("feature", Checkout::OtherWorktree),
        ])
        .unwrap();

        let Transition::Complete(branch) = app.update(Action::Confirm(Confirmation::Plain)) else {
            panic!("a branch in another worktree should be a merge source");
        };
        assert_eq!(branch.name(), "feature");
    }

    #[test]
    fn cancellation_exits_without_a_selection() {
        let mut app = AppImpl::switch(vec![branch("feature", Checkout::Available)]).unwrap();

        assert_eq!(app.update(Action::Cancel), Transition::Cancel);
    }

    #[test]
    fn rebase_promotes_selects_and_labels_the_last_target() {
        let mut app = AppImpl::rebase(
            vec![
                branch("release", Checkout::Available),
                branch("feature", Checkout::CurrentWorktree),
                branch("main", Checkout::OtherWorktree),
                branch("develop", Checkout::Available),
            ],
            Some("main".to_string()),
        )
        .unwrap();

        assert_eq!(
            app.branches
                .iter()
                .map(|branch| branch.branch.name())
                .collect::<Vec<_>>(),
            ["main", "develop", "release", "feature"]
        );
        assert_eq!(
            app.branches[0].branch_text(&app.mode),
            "main (other worktree) (last rebased onto)"
        );

        let Transition::Complete(branch) = app.update(Action::Confirm(Confirmation::Plain)) else {
            panic!("the remembered target should be initially selected");
        };
        assert_eq!(branch.name(), "main");
    }

    #[test]
    fn rebase_keeps_an_unselectable_last_target_at_the_bottom() {
        let app = AppImpl::rebase(
            vec![
                branch("develop", Checkout::Available),
                branch("feature", Checkout::Available),
                branch("main", Checkout::CurrentWorktree),
            ],
            Some("main".to_string()),
        )
        .unwrap();

        assert_eq!(branch_names(&app), ["develop", "feature", "main"]);
        assert_eq!(
            app.branches[2].branch_text(&app.mode),
            "main (current) (last rebased onto)"
        );
    }

    #[test]
    fn rebase_ignores_a_stale_last_target() {
        let app = AppImpl::rebase(
            vec![
                branch("main", Checkout::Available),
                branch("feature", Checkout::CurrentWorktree),
                branch("develop", Checkout::Available),
            ],
            Some("deleted".to_string()),
        )
        .unwrap();

        assert_eq!(
            app.branches
                .iter()
                .map(|branch| branch.branch_text(&app.mode))
                .collect::<Vec<_>>(),
            ["develop", "main", "feature (current)"]
        );
    }
}
