//! Maps crossterm key events to core [`Command`]s. Aware of the current
//! [`UiPhase`] since a dialog interprets keys differently than the normal
//! panel view — the mapping itself still performs no state mutation.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use filecommand_core::panel::CursorMove;
use filecommand_core::{Command, UiPhase};

pub fn map_key(key: KeyEvent, phase: &UiPhase, page_size: usize) -> Option<Command> {
    match phase {
        UiPhase::QuitConfirm => match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => Some(Command::ConfirmQuit),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => Some(Command::CancelQuit),
            _ => None,
        },
        _ => match key.code {
            KeyCode::Up => Some(Command::MoveCursor(CursorMove::Up(1))),
            KeyCode::Down => Some(Command::MoveCursor(CursorMove::Down(1))),
            KeyCode::PageUp if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Command::ParentDir),
            KeyCode::PageUp => Some(Command::MoveCursor(CursorMove::Up(page_size))),
            KeyCode::PageDown => Some(Command::MoveCursor(CursorMove::Down(page_size))),
            KeyCode::Home => Some(Command::MoveCursor(CursorMove::Home)),
            KeyCode::End => Some(Command::MoveCursor(CursorMove::End)),
            KeyCode::Tab => Some(Command::ToggleActivePanel),
            KeyCode::Enter => Some(Command::Enter),
            // The command line is display-only in M1 (always empty), so
            // Backspace always means "go to parent directory".
            KeyCode::Backspace => Some(Command::ParentDir),
            KeyCode::F(10) => Some(Command::RequestQuit),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventKind;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent { code, modifiers, kind: KeyEventKind::Press, state: crossterm::event::KeyEventState::NONE }
    }

    #[test]
    fn f10_requests_quit_in_panels_phase() {
        let cmd = map_key(key(KeyCode::F(10), KeyModifiers::NONE), &UiPhase::Panels, 5);
        assert_eq!(cmd, Some(Command::RequestQuit));
    }

    #[test]
    fn quit_dialog_maps_y_and_n() {
        assert_eq!(map_key(key(KeyCode::Char('y'), KeyModifiers::NONE), &UiPhase::QuitConfirm, 5), Some(Command::ConfirmQuit));
        assert_eq!(map_key(key(KeyCode::Char('n'), KeyModifiers::NONE), &UiPhase::QuitConfirm, 5), Some(Command::CancelQuit));
        assert_eq!(map_key(key(KeyCode::Esc, KeyModifiers::NONE), &UiPhase::QuitConfirm, 5), Some(Command::CancelQuit));
    }

    #[test]
    fn ctrl_pgup_is_parent_dir_plain_pgup_is_page_move() {
        assert_eq!(map_key(key(KeyCode::PageUp, KeyModifiers::CONTROL), &UiPhase::Panels, 7), Some(Command::ParentDir));
        assert_eq!(map_key(key(KeyCode::PageUp, KeyModifiers::NONE), &UiPhase::Panels, 7), Some(Command::MoveCursor(CursorMove::Up(7))));
    }

    #[test]
    fn tab_and_enter_map_to_expected_commands() {
        assert_eq!(map_key(key(KeyCode::Tab, KeyModifiers::NONE), &UiPhase::Panels, 5), Some(Command::ToggleActivePanel));
        assert_eq!(map_key(key(KeyCode::Enter, KeyModifiers::NONE), &UiPhase::Panels, 5), Some(Command::Enter));
    }

    #[test]
    fn backspace_is_parent_dir() {
        assert_eq!(map_key(key(KeyCode::Backspace, KeyModifiers::NONE), &UiPhase::Panels, 5), Some(Command::ParentDir));
    }
}
