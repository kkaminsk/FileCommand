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
    let state = state_with(UiPhase::QuitConfirm);
    assert_eq!(map(plain(KeyCode::Char('y')), &state), Some(Command::ConfirmQuit));
    assert_eq!(map(plain(KeyCode::Char('n')), &state), Some(Command::CancelQuit));
    assert_eq!(map(plain(KeyCode::Esc), &state), Some(Command::CancelQuit));
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
fn esc_clears_the_buffer_and_thereby_releases_up_down() {
    assert_eq!(map(plain(KeyCode::Esc), &typing("dir")), Some(Command::CommandLineClear));
    // With the buffer empty again, Up is a cursor move once more.
    assert_eq!(map(plain(KeyCode::Up), &panels()), Some(Command::MoveCursor(CursorMove::Up(1))));
    assert_eq!(map(plain(KeyCode::Esc), &panels()), None, "Esc with nothing typed is inert in the panels view");
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
    assert_eq!(map(plain(KeyCode::Esc), &state), Some(Command::QuickSearchEnd));
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
    for code in [KeyCode::Up, KeyCode::Enter, KeyCode::Tab, KeyCode::F(5)] {
        assert_eq!(map(plain(code), &state), Some(Command::QuickSearchEnd), "{code:?}");
    }
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
fn f3_opens_the_viewer_and_plain_f4_opens_the_external_editor() {
    assert_eq!(map(plain(KeyCode::F(3)), &panels()), Some(Command::RequestViewer));
    assert_eq!(map(plain(KeyCode::F(4)), &panels()), Some(Command::RequestExternalEditor));
}

#[test]
fn ctrl_f4_still_means_sort_by_extension_not_the_external_editor() {
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
