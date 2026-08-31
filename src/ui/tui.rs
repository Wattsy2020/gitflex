use std::io;

use ratatui::{
    DefaultTerminal,
    crossterm::{
        event::{
            self, Event, KeyCode, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
            PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
        },
        execute,
        terminal::supports_keyboard_enhancement,
    },
};
use tui_input::{InputRequest, backend::crossterm::to_input_request};

use super::app::{Action, App, Confirmation, Transition};

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

fn action_from_key(key: event::KeyEvent) -> Option<Action> {
    if key.kind != KeyEventKind::Press {
        return None;
    }

    match key.code {
        KeyCode::Esc => Some(Action::Cancel),
        KeyCode::Down => Some(Action::Next),
        KeyCode::Up => Some(Action::Previous),
        KeyCode::Char(' ') => Some(Action::Toggle),
        KeyCode::Enter => Some(Action::Confirm(
            if key
                .modifiers
                .intersects(KeyModifiers::SUPER | KeyModifiers::CONTROL)
            {
                Confirmation::Modified
            } else {
                Confirmation::Plain
            },
        )),
        _ => search_request_from_key(key).map(Action::Search),
    }
}

fn search_request_from_key(key: event::KeyEvent) -> Option<InputRequest> {
    let has_word_modifier = key
        .modifiers
        .intersects(KeyModifiers::ALT | KeyModifiers::META);

    match key.code {
        KeyCode::Left if has_word_modifier => Some(InputRequest::GoToPrevWord),
        KeyCode::Right if has_word_modifier => Some(InputRequest::GoToNextWord),
        KeyCode::Backspace if has_word_modifier => Some(InputRequest::DeletePrevWord),
        KeyCode::Delete if has_word_modifier => Some(InputRequest::DeleteNextWord),
        _ => to_input_request(&Event::Key(key)),
    }
}

fn run_with_terminal<A: App>(
    terminal: &mut DefaultTerminal,
    app: &mut A,
) -> io::Result<Option<A::Output>> {
    let _keyboard_enhancements = KeyboardEnhancementGuard::try_enable();

    loop {
        terminal.draw(|frame| app.draw(frame))?;

        let Event::Key(key) = event::read()? else {
            continue;
        };
        let Some(action) = action_from_key(key) else {
            continue;
        };

        match app.update(action) {
            Transition::Continue => {}
            Transition::Complete(output) => return Ok(Some(output)),
            Transition::Cancel => return Ok(None),
        }
    }
}

pub fn run<A: App>(mut app: A) -> io::Result<Option<A::Output>> {
    ratatui::run(|terminal| run_with_terminal(terminal, &mut app))
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use tui_input::InputRequest;

    use super::{Action, Confirmation, action_from_key};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn modified_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn printable_shortcuts_are_search_text() {
        assert_eq!(
            action_from_key(key(KeyCode::Char('q'))),
            Some(Action::Search(InputRequest::InsertChar('q')))
        );
        assert_eq!(
            action_from_key(key(KeyCode::Char('j'))),
            Some(Action::Search(InputRequest::InsertChar('j')))
        );
        assert_eq!(
            action_from_key(key(KeyCode::Char('k'))),
            Some(Action::Search(InputRequest::InsertChar('k')))
        );
    }

    #[test]
    fn branch_controls_take_precedence_over_search_editing() {
        assert_eq!(action_from_key(key(KeyCode::Esc)), Some(Action::Cancel));
        assert_eq!(action_from_key(key(KeyCode::Up)), Some(Action::Previous));
        assert_eq!(action_from_key(key(KeyCode::Down)), Some(Action::Next));
        assert_eq!(
            action_from_key(key(KeyCode::Char(' '))),
            Some(Action::Toggle)
        );
        assert_eq!(
            action_from_key(key(KeyCode::Enter)),
            Some(Action::Confirm(Confirmation::Plain))
        );
        assert_eq!(
            action_from_key(modified_key(KeyCode::Enter, KeyModifiers::CONTROL)),
            Some(Action::Confirm(Confirmation::Modified))
        );
    }

    #[test]
    fn search_supports_character_and_word_editing() {
        assert_eq!(
            action_from_key(key(KeyCode::Left)),
            Some(Action::Search(InputRequest::GoToPrevChar))
        );
        assert_eq!(
            action_from_key(key(KeyCode::Right)),
            Some(Action::Search(InputRequest::GoToNextChar))
        );
        assert_eq!(
            action_from_key(key(KeyCode::Backspace)),
            Some(Action::Search(InputRequest::DeletePrevChar))
        );
        assert_eq!(
            action_from_key(key(KeyCode::Delete)),
            Some(Action::Search(InputRequest::DeleteNextChar))
        );

        for modifiers in [KeyModifiers::ALT, KeyModifiers::META] {
            assert_eq!(
                action_from_key(modified_key(KeyCode::Left, modifiers)),
                Some(Action::Search(InputRequest::GoToPrevWord))
            );
            assert_eq!(
                action_from_key(modified_key(KeyCode::Right, modifiers)),
                Some(Action::Search(InputRequest::GoToNextWord))
            );
            assert_eq!(
                action_from_key(modified_key(KeyCode::Backspace, modifiers)),
                Some(Action::Search(InputRequest::DeletePrevWord))
            );
            assert_eq!(
                action_from_key(modified_key(KeyCode::Delete, modifiers)),
                Some(Action::Search(InputRequest::DeleteNextWord))
            );
        }
    }
}
