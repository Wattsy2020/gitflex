use std::num::NonZeroUsize;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::git::{Checkout, CleanBranch, LocalBranch, MergeHistory, SwitchHistory};

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

    fn marker(&self, branch: &Branch) -> &'static str;
    fn annotation(&self, branch: &LocalBranch) -> Option<&'static str>;
    fn confirms(&self, confirmation: Confirmation) -> bool;
    fn output(&self, branches: &[Branch], position: usize) -> Option<Self::Output>;
    fn help(&self) -> &'static str;
}

impl Mode for CleanMode {
    type Output = Vec<LocalBranch>;

    fn marker(&self, branch: &Branch) -> &'static str {
        if !branch.selectable {
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

impl SingleMode {
    fn is_selectable(&self, branch: &LocalBranch) -> bool {
        match &self.operation {
            SingleOperation::Switch => branch.is_switchable(),
            SingleOperation::Rebase { .. } => branch.is_rebase_target(),
            SingleOperation::Merge => branch.is_merge_source(),
        }
    }
}

impl Mode for SingleMode {
    type Output = LocalBranch;

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

    fn confirms(&self, _confirmation: Confirmation) -> bool {
        true
    }

    fn output(&self, branches: &[Branch], position: usize) -> Option<Self::Output> {
        branches
            .get(position)
            .filter(|branch| branch.selectable)
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
    selectable: bool,
    selected: bool,
}

impl Branch {
    fn new(branch: LocalBranch, selectable: bool, selected: bool) -> Self {
        Self {
            branch,
            selectable,
            selected,
        }
    }

    fn clean(branch: CleanBranch) -> Self {
        let selectable = branch.branch().is_deletable() && !branch.is_trunk();
        let selected = selectable && (branch.is_merged() || branch.is_authored_by_other());
        Self::new(branch.into_branch(), selectable, selected)
    }

    pub fn name(&self) -> &str {
        self.branch.name()
    }

    fn toggle(&mut self) {
        if self.selectable {
            self.selected = !self.selected;
        }
    }

    fn branch_text<M: Mode>(&self, mode: &M) -> String {
        let marker = mode.marker(self);
        let name = self.name();
        let annotation = mode.annotation(&self.branch).unwrap_or_default();
        let status = match self.branch.checkout() {
            Checkout::Available => "",
            Checkout::CurrentWorktree => " (current)",
            Checkout::OtherWorktree => " (other worktree)",
        };

        format!("{marker}{name}{status}{annotation}")
    }

    fn colour(&self, highlighted: bool) -> Color {
        if self.selected {
            SELECTED_COLOUR
        } else if highlighted {
            HIGHLIGHTED_COLOUR
        } else if self.selectable {
            SELECTABLE_COLOUR
        } else {
            UNSELECTABLE_COLOUR
        }
    }

    pub fn render<M: Mode>(&self, mode: &M, highlighted: bool) -> ListItem<'static> {
        ListItem::new(Line::from(Span::styled(
            self.branch_text(mode),
            Style::default().fg(self.colour(highlighted)),
        )))
    }
}

fn rank_branch(branch: &Branch) -> u32 {
    match (branch.selectable, branch.selected) {
        (true, true) => 0,
        (true, false) => 1,
        (false, _) => 2,
    }
}

pub struct AppImpl<M> {
    branches: Vec<Branch>,
    mode: M,
    selectable_count: NonZeroUsize,
    state: ListState,
}

impl AppImpl<CleanMode> {
    pub fn clean(branches: Vec<CleanBranch>) -> Option<Self> {
        let mut branches = branches.into_iter().map(Branch::clean).collect::<Vec<_>>();
        branches.sort_unstable_by(|left, right| {
            rank_branch(left)
                .cmp(&rank_branch(right))
                .then_with(|| left.name().cmp(right.name()))
        });
        Self::from_branches(branches, CleanMode)
    }
}

impl AppImpl<SingleMode> {
    pub fn switch(mut branches: Vec<LocalBranch>, history: &SwitchHistory) -> Option<Self> {
        branches.sort_by(|left, right| {
            history
                .rank(right.name())
                .cmp(&history.rank(left.name()))
                .then_with(|| left.name().cmp(right.name()))
        });

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

    pub fn merge(
        mut branches: Vec<LocalBranch>,
        destination: &str,
        history: &MergeHistory,
    ) -> Option<Self> {
        branches.sort_by(|left, right| {
            history
                .rank(destination, right.name())
                .cmp(&history.rank(destination, left.name()))
                .then_with(|| left.name().cmp(right.name()))
        });

        Self::new(
            branches,
            SingleMode {
                operation: SingleOperation::Merge,
            },
        )
    }

    fn new(mut branches: Vec<LocalBranch>, mode: SingleMode) -> Option<Self> {
        branches.sort_by_key(|branch| !mode.is_selectable(branch));
        let branches = branches
            .into_iter()
            .map(|branch| {
                let selectable = mode.is_selectable(&branch);
                Branch::new(branch, selectable, false)
            })
            .collect();
        Self::from_branches(branches, mode)
    }
}

impl<M> AppImpl<M> {
    fn from_branches(branches: Vec<Branch>, mode: M) -> Option<Self> {
        let selectable_count = NonZeroUsize::new(
            branches
                .iter()
                .take_while(|branch| branch.selectable)
                .count(),
        )?;
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

    fn toggle(&mut self) {
        let position = self.position();
        self.branches[position].toggle();
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
    use crate::git::{Checkout, CleanBranch, LocalBranch, MergeHistory, SwitchHistory};

    use super::{Action, AppImpl, Branch, Confirmation, Transition};

    fn branch(name: &str, checkout: Checkout) -> LocalBranch {
        LocalBranch::for_test(name, checkout)
    }

    fn selected(name: &str) -> CleanBranch {
        CleanBranch::for_test(branch(name, Checkout::Available), false, true, false)
    }

    fn unselected(name: &str) -> CleanBranch {
        CleanBranch::for_test(branch(name, Checkout::Available), false, false, false)
    }

    fn unselectable(name: &str, checkout: Checkout) -> CleanBranch {
        CleanBranch::for_test(branch(name, checkout), name == "main", false, false)
    }

    fn authored_by_other(name: &str) -> CleanBranch {
        CleanBranch::for_test(branch(name, Checkout::Available), false, false, true)
    }

    fn branch_names<M>(app: &AppImpl<M>) -> Vec<&str> {
        app.branches.iter().map(Branch::name).collect()
    }

    #[test]
    fn clean_confirms_only_initially_selected_branches() {
        let mut app = AppImpl::clean(vec![
            selected("feature-a"),
            authored_by_other("review"),
            unselected("feature-b"),
            unselectable("main", Checkout::CurrentWorktree),
        ])
        .unwrap();

        let Transition::Complete(branches) = app.update(Action::Confirm(Confirmation::Modified))
        else {
            panic!("a modified confirmation should complete clean selection");
        };
        assert_eq!(
            branches.iter().map(LocalBranch::name).collect::<Vec<_>>(),
            ["feature-a", "review"]
        );
    }

    #[test]
    fn clean_requires_modified_confirmation_and_toggle_changes_selection() {
        let mut app = AppImpl::clean(vec![selected("feature-a"), selected("feature-b")]).unwrap();

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

        let clean = AppImpl::clean(vec![
            unselectable("main", Checkout::Available),
            unselected("release"),
            unselectable("feature", Checkout::OtherWorktree),
            selected("develop"),
        ])
        .unwrap();
        assert_eq!(
            branch_names(&clean),
            ["develop", "release", "feature", "main"]
        );

        let switch = AppImpl::switch(branches(), &SwitchHistory::default()).unwrap();
        assert_eq!(
            branch_names(&switch),
            ["develop", "release", "feature", "main"]
        );

        let rebase = AppImpl::rebase(branches(), None).unwrap();
        assert_eq!(
            branch_names(&rebase),
            ["develop", "feature", "release", "main"]
        );

        let merge = AppImpl::merge(branches(), "main", &MergeHistory::default()).unwrap();
        assert_eq!(
            branch_names(&merge),
            ["develop", "feature", "release", "main"]
        );
    }

    #[test]
    fn switch_ranks_each_checkout_group_by_history_then_name() {
        let history = SwitchHistory::for_test(["review", "main", "develop", "deleted", "feature"]);
        let app = AppImpl::switch(
            vec![
                branch("zeta", Checkout::Available),
                branch("feature", Checkout::OtherWorktree),
                branch("alpha", Checkout::Available),
                branch("main", Checkout::CurrentWorktree),
                branch("develop", Checkout::Available),
                branch("review", Checkout::Available),
            ],
            &history,
        )
        .unwrap();

        assert_eq!(
            branch_names(&app),
            ["review", "develop", "alpha", "zeta", "main", "feature"]
        );
        assert_eq!(app.position(), 0);
    }

    #[test]
    fn merge_ranks_sources_for_the_current_destination_then_by_name() {
        let history = MergeHistory::for_test([
            ("main", "review"),
            ("release", "zeta"),
            ("main", "develop"),
            ("main", "deleted"),
            ("main", "feature"),
        ]);
        let app = AppImpl::merge(
            vec![
                branch("zeta", Checkout::Available),
                branch("feature", Checkout::OtherWorktree),
                branch("alpha", Checkout::Available),
                branch("main", Checkout::CurrentWorktree),
                branch("develop", Checkout::Available),
                branch("review", Checkout::Available),
            ],
            "main",
            &history,
        )
        .unwrap();

        assert_eq!(
            branch_names(&app),
            ["review", "develop", "feature", "alpha", "zeta", "main"]
        );
        assert_eq!(app.position(), 0);
    }

    #[test]
    fn clean_sorts_each_group_alphabetically_and_never_reorders_after_toggles() {
        let mut app = AppImpl::clean(vec![
            unselectable("main", Checkout::CurrentWorktree),
            unselected("gamma"),
            selected("zeta"),
            unselected("beta"),
            selected("alpha"),
        ])
        .unwrap();
        let initial_order = ["alpha", "zeta", "beta", "gamma", "main"];
        assert_eq!(branch_names(&app), initial_order);

        assert_eq!(app.update(Action::Toggle), Transition::Continue);
        assert_eq!(branch_names(&app), initial_order);
        assert_eq!(app.update(Action::Next), Transition::Continue);
        assert_eq!(app.update(Action::Toggle), Transition::Continue);
        assert_eq!(branch_names(&app), initial_order);
        assert_eq!(app.update(Action::Next), Transition::Continue);
        assert_eq!(app.update(Action::Toggle), Transition::Continue);
        assert_eq!(branch_names(&app), initial_order);

        let Transition::Complete(branches) = app.update(Action::Confirm(Confirmation::Modified))
        else {
            panic!("a modified confirmation should complete clean selection");
        };
        assert_eq!(
            branches.iter().map(LocalBranch::name).collect::<Vec<_>>(),
            ["beta"]
        );
    }

    #[test]
    fn clean_navigation_includes_unselected_but_skips_unselectable_branches() {
        let mut app = AppImpl::clean(vec![
            unselectable("main", Checkout::CurrentWorktree),
            unselected("feature-b"),
            selected("feature-a"),
        ])
        .unwrap();

        assert_eq!(app.position(), 0);
        assert_eq!(app.update(Action::Next), Transition::Continue);
        assert_eq!(app.position(), 1);
        assert_eq!(app.update(Action::Next), Transition::Continue);
        assert_eq!(app.position(), 0);
        assert_eq!(app.update(Action::Previous), Transition::Continue);
        assert_eq!(app.position(), 1);
    }

    #[test]
    fn navigation_wraps_without_highlighting_unselectable_branches() {
        let mut app = AppImpl::switch(
            vec![
                branch("develop", Checkout::Available),
                branch("feature", Checkout::Available),
                branch("main", Checkout::CurrentWorktree),
            ],
            &SwitchHistory::default(),
        )
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
        let mut app = AppImpl::merge(
            vec![
                branch("main", Checkout::CurrentWorktree),
                branch("feature", Checkout::OtherWorktree),
            ],
            "main",
            &MergeHistory::default(),
        )
        .unwrap();

        let Transition::Complete(branch) = app.update(Action::Confirm(Confirmation::Plain)) else {
            panic!("a branch in another worktree should be a merge source");
        };
        assert_eq!(branch.name(), "feature");
    }

    #[test]
    fn cancellation_exits_without_a_selection() {
        let mut app = AppImpl::switch(
            vec![branch("feature", Checkout::Available)],
            &SwitchHistory::default(),
        )
        .unwrap();

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
            branch_names(&app),
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
