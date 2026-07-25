use std::ops::Range;

use ratatui::{
    Frame,
    layout::{Position, Rect},
    widgets::{Block, Paragraph},
};
use regex::{Regex, RegexBuilder};
use tui_input::{Input, InputRequest};

#[derive(Debug, Default)]
pub(super) struct Search {
    input: Input,
    matcher: Option<Regex>,
}

impl Search {
    pub(super) fn is_active(&self) -> bool {
        self.matcher.is_some()
    }

    pub(super) fn edit(&mut self, request: InputRequest) -> bool {
        let value_changed = self
            .input
            .handle(request)
            .is_some_and(|change| change.value);

        if value_changed {
            let search_term = self.input.value();
            self.matcher = (!search_term.is_empty()).then(|| {
                RegexBuilder::new(&regex::escape(search_term))
                    .case_insensitive(true)
                    .build()
                    .expect("an escaped search query is always a valid regular expression")
            });
        }

        value_changed
    }

    pub(super) fn matches(&self, branch_name: &str) -> bool {
        self.matcher
            .as_ref()
            .is_some_and(|matcher| matcher.is_match(branch_name))
    }

    pub(super) fn match_ranges(&self, branch_name: &str) -> Vec<Range<usize>> {
        self.matcher
            .iter()
            .flat_map(|matcher| matcher.find_iter(branch_name))
            .map(|matched| matched.range())
            .collect()
    }

    pub(super) fn render(&self, frame: &mut Frame, area: Rect) {
        let block = Block::bordered().title("Search");
        let input_area = block.inner(area);
        let visible_width = usize::from(input_area.width.saturating_sub(1));
        let scroll = self.input.visual_scroll(visible_width);

        frame.render_widget(
            Paragraph::new(self.input.value())
                .scroll((0, u16::try_from(scroll).unwrap_or(u16::MAX)))
                .block(block),
            area,
        );

        let cursor_offset = self.input.visual_cursor().max(scroll) - scroll;
        let cursor_x = input_area
            .x
            .saturating_add(u16::try_from(cursor_offset).unwrap_or(u16::MAX))
            .min(input_area.right().saturating_sub(1));
        frame.set_cursor_position(Position::new(cursor_x, input_area.y));
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend, layout::Position};
    use tui_input::InputRequest;

    use super::Search;

    fn type_query(search: &mut Search, query: &str) {
        query.chars().for_each(|character| {
            assert!(search.edit(InputRequest::InsertChar(character)));
        });
    }

    #[test]
    fn matching_is_case_insensitive_and_literal() {
        let mut search = Search::default();
        type_query(&mut search, "F.A");

        assert!(search.matches("feature/f.a"));
        assert!(search.matches("F.A"));
        assert!(!search.matches("feature/fxa"));
    }

    #[test]
    fn match_ranges_refer_to_the_original_unicode_text() {
        let mut search = Search::default();
        type_query(&mut search, "CAFÉ");

        assert_eq!(search.match_ranges("fix/café-au-café"), [4..9, 13..18]);
    }

    #[test]
    fn empty_search_is_inactive_and_does_not_match() {
        let search = Search::default();

        assert!(!search.is_active());
        assert!(!search.matches("feature"));
        assert!(search.match_ranges("feature").is_empty());
    }

    #[test]
    fn rendering_scrolls_long_queries_and_keeps_the_cursor_inside_the_bar() {
        let mut search = Search::default();
        type_query(&mut search, "abcdefghijk");
        let mut terminal = Terminal::new(TestBackend::new(8, 3)).unwrap();

        terminal
            .draw(|frame| search.render(frame, frame.area()))
            .unwrap();

        let backend = terminal.backend();
        let row = (0..backend.buffer().area.width)
            .map(|x| backend.buffer()[(x, 1)].symbol())
            .fold(String::new(), |mut row, symbol| {
                row.push_str(symbol);
                row
            });
        assert!(row.contains("ghijk"));
        assert_eq!(backend.cursor_position(), Position::new(6, 1));
        assert!(backend.cursor_visible());
    }
}
