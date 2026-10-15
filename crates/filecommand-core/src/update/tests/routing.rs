//! The single precedence order and the overlay slot (reducer-overlay-
//! structure design D1/D2/D3/D5/D6; application-shell "Modal overlays are
//! mutually exclusive by construction").

use super::*;
use crate::dialogs::HelpState;
use crate::fs_ops::dialog::FileOpSetup;
use crate::fs_ops::ProgressInfo;
use crate::listing::FindMatch;
use crate::menu::{entries, MenuAction, MenuId, MenuState};

fn panels() -> State {
    test_state(UiPhase::Panels)
}

fn with_overlay(overlay: Overlay) -> State {
    let mut state = panels();
    state.open_overlay(overlay);
    state
}

fn delete_confirm_phase() -> UiPhase {
    UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm {
        sources: vec![],
        source_dir: PathBuf::from("/left"),
        needs_second_confirm: false,
        confirmed_once: false,
        hidden_by_filter: 0,
    })
}

fn viewer_phase() -> UiPhase {
    UiPhase::Viewer(ViewerState::new(PathBuf::from("/left/a.txt"), 10))
}

fn editor_phase() -> UiPhase {
    UiPhase::Editor(EditorState::from_bytes(PathBuf::from("/left/a.txt"), b"hello").expect("valid UTF-8 fixture"))
}

/// One state per overlay variant, each opened the way its opener does.
fn every_overlay() -> Vec<(&'static str, Overlay)> {
    vec![
        ("menu", Overlay::Menu(MenuState::opened())),
        ("drive select", Overlay::DriveSelect(DriveSelect::new(PanelSide::Left, vec!['C'], Some('C')))),
        ("fuzzy jump", Overlay::FuzzyJump(FuzzyJumpState::new())),
        ("find file", Overlay::FindFile(FindFileState::new(PathBuf::from("/left")))),
        ("user menu", Overlay::UserMenu(UserMenuState::new())),
        ("history dialog", Overlay::HistoryDialog(HistoryDialogState::new())),
        ("theme picker", Overlay::ThemePicker(ThemePickerState::open("nc-classic"))),
        ("file-action menu", Overlay::FileActionMenu(FileActionMenuState::new(OsString::from("a.txt"), false))),
        ("help", Overlay::Help(HelpState::new())),
        ("startup warning", Overlay::StartupWarning("usermenu.toml is malformed".to_string())),
    ]
}

// ---------------------------------------------------------------------
// `input_layer`: one unit test per layer (task 2.2; design D2)
// ---------------------------------------------------------------------

#[test]
fn quit_confirm_is_the_top_layer_over_every_context() {
    for phase in [UiPhase::Panels, UiPhase::Splash { started_at_ms: 0 }, delete_confirm_phase(), viewer_phase(), editor_phase()] {
        let mut state = test_state(phase);
        state.quit_confirm = true;
        assert_eq!(input_layer(&state), InputLayer::QuitConfirm);
        state.undersized = true;
        assert_eq!(input_layer(&state), InputLayer::QuitConfirm, "above the placeholder too");
    }
    for (name, overlay) in every_overlay() {
        let mut state = with_overlay(overlay);
        state.quit_confirm = true;
        assert_eq!(input_layer(&state), InputLayer::QuitConfirm, "{name}");
    }
}

#[test]
fn undersized_is_the_layer_below_quit_and_hides_every_screen_but_the_startup_warning() {
    for phase in [UiPhase::Panels, delete_confirm_phase(), viewer_phase(), editor_phase()] {
        let mut state = test_state(phase);
        state.undersized = true;
        assert_eq!(input_layer(&state), InputLayer::Undersized);
    }
    for (name, overlay) in every_overlay() {
        let mut state = with_overlay(overlay);
        state.undersized = true;
        let expected = if name == "startup warning" { InputLayer::Overlay } else { InputLayer::Undersized };
        assert_eq!(input_layer(&state), expected, "{name}");
    }
}

#[test]
fn a_file_operation_dialog_is_the_file_op_layer() {
    assert_eq!(input_layer(&test_state(delete_confirm_phase())), InputLayer::FileOp);
    assert_eq!(input_layer(&running_progress_state(JobKind::Copy, "/left", "/right")), InputLayer::FileOp);
    assert_eq!(input_layer(&test_state(UiPhase::FileOpSummary(vec![]))), InputLayer::FileOp);
}

#[test]
fn the_viewer_is_the_viewer_layer() {
    assert_eq!(input_layer(&test_state(viewer_phase())), InputLayer::Viewer);
}

#[test]
fn the_editor_is_the_editor_layer() {
    assert_eq!(input_layer(&test_state(editor_phase())), InputLayer::Editor);
}

#[test]
fn an_open_overlay_is_the_overlay_layer_over_the_panels_and_the_splash() {
    for (name, overlay) in every_overlay() {
        assert_eq!(input_layer(&with_overlay(overlay.clone())), InputLayer::Overlay, "{name} over the panels");
        let mut over_splash = test_state(UiPhase::Splash { started_at_ms: 0 });
        over_splash.open_overlay(overlay);
        assert_eq!(input_layer(&over_splash), InputLayer::Overlay, "{name} over the splash");
    }
}

#[test]
fn the_bare_panels_are_the_panels_layer() {
    assert_eq!(input_layer(&panels()), InputLayer::Panels);
    let mut typing = panels();
    typing.command_line = LineEdit::from("dir");
    typing.quick_search = Some("a".to_string());
    assert_eq!(input_layer(&typing), InputLayer::Panels, "the typing sinks are part of the panels layer");
}

#[test]
fn the_splash_is_the_splash_layer() {
    assert_eq!(input_layer(&test_state(UiPhase::Splash { started_at_ms: 0 })), InputLayer::Splash);
}

// ---------------------------------------------------------------------
// The overlay slot (design D1/D5)
// ---------------------------------------------------------------------

#[test]
fn the_typed_accessors_answer_only_for_their_own_variant() {
    let state = with_overlay(Overlay::Help(HelpState::new()));
    assert!(state.help().is_some());
    assert!(state.menu().is_none() && state.drive_select().is_none() && state.fuzzy_jump().is_none() && state.find_file().is_none());
    assert!(state.user_menu().is_none() && state.history_dialog().is_none() && state.theme_picker().is_none());
    assert!(state.file_action_menu().is_none() && state.startup_warning().is_none());
    assert!(matches!(state.overlay(), Some(Overlay::Help(_))));
    let mut state = state;
    assert!(matches!(state.close_overlay(), Some(Overlay::Help(_))));
    assert!(state.overlay().is_none());
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "open_overlay over an open")]
fn opening_an_overlay_over_another_is_a_routing_bug_caught_in_debug_builds() {
    let mut state = with_overlay(Overlay::Help(HelpState::new()));
    state.open_overlay(Overlay::Menu(MenuState::opened()));
}

/// Task 3.1 / design D5: startup warnings are last-writer-wins — the second
/// message replaces the first in the slot rather than asserting it empty.
#[test]
fn two_successive_startup_warnings_leave_the_second_in_the_slot() {
    let mut state = panels();
    state.set_startup_warning("first".to_string());
    state.set_startup_warning("second".to_string());
    assert_eq!(state.startup_warning(), Some("second"));
    assert!(matches!(state.overlay(), Some(Overlay::StartupWarning(_))));
    let (state, effects) = update(state, Command::DismissStartupWarning);
    assert!(effects.is_empty());
    assert!(state.overlay().is_none(), "any key dismisses the warning and empties the slot");
}

#[test]
fn a_malformed_user_menu_reload_raises_the_startup_warning_in_the_slot() {
    let state = panels();
    let result = config::UserMenuLoadResult { entries: vec![], warnings: vec![], created_default: false, malformed: true, problem: None };
    let (state, _) = update(state, Command::UserMenuReloaded(result));
    assert!(state.startup_warning().is_some_and(|m| m.contains(config::USERMENU_FILE)));
}

/// Task 3.5 (application-shell "The user menu's Themes entry replaces the
/// user menu"): confirming the built-in Themes slot swaps the overlay in
/// place, with the active theme highlighted.
#[test]
fn the_user_menus_themes_entry_replaces_it_with_the_theme_picker() {
    let state = with_overlay(Overlay::UserMenu(UserMenuState::new()));
    // No user entries, so cursor 0 is the compiled-in Themes slot.
    let (state, effects) = update(state, Command::UserMenuConfirm);
    assert!(effects.is_empty(), "no shell command runs");
    assert!(state.user_menu().is_none());
    let picker = state.theme_picker().expect("the theme picker opened in the user menu's place");
    assert_eq!(crate::theme::BUILTIN_THEME_NAMES[picker.highlight], state.theme.name);
}

/// Task 6.7 (design D5/D6), the reducer form of the key router's old
/// `the_drive_dialog_outranks_the_menu`: the drive list, the one
/// asynchronous opener, replaces whatever overlay is in the slot — drive
/// select wins, as it always did in the key router.
#[test]
fn drive_list_ready_replaces_an_open_menu_in_the_slot() {
    let state = with_overlay(Overlay::Menu(MenuState::opened()));
    let (state, effects) = update(state, Command::DriveListReady { target: PanelSide::Left, drives: vec!['C', 'D'] });
    assert!(state.menu().is_none(), "the menu is gone");
    let dialog = state.drive_select().expect("the drive dialog is in the slot");
    assert_eq!(dialog.target, PanelSide::Left);
    assert_eq!(effects.iter().filter(|e| matches!(e, Effect::FetchDriveLabel { .. })).count(), 2);
}

/// Task 3.6 / design D5: a drive label applies only to a drive dialog in
/// the slot with the matching target and generation; with another overlay
/// open it changes nothing.
#[test]
fn a_drive_label_for_a_closed_dialog_touches_no_other_overlay() {
    let (opened, _) = update(panels(), Command::DriveListReady { target: PanelSide::Left, drives: vec!['C'] });
    let generation = opened.drive_select().unwrap().generation;
    let (closed, _) = update(opened, Command::DriveSelectCancel);
    let mut help_open = closed;
    help_open.open_overlay(Overlay::Help(HelpState::new()));
    let before = help_open.clone();
    let (after, effects) =
        update(help_open, Command::DriveLabelResolved { target: PanelSide::Left, letter: 'C', label: Some("SYS".to_string()), generation });
    assert!(effects.is_empty());
    assert_eq!(after, before);
}

// ---------------------------------------------------------------------
// Reducer tests of the application-shell scenarios (tasks 6.1–6.6a)
// ---------------------------------------------------------------------

/// 6.1 "A menu item opens its dialog in place of the menu".
#[test]
fn activating_commands_find_file_leaves_only_the_find_file_dialog_in_the_slot() {
    let state = with_overlay(Overlay::Menu(MenuState::for_menu(MenuId::Commands)));
    let index = entries(MenuId::Commands)
        .iter()
        .position(|e| matches!(e, MenuEntry::Item(item) if item.action == MenuAction::FindFile))
        .expect("Commands lists Find file");
    let (state, effects) = update(state, Command::MenuItemClick(index));
    assert!(effects.is_empty());
    assert!(state.menu().is_none(), "the menu closed");
    let dialog = state.find_file().expect("the find-file dialog opened in its place");
    assert_eq!(dialog.root, PathBuf::from("/left"));
    // The same through the keyboard: highlight the item, then Enter.
    let mut keyboard = with_overlay(Overlay::Menu(MenuState::for_menu(MenuId::Commands)));
    keyboard.menu_mut().unwrap().selected = index;
    let (keyboard, _) = update(keyboard, Command::MenuActivate);
    assert!(keyboard.menu().is_none() && keyboard.find_file().is_some());
}

/// 6.4 "Late results of a dismissed search touch no other overlay".
#[test]
fn late_results_of_a_dismissed_search_leave_an_open_help_window_untouched() {
    let (state, _) = update(panels(), Command::FindFileOpen);
    let (state, effects) = update(state, Command::FindFileSubmit);
    let request = effects
        .iter()
        .find_map(|e| match e {
            Effect::FindInSubtree { request, .. } => Some(*request),
            _ => None,
        })
        .expect("the search started");
    let (state, _) = update(state, Command::FindFileCancel);
    assert!(state.find_file().is_none());
    let (state, _) = update(state, Command::HelpOpen);
    let before = state.clone();
    let late_match = FindMatch { relative_path: PathBuf::from("sub/a.txt"), entry: file_entry("a.txt", 1) };
    let (state, effects) = update(state, Command::FindFileMatch { request, m: late_match });
    assert!(effects.is_empty());
    assert_eq!(state, before, "the late match is dropped and the Help window is unchanged");
    let (state, effects) = update(state, Command::FindFileSearchDone { request });
    assert!(effects.is_empty());
    assert_eq!(state, before);
    assert!(state.find_file().is_none() && state.help().is_some());
}

/// 6.5 "An overlay command with its overlay closed does nothing".
#[test]
fn an_overlay_command_with_the_slot_empty_is_a_no_op() {
    let commands = [
        Command::MenuActivate,
        Command::MenuSelectNext,
        Command::MenuClose,
        Command::MenuItemClick(0),
        Command::DriveSelectConfirm,
        Command::DriveSelectMove(1),
        Command::FuzzyJumpConfirm,
        Command::FuzzyJumpMove(1),
        Command::FindFileConfirm,
        Command::FindFileSubmit,
        Command::FindFileMove(1),
        Command::FindFileMatch { request: 1, m: FindMatch { relative_path: PathBuf::from("a.txt"), entry: file_entry("a.txt", 1) } },
        Command::FindFileSearchDone { request: 1 },
        Command::UserMenuConfirm,
        Command::UserMenuMove(1),
        Command::HistoryDialogConfirm,
        Command::HistoryDialogMove(1),
        Command::ThemePickerConfirm,
        Command::ThemePickerMove(1),
        Command::FileActionMenuConfirm,
        Command::FileActionMenuMove(1),
        Command::FileActionMenuItemClick(0),
        Command::FileActionMenuHotkey('o'),
        Command::HelpActivate,
        Command::HelpMove(1),
        Command::HelpCancel,
        Command::DismissStartupWarning,
    ];
    let before = panels();
    for cmd in commands {
        let (after, effects) = update(before.clone(), cmd.clone());
        assert!(effects.is_empty(), "{cmd:?} returned {effects:?}");
        assert_eq!(after, before, "{cmd:?} changed the state");
    }
}

/// 6.6 "The quit dialog stacks above an open overlay": cancelling it leaves
/// the pull-down exactly as it was. (That the next key then maps to a menu
/// command is the key router's half, in `filecommand-tui`.)
#[test]
fn cancelling_the_quit_dialog_over_an_open_menu_leaves_the_menu_in_the_slot() {
    let state = with_overlay(Overlay::Menu(MenuState::for_menu(MenuId::Files)));
    let (state, _) = update(state, Command::MenuSelectNext);
    let menu_before = state.menu().cloned().expect("the menu is open");
    let (state, _) = update(state, Command::RequestQuit);
    assert!(state.quit_confirm && state.menu().is_some(), "the dialog opens above the menu, not instead of it");
    // The menu's own keys are not claimed by the quit layer, and the slot is
    // untouched by the cancel.
    let (state, effects) = update(state, Command::CancelQuit);
    assert!(effects.is_empty());
    assert!(!state.quit_confirm);
    assert_eq!(state.menu(), Some(&menu_before));
    let (state, _) = update(state, Command::MenuSelectNext);
    assert_eq!(state.menu().unwrap().selected, menu_before.selected + 1, "the menu receives the next command");
}

/// 6.6a "Worker replies pass beneath the quit dialog" (design D2): the quit
/// layer claims only its own two commands.
#[test]
fn job_progress_passes_beneath_the_quit_dialog_to_the_running_job() {
    let mut state = running_progress_state(JobKind::Copy, "/left", "/right");
    state.quit_confirm = true;
    let progress = ProgressInfo { files_done: 2, files_total: 3, bytes_done: 20, bytes_total: 30, current_file: OsString::from("b.txt") };
    let (state, effects) = update(state, Command::JobProgress(progress.clone()));
    assert!(effects.is_empty());
    assert!(state.quit_confirm, "the quit dialog stays open");
    match &state.phase {
        UiPhase::FileOpRunning { dialog: RunningDialog::Progress { progress: shown, .. }, .. } => assert_eq!(shown, &progress),
        other => panic!("the progress dialog underneath shows the new progress: {other:?}"),
    }
}
