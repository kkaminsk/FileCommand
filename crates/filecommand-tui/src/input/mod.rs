//! Maps crossterm key events to core [`Command`]s. Aware of the current
//! [`UiPhase`] since a dialog interprets keys differently than the normal
//! panel view — the mapping itself still performs no state mutation.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use filecommand_core::fs_ops::dialog::{FileOpSetup, RunningDialog};
use filecommand_core::fs_ops::{ConflictChoice, ErrorChoice};
use filecommand_core::panel::CursorMove;
use filecommand_core::{Command, UiPhase};

pub fn map_key(key: KeyEvent, phase: &UiPhase, page_size: usize) -> Option<Command> {
    match phase {
        UiPhase::QuitConfirm => match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => Some(Command::ConfirmQuit),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => Some(Command::CancelQuit),
            _ => None,
        },
        UiPhase::FileOpSetup(setup) => map_file_op_setup_key(key, setup),
        UiPhase::FileOpRunning { dialog, .. } => map_file_op_running_key(key, dialog),
        UiPhase::FileOpSummary(_) => Some(Command::FileOpConfirm),
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
            KeyCode::Char('r') | KeyCode::Char('R') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Command::RetryListing),
            KeyCode::Insert => Some(Command::ToggleSelectAtCursor),
            KeyCode::Char('+') => Some(Command::GroupSelectAll),
            KeyCode::Char('-') => Some(Command::GroupDeselectAll),
            KeyCode::Char('*') => Some(Command::InvertSelection),
            KeyCode::F(5) => Some(Command::RequestCopy),
            KeyCode::F(6) => Some(Command::RequestMove),
            KeyCode::F(7) => Some(Command::RequestMkdir),
            KeyCode::F(8) => Some(Command::RequestDelete),
            KeyCode::F(10) => Some(Command::RequestQuit),
            _ => None,
        },
    }
}

fn map_file_op_setup_key(key: KeyEvent, setup: &FileOpSetup) -> Option<Command> {
    match setup {
        FileOpSetup::DeleteConfirm { .. } => match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => Some(Command::FileOpConfirm),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => Some(Command::FileOpCancel),
            _ => None,
        },
        FileOpSetup::DestinationInput { .. } => match key.code {
            KeyCode::Enter => Some(Command::FileOpConfirm),
            KeyCode::Esc => Some(Command::FileOpCancel),
            KeyCode::Backspace => Some(Command::FileOpInputBackspace),
            KeyCode::Char(c) => Some(Command::FileOpInputChar(c)),
            _ => None,
        },
    }
}

fn map_file_op_running_key(key: KeyEvent, dialog: &RunningDialog) -> Option<Command> {
    match dialog {
        RunningDialog::Progress { .. } => match key.code {
            KeyCode::Esc | KeyCode::Char('c') | KeyCode::Char('C') => Some(Command::FileOpCancelJob),
            _ => None,
        },
        RunningDialog::Conflict { rename_input: Some(_), .. } => match key.code {
            KeyCode::Enter => Some(Command::FileOpConfirm),
            KeyCode::Esc => Some(Command::FileOpCancel),
            KeyCode::Backspace => Some(Command::FileOpInputBackspace),
            KeyCode::Char(c) => Some(Command::FileOpInputChar(c)),
            _ => None,
        },
        RunningDialog::Conflict { rename_input: None, .. } => match key.code {
            KeyCode::Char('o') | KeyCode::Char('O') => Some(Command::FileOpConflictChoice(ConflictChoice::Overwrite)),
            KeyCode::Char('w') | KeyCode::Char('W') => Some(Command::FileOpConflictChoice(ConflictChoice::OverwriteAll)),
            KeyCode::Char('s') | KeyCode::Char('S') => Some(Command::FileOpConflictChoice(ConflictChoice::Skip)),
            KeyCode::Char('a') | KeyCode::Char('A') => Some(Command::FileOpConflictChoice(ConflictChoice::SkipAll)),
            KeyCode::Char('r') | KeyCode::Char('R') => Some(Command::FileOpBeginRename),
            _ => None,
        },
        RunningDialog::Error { .. } => match key.code {
            KeyCode::Char('r') | KeyCode::Char('R') => Some(Command::FileOpErrorChoice(ErrorChoice::Retry)),
            KeyCode::Char('s') | KeyCode::Char('S') => Some(Command::FileOpErrorChoice(ErrorChoice::Skip)),
            KeyCode::Char('a') | KeyCode::Char('A') => Some(Command::FileOpErrorChoice(ErrorChoice::SkipAll)),
            KeyCode::Char('b') | KeyCode::Char('B') | KeyCode::Esc => Some(Command::FileOpErrorChoice(ErrorChoice::Abort)),
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

    #[test]
    fn f5_through_f8_map_to_file_op_requests() {
        assert_eq!(map_key(key(KeyCode::F(5), KeyModifiers::NONE), &UiPhase::Panels, 5), Some(Command::RequestCopy));
        assert_eq!(map_key(key(KeyCode::F(6), KeyModifiers::NONE), &UiPhase::Panels, 5), Some(Command::RequestMove));
        assert_eq!(map_key(key(KeyCode::F(7), KeyModifiers::NONE), &UiPhase::Panels, 5), Some(Command::RequestMkdir));
        assert_eq!(map_key(key(KeyCode::F(8), KeyModifiers::NONE), &UiPhase::Panels, 5), Some(Command::RequestDelete));
    }

    #[test]
    fn selection_keys_map_in_panels_phase() {
        assert_eq!(map_key(key(KeyCode::Insert, KeyModifiers::NONE), &UiPhase::Panels, 5), Some(Command::ToggleSelectAtCursor));
        assert_eq!(map_key(key(KeyCode::Char('+'), KeyModifiers::NONE), &UiPhase::Panels, 5), Some(Command::GroupSelectAll));
        assert_eq!(map_key(key(KeyCode::Char('-'), KeyModifiers::NONE), &UiPhase::Panels, 5), Some(Command::GroupDeselectAll));
        assert_eq!(map_key(key(KeyCode::Char('*'), KeyModifiers::NONE), &UiPhase::Panels, 5), Some(Command::InvertSelection));
    }

    #[test]
    fn ctrl_r_retries_listing() {
        assert_eq!(map_key(key(KeyCode::Char('r'), KeyModifiers::CONTROL), &UiPhase::Panels, 5), Some(Command::RetryListing));
    }

    fn destination_input_phase() -> UiPhase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput {
            kind: filecommand_core::fs_ops::JobKind::Copy,
            sources: vec![],
            source_dir: std::path::PathBuf::from("/left"),
            input: String::new(),
        })
    }

    #[test]
    fn destination_input_routes_typing_and_confirm_cancel() {
        let phase = destination_input_phase();
        assert_eq!(map_key(key(KeyCode::Char('x'), KeyModifiers::NONE), &phase, 5), Some(Command::FileOpInputChar('x')));
        assert_eq!(map_key(key(KeyCode::Backspace, KeyModifiers::NONE), &phase, 5), Some(Command::FileOpInputBackspace));
        assert_eq!(map_key(key(KeyCode::Enter, KeyModifiers::NONE), &phase, 5), Some(Command::FileOpConfirm));
        assert_eq!(map_key(key(KeyCode::Esc, KeyModifiers::NONE), &phase, 5), Some(Command::FileOpCancel));
    }

    fn delete_confirm_phase() -> UiPhase {
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm {
            sources: vec![],
            source_dir: std::path::PathBuf::from("/left"),
            needs_second_confirm: false,
            confirmed_once: false,
        })
    }

    #[test]
    fn delete_confirm_routes_y_n() {
        let phase = delete_confirm_phase();
        assert_eq!(map_key(key(KeyCode::Char('y'), KeyModifiers::NONE), &phase, 5), Some(Command::FileOpConfirm));
        assert_eq!(map_key(key(KeyCode::Char('n'), KeyModifiers::NONE), &phase, 5), Some(Command::FileOpCancel));
    }

    fn progress_phase() -> UiPhase {
        UiPhase::FileOpRunning {
            source_dir: std::path::PathBuf::from("/left"),
            dest_dir: std::path::PathBuf::from("/right"),
            dialog: RunningDialog::Progress {
                kind: filecommand_core::fs_ops::JobKind::Copy,
                progress: filecommand_core::fs_ops::ProgressInfo::starting(1, 1),
            },
        }
    }

    #[test]
    fn progress_dialog_cancel_key_maps_to_cancel_job() {
        assert_eq!(map_key(key(KeyCode::Esc, KeyModifiers::NONE), &progress_phase(), 5), Some(Command::FileOpCancelJob));
    }

    fn conflict_phase(rename_input: Option<String>) -> UiPhase {
        UiPhase::FileOpRunning {
            source_dir: std::path::PathBuf::from("/left"),
            dest_dir: std::path::PathBuf::from("/right"),
            dialog: RunningDialog::Conflict {
                kind: filecommand_core::fs_ops::JobKind::Copy,
                info: filecommand_core::fs_ops::ConflictInfo {
                    source_name: std::ffi::OsString::from("a.txt"),
                    source_size: 1,
                    source_modified: None,
                    target_path: std::path::PathBuf::from("/right/a.txt"),
                    target_size: 2,
                    target_modified: None,
                },
                progress: filecommand_core::fs_ops::ProgressInfo::starting(1, 1),
                rename_input,
            },
        }
    }

    #[test]
    fn conflict_dialog_routes_mnemonic_keys() {
        let phase = conflict_phase(None);
        assert_eq!(map_key(key(KeyCode::Char('o'), KeyModifiers::NONE), &phase, 5), Some(Command::FileOpConflictChoice(ConflictChoice::Overwrite)));
        assert_eq!(
            map_key(key(KeyCode::Char('w'), KeyModifiers::NONE), &phase, 5),
            Some(Command::FileOpConflictChoice(ConflictChoice::OverwriteAll))
        );
        assert_eq!(map_key(key(KeyCode::Char('s'), KeyModifiers::NONE), &phase, 5), Some(Command::FileOpConflictChoice(ConflictChoice::Skip)));
        assert_eq!(map_key(key(KeyCode::Char('a'), KeyModifiers::NONE), &phase, 5), Some(Command::FileOpConflictChoice(ConflictChoice::SkipAll)));
        assert_eq!(map_key(key(KeyCode::Char('r'), KeyModifiers::NONE), &phase, 5), Some(Command::FileOpBeginRename));
    }

    #[test]
    fn conflict_dialog_rename_mode_routes_typing_instead_of_mnemonics() {
        let phase = conflict_phase(Some(String::new()));
        assert_eq!(map_key(key(KeyCode::Char('o'), KeyModifiers::NONE), &phase, 5), Some(Command::FileOpInputChar('o')));
        assert_eq!(map_key(key(KeyCode::Enter, KeyModifiers::NONE), &phase, 5), Some(Command::FileOpConfirm));
        assert_eq!(map_key(key(KeyCode::Esc, KeyModifiers::NONE), &phase, 5), Some(Command::FileOpCancel));
    }

    fn error_phase() -> UiPhase {
        UiPhase::FileOpRunning {
            source_dir: std::path::PathBuf::from("/left"),
            dest_dir: std::path::PathBuf::from("/left"),
            dialog: RunningDialog::Error {
                kind: filecommand_core::fs_ops::JobKind::Delete,
                info: filecommand_core::fs_ops::ErrorInfo { path: std::path::PathBuf::from("/left/a.txt"), message: "denied".into() },
                progress: filecommand_core::fs_ops::ProgressInfo::starting(1, 1),
            },
        }
    }

    #[test]
    fn error_dialog_routes_mnemonic_keys() {
        let phase = error_phase();
        assert_eq!(map_key(key(KeyCode::Char('r'), KeyModifiers::NONE), &phase, 5), Some(Command::FileOpErrorChoice(ErrorChoice::Retry)));
        assert_eq!(map_key(key(KeyCode::Char('s'), KeyModifiers::NONE), &phase, 5), Some(Command::FileOpErrorChoice(ErrorChoice::Skip)));
        assert_eq!(map_key(key(KeyCode::Char('a'), KeyModifiers::NONE), &phase, 5), Some(Command::FileOpErrorChoice(ErrorChoice::SkipAll)));
        assert_eq!(map_key(key(KeyCode::Char('b'), KeyModifiers::NONE), &phase, 5), Some(Command::FileOpErrorChoice(ErrorChoice::Abort)));
        assert_eq!(map_key(key(KeyCode::Esc, KeyModifiers::NONE), &phase, 5), Some(Command::FileOpErrorChoice(ErrorChoice::Abort)));
    }

    #[test]
    fn summary_phase_dismisses_on_any_key() {
        let phase = UiPhase::FileOpSummary(vec![]);
        assert_eq!(map_key(key(KeyCode::Char('z'), KeyModifiers::NONE), &phase, 5), Some(Command::FileOpConfirm));
    }
}
