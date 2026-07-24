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

use super::app::{Action, App, AppImpl, CleanMode, Confirmation, SingleMode, Transition};
use crate::git::LocalBranch;

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
        KeyCode::Char('q') | KeyCode::Esc => Some(Action::Cancel),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::Next),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::Previous),
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
        _ => None,
    }
}

fn run<A: App>(terminal: &mut DefaultTerminal, app: &mut A) -> io::Result<Option<A::Output>> {
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

pub fn select_many(mut app: AppImpl<CleanMode>) -> io::Result<Option<Vec<LocalBranch>>> {
    ratatui::run(|terminal| run(terminal, &mut app))
}

pub fn select_one(mut app: AppImpl<SingleMode>) -> io::Result<Option<LocalBranch>> {
    ratatui::run(|terminal| run(terminal, &mut app))
}
