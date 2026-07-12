use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::git::{Checkout, LocalBranch};

struct Branch {
    branch: LocalBranch,
    selected: bool,
}

impl Branch {
    fn new(branch: LocalBranch) -> Self {
        Self {
            selected: branch.is_deletable(),
            branch,
        }
    }

    fn toggle(&mut self) {
        if self.branch.is_deletable() {
            self.selected = !self.selected;
        }
    }

    fn is_selected(&self) -> bool {
        self.selected
    }

    fn git_branch(&self) -> LocalBranch {
        self.branch.clone()
    }

    fn branch_text(&self) -> String {
        let name = self.branch.name();
        match self.branch.checkout() {
            Checkout::Available => {
                format!("{} {}", if self.selected { "[x]" } else { "[ ]" }, name)
            }
            Checkout::CurrentWorktree => format!("{} {} (current)", "[-]", name),
            Checkout::OtherWorktree => format!("{} {} (other worktree)", "[-]", name),
        }
    }

    fn branch_color(&self, is_highlighted: bool) -> Color {
        if !self.branch.is_deletable() {
            if is_highlighted {
                Color::Gray
            } else {
                Color::DarkGray
            }
        } else if self.selected {
            Color::Red
        } else {
            Color::Gray
        }
    }

    fn render_branch<'b>(&self, is_highlighted: bool) -> ListItem<'b> {
        ListItem::new(Line::from(Span::styled(
            self.branch_text(),
            Style::default().fg(self.branch_color(is_highlighted)),
        )))
    }
}

pub struct App {
    branches: Vec<Branch>,
    state: ListState,
}

impl App {
    pub fn new(branches: Vec<LocalBranch>) -> Self {
        let state = ListState::default().with_selected(Some(0));
        Self {
            branches: branches.into_iter().map(Branch::new).collect(),
            state,
        }
    }

    fn get_list_pos(&self) -> usize {
        self.state
            .selected()
            .expect("A list element is always selected")
    }

    pub fn next(&mut self) {
        let i = (self.get_list_pos() + 1).min(self.branches.len() - 1);
        self.state.select(Some(i));
    }

    pub fn prev(&mut self) {
        let i = self.get_list_pos().saturating_sub(1);
        self.state.select(Some(i));
    }

    pub fn toggle(&mut self) {
        let i = self.get_list_pos();
        self.branches[i].toggle();
    }

    pub fn branches_to_delete(&self) -> Vec<LocalBranch> {
        self.branches
            .iter()
            .filter_map(|branch| branch.is_selected().then_some(branch.git_branch()))
            .collect()
    }

    fn render_branches<'b>(&self) -> List<'b> {
        let items: Vec<ListItem> = self
            .branches
            .iter()
            .enumerate()
            .map(|(i, branch)| branch.render_branch(self.get_list_pos() == i))
            .collect();

        List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Branches"))
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .bg(Color::DarkGray),
            )
            .highlight_symbol("> ")
    }

    pub fn draw(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(f.area());

        let branch_list = self.render_branches();
        f.render_stateful_widget(branch_list, chunks[0], &mut self.state);

        let help = Paragraph::new(
            "↑/↓ navigate   space toggle   cmd/ctrl+enter delete selected   q/esc quit",
        )
        .block(Block::default().borders(Borders::ALL).title("Help"));
        f.render_widget(help, chunks[1]);
    }
}
