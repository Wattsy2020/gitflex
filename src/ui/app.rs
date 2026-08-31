use std::num::NonZeroUsize;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use tui_input::InputRequest;

use crate::{
    git::{Checkout, CleanBranch, LocalBranch},
    history::{MergeHistory, SwitchHistory},
    ui::search::Search,
};

const SELECTED_COLOUR: Color = Color::Red;
const SELECTABLE_COLOUR: Color = Color::Black;
const UNSELECTABLE_COLOUR: Color = Color::Gray;
const HIGHLIGHTED_COLOUR: Color = Color::White;
const MATCHED_COLOUR: Color = Color::Yellow;
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
    Search(InputRequest),
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
    Delete,
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
        "type search   ↑/↓ navigate   space toggle   cmd/ctrl+enter delete selected   esc quit"
    }
}

impl SingleMode {
    fn is_selectable(&self, branch: &LocalBranch) -> bool {
        match &self.operation {
            SingleOperation::Delete => branch.is_deletable(),
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
            SingleOperation::Delete => {
                "type search   ↑/↓ navigate   enter delete branch   esc quit"
            }
            SingleOperation::Switch => {
                "type search   ↑/↓ navigate   enter switch to branch   esc quit"
            }
            SingleOperation::Rebase { .. } => {
                "type search   ↑/↓ navigate   enter rebase onto branch   esc quit"
            }
            SingleOperation::Merge => "type search   ↑/↓ navigate   enter merge branch   esc quit",
        }
    }
}

struct Branch {
    branch: LocalBranch,
    selectable: bool,
    selected: bool,
    original_rank: usize,
}

impl Branch {
    fn new(branch: LocalBranch, selectable: bool, selected: bool, original_rank: usize) -> Self {
        Self {
            branch,
            selectable,
            selected,
            original_rank,
        }
    }

    fn clean(branch: CleanBranch) -> Self {
        let selectable = branch.branch().is_deletable() && !branch.is_trunk();
        let selected = selectable && (branch.is_merged() || branch.is_authored_by_other());
        Self::new(branch.into_branch(), selectable, selected, 0)
    }

    pub fn name(&self) -> &str {
        self.branch.name()
    }

    fn toggle(&mut self) {
        if self.selectable {
            self.selected = !self.selected;
        }
    }

    fn branch_annotation<M: Mode>(&self, mode: &M) -> String {
        let status = match self.branch.checkout() {
            Checkout::Available => "",
            Checkout::CurrentWorktree => " (current)",
            Checkout::OtherWorktree => " (other worktree)",
        };
        let annotation = mode.annotation(&self.branch).unwrap_or_default();
        format!("{status}{annotation}")
    }

    #[cfg(test)]
    fn branch_text<M: Mode>(&self, mode: &M) -> String {
        let marker = mode.marker(self);
        let name = self.name();
        let annotation = self.branch_annotation(mode);
        format!("{marker}{name}{annotation}")
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

    pub fn render<M: Mode>(
        &self,
        mode: &M,
        highlighted: bool,
        matched_ranges: &[std::ops::Range<usize>],
    ) -> ListItem<'static> {
        let style = Style::default().fg(self.colour(highlighted));
        let matched_style = style
            .fg(MATCHED_COLOUR)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);

        let name = self.name();
        let mut spans = vec![Span::styled(mode.marker(self), style)];
        let mut position = 0;

        // Add highlighted and non highlighted parts of the branch name
        let mut add_styled =
            |content: String, style: Style| spans.push(Span::styled(content, style));

        for matched_range in matched_ranges {
            if position < matched_range.start {
                add_styled(name[position..matched_range.start].to_owned(), style)
            }
            add_styled(name[matched_range.clone()].to_owned(), matched_style);
            position = matched_range.end;
        }

        if position < name.len() {
            add_styled(name[position..].to_owned(), style);
        }
        add_styled(self.branch_annotation(mode), style);

        ListItem::new(Line::from(spans))
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
    search: Search,
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
        for (original_rank, branch) in branches.iter_mut().enumerate() {
            branch.original_rank = original_rank;
        }
        Self::from_branches(branches, CleanMode, Search::default())
    }
}

impl AppImpl<SingleMode> {
    pub fn delete(mut branches: Vec<LocalBranch>, initial_search: Option<String>) -> Option<Self> {
        branches.sort_unstable_by(|left, right| left.name().cmp(right.name()));

        Self::new(
            branches,
            SingleMode {
                operation: SingleOperation::Delete,
            },
            initial_search,
        )
    }

    pub fn switch(
        mut branches: Vec<LocalBranch>,
        history: &SwitchHistory,
        initial_search: Option<String>,
    ) -> Option<Self> {
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
            initial_search,
        )
    }

    pub fn rebase(
        mut branches: Vec<LocalBranch>,
        last_target: Option<String>,
        initial_search: Option<String>,
    ) -> Option<Self> {
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
            initial_search,
        )
    }

    pub fn merge(
        mut branches: Vec<LocalBranch>,
        destination: &str,
        history: &MergeHistory,
        initial_search: Option<String>,
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
            initial_search,
        )
    }

    fn new(
        mut branches: Vec<LocalBranch>,
        mode: SingleMode,
        initial_search: Option<String>,
    ) -> Option<Self> {
        branches.sort_by_key(|branch| !mode.is_selectable(branch));
        let branches = branches
            .into_iter()
            .enumerate()
            .map(|(original_rank, branch)| {
                let selectable = mode.is_selectable(&branch);
                Branch::new(branch, selectable, false, original_rank)
            })
            .collect();
        Self::from_branches(
            branches,
            mode,
            Search::new(initial_search.unwrap_or_default()),
        )
    }
}

impl<M> AppImpl<M> {
    fn from_branches(branches: Vec<Branch>, mode: M, search: Search) -> Option<Self> {
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
            search,
            state: ListState::default(),
        };
        app.sort_for_search();
        app.select_first_branch();
        Some(app)
    }

    fn position(&self) -> usize {
        self.state
            .selected()
            .expect("a list element is always selected")
    }

    /// Find the first position which refers to a selectable branch
    fn find_position(&self, mut positions: impl Iterator<Item = usize>) -> Option<usize> {
        positions.find(|&position| self.branches[position].selectable)
    }

    fn next(&mut self) {
        if self.selectable_count.get() == 1 {
            return;
        }

        // find the next selectable branch, in the current order
        let position = self.position();
        let positions =
            (1..=self.branches.len()).map(|offset| (position + offset) % self.branches.len());
        let new_position = self
            .find_position(positions)
            .expect("at least one branch is selectable");
        self.state.select(Some(new_position));
    }

    fn previous(&mut self) {
        if self.selectable_count.get() == 1 {
            return;
        }

        // find the previous selectable branch, in the current order
        let position = self.position();
        let positions = (1..=self.branches.len())
            .map(|offset| (position + self.branches.len() - offset) % self.branches.len());
        let new_position = self
            .find_position(positions)
            .expect("at least one branch is selectable");
        self.state.select(Some(new_position));
    }

    fn select_first_branch(&mut self) {
        let position = self
            .branches
            .iter()
            .position(|branch| branch.selectable)
            .expect("we enforce one branch is selectable");

        // reset scroll position, then select the first item
        *self.state.offset_mut() = 0;
        self.state.select(Some(position));
    }

    fn toggle(&mut self) {
        let position = self.position();
        self.branches[position].toggle();
    }

    fn update_search(&mut self, request: InputRequest) {
        if !self.search.edit(request) {
            return;
        }

        self.sort_for_search();
        self.select_first_branch();
    }

    fn sort_for_search(&mut self) {
        let search = &self.search;
        self.branches
            .sort_unstable_by_key(|branch| (!search.matches(branch.name()), branch.original_rank));
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
            Action::Search(request) => self.update_search(request),
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
            .map(|(position, branch)| {
                branch.render(
                    &self.mode,
                    self.position() == position,
                    &self.search.match_ranges(branch.name()),
                )
            })
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
        let constraints = if self.search.is_active() {
            vec![
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(3),
            ]
        } else {
            vec![Constraint::Min(1), Constraint::Length(3)]
        };
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(frame.area());
        let (branches_area, help_area) = if self.search.is_active() {
            self.search.render(frame, chunks[0]);
            (chunks[1], chunks[2])
        } else {
            (chunks[0], chunks[1])
        };

        frame.render_stateful_widget(self.render_branches(), branches_area, &mut self.state);
        frame.render_widget(
            Paragraph::new(self.mode.help())
                .block(Block::default().borders(Borders::ALL).title("Help")),
            help_area,
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
    use ratatui::{
        Terminal,
        backend::TestBackend,
        buffer::Buffer,
        style::{Color, Modifier},
    };
    use tui_input::InputRequest;

    use crate::{
        git::{Checkout, CleanBranch, LocalBranch},
        history::{MergeHistory, SwitchHistory},
    };

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

    fn highlighted_branch_name<M>(app: &AppImpl<M>) -> &str {
        app.branches[app.position()].name()
    }

    fn type_query<M>(app: &mut AppImpl<M>, query: &str)
    where
        M: super::Mode,
    {
        query.chars().for_each(|character| {
            assert!(matches!(
                app.update(Action::Search(InputRequest::InsertChar(character))),
                Transition::Continue
            ));
        });
    }

    fn buffer_row(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .fold(String::new(), |mut row, symbol| {
                row.push_str(symbol);
                row
            })
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

        let delete = AppImpl::delete(branches(), None).unwrap();
        assert_eq!(
            branch_names(&delete),
            ["develop", "release", "feature", "main"]
        );

        let switch = AppImpl::switch(branches(), &SwitchHistory::default(), None).unwrap();
        assert_eq!(
            branch_names(&switch),
            ["develop", "release", "feature", "main"]
        );

        let rebase = AppImpl::rebase(branches(), None, None).unwrap();
        assert_eq!(
            branch_names(&rebase),
            ["develop", "feature", "release", "main"]
        );

        let merge = AppImpl::merge(branches(), "main", &MergeHistory::default(), None).unwrap();
        assert_eq!(
            branch_names(&merge),
            ["develop", "feature", "release", "main"]
        );
    }

    #[test]
    fn delete_orders_branches_by_name_and_selects_a_matching_deletable_branch() {
        let mut app = AppImpl::delete(
            vec![
                branch("zeta", Checkout::Available),
                branch("feature-review", Checkout::OtherWorktree),
                branch("main", Checkout::CurrentWorktree),
                branch("feature", Checkout::Available),
                branch("alpha", Checkout::Available),
            ],
            Some("feature".to_owned()),
        )
        .unwrap();

        assert_eq!(
            branch_names(&app),
            ["feature", "feature-review", "alpha", "zeta", "main"]
        );
        assert_eq!(highlighted_branch_name(&app), "feature");

        let Transition::Complete(branch) = app.update(Action::Confirm(Confirmation::Plain)) else {
            panic!("the matching deletable branch should be selected");
        };
        assert_eq!(branch.name(), "feature");
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
            None,
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
            None,
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
    fn search_stably_promotes_case_insensitive_matches_from_the_canonical_order() {
        let mut app = AppImpl::rebase(
            vec![
                branch("alpha", Checkout::Available),
                branch("feature-one", Checkout::Available),
                branch("beta-feature", Checkout::Available),
                branch("FEATURE-two", Checkout::Available),
                branch("main", Checkout::CurrentWorktree),
            ],
            None,
            None,
        )
        .unwrap();
        let canonical_order = [
            "alpha",
            "feature-one",
            "beta-feature",
            "FEATURE-two",
            "main",
        ];
        assert_eq!(branch_names(&app), canonical_order);

        *app.state.offset_mut() = 3;
        type_query(&mut app, "feature");
        assert_eq!(app.state.offset(), 0);
        assert_eq!(
            branch_names(&app),
            [
                "feature-one",
                "beta-feature",
                "FEATURE-two",
                "alpha",
                "main"
            ]
        );
        assert_eq!(highlighted_branch_name(&app), "feature-one");

        type_query(&mut app, "-t");
        assert_eq!(
            branch_names(&app),
            [
                "FEATURE-two",
                "alpha",
                "feature-one",
                "beta-feature",
                "main"
            ]
        );
        assert_eq!(highlighted_branch_name(&app), "FEATURE-two");

        assert_eq!(
            app.update(Action::Search(InputRequest::DeleteLine)),
            Transition::Continue
        );
        assert_eq!(branch_names(&app), canonical_order);
        assert_eq!(highlighted_branch_name(&app), "alpha");
    }

    #[test]
    fn search_falls_back_to_the_first_selectable_branch_when_matches_are_unselectable() {
        let mut app = AppImpl::rebase(
            vec![
                branch("develop", Checkout::Available),
                branch("feature", Checkout::CurrentWorktree),
                branch("main", Checkout::Available),
            ],
            None,
            None,
        )
        .unwrap();

        type_query(&mut app, "FEATURE");

        assert_eq!(branch_names(&app), ["feature", "develop", "main"]);
        assert_eq!(app.position(), 1);
        assert_eq!(highlighted_branch_name(&app), "develop");
    }

    #[test]
    fn navigation_and_confirmation_follow_the_search_order() {
        let mut app = AppImpl::rebase(
            vec![
                branch("alpha", Checkout::Available),
                branch("release-one", Checkout::Available),
                branch("beta-release", Checkout::Available),
                branch("main", Checkout::CurrentWorktree),
            ],
            None,
            None,
        )
        .unwrap();
        type_query(&mut app, "release");

        assert_eq!(highlighted_branch_name(&app), "release-one");
        assert_eq!(app.update(Action::Next), Transition::Continue);
        assert_eq!(highlighted_branch_name(&app), "beta-release");

        let Transition::Complete(branch) = app.update(Action::Confirm(Confirmation::Plain)) else {
            panic!("the highlighted search result should be selectable");
        };
        assert_eq!(branch.name(), "beta-release");
    }

    #[test]
    fn initial_search_uses_the_same_ordering_and_selectability_as_typed_search() {
        let app = AppImpl::rebase(
            vec![
                branch("develop", Checkout::Available),
                branch("feature", Checkout::CurrentWorktree),
                branch("feature-review", Checkout::Available),
                branch("main", Checkout::Available),
            ],
            None,
            Some("FEATURE".to_owned()),
        )
        .unwrap();

        assert_eq!(
            branch_names(&app),
            ["feature-review", "feature", "develop", "main"]
        );
        assert_eq!(highlighted_branch_name(&app), "feature-review");
        assert!(app.search.is_active());
        assert!(app.search.matches("feature-review"));
    }

    #[test]
    fn unmatched_initial_search_preserves_canonical_order() {
        let history = SwitchHistory::for_test(["review", "develop"]);
        let app = AppImpl::switch(
            vec![
                branch("alpha", Checkout::Available),
                branch("develop", Checkout::Available),
                branch("review", Checkout::Available),
                branch("main", Checkout::CurrentWorktree),
            ],
            &history,
            Some("missing".to_owned()),
        )
        .unwrap();

        assert_eq!(branch_names(&app), ["review", "develop", "alpha", "main"]);
        assert_eq!(highlighted_branch_name(&app), "review");
    }

    #[test]
    fn clean_toggles_the_highlighted_search_result() {
        let mut app = AppImpl::clean(vec![
            selected("alpha"),
            unselected("feature"),
            selected("release"),
        ])
        .unwrap();
        type_query(&mut app, "feature");

        assert_eq!(highlighted_branch_name(&app), "feature");
        assert_eq!(app.update(Action::Toggle), Transition::Continue);

        let Transition::Complete(branches) = app.update(Action::Confirm(Confirmation::Modified))
        else {
            panic!("the clean selection should complete");
        };
        let mut branch_names = branches.iter().map(LocalBranch::name).collect::<Vec<_>>();
        branch_names.sort_unstable();
        assert_eq!(branch_names, ["alpha", "feature", "release"]);
    }

    #[test]
    fn rendering_shows_the_search_bar_cursor_and_all_matched_substrings() {
        let mut app = AppImpl::rebase(
            vec![
                branch("feature/FEATURE", Checkout::Available),
                branch("main", Checkout::CurrentWorktree),
            ],
            None,
            None,
        )
        .unwrap();
        type_query(&mut app, "feat");

        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let backend = terminal.backend();
        let buffer = backend.buffer();

        assert!(buffer_row(buffer, 0).contains("Search"));
        assert!(buffer_row(buffer, 1).contains("feat"));
        assert!(buffer_row(buffer, 3).contains("Branches"));
        assert!(buffer_row(buffer, 4).contains("feature/FEATURE"));
        assert_eq!(
            backend.cursor_position(),
            ratatui::layout::Position::new(5, 1)
        );
        assert!(backend.cursor_visible());

        for x in (3..7).chain(11..15) {
            let cell = &buffer[(x, 4)];
            assert_eq!(cell.fg, Color::Yellow);
            assert!(cell.modifier.contains(Modifier::BOLD));
            assert!(cell.modifier.contains(Modifier::UNDERLINED));
        }
        assert_ne!(buffer[(7, 4)].fg, Color::Yellow);
    }

    #[test]
    fn rendering_an_initial_search_places_the_cursor_after_its_text() {
        let mut app = AppImpl::merge(
            vec![
                branch("feature", Checkout::Available),
                branch("main", Checkout::CurrentWorktree),
            ],
            "main",
            &MergeHistory::default(),
            Some("feat".to_owned()),
        )
        .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(30, 10)).unwrap();

        terminal.draw(|frame| app.draw(frame)).unwrap();

        let backend = terminal.backend();
        assert!(buffer_row(backend.buffer(), 1).contains("feat"));
        assert_eq!(
            backend.cursor_position(),
            ratatui::layout::Position::new(5, 1)
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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
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
