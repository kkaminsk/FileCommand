use super::*;
use crossterm::event::{KeyEventKind, KeyEventState};
use filecommand_core::drives::DriveSelect;
use filecommand_core::listing::{Entry, EntryKind};
use filecommand_core::menu::{MenuId, MenuState};
use filecommand_core::panel::PanelState;
use filecommand_core::theme::Theme;
use std::path::PathBuf;

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent { code, modifiers, kind: KeyEventKind::Press, state: KeyEventState::NONE }
}

fn plain(code: KeyCode) -> KeyEvent {
    key(code, KeyModifiers::NONE)
}

fn state_with(phase: UiPhase) -> State {
    State {
        left: PanelState::new(PathBuf::from("/left")),
        right: PanelState::new(PathBuf::from("/right")),
        phase,
        ..State::empty(Theme::classic())
    }
}

fn panels() -> State {
    state_with(UiPhase::Panels)
}

fn typing(text: &str) -> State {
    State { command_line: text.to_string(), ..panels() }
}

fn map(key: KeyEvent, state: &State) -> Option<Command> {
    map_key(key, state, 5, &Keys::default())
}

// ---------------------------------------------------------------------
// M1/M2 regression coverage
// ---------------------------------------------------------------------

#[test]
fn f10_requests_quit_in_panels_phase() {
    assert_eq!(map(plain(KeyCode::F(10)), &panels()), Some(Command::RequestQuit));
}

#[test]
fn quit_dialog_maps_y_and_n() {
    // The dialog is `state.quit_confirm: bool` now, not `UiPhase::QuitConfirm`
    // — it can be open above any phase (application-shell "Quit request keys
    // and confirmation"; design D5).
    let mut state = panels();
    state.quit_confirm = true;
    assert_eq!(map(plain(KeyCode::Char('y')), &state), Some(Command::ConfirmQuit));
    assert_eq!(map(plain(KeyCode::Char('Y')), &state), Some(Command::ConfirmQuit));
    assert_eq!(map(plain(KeyCode::Enter), &state), Some(Command::ConfirmQuit));
    assert_eq!(map(plain(KeyCode::Char('n')), &state), Some(Command::CancelQuit));
    assert_eq!(map(plain(KeyCode::Char('N')), &state), Some(Command::CancelQuit));
    assert_eq!(map(plain(KeyCode::Esc), &state), Some(Command::CancelQuit));
}

#[test]
fn quit_dialog_ignores_ctrl_c() {
    // BREAKING (clipboard-file-export): Ctrl+C no longer confirms inside the
    // dialog — the `quit-keys` "press Ctrl+C again to exit" convention is
    // reversed per the user's decision. The dialog stays open and nothing
    // happens (application-shell "Quit request keys and confirmation":
    // "Ctrl+C SHALL have no effect there").
    let mut state = panels();
    state.quit_confirm = true;
    assert_eq!(map(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &state), None);
    assert_eq!(map(key(KeyCode::Char('C'), KeyModifiers::CONTROL), &state), None);
}

#[test]
fn quit_dialog_outranks_every_other_overlay_and_phase() {
    // The dialog is checked first of all in `map_key`, above every other
    // overlay/phase — including the viewer and an open menu, which normally
    // bypass or precede the generic overlay checks (application-shell "Quit
    // request keys and confirmation"; design D5).
    let mut over_viewer = state_with(UiPhase::Viewer(filecommand_core::viewer::ViewerState::new(PathBuf::from("f.txt"), 100)));
    over_viewer.quit_confirm = true;
    assert_eq!(map(plain(KeyCode::Char('y')), &over_viewer), Some(Command::ConfirmQuit));

    let mut over_menu = panels();
    over_menu.menu = Some(MenuState::opened());
    over_menu.quit_confirm = true;
    assert_eq!(map(plain(KeyCode::Esc), &over_menu), Some(Command::CancelQuit), "not MenuCollapse");
}

#[test]
fn ctrl_pgup_is_parent_dir_plain_pgup_is_page_move() {
    assert_eq!(map(key(KeyCode::PageUp, KeyModifiers::CONTROL), &panels()), Some(Command::ParentDir));
    assert_eq!(map(plain(KeyCode::PageUp), &panels()), Some(Command::MoveCursor(CursorMove::Up(5))));
}

#[test]
fn tab_and_enter_map_to_expected_commands() {
    assert_eq!(map(plain(KeyCode::Tab), &panels()), Some(Command::ToggleActivePanel));
    assert_eq!(map(plain(KeyCode::Enter), &panels()), Some(Command::Enter));
}

#[test]
fn backspace_is_parent_dir_when_nothing_is_typed() {
    assert_eq!(map(plain(KeyCode::Backspace), &panels()), Some(Command::ParentDir));
}

#[test]
fn f5_through_f8_map_to_file_op_requests() {
    assert_eq!(map(plain(KeyCode::F(5)), &panels()), Some(Command::RequestCopy));
    assert_eq!(map(plain(KeyCode::F(6)), &panels()), Some(Command::RequestMove));
    assert_eq!(map(plain(KeyCode::F(7)), &panels()), Some(Command::RequestMkdir));
    assert_eq!(map(plain(KeyCode::F(8)), &panels()), Some(Command::RequestDelete));
}

#[test]
fn selection_keys_map_while_the_command_line_is_empty() {
    assert_eq!(map(plain(KeyCode::Insert), &panels()), Some(Command::ToggleSelectAtCursor));
    assert_eq!(map(plain(KeyCode::Char('+')), &panels()), Some(Command::GroupSelectAll));
    assert_eq!(map(plain(KeyCode::Char('-')), &panels()), Some(Command::GroupDeselectAll));
    assert_eq!(map(plain(KeyCode::Char('*')), &panels()), Some(Command::InvertSelection));
}

#[test]
fn ctrl_r_rereads_the_active_panel() {
    assert_eq!(map(key(KeyCode::Char('r'), KeyModifiers::CONTROL), &panels()), Some(Command::RereadPanel(PanelSide::Left)));
    let mut state = panels();
    state.active = PanelSide::Right;
    assert_eq!(map(key(KeyCode::Char('r'), KeyModifiers::CONTROL), &state), Some(Command::RereadPanel(PanelSide::Right)));
}

fn destination_input_state() -> State {
    state_with(UiPhase::FileOpSetup(FileOpSetup::DestinationInput {
        kind: filecommand_core::fs_ops::JobKind::Copy,
        sources: vec![],
        source_dir: PathBuf::from("/left"),
        input: String::new(),
        buttons: None,
    }))
}

#[test]
fn destination_input_routes_typing_and_confirm_cancel() {
    let state = destination_input_state();
    assert_eq!(map(plain(KeyCode::Char('x')), &state), Some(Command::FileOpInputChar('x')));
    assert_eq!(map(plain(KeyCode::Backspace), &state), Some(Command::FileOpInputBackspace));
    assert_eq!(map(plain(KeyCode::Enter), &state), Some(Command::FileOpConfirm));
    assert_eq!(map(plain(KeyCode::Esc), &state), Some(Command::FileOpCancel));
}

#[test]
fn delete_confirm_routes_y_n() {
    let state = state_with(UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm {
        sources: vec![],
        source_dir: PathBuf::from("/left"),
        needs_second_confirm: false,
        confirmed_once: false,
    }));
    assert_eq!(map(plain(KeyCode::Char('y')), &state), Some(Command::FileOpConfirm));
    assert_eq!(map(plain(KeyCode::Char('n')), &state), Some(Command::FileOpCancel));
}

fn progress_state() -> State {
    state_with(UiPhase::FileOpRunning {
        source_dir: PathBuf::from("/left"),
        dest_dir: PathBuf::from("/right"),
        dialog: RunningDialog::Progress {
            kind: filecommand_core::fs_ops::JobKind::Copy,
            progress: filecommand_core::fs_ops::ProgressInfo::starting(1, 1),
        },
    })
}

#[test]
fn progress_dialog_cancel_key_maps_to_cancel_job() {
    assert_eq!(map(plain(KeyCode::Esc), &progress_state()), Some(Command::FileOpCancelJob));
}

#[test]
fn progress_dialog_plain_c_still_cancels_but_ctrl_c_is_ignored() {
    // No-modifier guard added to the `c`/`C` -> FileOpCancelJob arm
    // (clipboard-export "Clipboard key bindings"; design D1) — see
    // `ctrl_c_no_longer_requests_quit_from_modal_dialogs_and_overlays` for
    // the Ctrl+C-is-ignored half.
    let state = progress_state();
    assert_eq!(map(plain(KeyCode::Char('c')), &state), Some(Command::FileOpCancelJob));
    assert_eq!(map(plain(KeyCode::Char('C')), &state), Some(Command::FileOpCancelJob));
    assert_eq!(map(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &state), None);
}

fn conflict_state(rename_input: Option<String>) -> State {
    state_with(UiPhase::FileOpRunning {
        source_dir: PathBuf::from("/left"),
        dest_dir: PathBuf::from("/right"),
        dialog: RunningDialog::Conflict {
            kind: filecommand_core::fs_ops::JobKind::Copy,
            info: filecommand_core::fs_ops::ConflictInfo {
                source_name: std::ffi::OsString::from("a.txt"),
                source_size: 1,
                source_modified: None,
                target_path: PathBuf::from("/right/a.txt"),
                target_size: 2,
                target_modified: None,
            },
            progress: filecommand_core::fs_ops::ProgressInfo::starting(1, 1),
            rename_input,
        },
    })
}

#[test]
fn conflict_dialog_routes_mnemonic_keys() {
    let state = conflict_state(None);
    assert_eq!(map(plain(KeyCode::Char('o')), &state), Some(Command::FileOpConflictChoice(ConflictChoice::Overwrite)));
    assert_eq!(map(plain(KeyCode::Char('w')), &state), Some(Command::FileOpConflictChoice(ConflictChoice::OverwriteAll)));
    assert_eq!(map(plain(KeyCode::Char('s')), &state), Some(Command::FileOpConflictChoice(ConflictChoice::Skip)));
    assert_eq!(map(plain(KeyCode::Char('a')), &state), Some(Command::FileOpConflictChoice(ConflictChoice::SkipAll)));
    assert_eq!(map(plain(KeyCode::Char('r')), &state), Some(Command::FileOpBeginRename));
}

#[test]
fn conflict_dialog_rename_mode_routes_typing_instead_of_mnemonics() {
    let state = conflict_state(Some(String::new()));
    assert_eq!(map(plain(KeyCode::Char('o')), &state), Some(Command::FileOpInputChar('o')));
    assert_eq!(map(plain(KeyCode::Enter), &state), Some(Command::FileOpConfirm));
    assert_eq!(map(plain(KeyCode::Esc), &state), Some(Command::FileOpCancel));
}

#[test]
fn error_dialog_routes_mnemonic_keys() {
    let state = state_with(UiPhase::FileOpRunning {
        source_dir: PathBuf::from("/left"),
        dest_dir: PathBuf::from("/left"),
        dialog: RunningDialog::Error {
            kind: filecommand_core::fs_ops::JobKind::Delete,
            info: filecommand_core::fs_ops::ErrorInfo { path: PathBuf::from("/left/a.txt"), message: "denied".into() },
            progress: filecommand_core::fs_ops::ProgressInfo::starting(1, 1),
        },
    });
    assert_eq!(map(plain(KeyCode::Char('r')), &state), Some(Command::FileOpErrorChoice(ErrorChoice::Retry)));
    assert_eq!(map(plain(KeyCode::Char('s')), &state), Some(Command::FileOpErrorChoice(ErrorChoice::Skip)));
    assert_eq!(map(plain(KeyCode::Char('a')), &state), Some(Command::FileOpErrorChoice(ErrorChoice::SkipAll)));
    assert_eq!(map(plain(KeyCode::Char('b')), &state), Some(Command::FileOpErrorChoice(ErrorChoice::Abort)));
    assert_eq!(map(plain(KeyCode::Esc), &state), Some(Command::FileOpErrorChoice(ErrorChoice::Abort)));
}

#[test]
fn summary_phase_dismisses_on_any_key() {
    assert_eq!(map(plain(KeyCode::Char('z')), &state_with(UiPhase::FileOpSummary(vec![]))), Some(Command::FileOpConfirm));
}

// ---------------------------------------------------------------------
// M3: command line
// ---------------------------------------------------------------------

#[test]
fn printable_keys_route_to_the_command_line() {
    assert_eq!(map(plain(KeyCode::Char('d')), &panels()), Some(Command::CommandLineChar('d')));
    assert_eq!(map(plain(KeyCode::Char(' ')), &panels()), Some(Command::CommandLineChar(' ')));
    assert_eq!(map(key(KeyCode::Char('D'), KeyModifiers::SHIFT), &panels()), Some(Command::CommandLineChar('D')));
}

#[test]
fn up_down_recall_history_only_while_something_is_typed() {
    assert_eq!(map(plain(KeyCode::Up), &panels()), Some(Command::MoveCursor(CursorMove::Up(1))));
    assert_eq!(map(plain(KeyCode::Down), &panels()), Some(Command::MoveCursor(CursorMove::Down(1))));
    assert_eq!(map(plain(KeyCode::Up), &typing("dir")), Some(Command::CommandLineHistoryPrev));
    assert_eq!(map(plain(KeyCode::Down), &typing("dir")), Some(Command::CommandLineHistoryNext));
}

#[test]
fn esc_requests_quit_unconditionally_and_does_not_touch_the_buffer() {
    // Esc no longer clears the command line — at panel level it always
    // requests quit, regardless of buffer content (command-line "Command
    // history navigation": "Esc SHALL NOT clear the buffer"; application-
    // shell "Quit request keys and confirmation").
    assert_eq!(map(plain(KeyCode::Esc), &typing("dir")), Some(Command::RequestQuit));
    assert_eq!(map(plain(KeyCode::Esc), &panels()), Some(Command::RequestQuit), "Esc requests quit even with nothing typed");
}

/// mouse-drag "Cancel and phase-change clear the drag": "Pressing Esc during
/// a drag SHALL cancel it" — overriding the otherwise-unconditional
/// Esc-requests-quit mapping just above.
#[test]
fn esc_cancels_an_in_progress_drag_instead_of_requesting_quit() {
    let mut state = panels();
    state.drag = Some(filecommand_core::update::DragState {
        source: PanelSide::Left,
        source_dir: PathBuf::from("/left"),
        items: vec![filecommand_core::fs_ops::SourceItem { original_name: "a.txt".into(), path: PathBuf::from("/left/a.txt"), is_dir: false }],
        op: filecommand_core::fs_ops::JobKind::Copy,
        target: None,
    });
    assert_eq!(map(plain(KeyCode::Esc), &state), Some(Command::DragCancel));
}

#[test]
fn backspacing_to_empty_releases_up_down_to_the_panel() {
    // Backspace is now the only way to hand Up/Down back to the panel
    // cursor: it already pops the buffer in `core::update`, and the mapper
    // recomputes `typing` fresh from the (now empty) buffer on the very
    // next key (command-line "Command history navigation": "Backspacing the
    // buffer to empty SHALL be the mechanism that hands Up/Down back to the
    // panel").
    assert_eq!(map(plain(KeyCode::Up), &typing("d")), Some(Command::CommandLineHistoryPrev));
    assert_eq!(map(plain(KeyCode::Backspace), &typing("d")), Some(Command::CommandLineBackspace));
    // Once the buffer is empty again, Up/Down move the panel cursor.
    assert_eq!(map(plain(KeyCode::Up), &panels()), Some(Command::MoveCursor(CursorMove::Up(1))));
    assert_eq!(map(plain(KeyCode::Down), &panels()), Some(Command::MoveCursor(CursorMove::Down(1))));
}

#[test]
fn backspace_edits_the_buffer_while_typing() {
    assert_eq!(map(plain(KeyCode::Backspace), &typing("dir")), Some(Command::CommandLineBackspace));
}

#[test]
fn plus_minus_star_type_into_a_non_empty_buffer_instead_of_selecting() {
    assert_eq!(map(plain(KeyCode::Char('+')), &typing("echo 1")), Some(Command::CommandLineChar('+')));
    assert_eq!(map(plain(KeyCode::Char('*')), &typing("dir ")), Some(Command::CommandLineChar('*')));
}

#[test]
fn default_paste_bindings_are_ctrl_enter_and_ctrl_bracket() {
    assert_eq!(map(key(KeyCode::Enter, KeyModifiers::CONTROL), &panels()), Some(Command::PasteCursorName));
    assert_eq!(map(key(KeyCode::Char(']'), KeyModifiers::CONTROL), &panels()), Some(Command::PasteCursorPath));
}

#[test]
fn paste_bindings_are_config_overridable() {
    let keys = Keys {
        paste_name: filecommand_core::config::KeyBinding::new(false, true, false, "n"),
        paste_path: filecommand_core::config::KeyBinding::new(false, true, false, "p"),
        ..Keys::default()
    };
    let state = panels();
    assert_eq!(map_key(key(KeyCode::Char('n'), KeyModifiers::ALT), &state, 5, &keys), Some(Command::PasteCursorName));
    assert_eq!(map_key(key(KeyCode::Char('p'), KeyModifiers::ALT), &state, 5, &keys), Some(Command::PasteCursorPath));
    // The defaults no longer apply once rebound: Ctrl+Enter falls through
    // to the plain Enter meaning rather than still pasting.
    assert_eq!(map_key(key(KeyCode::Enter, KeyModifiers::CONTROL), &state, 5, &keys), Some(Command::Enter));
    assert_eq!(map_key(key(KeyCode::Char(']'), KeyModifiers::CONTROL), &state, 5, &keys), None);
}

#[test]
fn binding_match_requires_exact_ctrl_and_alt() {
    let binding = filecommand_core::config::KeyBinding::new(true, false, false, "enter");
    assert!(matches_binding(&key(KeyCode::Enter, KeyModifiers::CONTROL), &binding));
    assert!(!matches_binding(&plain(KeyCode::Enter), &binding));
    assert!(!matches_binding(&key(KeyCode::Enter, KeyModifiers::CONTROL | KeyModifiers::ALT), &binding));
}

#[test]
fn ctrl_o_maps_to_the_scrollback_view() {
    assert_eq!(map(key(KeyCode::Char('o'), KeyModifiers::CONTROL), &panels()), Some(Command::ShowScrollback));
}

// ---------------------------------------------------------------------
// M3: sort modes, drive select, Info
// ---------------------------------------------------------------------

#[test]
fn ctrl_f3_through_f7_pick_the_sort_modes() {
    let expected = [
        (3u8, SortMode::Name),
        (4, SortMode::Extension),
        (5, SortMode::Time),
        (6, SortMode::Size),
        (7, SortMode::Unsorted),
    ];
    for (n, mode) in expected {
        assert_eq!(
            map(key(KeyCode::F(n), KeyModifiers::CONTROL), &panels()),
            Some(Command::SetSortMode { side: PanelSide::Left, mode }),
            "Ctrl+F{n}"
        );
    }
}

#[test]
fn plain_f5_to_f7_still_mean_copy_mkdir_not_sort() {
    assert_eq!(map(plain(KeyCode::F(5)), &panels()), Some(Command::RequestCopy));
    assert_eq!(map(plain(KeyCode::F(7)), &panels()), Some(Command::RequestMkdir));
}

#[test]
fn sort_keys_target_whichever_panel_is_active() {
    let mut state = panels();
    state.active = PanelSide::Right;
    assert_eq!(
        map(key(KeyCode::F(6), KeyModifiers::CONTROL), &state),
        Some(Command::SetSortMode { side: PanelSide::Right, mode: SortMode::Size })
    );
}

#[test]
fn alt_f1_and_f2_target_left_and_right_regardless_of_focus() {
    let mut state = panels();
    state.active = PanelSide::Right;
    assert_eq!(map(key(KeyCode::F(1), KeyModifiers::ALT), &state), Some(Command::OpenDriveSelect(PanelSide::Left)));
    assert_eq!(map(key(KeyCode::F(2), KeyModifiers::ALT), &state), Some(Command::OpenDriveSelect(PanelSide::Right)));
}

#[test]
fn ctrl_l_toggles_info_for_the_active_panel() {
    assert_eq!(map(key(KeyCode::Char('l'), KeyModifiers::CONTROL), &panels()), Some(Command::ToggleInfoMode(PanelSide::Left)));
}

#[test]
fn drive_dialog_claims_navigation_confirm_and_cancel() {
    let mut state = panels();
    state.drive_select = Some(DriveSelect::new(PanelSide::Left, vec!['A', 'C'], None));
    assert_eq!(map(plain(KeyCode::Up), &state), Some(Command::DriveSelectMove(-1)));
    assert_eq!(map(plain(KeyCode::Down), &state), Some(Command::DriveSelectMove(1)));
    assert_eq!(map(plain(KeyCode::Enter), &state), Some(Command::DriveSelectConfirm));
    assert_eq!(map(plain(KeyCode::Esc), &state), Some(Command::DriveSelectCancel));
}

#[test]
fn the_drive_dialog_swallows_keys_that_would_otherwise_type() {
    let mut state = panels();
    state.drive_select = Some(DriveSelect::new(PanelSide::Left, vec!['C'], None));
    assert_eq!(map(plain(KeyCode::Char('d')), &state), None, "typing must not leak past the dialog");
    assert_eq!(map(plain(KeyCode::F(5)), &state), None);
}

// ---------------------------------------------------------------------
// M3: F9 menus
// ---------------------------------------------------------------------

#[test]
fn f9_opens_the_menu_bar() {
    assert_eq!(map(plain(KeyCode::F(9)), &panels()), Some(Command::MenuOpen));
}

#[test]
fn menu_navigation_keys_route_to_the_state_machine() {
    let mut state = panels();
    state.menu = Some(MenuState::opened());
    assert_eq!(map(plain(KeyCode::Up), &state), Some(Command::MenuSelectPrev));
    assert_eq!(map(plain(KeyCode::Down), &state), Some(Command::MenuSelectNext));
    assert_eq!(map(plain(KeyCode::Left), &state), Some(Command::MenuPrevMenu));
    assert_eq!(map(plain(KeyCode::Right), &state), Some(Command::MenuNextMenu));
    assert_eq!(map(plain(KeyCode::Enter), &state), Some(Command::MenuActivate));
    assert_eq!(map(plain(KeyCode::Esc), &state), Some(Command::MenuCollapse));
    assert_eq!(map(plain(KeyCode::F(9)), &state), Some(Command::MenuClose));
}

#[test]
fn a_letter_while_the_bar_is_open_is_a_hotkey_not_typing() {
    let mut state = panels();
    state.menu = Some(MenuState::for_menu(MenuId::Files));
    assert_eq!(map(plain(KeyCode::Char('c')), &state), Some(Command::MenuHotkey('c')));
}

#[test]
fn the_menu_claims_keys_ahead_of_the_command_line() {
    let mut state = typing("dir");
    state.menu = Some(MenuState::opened());
    assert_eq!(map(plain(KeyCode::Up), &state), Some(Command::MenuSelectPrev), "not history recall");
    assert_eq!(map(plain(KeyCode::Char('x')), &state), Some(Command::MenuHotkey('x')), "not a typed character");
}

#[test]
fn the_drive_dialog_outranks_the_menu() {
    let mut state = panels();
    state.menu = Some(MenuState::opened());
    state.drive_select = Some(DriveSelect::new(PanelSide::Left, vec!['C'], None));
    assert_eq!(map(plain(KeyCode::Esc), &state), Some(Command::DriveSelectCancel));
}

// ---------------------------------------------------------------------
// M3: typing-sink arbitration
// ---------------------------------------------------------------------

#[test]
fn alt_letter_starts_quick_search_rather_than_typing() {
    assert_eq!(map(key(KeyCode::Char('r'), KeyModifiers::ALT), &panels()), Some(Command::QuickSearchStart('r')));
}

#[test]
fn quick_search_owns_printables_and_the_command_line_never_sees_them() {
    let mut state = panels();
    state.quick_search = Some("r".to_string());
    assert_eq!(map(plain(KeyCode::Char('e')), &state), Some(Command::QuickSearchChar('e')));
    assert_eq!(map(plain(KeyCode::Backspace), &state), Some(Command::QuickSearchBackspace));
}

#[test]
fn esc_requests_quit_during_type_ahead_instead_of_exiting_it() {
    // Esc is no longer a type-ahead exit key — it requests quit, and
    // cancelling that dialog leaves type-ahead active (type-ahead-jump
    // "Exiting type-ahead and restoring command-line routing": "Esc SHALL
    // NOT exit type-ahead ... it requests application quit").
    let mut state = panels();
    state.quick_search = Some("r".to_string());
    assert_eq!(map(plain(KeyCode::Esc), &state), Some(Command::RequestQuit));
}

#[test]
fn exactly_one_typing_sink_claims_any_given_key() {
    // The same physical key must never produce both a command-line command
    // and a quick-search one.
    let mut searching = panels();
    searching.quick_search = Some("a".to_string());
    for c in ['a', 'z', '1', '.', ' '] {
        let while_typing = map(plain(KeyCode::Char(c)), &panels());
        let while_searching = map(plain(KeyCode::Char(c)), &searching);
        assert!(matches!(while_typing, Some(Command::CommandLineChar(_)) | Some(Command::GroupSelectAll)));
        assert!(matches!(while_searching, Some(Command::QuickSearchChar(_))));
        assert_ne!(while_typing, while_searching, "`{c}` was claimed by both sinks");
    }
}

#[test]
fn a_non_printable_key_dismisses_quick_search() {
    let mut state = panels();
    state.quick_search = Some("r".to_string());
    for code in [KeyCode::Enter, KeyCode::Tab, KeyCode::F(5)] {
        assert_eq!(map(plain(code), &state), Some(Command::QuickSearchEnd), "{code:?}");
    }
}

#[test]
fn a_movement_key_exits_quick_search_and_is_still_applied_to_the_panel() {
    // Unlike Esc/Enter/Tab (plain dismissal), a movement key both ends
    // type-ahead and moves the cursor in the same keystroke — the mapper
    // emits the `MoveCursor` command directly, and `core::update` clears
    // `quick_search` as a side effect of applying it (type-ahead-jump "A
    // movement key exits type-ahead and is applied to the panel"; design
    // D5).
    let mut state = panels();
    state.quick_search = Some("r".to_string());
    assert_eq!(map(plain(KeyCode::Down), &state), Some(Command::MoveCursor(CursorMove::Down(1))));
    assert_eq!(map(plain(KeyCode::Up), &state), Some(Command::MoveCursor(CursorMove::Up(1))));
    assert_eq!(map(plain(KeyCode::PageDown), &state), Some(Command::MoveCursor(CursorMove::Down(5))));
    assert_eq!(map(plain(KeyCode::Home), &state), Some(Command::MoveCursor(CursorMove::Home)));
    assert_eq!(map(plain(KeyCode::End), &state), Some(Command::MoveCursor(CursorMove::End)));
}

#[test]
fn a_dialog_outranks_both_typing_sinks() {
    let mut state = destination_input_state();
    state.quick_search = Some("r".to_string());
    state.command_line = "dir".to_string();
    assert_eq!(map(plain(KeyCode::Char('x')), &state), Some(Command::FileOpInputChar('x')));
}

#[test]
fn entries_are_irrelevant_to_key_mapping() {
    // The mapper reads modes, not contents: a populated panel maps the same
    // keys as an empty one.
    let mut state = panels();
    state.left.entries = vec![Entry { name: "a.txt".into(), kind: EntryKind::File, size: 1, modified: None }];
    assert_eq!(map(plain(KeyCode::Char('d')), &state), Some(Command::CommandLineChar('d')));
}

// ---------------------------------------------------------------------
// M4: F3 viewer / F4 external editor
// ---------------------------------------------------------------------

#[test]
fn f3_opens_the_viewer_and_plain_f4_requests_the_editor() {
    assert_eq!(map(plain(KeyCode::F(3)), &panels()), Some(Command::RequestViewer));
    // `RequestEditor` (not the M4 `RequestExternalEditor` directly) is the
    // F4 keybinding target from M5 on — it resolves the external-editor
    // precedence itself (builtin-editor "External editor takes
    // precedence").
    assert_eq!(map(plain(KeyCode::F(4)), &panels()), Some(Command::RequestEditor));
}

#[test]
fn ctrl_f4_still_means_sort_by_extension_not_the_editor() {
    assert_eq!(map(key(KeyCode::F(4), KeyModifiers::CONTROL), &panels()), Some(Command::SetSortMode { side: PanelSide::Left, mode: SortMode::Extension }));
}

#[test]
fn the_viewer_phase_is_not_routed_through_the_panel_key_mapper() {
    // `map_key` hands the viewer off to `map_viewer_key` (called directly by
    // the event loop, since it needs I/O `map_key` cannot perform) rather
    // than falling through to panel commands.
    let src = filecommand_core::viewer::ViewerState::new(PathBuf::from("f.txt"), 100);
    let state = state_with(UiPhase::Viewer(src));
    assert_eq!(map(plain(KeyCode::F(5)), &state), None);
}

fn open_viewer(mode: filecommand_core::viewer::ViewMode) -> filecommand_core::viewer::ViewerState {
    let mut v = filecommand_core::viewer::ViewerState::new(PathBuf::from("f.txt"), 1000);
    v.mode = mode;
    v
}

#[test]
fn viewer_f_keys_map_to_the_expected_commands() {
    let v = open_viewer(filecommand_core::viewer::ViewMode::Text);
    assert_eq!(map_viewer_key(plain(KeyCode::F(2)), &v, 20), Some(ViewerInput::Cmd(Command::ViewerToggleWrap)));
    assert_eq!(map_viewer_key(plain(KeyCode::F(4)), &v, 20), Some(ViewerInput::Cmd(Command::ViewerToggleMode)));
    assert_eq!(map_viewer_key(plain(KeyCode::F(7)), &v, 20), Some(ViewerInput::Cmd(Command::ViewerSearchStart)));
    assert_eq!(map_viewer_key(plain(KeyCode::F(10)), &v, 20), Some(ViewerInput::Cmd(Command::ViewerClose)));
    assert_eq!(map_viewer_key(plain(KeyCode::Esc), &v, 20), Some(ViewerInput::Cmd(Command::ViewerClose)));
}

#[test]
fn viewer_navigation_keys_map_to_scroll_deltas_sized_by_the_page() {
    let v = open_viewer(filecommand_core::viewer::ViewMode::Text);
    assert_eq!(map_viewer_key(plain(KeyCode::Up), &v, 20), Some(ViewerInput::ScrollLines(-1)));
    assert_eq!(map_viewer_key(plain(KeyCode::Down), &v, 20), Some(ViewerInput::ScrollLines(1)));
    assert_eq!(map_viewer_key(plain(KeyCode::PageUp), &v, 20), Some(ViewerInput::ScrollLines(-20)));
    assert_eq!(map_viewer_key(plain(KeyCode::PageDown), &v, 20), Some(ViewerInput::ScrollLines(20)));
    assert_eq!(map_viewer_key(plain(KeyCode::Home), &v, 20), Some(ViewerInput::Home));
    assert_eq!(map_viewer_key(plain(KeyCode::End), &v, 20), Some(ViewerInput::End));
}

#[test]
fn viewer_left_right_scroll_only_in_unwrap_mode() {
    let mut v = open_viewer(filecommand_core::viewer::ViewMode::Text);
    assert_eq!(map_viewer_key(plain(KeyCode::Right), &v, 20), Some(ViewerInput::ScrollCols(4)));
    assert_eq!(map_viewer_key(plain(KeyCode::Left), &v, 20), Some(ViewerInput::ScrollCols(-4)));
    v.wrap = true;
    assert_eq!(map_viewer_key(plain(KeyCode::Right), &v, 20), None, "wrap mode has no horizontal scroll");
}

#[test]
fn viewer_search_prompt_owns_the_keyboard_while_open() {
    let mut v = open_viewer(filecommand_core::viewer::ViewMode::Text);
    v.search_input = Some("ab".to_string());
    // F-keys that would otherwise toggle mode/wrap are swallowed by the
    // prompt, matching the command line / quick-search precedent.
    assert_eq!(map_viewer_key(plain(KeyCode::F(4)), &v, 20), None);
    assert_eq!(map_viewer_key(plain(KeyCode::Char('c')), &v, 20), Some(ViewerInput::Cmd(Command::ViewerSearchChar('c'))));
    assert_eq!(map_viewer_key(plain(KeyCode::Backspace), &v, 20), Some(ViewerInput::Cmd(Command::ViewerSearchBackspace)));
    assert_eq!(map_viewer_key(plain(KeyCode::Enter), &v, 20), Some(ViewerInput::Cmd(Command::ViewerSearchConfirm)));
    assert_eq!(map_viewer_key(plain(KeyCode::Esc), &v, 20), Some(ViewerInput::Cmd(Command::ViewerSearchCancel)));
}

// ---------------------------------------------------------------------
// M5: F4 built-in editor
// ---------------------------------------------------------------------

fn open_editor(text: &str) -> filecommand_core::editor::EditorState {
    filecommand_core::editor::EditorState::from_bytes(PathBuf::from("f.txt"), text.as_bytes())
}

#[test]
fn editor_phase_is_not_routed_through_map_key() {
    let state = state_with(UiPhase::Editor(open_editor("abc\n")));
    assert_eq!(map(plain(KeyCode::Char('x')), &state), None, "the event loop must call map_editor_key directly instead");
}

#[test]
fn editor_fkeys_map_to_save_mark_replace_search_and_quit() {
    let e = open_editor("abc\n");
    assert_eq!(map_editor_key(plain(KeyCode::F(2)), &e, 20), Some(Command::EditorSave));
    assert_eq!(map_editor_key(plain(KeyCode::F(3)), &e, 20), Some(Command::EditorMark));
    assert_eq!(map_editor_key(plain(KeyCode::F(4)), &e, 20), Some(Command::EditorReplaceStart));
    assert_eq!(map_editor_key(plain(KeyCode::F(7)), &e, 20), Some(Command::EditorSearchStart));
    assert_eq!(map_editor_key(plain(KeyCode::F(10)), &e, 20), Some(Command::EditorRequestQuit));
}

#[test]
fn editor_printable_keys_type_and_insert_toggles_overwrite() {
    let e = open_editor("abc\n");
    assert_eq!(map_editor_key(plain(KeyCode::Char('x')), &e, 20), Some(Command::EditorChar('x')));
    assert_eq!(map_editor_key(plain(KeyCode::Enter), &e, 20), Some(Command::EditorNewline));
    assert_eq!(map_editor_key(plain(KeyCode::Backspace), &e, 20), Some(Command::EditorBackspace));
    assert_eq!(map_editor_key(plain(KeyCode::Insert), &e, 20), Some(Command::EditorToggleMode));
}

#[test]
fn editor_movement_keys_carry_the_visible_row_count_for_paging() {
    let e = open_editor("abc\n");
    assert_eq!(map_editor_key(plain(KeyCode::Left), &e, 20), Some(Command::EditorMove(EditorMove::Left)));
    assert_eq!(map_editor_key(plain(KeyCode::Right), &e, 20), Some(Command::EditorMove(EditorMove::Right)));
    assert_eq!(map_editor_key(plain(KeyCode::Up), &e, 20), Some(Command::EditorMove(EditorMove::Up)));
    assert_eq!(map_editor_key(plain(KeyCode::Down), &e, 20), Some(Command::EditorMove(EditorMove::Down)));
    assert_eq!(map_editor_key(plain(KeyCode::Home), &e, 20), Some(Command::EditorMove(EditorMove::Home)));
    assert_eq!(map_editor_key(plain(KeyCode::End), &e, 20), Some(Command::EditorMove(EditorMove::End)));
    assert_eq!(map_editor_key(plain(KeyCode::PageUp), &e, 20), Some(Command::EditorMove(EditorMove::PageUp(20))));
    assert_eq!(map_editor_key(plain(KeyCode::PageDown), &e, 20), Some(Command::EditorMove(EditorMove::PageDown(20))));
}

#[test]
fn editor_cut_copy_paste_undo_use_ctrl_bindings() {
    let e = open_editor("abc\n");
    assert_eq!(map_editor_key(key(KeyCode::Char('x'), KeyModifiers::CONTROL), &e, 20), Some(Command::EditorCut));
    assert_eq!(map_editor_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &e, 20), Some(Command::EditorCopy));
    assert_eq!(map_editor_key(key(KeyCode::Char('v'), KeyModifiers::CONTROL), &e, 20), Some(Command::EditorPaste));
    assert_eq!(map_editor_key(key(KeyCode::Char('z'), KeyModifiers::CONTROL), &e, 20), Some(Command::EditorUndo));
}

#[test]
fn editor_search_prompt_owns_the_keyboard_while_open() {
    let mut e = open_editor("abc\n");
    e.search_prompt = Some("ab".to_string());
    assert_eq!(map_editor_key(plain(KeyCode::F(2)), &e, 20), None, "F-keys are swallowed by the prompt");
    assert_eq!(map_editor_key(plain(KeyCode::Char('c')), &e, 20), Some(Command::EditorSearchChar('c')));
    assert_eq!(map_editor_key(plain(KeyCode::Backspace), &e, 20), Some(Command::EditorSearchBackspace));
    assert_eq!(map_editor_key(plain(KeyCode::Enter), &e, 20), Some(Command::EditorSearchConfirm));
    assert_eq!(map_editor_key(plain(KeyCode::Esc), &e, 20), Some(Command::EditorSearchCancel));
}

#[test]
fn editor_replace_prompt_owns_the_keyboard_while_open() {
    let mut e = open_editor("abc\n");
    e.replace_prompt = Some(filecommand_core::editor::ReplacePrompt::Pattern("x".to_string()));
    assert_eq!(map_editor_key(plain(KeyCode::F(7)), &e, 20), None, "F-keys are swallowed by the prompt");
    assert_eq!(map_editor_key(plain(KeyCode::Char('y')), &e, 20), Some(Command::EditorReplaceChar('y')));
    assert_eq!(map_editor_key(plain(KeyCode::Backspace), &e, 20), Some(Command::EditorReplaceBackspace));
    assert_eq!(map_editor_key(plain(KeyCode::Enter), &e, 20), Some(Command::EditorReplaceConfirm));
    assert_eq!(map_editor_key(plain(KeyCode::Esc), &e, 20), Some(Command::EditorReplaceCancel));
}

#[test]
fn editor_quit_confirm_owns_y_n_and_esc_over_everything_else() {
    let mut e = open_editor("abc\n");
    e.quit_confirm = true;
    assert_eq!(map_editor_key(plain(KeyCode::Char('y')), &e, 20), Some(Command::EditorConfirmQuitSave));
    assert_eq!(map_editor_key(plain(KeyCode::Char('Y')), &e, 20), Some(Command::EditorConfirmQuitSave));
    assert_eq!(map_editor_key(plain(KeyCode::Enter), &e, 20), Some(Command::EditorConfirmQuitSave));
    assert_eq!(map_editor_key(plain(KeyCode::Char('n')), &e, 20), Some(Command::EditorConfirmQuitDiscard));
    assert_eq!(map_editor_key(plain(KeyCode::Esc), &e, 20), Some(Command::EditorCancelQuit));
    // F2 (save) must not leak through the confirm.
    assert_eq!(map_editor_key(plain(KeyCode::F(2)), &e, 20), None);
}

// ---------------------------------------------------------------------
// M5: quick filter, tabs, and dialog key bindings
// ---------------------------------------------------------------------

#[test]
fn ctrl_p_and_ctrl_j_open_quick_filter_and_fuzzy_jump_by_default() {
    let state = panels();
    assert_eq!(map(key(KeyCode::Char('p'), KeyModifiers::CONTROL), &state), Some(Command::QuickFilterStart));
    assert_eq!(map(key(KeyCode::Char('j'), KeyModifiers::CONTROL), &state), Some(Command::FuzzyJumpOpen));
}

#[test]
fn quick_filter_owns_printables_and_backspace_but_not_esc_or_movement() {
    let mut state = panels();
    state.left.quick_filter = Some("re".to_string());
    assert_eq!(map(plain(KeyCode::Char('p')), &state), Some(Command::QuickFilterChar('p')));
    assert_eq!(map(plain(KeyCode::Backspace), &state), Some(Command::QuickFilterBackspace));
    // Esc no longer exits the filter — it falls through to the unconditional
    // panel-level quit request (quick-filter "Clearing the quick filter":
    // "Esc SHALL NOT exit the filter ... it requests application quit").
    assert_eq!(map(plain(KeyCode::Esc), &state), Some(Command::RequestQuit));
    // Movement/Enter fall through to the ordinary panel mapping — the
    // filter narrows what they can land on, but doesn't intercept them
    // (quick-filter "Navigation is restricted to matching entries").
    assert_eq!(map(plain(KeyCode::Down), &state), Some(Command::MoveCursor(CursorMove::Down(1))));
    assert_eq!(map(plain(KeyCode::Enter), &state), Some(Command::Enter));
}

#[test]
fn quick_filter_activation_key_toggles_the_filter_off_when_active() {
    // The activation key is a toggle: with no filter active it starts one;
    // with a filter already active on the panel, it exits and clears it
    // instead (quick-filter "Clearing the quick filter": "the activation
    // key toggles the filter").
    let mut state = panels();
    assert_eq!(map(key(KeyCode::Char('p'), KeyModifiers::CONTROL), &state), Some(Command::QuickFilterStart));
    state.left.quick_filter = Some("re".to_string());
    assert_eq!(map(key(KeyCode::Char('p'), KeyModifiers::CONTROL), &state), Some(Command::QuickFilterEnd));
}

#[test]
fn quick_filter_is_scoped_to_the_active_panel_only() {
    let mut state = panels();
    state.right.quick_filter = Some("x".to_string()); // right is inactive here
    assert_eq!(
        map(plain(KeyCode::Char('a')), &state),
        Some(Command::CommandLineChar('a')),
        "the inactive panel's filter must not steal printables from the command line"
    );
}

#[test]
fn alt_letter_does_not_start_type_ahead_while_the_quick_filter_is_active() {
    // Regression: Alt+letter must not be able to start type-ahead
    // (`QuickSearchStart`) while `panel.quick_filter` is already active on
    // the active panel — the two input modes are mutually exclusive, and
    // the activation key toggling the filter off is the documented way out
    // of the filter first (quick-filter "Clearing the quick filter").
    let mut state = panels();
    state.left.quick_filter = Some("re".to_string());
    assert_eq!(
        map(key(KeyCode::Char('r'), KeyModifiers::ALT), &state),
        None,
        "Alt+letter must be a no-op while the quick filter owns this panel's input"
    );
    // Once the filter is cleared, Alt+letter starts type-ahead normally.
    state.left.quick_filter = None;
    assert_eq!(map(key(KeyCode::Char('r'), KeyModifiers::ALT), &state), Some(Command::QuickSearchStart('r')));
}

// ---------------------------------------------------------------------
// clipboard-file-export: Ctrl+C is the Files clipboard action, not quit,
// anywhere (BREAKING relative to quit-keys); Ctrl+Ins/Ctrl+Shift+Ins are the
// other two clipboard chords.
// ---------------------------------------------------------------------

#[test]
fn ctrl_c_copies_files_from_panels_in_any_command_line_state() {
    // Idle panel, mid-typing, and under an active quick filter all route
    // Ctrl+C to the Files clipboard action rather than `RequestQuit`
    // (clipboard-export "Clipboard key bindings": "Ctrl+C copies files over
    // the panels").
    let files = Some(Command::CopyToClipboard(ClipboardPayloadKind::Files));
    assert_eq!(map(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &panels()), files);
    assert_eq!(map(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &typing("dir")), files);

    let mut filtering = panels();
    filtering.left.quick_filter = Some("re".to_string());
    assert_eq!(map(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &filtering), files);
}

#[test]
fn ctrl_ins_is_a_fixed_alias_for_files_and_ctrl_shift_ins_is_paths() {
    // clipboard-export "Clipboard key bindings": "Ctrl+Ins alias" and the
    // default `clipboard_paths` binding.
    let files = Some(Command::CopyToClipboard(ClipboardPayloadKind::Files));
    let paths = Some(Command::CopyToClipboard(ClipboardPayloadKind::Paths));
    assert_eq!(map(key(KeyCode::Insert, KeyModifiers::CONTROL), &panels()), files);
    assert_eq!(map(key(KeyCode::Insert, KeyModifiers::CONTROL | KeyModifiers::SHIFT), &panels()), paths);
}

#[test]
fn ctrl_ins_no_longer_toggles_selection() {
    // Regression for the breaking key-map change: Ctrl+Ins used to fall
    // through to the unguarded `Insert -> ToggleSelectAtCursor` arm; the
    // clipboard Files chord must win instead (clipboard-export "Clipboard
    // key bindings"; plain Insert is untouched — see
    // `selection_keys_map_while_the_command_line_is_empty`).
    assert_ne!(map(key(KeyCode::Insert, KeyModifiers::CONTROL), &panels()), Some(Command::ToggleSelectAtCursor));
    assert_eq!(map(key(KeyCode::Insert, KeyModifiers::CONTROL), &panels()), Some(Command::CopyToClipboard(ClipboardPayloadKind::Files)));
}

#[test]
fn rebinding_the_files_chord_leaves_ctrl_ins_working() {
    // clipboard-export "Clipboard key bindings": "Rebinding the files
    // chord" — `key.clipboard_files = "ctrl+k"` moves the rebindable chord
    // and Ctrl+C stops doing anything over the panels, while the fixed
    // Ctrl+Ins alias still runs Files.
    let keys = Keys { clipboard_files: filecommand_core::config::KeyBinding::new(true, false, false, "k"), ..Keys::default() };
    let state = panels();
    let files = Some(Command::CopyToClipboard(ClipboardPayloadKind::Files));
    assert_eq!(map_key(key(KeyCode::Char('k'), KeyModifiers::CONTROL), &state, 5, &keys), files);
    assert_eq!(map_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &state, 5, &keys), None);
    assert_eq!(map_key(key(KeyCode::Insert, KeyModifiers::CONTROL), &state, 5, &keys), files);
}

#[test]
fn ctrl_c_no_longer_requests_quit_from_the_viewer() {
    let v = open_viewer(filecommand_core::viewer::ViewMode::Text);
    assert_eq!(map_viewer_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &v, 20), None);
    // Still true while the F7 search prompt is open.
    let mut v = v;
    v.search_input = Some(String::new());
    assert_eq!(map_viewer_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &v, 20), None);
}

#[test]
fn ctrl_c_in_the_built_in_editor_still_copies() {
    // The sole exclusion: the editor's own Ctrl+C=Copy binding must survive
    // untouched (application-shell "Quit request keys and confirmation":
    // "in the built-in editor Ctrl+C SHALL remain Copy").
    let e = EditorState::from_bytes(PathBuf::from("f.txt"), b"");
    assert_eq!(map_editor_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &e, 20), Some(Command::EditorCopy));
}

#[test]
fn ctrl_c_no_longer_requests_quit_with_a_pull_down_menu_open() {
    let mut state = panels();
    state.menu = Some(MenuState::for_menu(MenuId::Files));
    assert_eq!(map(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &state), None);
}

#[test]
fn ctrl_c_no_longer_requests_quit_from_modal_dialogs_and_overlays() {
    // A representative sample of the modal dialogs/overlays beside the
    // phase: fuzzy jump, find file, user menu, drive select, and the
    // file-op running dialog all now ignore Ctrl+C outright.
    let mut fuzzy = panels();
    fuzzy.fuzzy_jump = Some(filecommand_core::quicksearch::FuzzyJumpState::new());
    assert_eq!(map(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &fuzzy), None);

    let mut find = panels();
    find.find_file = Some(filecommand_core::find_file::FindFileState::new(PathBuf::from("/left")));
    assert_eq!(map(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &find), None);

    let mut user_menu = panels();
    user_menu.user_menu = Some(filecommand_core::dialogs::UserMenuState::new());
    assert_eq!(map(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &user_menu), None);

    let mut drive = panels();
    drive.drive_select = Some(DriveSelect::new(PanelSide::Left, vec![], None));
    assert_eq!(map(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &drive), None);

    assert_eq!(map(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &progress_state()), None);
}

#[test]
fn ctrl_c_types_a_literal_c_in_a_file_op_text_input() {
    // `DestinationInput`/`RenameInput`'s `Char(c) => FileOpInputChar(c)` arm
    // has never filtered by modifier (every other Ctrl-letter already types
    // its base character there), so removing the old Ctrl+C intercept makes
    // Ctrl+C consistent with that existing behavior rather than a new
    // special case — it is not treated as "over the panels" for clipboard
    // purposes (clipboard-export "Clipboard key bindings": "over the
    // panels").
    assert_eq!(
        map(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &destination_input_state()),
        Some(Command::FileOpInputChar('c'))
    );
}

#[test]
fn quick_search_treats_ctrl_c_as_an_unrecognized_key_and_ends_type_ahead() {
    // Type-ahead is a distinct keyboard-owning mode (`map_quick_search_key`,
    // not `map_panel_key`), so it never sees the clipboard chords; Ctrl+C
    // now falls to its existing "any other key ends type-ahead" catch-all,
    // same as any other key the mode doesn't recognize.
    let mut searching = panels();
    searching.quick_search = Some("r".to_string());
    assert_eq!(map(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &searching), Some(Command::QuickSearchEnd));
}

#[test]
fn ctrl_t_ctrl_w_and_alt_digit_map_to_tab_commands() {
    let state = panels();
    assert_eq!(map(key(KeyCode::Char('t'), KeyModifiers::CONTROL), &state), Some(Command::OpenTab));
    assert_eq!(map(key(KeyCode::Char('w'), KeyModifiers::CONTROL), &state), Some(Command::CloseTab));
    assert_eq!(map(key(KeyCode::Char('3'), KeyModifiers::ALT), &state), Some(Command::SwitchTab(3)));
    assert_eq!(map(key(KeyCode::Char('9'), KeyModifiers::ALT), &state), Some(Command::SwitchTab(9)));
}

#[test]
fn f1_f2_and_alt_f7_open_help_user_menu_and_find_file() {
    let state = panels();
    assert_eq!(map(plain(KeyCode::F(1)), &state), Some(Command::HelpOpen));
    assert_eq!(map(plain(KeyCode::F(2)), &state), Some(Command::UserMenuOpen));
    assert_eq!(map(key(KeyCode::F(7), KeyModifiers::ALT), &state), Some(Command::FindFileOpen));
    // Alt+F1/F2 still mean drive select, not Help/user menu.
    assert_eq!(map(key(KeyCode::F(1), KeyModifiers::ALT), &state), Some(Command::OpenDriveSelect(PanelSide::Left)));
    assert_eq!(map(key(KeyCode::F(2), KeyModifiers::ALT), &state), Some(Command::OpenDriveSelect(PanelSide::Right)));
}

#[test]
fn fuzzy_jump_dialog_owns_typing_movement_and_dismissal() {
    let mut state = panels();
    state.fuzzy_jump = Some(filecommand_core::quicksearch::FuzzyJumpState::new());
    assert_eq!(map(plain(KeyCode::Char('d')), &state), Some(Command::FuzzyJumpChar('d')));
    assert_eq!(map(plain(KeyCode::Backspace), &state), Some(Command::FuzzyJumpBackspace));
    assert_eq!(map(plain(KeyCode::Up), &state), Some(Command::FuzzyJumpMove(-1)));
    assert_eq!(map(plain(KeyCode::Down), &state), Some(Command::FuzzyJumpMove(1)));
    assert_eq!(map(plain(KeyCode::Enter), &state), Some(Command::FuzzyJumpConfirm));
    assert_eq!(map(plain(KeyCode::Esc), &state), Some(Command::FuzzyJumpCancel));
}

#[test]
fn find_file_dialog_input_stage_owns_typing_then_switches_to_result_navigation() {
    let mut state = panels();
    state.find_file = Some(filecommand_core::find_file::FindFileState::new(PathBuf::from("/left")));
    assert_eq!(map(plain(KeyCode::Char('r')), &state), Some(Command::FindFileChar('r')));
    assert_eq!(map(plain(KeyCode::Backspace), &state), Some(Command::FindFileBackspace));
    assert_eq!(map(plain(KeyCode::Enter), &state), Some(Command::FindFileSubmit));
    assert_eq!(map(plain(KeyCode::Esc), &state), Some(Command::FindFileCancel));

    let mut submitted = filecommand_core::find_file::FindFileState::new(PathBuf::from("/left"));
    submitted.submit(1);
    state.find_file = Some(submitted);
    assert_eq!(map(plain(KeyCode::Char('r')), &state), None, "the pattern is fixed once a search is in flight");
    assert_eq!(map(plain(KeyCode::Up), &state), Some(Command::FindFileMove(-1)));
    assert_eq!(map(plain(KeyCode::Down), &state), Some(Command::FindFileMove(1)));
    assert_eq!(map(plain(KeyCode::Enter), &state), Some(Command::FindFileConfirm));
    assert_eq!(map(plain(KeyCode::Esc), &state), Some(Command::FindFileCancel));
}

#[test]
fn user_menu_dialog_owns_movement_confirm_and_dismissal() {
    let mut state = panels();
    state.user_menu = Some(filecommand_core::dialogs::UserMenuState::new());
    assert_eq!(map(plain(KeyCode::Up), &state), Some(Command::UserMenuMove(-1)));
    assert_eq!(map(plain(KeyCode::Down), &state), Some(Command::UserMenuMove(1)));
    assert_eq!(map(plain(KeyCode::Enter), &state), Some(Command::UserMenuConfirm));
    assert_eq!(map(plain(KeyCode::Esc), &state), Some(Command::UserMenuCancel));
    // Printables are not routed to the command line while the menu is open.
    assert_eq!(map(plain(KeyCode::Char('a')), &state), None);
}

#[test]
fn help_dialog_precedence_list_then_page_then_about() {
    let mut state = panels();
    let mut help = filecommand_core::dialogs::HelpState::new();
    state.help = Some(help);
    assert_eq!(map(plain(KeyCode::Down), &state), Some(Command::HelpMove(1)));
    assert_eq!(map(plain(KeyCode::Enter), &state), Some(Command::HelpActivate));
    assert_eq!(map(plain(KeyCode::Char('c')), &state), Some(Command::HelpCancel));
    assert_eq!(map(plain(KeyCode::Esc), &state), Some(Command::HelpCancel));

    help.page = Some(1);
    state.help = Some(help);
    assert_eq!(map(plain(KeyCode::Esc), &state), Some(Command::HelpCancel));
    assert_eq!(map(plain(KeyCode::Down), &state), None, "a topic page does not scroll a list that isn't shown");

    help.page = None;
    help.about_open = true;
    state.help = Some(help);
    assert_eq!(map(plain(KeyCode::Esc), &state), Some(Command::HelpCancel));
    assert_eq!(map(plain(KeyCode::Enter), &state), Some(Command::HelpCancel));
    assert_eq!(map(plain(KeyCode::Char('o')), &state), Some(Command::HelpCancel));
}

#[test]
fn help_ignores_ctrl_c_but_plain_c_still_cancels() {
    // No-modifier guard added to the top-level list's `c`/`C` -> HelpCancel
    // arm (clipboard-export "Clipboard key bindings"; design D1): Ctrl+C
    // must fall through to `_ => None` rather than closing Help, while the
    // plain key is untouched.
    let mut state = panels();
    state.help = Some(filecommand_core::dialogs::HelpState::new());
    assert_eq!(map(plain(KeyCode::Char('c')), &state), Some(Command::HelpCancel));
    assert_eq!(map(plain(KeyCode::Char('C')), &state), Some(Command::HelpCancel));
    assert_eq!(map(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &state), None);
    assert_eq!(map(key(KeyCode::Char('C'), KeyModifiers::CONTROL), &state), None);
}

#[test]
fn m5_dialogs_outrank_the_command_line_and_quick_search() {
    let mut state = typing("dir");
    state.quick_search = Some("z".to_string());
    state.fuzzy_jump = Some(filecommand_core::quicksearch::FuzzyJumpState::new());
    assert_eq!(map(plain(KeyCode::Char('a')), &state), Some(Command::FuzzyJumpChar('a')));
}

// ---------------------------------------------------------------------
// M5 review fix: startup-warning modal (malformed usermenu.toml)
// ---------------------------------------------------------------------

#[test]
fn startup_warning_is_dismissed_by_any_key() {
    let mut state = panels();
    state.startup_warning = Some("usermenu.toml is malformed; F2 uses the default user menu".to_string());
    assert_eq!(map(plain(KeyCode::Esc), &state), Some(Command::DismissStartupWarning));
    assert_eq!(map(plain(KeyCode::Enter), &state), Some(Command::DismissStartupWarning));
    assert_eq!(map(plain(KeyCode::Char('x')), &state), Some(Command::DismissStartupWarning));
}

#[test]
fn startup_warning_outranks_every_other_modal_and_typing_sink() {
    // The dialog can only ever be up at session start, before anything else
    // has had a chance to open, but it must still win the precedence check
    // deterministically rather than relying on that invariant alone.
    let mut state = typing("dir");
    state.startup_warning = Some("usermenu.toml is malformed".to_string());
    state.quick_search = Some("z".to_string());
    state.menu = Some(MenuState::opened());
    assert_eq!(map(plain(KeyCode::Char('a')), &state), Some(Command::DismissStartupWarning));
}
