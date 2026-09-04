use super::*;
use crate::drives::DriveEntry;
use crate::listing::{Entry, EntryKind};
use crate::menu::MenuEntry;
use std::ffi::OsString;

fn test_state(phase: UiPhase) -> State {
    State {
        left: PanelState::new(PathBuf::from("/left")),
        right: PanelState::new(PathBuf::from("/right")),
        phase,
        ..State::empty(Theme::classic())
    }
}

fn file_entry(name: &str, size: u64) -> Entry {
    Entry { name: OsString::from(name), kind: EntryKind::File, size, modified: None }
}

fn dir_entry(name: &str) -> Entry {
    Entry { name: OsString::from(name), kind: EntryKind::Directory, size: 0, modified: None }
}

/// Every navigation/re-read now also issues `Effect::QueryGitInfo`
/// alongside `Effect::StartListing` (git-info "Query re-issued on
/// navigation"), and `Effect::PersistHistory` (fuzzy-jump "Navigation
/// records history"; design D6). Tests that predate M5 and only care about
/// the listing-related effects filter both out with this helper rather than
/// asserting on request ids/frecency bookkeeping that are incidental to
/// what they're testing.
fn without_git_info_effects(effects: Vec<Effect>) -> Vec<Effect> {
    effects.into_iter().filter(|e| !matches!(e, Effect::QueryGitInfo { .. } | Effect::PersistHistory(_))).collect()
}

// ---------------------------------------------------------------------
// M1/M2 regression coverage
// ---------------------------------------------------------------------

#[test]
fn update_is_deterministic() {
    let s1 = test_state(UiPhase::Panels);
    let s2 = test_state(UiPhase::Panels);
    let (r1, e1) = update(s1, Command::ToggleActivePanel);
    let (r2, e2) = update(s2, Command::ToggleActivePanel);
    assert_eq!(r1, r2);
    assert_eq!(e1, e2);
}

#[test]
fn directory_read_returns_intent_effect_without_io() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("sub")];
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(without_git_info_effects(effects), vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left/sub") }]);
    // The panel's own entries were reset locally — no filesystem was touched.
    assert!(state.left.entries.is_empty());
    assert!(matches!(state.left.progress, crate::panel::ListingProgress::Streaming { count: 0 }));
}

#[test]
fn worker_events_reenter_through_update() {
    let state = test_state(UiPhase::Panels);
    let entries = vec![file_entry("a", 1)];
    let (state, effects) = update(state, Command::ListingChunk { panel: PanelSide::Left, entries: entries.clone() });
    assert!(effects.is_empty());
    assert_eq!(state.left.entries, entries);
    let (state, effects) = update(state, Command::ListingComplete { panel: PanelSide::Left, total: 1 });
    assert!(effects.is_empty());
    assert_eq!(state.left.progress, crate::panel::ListingProgress::Complete { count: 1 });
}

#[test]
fn toggle_active_panel_is_exclusive() {
    let state = test_state(UiPhase::Panels);
    assert_eq!(state.active, PanelSide::Left);
    let (state, _) = update(state, Command::ToggleActivePanel);
    assert_eq!(state.active, PanelSide::Right);
    let (state, _) = update(state, Command::ToggleActivePanel);
    assert_eq!(state.active, PanelSide::Left);
}

#[test]
fn enter_on_parent_dir_navigates_up_and_resets_cursor() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from("/a/b");
    state.left.entries = vec![Entry::parent_dir()];
    state.left.cursor = 0;
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(without_git_info_effects(effects), vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/a") }]);
    assert_eq!(state.left.cursor, 0);
}

#[test]
fn parent_nav_at_root_is_no_op() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from("/");
    let (state, effects) = update(state, Command::ParentDir);
    assert!(effects.is_empty());
    assert_eq!(state.left.cwd, PathBuf::from("/"));
}

#[test]
fn f10_raises_the_quit_confirm_overlay_and_confirm_quits() {
    // Post-quit-keys: the dialog is an overlay beside the phase
    // (`State::quit_confirm: bool`), not a `UiPhase::QuitConfirm` variant —
    // `RequestQuit` must not disturb `state.phase` at all (application-shell
    // "Quit request keys and confirmation"; design D5).
    let state = test_state(UiPhase::Panels);
    let (state, effects) = update(state, Command::RequestQuit);
    assert!(state.quit_confirm, "RequestQuit must raise the overlay");
    assert_eq!(state.phase, UiPhase::Panels, "the overlay lives beside the phase, not inside it");
    assert!(effects.is_empty());
    let (state, effects) = update(state, Command::ConfirmQuit);
    assert_eq!(effects, vec![Effect::Quit]);
    assert!(!state.quit_confirm);
}

#[test]
fn quit_confirm_cancel_clears_only_the_overlay_flag() {
    let mut state = test_state(UiPhase::Panels);
    state.quit_confirm = true;
    let (state, effects) = update(state, Command::CancelQuit);
    assert!(!state.quit_confirm);
    assert_eq!(state.phase, UiPhase::Panels);
    assert!(effects.is_empty());
}

// ---------------------------------------------------------------------
// quit-keys: the quit-confirmation overlay opens from every context and
// cancelling restores that context bit-for-bit (application-shell "Quit
// request keys and confirmation"; design D5; tasks 19.1/19.2/19.3)
// ---------------------------------------------------------------------

#[test]
fn request_quit_opens_over_idle_panels_and_cancel_restores_them_exactly() {
    let before = test_state(UiPhase::Panels);
    let (state, effects) = update(before.clone(), Command::RequestQuit);
    assert!(state.quit_confirm);
    assert!(effects.is_empty());
    let (state, effects) = update(state, Command::CancelQuit);
    assert!(!state.quit_confirm);
    assert!(effects.is_empty());
    assert_eq!(state, before, "cancel must restore idle panels bit-for-bit");
}

#[test]
fn request_quit_opens_mid_command_line_and_cancel_restores_the_buffer() {
    let before = type_line(test_state(UiPhase::Panels), "dir");
    let (state, _) = update(before.clone(), Command::RequestQuit);
    assert!(state.quit_confirm);
    assert_eq!(state.command_line, "dir", "the typed buffer must survive RequestQuit untouched");
    let (state, _) = update(state, Command::CancelQuit);
    assert_eq!(state, before, "cancel must restore the typed command line bit-for-bit");
}

#[test]
fn request_quit_opens_under_an_active_quick_filter_and_cancel_restores_it() {
    let mut before = test_state(UiPhase::Panels);
    before.left.quick_filter = Some("rep".to_string());
    let (state, _) = update(before.clone(), Command::RequestQuit);
    assert!(state.quit_confirm);
    assert_eq!(state.left.quick_filter.as_deref(), Some("rep"));
    let (state, _) = update(state, Command::CancelQuit);
    assert_eq!(state, before, "cancel must restore the active quick filter bit-for-bit");
}

#[test]
fn request_quit_opens_during_type_ahead_and_cancel_restores_it() {
    let mut before = test_state(UiPhase::Panels);
    before.quick_search = Some("re".to_string());
    let (state, _) = update(before.clone(), Command::RequestQuit);
    assert!(state.quit_confirm);
    assert_eq!(state.quick_search.as_deref(), Some("re"));
    let (state, _) = update(state, Command::CancelQuit);
    assert_eq!(state, before, "cancel must restore type-ahead bit-for-bit");
}

#[test]
fn request_quit_opens_above_the_viewer_and_cancel_restores_it() {
    let (before, _) =
        update(test_state(UiPhase::Panels), Command::ViewerOpened { path: PathBuf::from("/left/big.log"), file_len: 5_000_000_000 });
    assert!(matches!(before.phase, UiPhase::Viewer(_)), "precondition: viewer must be open");
    let (state, effects) = update(before.clone(), Command::RequestQuit);
    assert!(state.quit_confirm, "Ctrl+C/RequestQuit must open the overlay while the viewer is open");
    assert!(matches!(state.phase, UiPhase::Viewer(_)), "the overlay must not replace the viewer underneath it");
    assert!(effects.is_empty());
    let (state, _) = update(state, Command::CancelQuit);
    assert_eq!(state, before, "cancel must leave the viewer open, untouched, bit-for-bit");
}

#[test]
fn request_quit_opens_with_a_pull_down_menu_open_and_cancel_restores_it() {
    let (before, _) = update(test_state(UiPhase::Panels), Command::MenuOpen);
    assert!(before.menu.is_some(), "precondition: the menu must be open");
    let (state, effects) = update(before.clone(), Command::RequestQuit);
    assert!(state.quit_confirm);
    assert_eq!(state.menu, before.menu, "the open menu must not be disturbed by RequestQuit");
    assert!(effects.is_empty());
    let (state, _) = update(state, Command::CancelQuit);
    assert_eq!(state, before, "cancel must leave the menu open, untouched, bit-for-bit");
}

#[test]
fn request_quit_opens_with_a_modal_dialog_open_and_cancel_restores_it() {
    let (before, _) = update(test_state(UiPhase::Panels), Command::FuzzyJumpOpen);
    assert!(before.fuzzy_jump.is_some(), "precondition: the fuzzy-jump dialog must be open");
    let (state, effects) = update(before.clone(), Command::RequestQuit);
    assert!(state.quit_confirm);
    assert_eq!(state.fuzzy_jump, before.fuzzy_jump, "the open dialog must not be disturbed by RequestQuit");
    assert!(effects.is_empty());
    let (state, _) = update(state, Command::CancelQuit);
    assert_eq!(state, before, "cancel must leave the modal dialog open, untouched, bit-for-bit");
}

#[test]
fn confirming_quit_while_a_job_is_running_aborts_it_before_quitting() {
    // Confirming quit while a file operation is running must abort the job
    // through the existing cancel path (`Effect::CancelJob`, the same one
    // the Progress dialog's own Cancel key uses) before `Effect::Quit` —
    // order matters, since the caller must stop the worker before tearing
    // the app down (application-shell "Quit request keys and
    // confirmation"; design D3; task 19.3).
    let mut state = running_progress_state(JobKind::Copy, "/left", "/right");
    state.quit_confirm = true;
    let (state, effects) = update(state, Command::ConfirmQuit);
    assert_eq!(effects, vec![Effect::CancelJob, Effect::Quit], "the job must be cancelled before the app quits");
    assert!(!state.quit_confirm);
}

#[test]
fn request_quit_while_a_job_is_running_does_not_touch_the_job_until_confirmed() {
    let before = running_progress_state(JobKind::Copy, "/left", "/right");
    let (state, effects) = update(before.clone(), Command::RequestQuit);
    assert!(state.quit_confirm);
    assert!(effects.is_empty(), "merely opening the dialog must not cancel the running job");
    assert!(matches!(state.phase, UiPhase::FileOpRunning { .. }));
    let (state, effects) = update(state, Command::CancelQuit);
    assert!(effects.is_empty(), "cancelling the quit dialog must not touch the job either");
    assert_eq!(state, before, "cancel must leave the running job untouched, bit-for-bit");
}

#[test]
fn splash_dismissed_immediately_by_key_before_min_hold() {
    let state = test_state(UiPhase::Splash { started_at_ms: 1_000 });
    let (state, effects) = update(state, Command::ToggleActivePanel);
    assert_eq!(state.phase, UiPhase::Panels);
    // The dismissing key was consumed, not forwarded: active panel must
    // still be Left (unaffected by the ToggleActivePanel command).
    assert_eq!(state.active, PanelSide::Left);
    assert!(effects.is_empty());
}

#[test]
fn splash_auto_dismisses_after_min_hold_via_tick() {
    let state = test_state(UiPhase::Splash { started_at_ms: 1_000 });
    let (state, _) = update(state, Command::Tick(1_799));
    assert!(matches!(state.phase, UiPhase::Splash { .. }));
    let (state, _) = update(state, Command::Tick(1_800));
    assert_eq!(state.phase, UiPhase::Panels);
}

#[test]
fn placeholder_below_min_and_grows_back_to_panels_never_splash() {
    let state = test_state(UiPhase::Splash { started_at_ms: 0 });
    let (state, _) = update(state, Command::Resize(40, 10));
    assert_eq!(state.phase, UiPhase::Placeholder);
    let (state, _) = update(state, Command::Resize(80, 24));
    assert_eq!(state.phase, UiPhase::Panels);
}

#[test]
fn too_small_boundary_at_59x16_is_placeholder() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::Resize(59, 16));
    assert_eq!(state.phase, UiPhase::Placeholder);
}

#[test]
fn too_small_boundary_at_60x15_is_placeholder() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::Resize(60, 15));
    assert_eq!(state.phase, UiPhase::Placeholder);
}

#[test]
fn too_small_boundary_at_60x16_is_panels() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::Resize(60, 16));
    assert_eq!(state.phase, UiPhase::Panels);
}

#[test]
fn splash_skipped_when_starting_below_60x16_floor() {
    let (state, _) = State::initial(Theme::classic(), (59, 16), 0, PathBuf::from("/l"), PathBuf::from("/r"), true);
    assert_eq!(state.phase, UiPhase::Placeholder);
}

#[test]
fn splash_starts_normally_at_exactly_60x16() {
    let (state, _) = State::initial(Theme::classic(), (60, 16), 0, PathBuf::from("/l"), PathBuf::from("/r"), true);
    assert_eq!(state.phase, UiPhase::Splash { started_at_ms: 0 });
}

#[test]
fn placeholder_replaces_splash_when_resized_below_new_floor_mid_splash() {
    let state = test_state(UiPhase::Splash { started_at_ms: 0 });
    let (state, _) = update(state, Command::Resize(59, 16));
    assert_eq!(state.phase, UiPhase::Placeholder);
    // Enlarging back never returns to Splash, only Panels.
    let (state, _) = update(state, Command::Resize(60, 16));
    assert_eq!(state.phase, UiPhase::Panels);
}

// ---------------------------------------------------------------------
// Adjustable panel split (panel-split)
// ---------------------------------------------------------------------

#[test]
fn split_grow_moves_divider_two_columns_right() {
    // panel-split "Divider moves in 2-column steps": 50/50 at 100 columns,
    // Ctrl+Right widens the left panel to 52.
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (100, 24);
    let (state, effects) = update(state, Command::SplitGrow);
    assert_eq!(state.split_percent, 52);
    assert_eq!(effects, vec![Effect::PersistPanelSplit(52)]);
}

#[test]
fn split_shrink_moves_divider_two_columns_left() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (100, 24);
    let (state, effects) = update(state, Command::SplitShrink);
    assert_eq!(state.split_percent, 48);
    assert_eq!(effects, vec![Effect::PersistPanelSplit(48)]);
}

#[test]
fn split_shrink_is_a_no_op_at_the_minimum() {
    // panel-split "Adjustment at the limit is a no-op": the right panel is
    // already at its 20-column minimum (left = 80 of 100).
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (100, 24);
    state.split_percent = 80;
    let (state, effects) = update(state, Command::SplitGrow);
    assert_eq!(state.split_percent, 80, "no change at the limit");
    assert!(effects.is_empty());
}

#[test]
fn split_reset_restores_fifty_fifty() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (100, 24);
    state.split_percent = 66;
    let (state, effects) = update(state, Command::SplitReset);
    assert_eq!(state.split_percent, 50);
    assert_eq!(effects, vec![Effect::PersistPanelSplit(50)]);
}

#[test]
fn split_reset_is_a_no_op_when_already_at_default() {
    let state = test_state(UiPhase::Panels);
    assert_eq!(state.split_percent, 50);
    let (state, effects) = update(state, Command::SplitReset);
    assert_eq!(state.split_percent, 50);
    assert!(effects.is_empty(), "no redundant persist when already at the default");
}

#[test]
fn split_change_persists_via_effect() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (100, 24);
    let (_, effects) = update(state, Command::SplitGrow);
    assert!(matches!(effects.as_slice(), [Effect::PersistPanelSplit(_)]));
}

#[test]
fn listing_chunk_during_splash_still_updates_panel_state() {
    let state = test_state(UiPhase::Splash { started_at_ms: 0 });
    let entries = vec![file_entry("a", 0)];
    let (state, _) = update(state, Command::ListingChunk { panel: PanelSide::Left, entries: entries.clone() });
    assert_eq!(state.left.entries, entries);
    assert!(matches!(state.phase, UiPhase::Splash { .. }));
}

#[test]
fn initial_builds_start_listing_effects_for_both_panels() {
    let (state, effects) = State::initial(Theme::classic(), (80, 24), 0, PathBuf::from("/l"), PathBuf::from("/r"), true);
    assert_eq!(state.phase, UiPhase::Splash { started_at_ms: 0 });
    assert_eq!(
        without_git_info_effects(effects),
        vec![
            Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/l") },
            Effect::StartListing { panel: PanelSide::Right, path: PathBuf::from("/r") },
        ]
    );
}

#[test]
fn initial_below_min_size_skips_straight_to_placeholder() {
    let (state, _) = State::initial(Theme::classic(), (40, 10), 0, PathBuf::from("/l"), PathBuf::from("/r"), true);
    assert_eq!(state.phase, UiPhase::Placeholder);
}

#[test]
fn initial_without_splash_starts_at_panels() {
    let (state, _) = State::initial(Theme::classic(), (80, 24), 0, PathBuf::from("/l"), PathBuf::from("/r"), false);
    assert_eq!(state.phase, UiPhase::Panels);
}

#[test]
fn request_copy_with_no_selection_and_no_cursor_entry_is_noop() {
    let state = test_state(UiPhase::Panels);
    let (state, effects) = update(state, Command::RequestCopy);
    assert_eq!(state.phase, UiPhase::Panels);
    assert!(effects.is_empty());
}

#[test]
fn request_copy_uses_cursor_entry_when_nothing_explicitly_selected() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 10)];
    let (state, _) = update(state, Command::RequestCopy);
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, .. }) => {
            assert_eq!(kind, JobKind::Copy);
            assert_eq!(sources.len(), 1);
            assert_eq!(sources[0].original_name, OsString::from("a.txt"));
        }
        other => panic!("expected FileOpSetup::DestinationInput, got {other:?}"),
    }
}

#[test]
fn request_copy_prefills_destination_with_opposite_panel_cwd() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 10)];
    let (state, _) = update(state, Command::RequestCopy);
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { input, .. }) => {
            assert_eq!(input, PathBuf::from("/right").display().to_string());
        }
        other => panic!("expected FileOpSetup::DestinationInput, got {other:?}"),
    }
}

#[test]
fn request_copy_uses_explicit_selection_over_cursor() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1), file_entry("b.txt", 2)];
    state.left.selected.insert(OsString::from("b.txt"));
    let (state, _) = update(state, Command::RequestCopy);
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { sources, .. }) => {
            assert_eq!(sources.len(), 1);
            assert_eq!(sources[0].original_name, OsString::from("b.txt"));
        }
        other => panic!("expected FileOpSetup::DestinationInput, got {other:?}"),
    }
}

#[test]
fn request_mkdir_is_always_available_even_with_empty_panel() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::RequestMkdir);
    assert!(matches!(state.phase, UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind: JobKind::Mkdir, .. })));
}

#[test]
fn destination_input_typing_and_confirm_starts_job() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::RequestCopy);
    let (state, _) = update(state, Command::FileOpInputBackspace); // backspace on prefilled input
    let (state, _) = update(state, Command::FileOpInputChar('X'));
    let (state, effects) = update(state, Command::FileOpConfirm);
    match &state.phase {
        UiPhase::FileOpRunning { source_dir, dest_dir, dialog: RunningDialog::Progress { kind, .. } } => {
            assert_eq!(*kind, JobKind::Copy);
            assert_eq!(source_dir, &PathBuf::from("/left"));
            assert!(dest_dir.to_string_lossy().ends_with('X'));
        }
        other => panic!("expected FileOpRunning Progress, got {other:?}"),
    }
    assert!(matches!(effects.as_slice(), [Effect::RunJob(_)]));
}

#[test]
fn destination_input_confirm_with_empty_input_is_noop() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::RequestMkdir);
    let (state, effects) = update(state, Command::FileOpConfirm);
    assert!(effects.is_empty());
    assert!(matches!(state.phase, UiPhase::FileOpSetup(FileOpSetup::DestinationInput { .. })));
}

#[test]
fn escape_cancels_destination_input_back_to_panels() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::RequestCopy);
    let (state, effects) = update(state, Command::FileOpCancel);
    assert_eq!(state.phase, UiPhase::Panels);
    assert!(effects.is_empty());
}

#[test]
fn delete_confirm_requires_second_confirmation_for_a_directory() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("sub")];
    let (state, effects) = update(state, Command::RequestDelete);
    assert!(effects.is_empty());
    assert!(matches!(
        state.phase,
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { needs_second_confirm: true, confirmed_once: false, .. })
    ));

    let (state, effects) = update(state, Command::FileOpConfirm);
    assert!(effects.is_empty(), "first confirm just arms the second confirmation");
    assert!(matches!(
        state.phase,
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { needs_second_confirm: true, confirmed_once: true, .. })
    ));

    let (state, effects) = update(state, Command::FileOpConfirm);
    assert!(matches!(effects.as_slice(), [Effect::RunJob(Job { kind: JobKind::Delete, .. })]));
    assert!(matches!(state.phase, UiPhase::FileOpRunning { .. }));
}

#[test]
fn delete_confirm_single_file_needs_only_one_confirmation() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::RequestDelete);
    let (state, effects) = update(state, Command::FileOpConfirm);
    assert!(matches!(effects.as_slice(), [Effect::RunJob(_)]));
    assert!(matches!(state.phase, UiPhase::FileOpRunning { .. }));
}

fn running_progress_state(kind: JobKind, source_dir: &str, dest_dir: &str) -> State {
    let mut state = test_state(UiPhase::FileOpRunning {
        source_dir: PathBuf::from(source_dir),
        dest_dir: PathBuf::from(dest_dir),
        dialog: RunningDialog::Progress { kind, progress: ProgressInfo::starting(3, 30) },
    });
    state.left.cwd = PathBuf::from(source_dir);
    state.right.cwd = PathBuf::from(dest_dir);
    state
}

#[test]
fn job_progress_event_updates_progress_dialog() {
    let state = running_progress_state(JobKind::Copy, "/left", "/right");
    let info = ProgressInfo { files_done: 1, files_total: 3, bytes_done: 10, bytes_total: 30, current_file: OsString::from("a.txt") };
    let (state, effects) = update(state, Command::JobProgress(info.clone()));
    assert!(effects.is_empty());
    match state.phase {
        UiPhase::FileOpRunning { dialog: RunningDialog::Progress { progress, .. }, .. } => assert_eq!(progress, info),
        other => panic!("expected Progress dialog, got {other:?}"),
    }
}

fn sample_conflict() -> ConflictInfo {
    ConflictInfo {
        source_name: OsString::from("a.txt"),
        source_size: 1,
        source_modified: None,
        target_path: PathBuf::from("/right/a.txt"),
        target_size: 2,
        target_modified: None,
    }
}

#[test]
fn job_conflict_event_switches_to_conflict_dialog_and_reply_returns_to_progress() {
    let state = running_progress_state(JobKind::Copy, "/left", "/right");
    let (state, _) = update(state, Command::JobConflict(sample_conflict()));
    assert!(matches!(state.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Conflict { .. }, .. }));

    let (state, effects) = update(state, Command::FileOpConflictChoice(ConflictChoice::Overwrite));
    assert_eq!(effects, vec![Effect::SendConflictReply(ConflictChoice::Overwrite)]);
    assert!(matches!(state.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Progress { .. }, .. }));
}

#[test]
fn conflict_rename_flow_composes_a_name_then_replies() {
    let state = running_progress_state(JobKind::Copy, "/left", "/right");
    let (state, _) = update(state, Command::JobConflict(sample_conflict()));
    let (state, _) = update(state, Command::FileOpBeginRename);
    let (state, _) = update(state, Command::FileOpInputChar('b'));
    let (state, _) = update(state, Command::FileOpInputChar('.'));
    let (state, _) = update(state, Command::FileOpInputChar('c'));
    let (state, effects) = update(state, Command::FileOpConfirm);
    assert_eq!(effects, vec![Effect::SendConflictReply(ConflictChoice::Rename(OsString::from("b.c")))]);
    assert!(matches!(state.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Progress { .. }, .. }));
}

#[test]
fn job_error_event_switches_to_error_dialog_and_reply_returns_to_progress() {
    let state = running_progress_state(JobKind::Delete, "/left", "/left");
    let info = ErrorInfo { path: PathBuf::from("/left/a.txt"), message: "permission denied".to_string() };
    let (state, _) = update(state, Command::JobError(info));
    assert!(matches!(state.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Error { .. }, .. }));
    let (state, effects) = update(state, Command::FileOpErrorChoice(ErrorChoice::Skip));
    assert_eq!(effects, vec![Effect::SendErrorReply(ErrorChoice::Skip)]);
    assert!(matches!(state.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Progress { .. }, .. }));
}

#[test]
fn job_cancel_sends_cancel_effect_without_leaving_progress_dialog() {
    let state = running_progress_state(JobKind::Copy, "/left", "/right");
    let (state, effects) = update(state, Command::FileOpCancelJob);
    assert_eq!(effects, vec![Effect::CancelJob]);
    assert!(matches!(state.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Progress { .. }, .. }));
}

#[test]
fn job_done_with_no_skips_rereads_matching_panels_and_returns_to_panels() {
    let state = running_progress_state(JobKind::Copy, "/left", "/right");
    let (state, effects) = update(
        state,
        Command::JobDone { outcome: JobOutcome::Completed { skipped: vec![] }, source_dir: PathBuf::from("/left"), dest_dir: PathBuf::from("/right") },
    );
    assert_eq!(state.phase, UiPhase::Panels);
    assert_eq!(
        without_git_info_effects(effects),
        vec![
            Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left") },
            Effect::StartListing { panel: PanelSide::Right, path: PathBuf::from("/right") },
        ]
    );
}

#[test]
fn job_done_with_skips_shows_summary_instead_of_panels() {
    let mut state = test_state(UiPhase::FileOpRunning {
        source_dir: PathBuf::from("/left"),
        dest_dir: PathBuf::from("/left"),
        dialog: RunningDialog::Progress { kind: JobKind::Delete, progress: ProgressInfo::starting(3, 30) },
    });
    state.left.cwd = PathBuf::from("/left");
    state.right.cwd = PathBuf::from("/right"); // unaffected panel: must not be re-read
    let skipped = vec![SkippedItem { path: PathBuf::from("/left/a.txt"), reason: "denied".to_string() }];
    let (state, effects) = update(
        state,
        Command::JobDone { outcome: JobOutcome::Completed { skipped: skipped.clone() }, source_dir: PathBuf::from("/left"), dest_dir: PathBuf::from("/left") },
    );
    assert_eq!(state.phase, UiPhase::FileOpSummary(skipped));
    assert_eq!(without_git_info_effects(effects), vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left") }]);

    let (state, _) = update(state, Command::FileOpConfirm);
    assert_eq!(state.phase, UiPhase::Panels);
}

#[test]
fn job_done_only_rereads_panels_whose_cwd_matches_source_or_dest() {
    let mut state = test_state(UiPhase::FileOpRunning {
        source_dir: PathBuf::from("/left"),
        dest_dir: PathBuf::from("/somewhere/else"),
        dialog: RunningDialog::Progress { kind: JobKind::Copy, progress: ProgressInfo::starting(1, 1) },
    });
    state.left.cwd = PathBuf::from("/left");
    state.right.cwd = PathBuf::from("/right"); // does not match dest_dir
    let (_, effects) = update(
        state,
        Command::JobDone {
            outcome: JobOutcome::Completed { skipped: vec![] },
            source_dir: PathBuf::from("/left"),
            dest_dir: PathBuf::from("/somewhere/else"),
        },
    );
    assert_eq!(without_git_info_effects(effects), vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left") }]);
}

#[test]
fn job_done_delete_rereads_both_panels_when_both_browse_the_same_directory() {
    // BIG-162 base case: both panels' *active* tabs already sat on the
    // directory a Delete completed in — this must keep re-reading both
    // immediately, unchanged by the background-tab staleness work
    // (file-operations "The opposite panel sharing the affected directory
    // also refreshes").
    let mut state = test_state(UiPhase::FileOpRunning {
        source_dir: PathBuf::from("/shared"),
        dest_dir: PathBuf::from("/shared"),
        dialog: RunningDialog::Progress { kind: JobKind::Delete, progress: ProgressInfo::starting(1, 1) },
    });
    state.left.cwd = PathBuf::from("/shared");
    state.right.cwd = PathBuf::from("/shared");
    let (state, effects) = update(
        state,
        Command::JobDone {
            outcome: JobOutcome::Completed { skipped: vec![] },
            source_dir: PathBuf::from("/shared"),
            dest_dir: PathBuf::from("/shared"),
        },
    );
    assert_eq!(state.phase, UiPhase::Panels);
    assert_eq!(
        without_git_info_effects(effects),
        vec![
            Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/shared") },
            Effect::StartListing { panel: PanelSide::Right, path: PathBuf::from("/shared") },
        ]
    );
}

#[test]
fn job_done_marks_background_tab_stale_without_eager_reread() {
    // A background tab (not the active one) sitting on the deleted-from
    // directory must be marked stale, not eagerly re-read — the read is
    // deferred until it becomes active (file-operations "A background tab
    // on the affected directory is marked stale, not eagerly re-read").
    let mut state = test_state(UiPhase::FileOpRunning {
        source_dir: PathBuf::from("/left"),
        dest_dir: PathBuf::from("/left"),
        dialog: RunningDialog::Progress { kind: JobKind::Delete, progress: ProgressInfo::starting(1, 1) },
    });
    state.left.cwd = PathBuf::from("/left");
    state.left.open_tab(); // tab 0 stashed at "/left" (background); tab 1 (active) also starts at "/left"
    state.left.begin_new_listing(PathBuf::from("/left/other")); // active tab moves elsewhere; tab 0 remains at "/left"
    state.right.cwd = PathBuf::from("/right"); // unaffected panel

    let (state, effects) = update(
        state,
        Command::JobDone {
            outcome: JobOutcome::Completed { skipped: vec![] },
            source_dir: PathBuf::from("/left"),
            dest_dir: PathBuf::from("/left"),
        },
    );
    assert_eq!(state.phase, UiPhase::Panels);
    assert!(without_git_info_effects(effects).is_empty(), "a background tab must not be eagerly re-read");
    assert_eq!(state.left.tabs.len(), 1);
    assert!(state.left.tabs[0].stale, "the background tab on the deleted-from directory must be marked stale");
}

#[test]
fn listing_complete_reconciles_selection_against_fresh_entries() {
    let mut state = test_state(UiPhase::Panels);
    state.left.selected.insert(OsString::from("gone.txt"));
    state.left.selected.insert(OsString::from("stays.txt"));
    state.left.entries = vec![file_entry("stays.txt", 1)];
    let (state, _) = update(state, Command::ListingComplete { panel: PanelSide::Left, total: 1 });
    assert_eq!(state.left.selected, std::collections::HashSet::from([OsString::from("stays.txt")]));
}

#[test]
fn selection_commands_operate_on_active_panel() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1), file_entry("b.txt", 2)];
    let (state, _) = update(state, Command::ToggleSelectAtCursor);
    assert!(state.left.selected.contains(&OsString::from("a.txt")));
    let (state, _) = update(state, Command::InvertSelection);
    assert_eq!(state.left.selected, std::collections::HashSet::from([OsString::from("b.txt")]));
    let (state, _) = update(state, Command::GroupSelectAll);
    assert_eq!(state.left.selected.len(), 2);
    let (state, _) = update(state, Command::GroupDeselectAll);
    assert!(state.left.selected.is_empty());
}

#[test]
fn reread_reissues_start_listing_and_clears_the_error() {
    let mut state = test_state(UiPhase::Panels);
    state.left.last_error = Some("boom".to_string());
    state.left.cwd = PathBuf::from("/left");
    let (state, effects) = update(state, Command::RereadPanel(PanelSide::Left));
    assert_eq!(without_git_info_effects(effects), vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left") }]);
    assert!(state.left.last_error.is_none());
}

// ---------------------------------------------------------------------
// M3: command line
// ---------------------------------------------------------------------

fn type_line(state: State, text: &str) -> State {
    text.chars().fold(state, |s, c| update(s, Command::CommandLineChar(c)).0)
}

#[test]
fn prompt_shows_the_active_panel_path_and_follows_focus() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    state.right.cwd = PathBuf::from(r"D:\WORK");
    assert_eq!(state.prompt(), format!("{}>", PathBuf::from(r"C:\NORTON").display()));

    let (state, _) = update(state, Command::ToggleActivePanel);
    assert_eq!(state.prompt(), format!("{}>", PathBuf::from(r"D:\WORK").display()));
}

#[test]
fn prompt_follows_the_active_panel_into_a_subdirectory() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from("/left");
    state.left.entries = vec![dir_entry("sub")];
    let (state, _) = update(state, Command::Enter);
    assert_eq!(state.prompt(), format!("{}>", PathBuf::from("/left").join("sub").display()));
}

#[test]
fn printable_keys_build_the_command_buffer_without_moving_the_cursor() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a", 1), file_entry("b", 2)];
    state.left.cursor = 1;
    let state = type_line(state, "dir");
    assert_eq!(state.command_line, "dir");
    assert_eq!(state.left.cursor, 1, "typing must not move the panel cursor");
}

#[test]
fn backspace_edits_the_buffer_and_clears_the_history_cursor() {
    let mut state = test_state(UiPhase::Panels);
    state.history = vec!["old".to_string()];
    let state = type_line(state, "dir");
    let (state, _) = update(state, Command::CommandLineHistoryPrev);
    assert_eq!(state.command_line, "old");
    assert_eq!(state.history_cursor, Some(0));

    let (state, _) = update(state, Command::CommandLineBackspace);
    assert_eq!(state.command_line, "ol");
    assert_eq!(state.history_cursor, None, "editing leaves history recall");
}

#[test]
fn command_line_clear_command_still_clears_the_buffer() {
    // `Command::CommandLineClear` still exists and still behaves as a plain
    // "clear the buffer" reducer action — only its trigger changed: Esc no
    // longer dispatches it (`input::map_panel_key` maps panel-level Esc to
    // `RequestQuit` unconditionally instead), so nothing in the shipped
    // key-mapping layer emits this command anymore (command-line "Command
    // history navigation": "Esc SHALL NOT clear the buffer").
    let state = type_line(test_state(UiPhase::Panels), "dir");
    let (state, _) = update(state, Command::CommandLineClear);
    assert!(state.command_line.is_empty());
    assert_eq!(state.history_cursor, None);
}

#[test]
fn quick_search_mode_consumes_printables_instead_of_the_command_line() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("alpha", 1), file_entry("beta", 2)];
    let (state, _) = update(state, Command::QuickSearchStart('b'));
    assert_eq!(state.quick_search.as_deref(), Some("b"));
    assert_eq!(state.left.cursor, 1, "quick search jumped to the first match");

    let (state, _) = update(state, Command::QuickSearchChar('e'));
    assert_eq!(state.quick_search.as_deref(), Some("be"));
    assert!(state.command_line.is_empty(), "the command line never saw those keys");
}

#[test]
fn leaving_quick_search_hands_printables_back_to_the_command_line() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("alpha", 1)];
    let (state, _) = update(state, Command::QuickSearchStart('a'));
    let (state, _) = update(state, Command::QuickSearchEnd);
    assert_eq!(state.quick_search, None);
    let state = type_line(state, "dir");
    assert_eq!(state.command_line, "dir");
}

#[test]
fn quick_search_backspace_shrinks_and_stays_active_when_emptied() {
    // type-ahead-jump "Backspace on a single-character pattern": the
    // pattern becomes empty but type-ahead mode itself stays active (only a
    // movement key exits it now — Esc no longer does; it requests quit
    // instead) and the cursor holds its position rather than re-jumping
    // against an empty pattern.
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("alpha", 1)];
    let (state, _) = update(state, Command::QuickSearchStart('a'));
    let (state, _) = update(state, Command::QuickSearchChar('l'));
    assert_eq!(state.quick_search.as_deref(), Some("al"));
    let cursor_before = state.left.cursor;
    let (state, _) = update(state, Command::QuickSearchBackspace);
    assert_eq!(state.quick_search.as_deref(), Some("a"));
    let (state, _) = update(state, Command::QuickSearchBackspace);
    assert_eq!(state.quick_search.as_deref(), Some(""), "type-ahead must remain active with an empty pattern");
    assert_eq!(state.left.cursor, cursor_before, "the cursor must hold position rather than jump on an empty pattern");
    let (state, _) = update(state, Command::QuickSearchEnd);
    assert_eq!(state.quick_search, None, "QuickSearchEnd (now dispatched by a movement key, not Esc) still exits type-ahead");
}

// ---------------------------------------------------------------------
// Quick filter (Ctrl+P), reducer-level (task 15.2)
// ---------------------------------------------------------------------

#[test]
fn quick_filter_start_char_and_end_are_scoped_to_the_active_panel() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("report.txt", 1), file_entry("readme.md", 2)];
    state.right.entries = vec![file_entry("report.txt", 1), file_entry("readme.md", 2)];

    let (state, _) = update(state, Command::QuickFilterStart);
    assert_eq!(state.left.quick_filter.as_deref(), Some(""));
    assert_eq!(state.right.quick_filter, None, "the opposite panel must be unaffected");

    let (state, _) = update(state, Command::QuickFilterChar('r'));
    let (state, _) = update(state, Command::QuickFilterChar('e'));
    let (state, _) = update(state, Command::QuickFilterChar('p'));
    assert_eq!(state.left.quick_filter.as_deref(), Some("rep"));
    let visible: Vec<String> = state.left.visible_indices().into_iter().map(|i| state.left.entries[i].name.to_string_lossy().into_owned()).collect();
    assert_eq!(visible, vec!["report.txt"]);

    let (state, _) = update(state, Command::QuickFilterBackspace);
    assert_eq!(state.left.quick_filter.as_deref(), Some("re"));

    let (state, _) = update(state, Command::QuickFilterEnd);
    assert_eq!(state.left.quick_filter, None);
}

#[test]
fn jump_to_prefix_only_lands_within_the_active_quick_filter() {
    // Input routing normally keeps type-ahead and the quick filter mutually
    // exclusive, but `jump_to_prefix` (backing `QuickSearchStart`/`Char`)
    // must still be safe if both are ever active together: it must not
    // land the cursor on an entry the filter hides. Both "beta" and
    // "berry" start with "b", so an unfiltered type-ahead jump would land
    // on "beta" (it comes first); the active filter "rr" hides "beta" but
    // not "berry", so the filtered jump must land on "berry" instead.
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("beta", 1), file_entry("berry", 2)];
    state.left.quick_filter = Some("rr".to_string());
    let (state, _) = update(state, Command::QuickSearchStart('b'));
    assert_eq!(
        state.left.entries[state.left.cursor].name.to_string_lossy(),
        "berry",
        "the jump must only search entries the active filter leaves visible"
    );
}

// ---------------------------------------------------------------------
// Panel tabs (Ctrl+T / Ctrl+W / Alt+1..9), reducer-level (task 15.5)
// ---------------------------------------------------------------------

#[test]
fn tab_commands_are_scoped_to_the_active_panel() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Left;

    let (state, _) = update(state, Command::OpenTab);
    assert_eq!(state.left.tab_count(), 2);
    assert_eq!(state.right.tab_count(), 1, "the opposite panel's tabs must be untouched");

    let (mut state, _) = update(state, Command::CloseTab);
    assert_eq!(state.left.tab_count(), 1);

    state.left.open_tab();
    state.left.begin_new_listing(PathBuf::from("/left/other"));
    let (state, _) = update(state, Command::SwitchTab(1));
    assert_eq!(state.left.cwd, PathBuf::from("/left"));
}

#[test]
fn switch_tab_out_of_range_is_a_no_op() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Right;
    let before = state.right.cwd.clone();
    let (state, _) = update(state, Command::SwitchTab(9));
    assert_eq!(state.right.cwd, before);
    assert_eq!(state.right.tab_count(), 1);
}

// ---------------------------------------------------------------------
// Stale background tab refresh on activation (refresh-inactive-panel-on-
// delete; BIG-162)
// ---------------------------------------------------------------------

#[test]
fn switch_tab_to_stale_tab_issues_fresh_read_and_clears_the_flag() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Left;
    state.left.cwd = PathBuf::from("/left");
    state.left.open_tab(); // tab 0 stashed at "/left" (background); tab 1 (active) also starts at "/left"
    state.left.begin_new_listing(PathBuf::from("/left/other")); // active tab moves elsewhere
    state.left.mark_background_tabs_stale(Path::new("/left"));
    assert!(state.left.tabs[0].stale);

    let (state, effects) = update(state, Command::SwitchTab(1));
    assert_eq!(state.left.cwd, PathBuf::from("/left"), "activated the previously-stale background tab");
    assert_eq!(
        without_git_info_effects(effects),
        vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left") }],
        "activating a stale tab must issue a fresh read"
    );

    // The flag must be consumed, not left set: switching away and back
    // must not trigger a second read.
    let (state, _) = update(state, Command::SwitchTab(2));
    assert_eq!(state.left.cwd, PathBuf::from("/left/other"));
    let (_, effects) = update(state, Command::SwitchTab(1));
    assert!(without_git_info_effects(effects).is_empty(), "the stale flag must not persist past its one consuming activation");
}

#[test]
fn close_tab_falling_back_to_stale_neighbor_issues_fresh_read() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Left;
    state.left.cwd = PathBuf::from("/left");
    state.left.open_tab(); // tab 0 stashed at "/left" (background); tab 1 (active) also starts at "/left"
    state.left.begin_new_listing(PathBuf::from("/left/other")); // active tab moves elsewhere
    state.left.mark_background_tabs_stale(Path::new("/left"));

    let (state, effects) = update(state, Command::CloseTab);
    assert_eq!(state.left.tab_count(), 1, "closed the active tab, falling back to the one remaining tab");
    assert_eq!(state.left.cwd, PathBuf::from("/left"), "fell back to the previously-stale background tab");
    assert_eq!(
        without_git_info_effects(effects),
        vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left") }],
        "falling back to a stale neighbor must issue a fresh read"
    );
}

#[test]
fn switch_tab_to_non_stale_tab_issues_no_listing_effects() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Left;
    state.left.cwd = PathBuf::from("/left");
    state.left.open_tab();
    state.left.begin_new_listing(PathBuf::from("/left/other"));
    // No mark_background_tabs_stale call: tab 0 is not stale.

    let (state, effects) = update(state, Command::SwitchTab(1));
    assert_eq!(state.left.cwd, PathBuf::from("/left"));
    assert!(without_git_info_effects(effects).is_empty(), "switching to a tab with no pending staleness must not re-read (unchanged behavior)");
}

#[test]
fn history_up_recalls_previous_commands_newest_first() {
    let mut state = test_state(UiPhase::Panels);
    state.history = vec!["first".to_string(), "second".to_string(), "third".to_string()];
    let state = type_line(state, "x");

    let (state, _) = update(state, Command::CommandLineHistoryPrev);
    assert_eq!(state.command_line, "third");
    let (state, _) = update(state, Command::CommandLineHistoryPrev);
    assert_eq!(state.command_line, "second");
    let (state, _) = update(state, Command::CommandLineHistoryNext);
    assert_eq!(state.command_line, "third");
}

#[test]
fn history_up_stops_at_the_oldest_entry() {
    let mut state = test_state(UiPhase::Panels);
    state.history = vec!["only".to_string()];
    let state = type_line(state, "x");
    let (state, _) = update(state, Command::CommandLineHistoryPrev);
    let (state, _) = update(state, Command::CommandLineHistoryPrev);
    assert_eq!(state.command_line, "only");
}

#[test]
fn history_navigation_never_moves_the_panel_cursor() {
    let mut state = test_state(UiPhase::Panels);
    state.history = vec!["dir".to_string()];
    state.left.entries = vec![file_entry("a", 1), file_entry("b", 2)];
    state.left.cursor = 1;
    let state = type_line(state, "x");
    let (state, _) = update(state, Command::CommandLineHistoryPrev);
    assert_eq!(state.left.cursor, 1);
}

#[test]
fn history_recall_with_empty_history_is_a_noop() {
    let state = type_line(test_state(UiPhase::Panels), "x");
    let (state, effects) = update(state, Command::CommandLineHistoryPrev);
    assert_eq!(state.command_line, "x");
    assert!(effects.is_empty());
}

#[test]
fn running_a_command_records_history_clears_the_buffer_and_persists() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    let state = type_line(state, "dir");
    let (state, effects) = update(state, Command::Enter);

    assert!(state.command_line.is_empty(), "the buffer is cleared once the command is dispatched");
    assert_eq!(state.history, vec!["dir".to_string()]);
    assert_eq!(state.prompt(), format!("{}>", PathBuf::from(r"C:\NORTON").display()));

    match effects.as_slice() {
        [Effect::RunShellCommand(inv, side), Effect::PersistHistory(file)] => {
            assert_eq!(inv.cwd, PathBuf::from(r"C:\NORTON"));
            assert_eq!(inv.args.last().unwrap(), "dir");
            assert_eq!(*side, PanelSide::Left, "the command ran in the active panel, so that panel is re-read");
            assert_eq!(file.commands, vec!["dir".to_string()]);
        }
        other => panic!("expected a shell command plus a history write, got {other:?}"),
    }
}

#[test]
fn running_a_command_uses_the_configured_shell() {
    let mut state = test_state(UiPhase::Panels);
    state.shell.shell = Some("powershell".to_string());
    let state = type_line(state, "Get-ChildItem");
    let (_, effects) = update(state, Command::Enter);
    match effects.first() {
        Some(Effect::RunShellCommand(inv, _)) => {
            assert_eq!(inv.program, "powershell");
            assert_eq!(inv.args, vec!["-NoLogo".to_string(), "-Command".to_string(), "Get-ChildItem".to_string()]);
        }
        other => panic!("expected a shell invocation, got {other:?}"),
    }
}

#[test]
fn enter_with_an_empty_buffer_still_navigates() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("sub")];
    let (_, effects) = update(state, Command::Enter);
    assert_eq!(without_git_info_effects(effects), vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left/sub") }]);
}

// Superseded by the file-action menu (BREAKING, command-line "Enter on an
// executable opens the menu instead of spawning"): Enter on an executable no
// longer spawns it directly on the first keystroke — it opens the menu with
// Run first and highlighted, so `enter_on_an_executable_target_spawns_it_
// through_the_shell` becomes the two tests below: the first Enter opens the
// menu (no spawn), and a second Enter (activating the highlighted Run entry)
// is what actually spawns it.
#[test]
fn enter_on_an_executable_target_opens_the_menu_with_run_first_and_does_not_spawn() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("setup.exe", 1)];
    let (state, effects) = update(state, Command::Enter);
    assert!(effects.is_empty(), "no direct spawn on the first Enter, got {effects:?}");
    let menu = state.file_action_menu.as_ref().expect("Enter on a file opens the file-action menu");
    assert_eq!(menu.target_name, OsString::from("setup.exe"));
    assert_eq!(menu.entries[0], FileActionMenuEntry::Run, "an executable target lists Run first");
    assert_eq!(menu.selected(), FileActionMenuEntry::Run, "Run is highlighted by default");
}

#[test]
fn enter_enter_on_an_executable_spawns_it_through_the_shell() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("setup.exe", 1)];
    let (state, _) = update(state, Command::Enter); // opens the menu, Run highlighted
    let (_, effects) = update(state, Command::FileActionMenuConfirm); // activates Run
    match effects.as_slice() {
        [Effect::RunShellCommand(inv, side)] => {
            assert_eq!(inv.args.last().unwrap(), "\"setup.exe\"");
            assert_eq!(inv.cwd, PathBuf::from("/left"));
            assert_eq!(*side, PanelSide::Left);
        }
        other => panic!("expected the executable to spawn through the shell, got {other:?}"),
    }
}

// Superseded by the file-action menu (file-action-menu "Enter on a file
// opens the action menu"): Enter on a non-executable file used to be a dead
// key; it now opens the menu instead of doing nothing.
#[test]
fn enter_on_a_plain_file_opens_the_action_menu_without_spawning_anything() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("readme.txt", 1)];
    let (state, effects) = update(state, Command::Enter);
    assert!(effects.is_empty());
    let menu = state.file_action_menu.as_ref().expect("Enter on a non-executable file opens the file-action menu");
    assert_eq!(menu.target_name, OsString::from("readme.txt"));
    assert!(!menu.entries.contains(&FileActionMenuEntry::Run), "non-executable: no Run entry");
    assert_eq!(menu.selected(), FileActionMenuEntry::View, "View is highlighted by default");
}

// ---------------------------------------------------------------------
// file-action-menu: navigation, dismissal, selection independence,
// precedence over a non-empty command buffer, and per-action routing.
// ---------------------------------------------------------------------

/// Cursor on `cursor_on` among `names`, menu already open.
fn opened_menu_state(names: &[&str], cursor_on: &str) -> State {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = names.iter().map(|n| file_entry(n, 1)).collect();
    state.left.cursor = names.iter().position(|n| *n == cursor_on).unwrap();
    let (state, _) = update(state, Command::Enter);
    assert!(state.file_action_menu.is_some(), "setup precondition: the menu must be open");
    state
}

#[test]
fn file_action_menu_does_not_consume_or_alter_the_selection() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1), file_entry("report.txt", 2), file_entry("c.txt", 3), file_entry("d.txt", 4), file_entry("e.txt", 5)];
    for n in ["a.txt", "report.txt", "c.txt", "d.txt", "e.txt"] {
        state.left.selected.insert(OsString::from(n));
    }
    state.left.cursor = 1; // report.txt
    let selected_before = state.left.selected.clone();

    let (state, _) = update(state, Command::Enter);
    let menu = state.file_action_menu.as_ref().expect("menu opens");
    assert_eq!(menu.target_name, OsString::from("report.txt"), "the menu targets only the cursor entry");
    assert_eq!(state.left.selected, selected_before, "opening the menu does not touch the selection");
    assert_eq!(state.left.selected.len(), 5);

    let (state, _) = update(state, Command::FileActionMenuCancel);
    assert_eq!(state.left.selected, selected_before, "closing the menu leaves the selection intact");
}

#[test]
fn file_action_menu_does_not_open_when_the_command_buffer_is_non_empty() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    state.left.entries = vec![file_entry("readme.txt", 1)];
    let state = type_line(state, "dir");
    let (state, effects) = update(state, Command::Enter);
    assert!(state.file_action_menu.is_none(), "a non-empty command buffer takes precedence over the menu");
    assert!(matches!(effects.first(), Some(Effect::RunShellCommand(..))), "the typed command still runs, got {effects:?}");
}

#[test]
fn file_action_menu_does_not_open_for_a_directory_which_still_navigates() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("sub")];
    let (state, effects) = update(state, Command::Enter);
    assert!(state.file_action_menu.is_none(), "Enter on a directory must not open the menu");
    assert_eq!(without_git_info_effects(effects), vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left/sub") }]);
}

#[test]
fn file_action_menu_up_down_moves_the_highlight_clamped_at_both_ends() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt");
    // Non-executable: View, Edit, Copy, Rename, Move, Delete, Send to
    // clipboard (7 entries).
    let (state, _) = update(state, Command::FileActionMenuMove(-5));
    assert_eq!(state.file_action_menu.as_ref().unwrap().cursor, 0, "Up from the first entry holds, it does not wrap");
    let (state, _) = update(state, Command::FileActionMenuMove(100));
    let menu = state.file_action_menu.as_ref().unwrap();
    assert_eq!(menu.cursor, menu.entries.len() - 1, "Down past the last entry clamps, it does not wrap");
    assert_eq!(menu.selected(), FileActionMenuEntry::SendToClipboard);
}

#[test]
fn file_action_menu_esc_closes_with_no_action_and_leaves_the_cursor_and_panel_untouched() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt");
    let (state, effects) = update(state, Command::FileActionMenuCancel);
    assert!(state.file_action_menu.is_none());
    assert!(effects.is_empty());
    assert_eq!(state.phase, UiPhase::Panels);
    assert_eq!(state.left.cursor, 0);
}

#[test]
fn file_action_menu_first_letter_hotkey_activates_directly() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt");
    let (state, _) = update(state, Command::FileActionMenuHotkey('D'));
    assert!(state.file_action_menu.is_none(), "the hotkey closes the menu");
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { sources, .. }) => {
            assert_eq!(sources.len(), 1);
            assert_eq!(sources[0].original_name, OsString::from("notes.txt"));
        }
        other => panic!("expected the Delete hotkey to open DeleteConfirm, got {other:?}"),
    }
}

#[test]
fn file_action_menu_unknown_hotkey_is_a_no_op() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt");
    let (state, effects) = update(state, Command::FileActionMenuHotkey('Z'));
    assert!(state.file_action_menu.is_some(), "an unmatched hotkey leaves the menu open");
    assert!(effects.is_empty());
}

#[test]
fn file_action_menu_targets_the_entry_it_opened_on_even_if_the_cursor_drifts_from_a_background_listing_refresh() {
    // Regression: ListingChunk is applied unconditionally regardless of any
    // open modal (including this menu), and insert_streamed re-pins the
    // cursor to row 0 whenever cursor_user_moved is still false — the
    // normal state right after Enter opened the menu. The menu must keep
    // acting on the entry it was opened on (`b.txt`), never on whatever a
    // background listing update lands under the cursor in the meantime.
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("b.txt", 1)];
    state.left.cursor = 0;
    let (state, _) = update(state, Command::Enter); // opens the menu on b.txt
    let menu = state.file_action_menu.as_ref().unwrap();
    assert_eq!(menu.target_name, OsString::from("b.txt"));

    // A background listing chunk inserts a.txt, which sorts ahead of
    // b.txt, and re-pins the (not-yet-user-moved) cursor to row 0.
    let (state, _) = update(state, Command::ListingChunk { panel: PanelSide::Left, entries: vec![file_entry("a.txt", 1)] });
    assert_eq!(state.left.cursor, 0, "setup precondition: the cursor drifted onto a.txt");
    assert_eq!(state.left.entries[state.left.cursor].name, OsString::from("a.txt"));

    let (state, _) = update(state, Command::FileActionMenuHotkey('D'));
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { sources, .. }) => {
            assert_eq!(sources.len(), 1);
            assert_eq!(sources[0].original_name, OsString::from("b.txt"), "must target the entry the menu opened on, not the drifted cursor");
        }
        other => panic!("expected DeleteConfirm targeting b.txt (the menu's original target), got {other:?}"),
    }
}

#[test]
fn file_action_menu_view_routes_to_the_f3_viewer_path() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt");
    let (state, effects) = update(state, Command::FileActionMenuConfirm); // View is highlighted first
    assert!(state.file_action_menu.is_none());
    assert_eq!(effects, vec![Effect::OpenViewer { path: PathBuf::from("/left/notes.txt") }]);
    assert_eq!(state.phase, UiPhase::Panels, "opening is I/O; the phase flips only once ViewerOpened comes back");
}

#[test]
fn file_action_menu_edit_routes_to_the_f4_edit_path() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt");
    let (state, _) = update(state, Command::FileActionMenuMove(1)); // Edit
    let (state, effects) = update(state, Command::FileActionMenuConfirm);
    assert!(state.file_action_menu.is_none());
    assert_eq!(effects, vec![Effect::OpenEditor { path: PathBuf::from("/left/notes.txt") }]);
}

#[test]
fn file_action_menu_copy_opens_the_f5_destination_dialog_scoped_to_the_target_only() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1), file_entry("report.txt", 2)];
    state.left.selected.insert(OsString::from("a.txt")); // an unrelated selection must not leak in
    state.left.cursor = 1; // report.txt
    let (state, _) = update(state, Command::Enter);
    let (state, _) = update(state, Command::FileActionMenuMove(2)); // View, Edit, Copy
    let (state, effects) = update(state, Command::FileActionMenuConfirm);
    assert!(effects.is_empty(), "opening the destination dialog is not itself a mutation");
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, input, .. }) => {
            assert_eq!(kind, JobKind::Copy);
            assert_eq!(sources.len(), 1, "scoped to the menu's target only, not the selection");
            assert_eq!(sources[0].original_name, OsString::from("report.txt"));
            assert_eq!(input, PathBuf::from("/right").display().to_string(), "pre-filled with the opposite panel's path");
        }
        other => panic!("expected FileOpSetup::DestinationInput, got {other:?}"),
    }
}

#[test]
fn file_action_menu_move_opens_the_f6_destination_dialog_scoped_to_the_target_only() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt");
    let (state, _) = update(state, Command::FileActionMenuMove(4)); // View, Edit, Copy, Rename, Move
    let (state, _) = update(state, Command::FileActionMenuConfirm);
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, .. }) => {
            assert_eq!(kind, JobKind::Move);
            assert_eq!(sources.len(), 1);
            assert_eq!(sources[0].original_name, OsString::from("notes.txt"));
        }
        other => panic!("expected FileOpSetup::DestinationInput, got {other:?}"),
    }
}

#[test]
fn file_action_menu_delete_requires_the_existing_confirmation_and_declining_deletes_nothing() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt");
    let (state, _) = update(state, Command::FileActionMenuHotkey('D'));
    let (state, effects) = update(state, Command::FileOpCancel);
    assert_eq!(state.phase, UiPhase::Panels, "declining returns to the panels");
    assert!(effects.is_empty(), "nothing runs, nothing is deleted");
}

#[test]
fn activating_a_mutating_menu_action_changes_nothing_before_its_dialog_is_accepted() {
    // Copy, Move, Rename, and Delete each only *open* a setup dialog on
    // activation — no `Effect::RunJob` (the only mutation-triggering effect)
    // is emitted until that dialog's own Confirm (file-action-menu "No
    // mutation without an intervening dialog").
    for hotkey in ['C', 'M', 'R', 'D'] {
        let state = opened_menu_state(&["notes.txt"], "notes.txt");
        let (state, effects) = update(state, Command::FileActionMenuHotkey(hotkey));
        assert!(!effects.iter().any(|e| matches!(e, Effect::RunJob(_))), "hotkey {hotkey:?} must not run a job on activation alone");
        assert!(matches!(state.phase, UiPhase::FileOpSetup(_)), "hotkey {hotkey:?} should have opened a setup dialog, got {:?}", state.phase);
    }
}

#[test]
fn cancelling_the_interposed_destination_dialog_after_move_starts_no_job() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt");
    let (state, _) = update(state, Command::FileActionMenuHotkey('M'));
    let (state, effects) = update(state, Command::FileOpCancel);
    assert_eq!(state.phase, UiPhase::Panels);
    assert!(effects.is_empty());
}

// ---------------------------------------------------------------------
// file-action-menu: in-place Rename
// ---------------------------------------------------------------------

#[test]
fn rename_pre_fills_the_current_name() {
    let state = opened_menu_state(&["draft.txt"], "draft.txt");
    let (state, _) = update(state, Command::FileActionMenuHotkey('R'));
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::RenameInput { original_name, input, .. }) => {
            assert_eq!(original_name, OsString::from("draft.txt"));
            assert_eq!(input, "draft.txt");
        }
        other => panic!("expected FileOpSetup::RenameInput, got {other:?}"),
    }
}

#[test]
fn rename_confirm_dispatches_a_rename_job_scoped_to_the_target_in_its_own_directory() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    state.left.entries = vec![file_entry("draft.txt", 1)];
    let (state, _) = update(state, Command::Enter);
    let (state, _) = update(state, Command::FileActionMenuHotkey('R'));
    // Clear the "draft.txt" pre-fill via Backspace, then type the new name —
    // driven the same way a real edit of the input would be (file-action-menu
    // "Accepting the dialog SHALL rename the entry ... with the value
    // `final.txt`").
    let state = (0.."draft.txt".len()).fold(state, |s, _| update(s, Command::FileOpInputBackspace).0);
    let state = "final.txt".chars().fold(state, |s, c| update(s, Command::FileOpInputChar(c)).0);
    match &state.phase {
        UiPhase::FileOpSetup(FileOpSetup::RenameInput { input, .. }) => assert_eq!(input, "final.txt"),
        other => panic!("expected FileOpSetup::RenameInput, got {other:?}"),
    }
    let (state, effects) = update(state, Command::FileOpConfirm);
    match effects.as_slice() {
        [Effect::RunJob(job)] => {
            assert_eq!(job.kind, JobKind::Rename);
            assert_eq!(job.sources.len(), 1);
            assert_eq!(job.sources[0].original_name, OsString::from("draft.txt"));
            assert_eq!(job.sources[0].path, PathBuf::from(r"C:\NORTON\draft.txt"));
            assert_eq!(job.source_dir, PathBuf::from(r"C:\NORTON"));
            assert_eq!(job.dest_dir, PathBuf::from(r"C:\NORTON"), "the entry stays in the same directory");
            assert_eq!(job.new_dir_name, Some(OsString::from("final.txt")));
        }
        other => panic!("expected exactly one RunJob effect, got {other:?}"),
    }
    assert!(matches!(state.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Progress { kind: JobKind::Rename, .. }, .. }));
}

/// The reducer carries a case-only edit of the pre-filled name through to
/// the job exactly as typed, without normalizing it away — the identity-
/// aware case-only rename itself happens in the job engine, once dispatched
/// (`fs_ops::worker::run_rename`; see `rename_case_only_change_succeeds_via_
/// identity_check` in `fs_ops::worker::tests`) (file-action-menu "Case-only
/// rename works").
#[test]
fn rename_case_only_edit_reaches_the_job_unchanged() {
    let state = opened_menu_state(&["readme.md"], "readme.md");
    let (state, _) = update(state, Command::FileActionMenuHotkey('R'));
    let state = (0.."readme.md".len()).fold(state, |s, _| update(s, Command::FileOpInputBackspace).0);
    let state = "README.md".chars().fold(state, |s, c| update(s, Command::FileOpInputChar(c)).0);
    let (_, effects) = update(state, Command::FileOpConfirm);
    match effects.as_slice() {
        [Effect::RunJob(job)] => {
            assert_eq!(job.sources[0].original_name, OsString::from("readme.md"));
            assert_eq!(job.new_dir_name, Some(OsString::from("README.md")));
        }
        other => panic!("expected exactly one RunJob effect, got {other:?}"),
    }
}

#[test]
fn rename_esc_renames_nothing() {
    let state = opened_menu_state(&["draft.txt"], "draft.txt");
    let (state, _) = update(state, Command::FileActionMenuHotkey('R'));
    let (state, effects) = update(state, Command::FileOpCancel);
    assert_eq!(state.phase, UiPhase::Panels);
    assert!(effects.is_empty(), "Esc must not dispatch a job");
}

#[test]
fn rename_collision_and_error_surface_through_the_existing_operation_dialogs() {
    // Once dispatched, a Rename job is just another `Job` running through
    // the ordinary `FileOpRunning` machinery — `JobConflict`/`JobError`
    // fold into `RunningDialog::Conflict`/`RunningDialog::Error` exactly as
    // they do for Copy/Move/Delete, regardless of `kind` (file-action-menu
    // "Rename collisions/failures must surface through the existing
    // overwrite-conflict and error-recovery dialogs").
    let running = running_progress_state(JobKind::Rename, "/left", "/left");
    let (state, _) = update(running, Command::JobConflict(sample_conflict()));
    assert!(
        matches!(state.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Conflict { kind: JobKind::Rename, .. }, .. }),
        "a rename collision surfaces the existing conflict dialog, got {:?}",
        state.phase
    );

    let running = running_progress_state(JobKind::Rename, "/left", "/left");
    let err = ErrorInfo { path: PathBuf::from("/left/draft.txt"), message: "access denied".to_string() };
    let (state, _) = update(running, Command::JobError(err));
    assert!(
        matches!(state.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Error { kind: JobKind::Rename, .. }, .. }),
        "a rename error surfaces the existing error dialog, got {:?}",
        state.phase
    );
}

#[test]
fn rename_success_rereads_the_affected_panel() {
    let mut state = test_state(UiPhase::FileOpRunning {
        source_dir: PathBuf::from("/left"),
        dest_dir: PathBuf::from("/left"),
        dialog: RunningDialog::Progress { kind: JobKind::Rename, progress: ProgressInfo::starting(1, 0) },
    });
    state.left.cwd = PathBuf::from("/left");
    let (state, effects) = update(
        state,
        Command::JobDone { outcome: JobOutcome::Completed { skipped: vec![] }, source_dir: PathBuf::from("/left"), dest_dir: PathBuf::from("/left") },
    );
    assert_eq!(state.phase, UiPhase::Panels);
    assert!(
        effects.iter().any(|e| matches!(e, Effect::StartListing { panel: PanelSide::Left, path } if path.as_path() == std::path::Path::new("/left"))),
        "the panel showing the renamed entry's directory is re-read, got {effects:?}"
    );
}

#[test]
fn ctrl_enter_pastes_the_cursor_filename_and_ctrl_bracket_the_full_path() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    state.left.entries = vec![file_entry("README.md", 1)];

    let (name_state, _) = update(state.clone(), Command::PasteCursorName);
    assert_eq!(name_state.command_line, "README.md");

    let (path_state, _) = update(state, Command::PasteCursorPath);
    assert_eq!(path_state.command_line, PathBuf::from(r"C:\NORTON").join("README.md").display().to_string());
}

#[test]
fn pasting_onto_a_non_empty_buffer_separates_with_a_space() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let state = type_line(state, "type");
    let (state, _) = update(state, Command::PasteCursorName);
    assert_eq!(state.command_line, "type a.txt");
}

#[test]
fn pasting_from_an_empty_panel_is_a_noop() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::PasteCursorName);
    assert!(state.command_line.is_empty());
}

#[test]
fn ctrl_o_asks_the_tui_for_the_host_scrollback() {
    let (_, effects) = update(test_state(UiPhase::Panels), Command::ShowScrollback);
    assert_eq!(effects, vec![Effect::ShowScrollback]);
}

#[test]
fn cd_navigates_the_panel_instead_of_spawning_a_shell() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from("/left");
    let state = type_line(state, "cd sub");
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(state.left.cwd, PathBuf::from("/left/sub"));
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::RunShellCommand(..))),
        "`cd` in a fresh child would be a no-op, so it must not reach the shell"
    );
    assert!(effects.contains(&Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left/sub") }));
    assert_eq!(state.history, vec!["cd sub".to_string()], "`cd` is still a history entry");
}

#[test]
fn cd_accepts_a_manually_entered_unc_path() {
    let state = type_line(test_state(UiPhase::Panels), r"cd \\server\share");
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(state.left.cwd, PathBuf::from(r"\\server\share"));
    assert!(effects.contains(&Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from(r"\\server\share") }));
}

#[test]
fn an_unreachable_cd_target_surfaces_the_panel_error_state() {
    let state = type_line(test_state(UiPhase::Panels), r"cd \\nosuchserver\share");
    let (state, _) = update(state, Command::Enter);
    // The read itself happens on the worker thread; its failure comes back
    // through the same event path as any other listing error.
    let (state, _) = update(state, Command::ListingFailed { panel: PanelSide::Left, message: "network path not found".into() });
    assert_eq!(state.left.last_error.as_deref(), Some("network path not found"));
}

#[test]
fn cd_dotdot_and_bare_drive_letters_resolve() {
    assert_eq!(resolve_cd_target(Path::new("/a/b"), ".."), Some(PathBuf::from("/a")));
    assert_eq!(resolve_cd_target(Path::new("/a/b"), "."), Some(PathBuf::from("/a/b")));
    assert_eq!(resolve_cd_target(Path::new("/a"), "D:"), Some(PathBuf::from(r"D:\")));
    assert_eq!(parse_cd("dir"), None);
    assert_eq!(parse_cd("cd  \"C:\\Program Files\" "), Some(r"C:\Program Files".to_string()));
}

// ---------------------------------------------------------------------
// M3: sort modes
// ---------------------------------------------------------------------

#[test]
fn sort_mode_commands_reorder_without_issuing_a_read() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("b.txt", 10), file_entry("a.txt", 30), file_entry("c.txt", 20)];

    let (state, effects) = update(state, Command::SetSortMode { side: PanelSide::Left, mode: SortMode::Size });
    assert!(effects.is_empty(), "sorting must not issue a directory read");
    let names: Vec<String> = state.left.entries.iter().map(|e| e.name.to_string_lossy().into_owned()).collect();
    assert_eq!(names, vec!["b.txt", "c.txt", "a.txt"]);
    assert_eq!(state.left.sort_mode, SortMode::Size);
}

#[test]
fn sort_mode_is_set_per_panel_and_leaves_the_other_alone() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("b", 1), file_entry("a", 2)];
    state.right.entries = vec![file_entry("b", 1), file_entry("a", 2)];
    let (state, _) = update(state, Command::SetSortMode { side: PanelSide::Left, mode: SortMode::Size });
    assert_eq!(state.left.sort_mode, SortMode::Size);
    assert_eq!(state.right.sort_mode, SortMode::Name);
    let right_names: Vec<String> = state.right.entries.iter().map(|e| e.name.to_string_lossy().into_owned()).collect();
    assert_eq!(right_names, vec!["b", "a"], "the untouched panel keeps its order");
}

#[test]
fn every_sort_mode_command_lands_on_its_mode() {
    for mode in [SortMode::Name, SortMode::Extension, SortMode::Time, SortMode::Size, SortMode::Unsorted] {
        let state = test_state(UiPhase::Panels);
        let (state, _) = update(state, Command::SetSortMode { side: PanelSide::Left, mode });
        assert_eq!(state.left.sort_mode, mode);
    }
}

#[test]
fn reread_preserves_the_sort_mode() {
    let mut state = test_state(UiPhase::Panels);
    state.left.sort_mode = SortMode::Size;
    let (state, effects) = update(state, Command::RereadPanel(PanelSide::Left));
    assert_eq!(state.left.sort_mode, SortMode::Size);
    assert_eq!(without_git_info_effects(effects), vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left") }]);
    assert!(state.left.entries.is_empty(), "a re-read discards the current entries and streams fresh ones");
    assert!(state.left.progress.is_streaming());
}

// ---------------------------------------------------------------------
// M3: F9 menus
// ---------------------------------------------------------------------

#[test]
fn f9_opens_the_bar_with_the_left_pulldown_showing() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::MenuOpen);
    let menu = state.menu.expect("F9 opens the bar");
    assert_eq!(menu.active, MenuId::Left);
    assert!(menu.pulldown_open);
}

#[test]
fn esc_closes_the_pulldown_first_then_the_bar() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::MenuOpen);
    let (state, _) = update(state, Command::MenuCollapse);
    let menu = state.menu.as_ref().expect("the bar stays open after the first Esc");
    assert!(!menu.pulldown_open);
    assert_eq!(menu.active, MenuId::Left, "the active title stays highlighted");

    let (state, _) = update(state, Command::MenuCollapse);
    assert!(state.menu.is_none(), "the second Esc closes the bar");
}

#[test]
fn hotkey_letter_jumps_to_another_menu_with_its_pulldown_open() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::MenuOpen);
    let (state, _) = update(state, Command::MenuHotkey('c'));
    let menu = state.menu.expect("bar still open");
    assert_eq!(menu.active, MenuId::Commands);
    assert!(menu.pulldown_open);
}

#[test]
fn horizontal_movement_keeps_a_pulldown_open_and_wraps() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::MenuOpen);
    let (state, _) = update(state, Command::MenuNextMenu);
    assert_eq!(state.menu.as_ref().unwrap().active, MenuId::Files);
    assert!(state.menu.as_ref().unwrap().pulldown_open);

    // Left -> Files -> Commands -> Options -> Right -> wraps to Left.
    let state = (0..4).fold(state, |s, _| update(s, Command::MenuNextMenu).0);
    assert_eq!(state.menu.as_ref().unwrap().active, MenuId::Left);

    let (state, _) = update(state, Command::MenuPrevMenu);
    assert_eq!(state.menu.as_ref().unwrap().active, MenuId::Right);
    assert!(state.menu.as_ref().unwrap().pulldown_open);
}

#[test]
fn vertical_selection_skips_disabled_items_and_separators() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::MenuOpen);
    let (state, _) = update(state, Command::MenuHotkey('f'));
    // Files opens on Copy (View/Edit are M4 and disabled).
    assert_eq!(state.menu.as_ref().unwrap().selected_item().map(|i| i.label), Some("Copy"));
    let (state, _) = update(state, Command::MenuSelectNext);
    assert_eq!(state.menu.as_ref().unwrap().selected_item().map(|i| i.label), Some("Rename/Move"));
    let (state, _) = update(state, Command::MenuSelectPrev);
    assert_eq!(state.menu.as_ref().unwrap().selected_item().map(|i| i.label), Some("Copy"));
}

#[test]
fn enter_dispatches_the_selected_item_and_closes_the_overlay() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::MenuOpen);
    let (state, _) = update(state, Command::MenuHotkey('f'));
    let (state, _) = update(state, Command::MenuActivate); // Copy

    assert!(state.menu.is_none(), "activating an item closes the whole overlay");
    assert!(
        matches!(state.phase, UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind: JobKind::Copy, .. })),
        "the item's action ran, got {:?}",
        state.phase
    );
}

#[test]
fn left_menu_sorts_the_left_panel_even_when_the_right_one_is_focused() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Right;
    state.left.entries = vec![file_entry("b", 1), file_entry("a", 2)];

    let (state, _) = update(state, Command::MenuOpen); // Left menu
    // Opens on Brief; Down walks Full, Tree, Quick view, Info, then (the
    // separator skipped, not counted) Name, Extension, Modif. time, Size.
    let state = (0..8).fold(state, |s, _| update(s, Command::MenuSelectNext).0);
    assert_eq!(state.menu.as_ref().unwrap().selected_item().map(|i| i.label), Some("Size"));
    let (state, _) = update(state, Command::MenuActivate);

    assert_eq!(state.left.sort_mode, SortMode::Size, "the Left menu targets the left panel regardless of focus");
    assert_eq!(state.right.sort_mode, SortMode::Name);
}

#[test]
fn right_menu_targets_the_right_panel() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::MenuOpen);
    let (state, _) = update(state, Command::MenuHotkey('r'));
    // Opens on Brief; Down four times walks Full, Tree, Quick view, Info.
    let state = (0..4).fold(state, |s, _| update(s, Command::MenuSelectNext).0);
    let (state, effects) = update(state, Command::MenuActivate); // Info
    assert_eq!(state.right.display_mode, DisplayMode::Info);
    assert_eq!(state.left.display_mode, DisplayMode::Full);
    assert!(effects.iter().any(|e| matches!(e, Effect::QueryInfo { panel: PanelSide::Right, .. })));
}

#[test]
fn activating_options_themes_opens_the_theme_picker_and_closes_the_menu() {
    // Options used to be entirely disabled (the old target of this test);
    // Themes (visual-themes) is now its first enabled item, so activating
    // Options and pressing Enter opens the picker instead of doing nothing
    // (theme-selection "Options menu opens the theme picker").
    let (state, _) = update(test_state(UiPhase::Panels), Command::MenuOpen);
    let (state, _) = update(state, Command::MenuHotkey('o')); // Options: Themes is first-selectable
    let (state, effects) = update(state, Command::MenuActivate);
    assert!(state.menu.is_none(), "activating an item closes the whole menu overlay");
    assert!(state.theme_picker.is_some(), "Options -> Themes opens the picker");
    assert!(effects.is_empty());
}

#[test]
fn panel_keys_do_not_leak_through_an_open_menu() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a", 1), file_entry("b", 2)];
    let (state, _) = update(state, Command::MenuOpen);
    // MoveCursor is not a menu command; the mapper would never emit it while
    // the bar is open, but the overlay must not be disturbed if it does.
    let (state, _) = update(state, Command::MenuSelectNext);
    assert_eq!(state.left.cursor, 0);
    assert!(state.menu.is_some());
}

#[test]
fn every_menu_item_that_is_enabled_maps_to_a_command() {
    for id in crate::menu::ALL_MENUS {
        for entry in crate::menu::entries(id) {
            if let MenuEntry::Item(item) = entry {
                if item.is_enabled() {
                    assert!(
                        menu_action_command(item.action, PanelSide::Left).is_some(),
                        "`{}` is enabled but dispatches nothing",
                        item.label
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------
// M3: drive select
// ---------------------------------------------------------------------

#[test]
fn alt_f1_and_alt_f2_target_their_own_panels() {
    for side in [PanelSide::Left, PanelSide::Right] {
        let (_, effects) = update(test_state(UiPhase::Panels), Command::OpenDriveSelect(side));
        assert_eq!(effects, vec![Effect::EnumerateDrives(side)]);
    }
}

#[test]
fn drive_list_opens_the_dialog_and_requests_every_label_lazily() {
    let (state, effects) = update(
        test_state(UiPhase::Panels),
        Command::DriveListReady { target: PanelSide::Left, drives: vec!['A', 'C', 'D'] },
    );
    let dialog = state.drive_select.as_ref().expect("the dialog opened");
    assert_eq!(dialog.target, PanelSide::Left);
    assert_eq!(dialog.drives.len(), 3, "every letter is present on the first frame");
    assert!(dialog.drives.iter().all(|d| d.label.is_none()), "labels are still pending");
    let generation = dialog.generation;
    assert_eq!(
        effects,
        vec![
            Effect::FetchDriveLabel { target: PanelSide::Left, letter: 'A', generation },
            Effect::FetchDriveLabel { target: PanelSide::Left, letter: 'C', generation },
            Effect::FetchDriveLabel { target: PanelSide::Left, letter: 'D', generation },
        ]
    );
}

#[test]
fn the_dialog_opens_on_the_target_panel_s_current_drive() {
    let mut state = test_state(UiPhase::Panels);
    state.right.cwd = PathBuf::from(r"D:\work");
    let (state, _) = update(state, Command::DriveListReady { target: PanelSide::Right, drives: vec!['C', 'D'] });
    assert_eq!(state.drive_select.unwrap().selected_letter(), Some('D'));
}

#[test]
fn a_resolved_label_fills_in_place_without_moving_other_rows() {
    let (state, _) = update(
        test_state(UiPhase::Panels),
        Command::DriveListReady { target: PanelSide::Left, drives: vec!['A', 'C'] },
    );
    let generation = state.drive_select.as_ref().unwrap().generation;
    let (state, effects) = update(
        state,
        Command::DriveLabelResolved { target: PanelSide::Left, letter: 'C', label: Some("OS".to_string()), generation },
    );
    assert!(effects.is_empty());
    let dialog = state.drive_select.unwrap();
    assert_eq!(dialog.drives, vec![DriveEntry { letter: 'A', label: None }, DriveEntry { letter: 'C', label: Some("OS".to_string()) }]);
}

#[test]
fn a_label_arriving_after_the_dialog_closed_is_discarded() {
    let (state, _) = update(
        test_state(UiPhase::Panels),
        Command::DriveListReady { target: PanelSide::Left, drives: vec!['C'] },
    );
    let generation = state.drive_select.as_ref().unwrap().generation;
    let (state, _) = update(state, Command::DriveSelectCancel);
    assert!(state.drive_select.is_none());

    let (state, effects) = update(
        state,
        Command::DriveLabelResolved { target: PanelSide::Left, letter: 'C', label: Some("OS".to_string()), generation },
    );
    assert!(state.drive_select.is_none(), "a stale result must not resurrect the dialog");
    assert!(effects.is_empty());
    assert_eq!(state.left.cwd, PathBuf::from("/left"), "and must not touch the panel");
}

#[test]
fn a_label_for_a_superseded_target_panel_is_discarded() {
    let (state, _) = update(
        test_state(UiPhase::Panels),
        Command::DriveListReady { target: PanelSide::Left, drives: vec!['C'] },
    );
    let first_generation = state.drive_select.as_ref().unwrap().generation;
    // The dialog was reopened for the other panel before the label landed.
    let (state, _) = update(state, Command::DriveListReady { target: PanelSide::Right, drives: vec!['C'] });
    let (state, _) = update(
        state,
        Command::DriveLabelResolved { target: PanelSide::Left, letter: 'C', label: Some("OS".to_string()), generation: first_generation },
    );
    assert_eq!(state.drive_select.unwrap().drives[0].label, None);
}

#[test]
fn a_stale_drive_label_from_a_quick_reopen_of_the_same_target_is_discarded_but_the_fresher_one_applies() {
    // Regression coverage for the out-of-order-completion race: the user
    // cancels and reopens the drive dialog for the *same* panel before the
    // first fetch lands (Alt+F1, Esc, Alt+F1 again). `target` alone can't
    // tell the two sessions apart — only `generation` can.
    let (state, _) =
        update(test_state(UiPhase::Panels), Command::DriveListReady { target: PanelSide::Left, drives: vec!['C'] });
    let first_generation = state.drive_select.as_ref().unwrap().generation;

    let (state, _) = update(state, Command::DriveSelectCancel);
    let (state, _) =
        update(state, Command::DriveListReady { target: PanelSide::Left, drives: vec!['C'] });
    let second_generation = state.drive_select.as_ref().unwrap().generation;
    assert_ne!(first_generation, second_generation, "reopening mints a fresh generation");

    let (state, _) = update(
        state,
        Command::DriveLabelResolved { target: PanelSide::Left, letter: 'C', label: Some("STALE".to_string()), generation: first_generation },
    );
    assert_eq!(state.drive_select.as_ref().unwrap().drives[0].label, None, "the stale (first-generation) answer is dropped");

    let (state, _) = update(
        state,
        Command::DriveLabelResolved { target: PanelSide::Left, letter: 'C', label: Some("FRESH".to_string()), generation: second_generation },
    );
    assert_eq!(state.drive_select.unwrap().drives[0].label, Some("FRESH".to_string()), "the current generation's answer applies");
}

#[test]
fn esc_dismisses_the_dialog_leaving_the_panel_where_it_was() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\Users");
    let (state, _) = update(state, Command::DriveListReady { target: PanelSide::Left, drives: vec!['C', 'D'] });
    let (state, effects) = update(state, Command::DriveSelectCancel);
    assert!(state.drive_select.is_none());
    assert!(effects.is_empty());
    assert_eq!(state.left.cwd, PathBuf::from(r"C:\Users"));
}

#[test]
fn selecting_a_drive_switches_the_target_panel_to_its_root() {
    let (state, _) = update(
        test_state(UiPhase::Panels),
        Command::DriveListReady { target: PanelSide::Right, drives: vec!['C', 'D'] },
    );
    let (state, _) = update(state, Command::DriveSelectMove(1));
    let (state, effects) = update(state, Command::DriveSelectConfirm);
    assert!(state.drive_select.is_none(), "confirming closes the dialog");
    assert_eq!(state.right.cwd, PathBuf::from(r"D:\"));
    assert_eq!(without_git_info_effects(effects), vec![Effect::StartListing { panel: PanelSide::Right, path: PathBuf::from(r"D:\") }]);
}

#[test]
fn selecting_an_unavailable_drive_surfaces_the_panel_error_state() {
    let (state, _) = update(
        test_state(UiPhase::Panels),
        Command::DriveListReady { target: PanelSide::Left, drives: vec!['A'] },
    );
    let (state, effects) = update(state, Command::DriveSelectConfirm);
    assert_eq!(without_git_info_effects(effects), vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from(r"A:\") }]);
    // The worker's read fails and comes back as a normal listing failure —
    // no dedicated hang-prone path.
    let (state, _) = update(state, Command::ListingFailed { panel: PanelSide::Left, message: "device not ready".into() });
    assert_eq!(state.left.last_error.as_deref(), Some("device not ready"));
}

#[test]
fn drive_select_claims_navigation_keys_while_open() {
    let (state, _) = update(
        test_state(UiPhase::Panels),
        Command::DriveListReady { target: PanelSide::Left, drives: vec!['A', 'C', 'D'] },
    );
    let (state, _) = update(state, Command::DriveSelectMove(2));
    assert_eq!(state.drive_select.as_ref().unwrap().selected_letter(), Some('D'));
    let (state, _) = update(state, Command::DriveSelectMove(-1));
    assert_eq!(state.drive_select.unwrap().selected_letter(), Some('C'));
}

// ---------------------------------------------------------------------
// M3: Info mode
// ---------------------------------------------------------------------

#[test]
fn ctrl_l_toggles_info_for_one_panel_only() {
    let state = test_state(UiPhase::Panels);
    let (state, effects) = update(state, Command::ToggleInfoMode(PanelSide::Left));
    assert_eq!(state.left.display_mode, DisplayMode::Info);
    assert_eq!(state.right.display_mode, DisplayMode::Full, "the opposite panel is untouched");
    let request = state.left.info_request.expect("a request id was minted");
    assert_eq!(effects, vec![Effect::QueryInfo { panel: PanelSide::Left, path: PathBuf::from("/left"), request }]);

    let (state, effects) = update(state, Command::ToggleInfoMode(PanelSide::Left));
    assert_eq!(state.left.display_mode, DisplayMode::Full);
    assert!(effects.is_empty(), "leaving Info mode queries nothing");
}

#[test]
fn info_values_start_pending_and_resolve_in_place() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::ToggleInfoMode(PanelSide::Left));
    assert_eq!(state.left.info, InfoValues::default(), "every value starts unresolved");

    let request = state.left.info_request.unwrap();
    let values = InfoValues { file_count: Some(12), dir_count: Some(3), ..InfoValues::default() };
    let (state, effects) = update(
        state,
        Command::InfoResolved { panel: PanelSide::Left, path: PathBuf::from("/left"), request, values: values.clone() },
    );
    assert!(effects.is_empty());
    assert_eq!(state.left.info, values);
}

#[test]
fn an_info_result_for_a_directory_the_panel_left_is_discarded() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::ToggleInfoMode(PanelSide::Left));
    let request = state.left.info_request.unwrap();
    let values = InfoValues { file_count: Some(12), ..InfoValues::default() };
    let (state, _) =
        update(state, Command::InfoResolved { panel: PanelSide::Left, path: PathBuf::from("/elsewhere"), request, values });
    assert_eq!(state.left.info, InfoValues::default(), "a result for another directory is dropped");
}

#[test]
fn an_info_result_arriving_after_info_mode_was_left_is_discarded() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::ToggleInfoMode(PanelSide::Left));
    let request = state.left.info_request.unwrap();
    let (state, _) = update(state, Command::ToggleInfoMode(PanelSide::Left)); // back to Full
    let values = InfoValues { file_count: Some(12), ..InfoValues::default() };
    let (state, _) = update(state, Command::InfoResolved { panel: PanelSide::Left, path: PathBuf::from("/left"), request, values });
    assert_eq!(state.left.info, InfoValues::default());
}

#[test]
fn navigating_while_in_info_mode_re_queries_for_the_new_directory() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("sub")];
    let (state, _) = update(state, Command::ToggleInfoMode(PanelSide::Left));
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(state.left.cwd, PathBuf::from("/left/sub"));
    let request = state.left.info_request.expect("navigating while in Info mode mints a fresh request");
    assert!(effects.contains(&Effect::QueryInfo { panel: PanelSide::Left, path: PathBuf::from("/left/sub"), request }));
    assert_eq!(state.left.info, InfoValues::default(), "the previous directory's figures are cleared");
}

#[test]
fn a_stale_info_result_from_an_out_of_order_reread_is_discarded_but_the_fresher_one_applies() {
    // Regression coverage for the out-of-order-completion race: a double
    // Ctrl+R (or any two RereadPanel commands landing before the first
    // reply arrives) mints two different request ids for the *same*
    // directory, so `path` equality alone can't tell the stale reply apart
    // from the current one.
    let (state, _) = update(test_state(UiPhase::Panels), Command::ToggleInfoMode(PanelSide::Left));
    let first_request = state.left.info_request.unwrap();

    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left));
    let second_request = state.left.info_request.unwrap();
    assert_ne!(first_request, second_request, "re-reading mints a fresh request id even for the same path");

    let stale_values = InfoValues { file_count: Some(999), ..InfoValues::default() };
    let (state, _) = update(
        state,
        Command::InfoResolved { panel: PanelSide::Left, path: PathBuf::from("/left"), request: first_request, values: stale_values },
    );
    assert_eq!(state.left.info, InfoValues::default(), "the stale (first-request) answer is dropped");

    let fresh_values = InfoValues { file_count: Some(7), ..InfoValues::default() };
    let (state, _) = update(
        state,
        Command::InfoResolved {
            panel: PanelSide::Left,
            path: PathBuf::from("/left"),
            request: second_request,
            values: fresh_values.clone(),
        },
    );
    assert_eq!(state.left.info, fresh_values, "the current request's answer applies");
}

#[test]
fn a_panel_not_in_info_mode_issues_no_info_query_when_it_navigates() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("sub")];
    let (_, effects) = update(state, Command::Enter);
    assert!(!effects.iter().any(|e| matches!(e, Effect::QueryInfo { .. })));
}

// ---------------------------------------------------------------------
// M4: F3 viewer & F4 external editor
// ---------------------------------------------------------------------

#[test]
fn f3_on_a_file_dispatches_open_viewer_without_touching_the_filesystem() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("notes.txt", 42)];
    let (state, effects) = update(state, Command::RequestViewer);
    assert_eq!(effects, vec![Effect::OpenViewer { path: PathBuf::from("/left/notes.txt") }]);
    // Nothing changed yet — the phase flips only once `ViewerOpened` comes
    // back, since opening is I/O and `update` performs none itself.
    assert_eq!(state.phase, UiPhase::Panels);
}

#[test]
fn f3_on_a_directory_or_parent_dir_is_a_no_op() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("sub")];
    let (_, effects) = update(state, Command::RequestViewer);
    assert!(effects.is_empty());

    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![Entry::parent_dir()];
    let (_, effects) = update(state, Command::RequestViewer);
    assert!(effects.is_empty());
}

#[test]
fn f3_on_an_empty_panel_is_a_no_op() {
    let state = test_state(UiPhase::Panels);
    let (_, effects) = update(state, Command::RequestViewer);
    assert!(effects.is_empty());
}

#[test]
fn viewer_opened_enters_the_viewer_phase_at_the_start_of_the_file() {
    let state = test_state(UiPhase::Panels);
    let (state, effects) = update(state, Command::ViewerOpened { path: PathBuf::from("/left/big.log"), file_len: 5_000_000_000 });
    assert!(effects.is_empty());
    match state.phase {
        UiPhase::Viewer(v) => {
            assert_eq!(v.path, PathBuf::from("/left/big.log"));
            assert_eq!(v.file_len, 5_000_000_000);
            assert_eq!(v.top_offset, 0);
            assert_eq!(v.mode, crate::viewer::ViewMode::Text);
            assert!(!v.wrap);
        }
        other => panic!("expected UiPhase::Viewer, got {other:?}"),
    }
}

#[test]
fn viewer_open_failed_surfaces_an_inline_error_on_the_active_panel_instead_of_opening() {
    let state = test_state(UiPhase::Panels);
    let (state, effects) = update(state, Command::ViewerOpenFailed { message: "access denied".to_string() });
    assert!(effects.is_empty());
    assert_eq!(state.phase, UiPhase::Panels);
    assert_eq!(state.left.last_error.as_deref(), Some("access denied"));
}

#[test]
fn a_subsequently_successful_f3_open_clears_a_stale_last_error_from_an_earlier_failed_attempt() {
    let mut state = test_state(UiPhase::Panels);
    let (state_after_failure, _) = update(state.clone(), Command::ViewerOpenFailed { message: "access denied".to_string() });
    assert_eq!(state_after_failure.left.last_error.as_deref(), Some("access denied"));

    state.left.last_error = Some("access denied".to_string());
    let (state, _) = update(state, Command::ViewerOpened { path: PathBuf::from("/left/notes.txt"), file_len: 10 });
    assert_eq!(
        state.left.last_error, None,
        "a successful F3 open must clear a stale error left by an earlier failed attempt"
    );
    assert!(matches!(state.phase, UiPhase::Viewer(_)));
}

fn opened_viewer_state(file_len: u64) -> State {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::ViewerOpened { path: PathBuf::from("/left/f.txt"), file_len });
    state
}

#[test]
fn f10_closes_the_viewer_and_returns_focus_to_the_panels() {
    let state = opened_viewer_state(1000);
    let (state, effects) = update(state, Command::ViewerClose);
    assert_eq!(state.phase, UiPhase::Panels);
    assert!(effects.is_empty());
}

#[test]
fn f4_in_the_viewer_toggles_mode_and_the_key_bar_label_swaps() {
    let state = opened_viewer_state(1000);
    let (state, effects) = update(state, Command::ViewerToggleMode);
    assert!(effects.is_empty());
    let UiPhase::Viewer(v) = &state.phase else { panic!("expected viewer phase") };
    assert_eq!(v.mode, crate::viewer::ViewMode::Hex);
    assert_eq!(v.mode.toggle_label(), "ASCII");

    let (state, _) = update(state, Command::ViewerToggleMode);
    let UiPhase::Viewer(v) = &state.phase else { panic!("expected viewer phase") };
    assert_eq!(v.mode, crate::viewer::ViewMode::Text);
    assert_eq!(v.mode.toggle_label(), "Hex");
}

#[test]
fn f2_in_the_viewer_toggles_wrap_and_resets_horizontal_scroll() {
    let mut state = opened_viewer_state(1000);
    if let UiPhase::Viewer(v) = &mut state.phase {
        v.h_scroll = 12;
    }
    let (state, _) = update(state, Command::ViewerToggleWrap);
    let UiPhase::Viewer(v) = &state.phase else { panic!("expected viewer phase") };
    assert!(v.wrap);
    assert_eq!(v.h_scroll, 0);
}

#[test]
fn viewer_set_top_clamps_to_the_file_length() {
    let state = opened_viewer_state(100);
    let (state, _) = update(state, Command::ViewerSetTop(1_000_000));
    let UiPhase::Viewer(v) = &state.phase else { panic!("expected viewer phase") };
    assert_eq!(v.top_offset, 100);
}

#[test]
fn viewer_set_h_scroll_updates_the_column_indicator() {
    let state = opened_viewer_state(1000);
    let (state, _) = update(state, Command::ViewerSetHScroll(30));
    let UiPhase::Viewer(v) = &state.phase else { panic!("expected viewer phase") };
    assert_eq!(v.h_scroll, 30);
}

#[test]
fn viewer_keys_are_not_forwarded_to_panels_while_the_viewer_is_open() {
    // ToggleActivePanel is a Panels-phase command; while the viewer owns
    // the phase it must be swallowed rather than mutating panel focus
    // (viewer: Frame-less full-screen chrome — "Viewer owns focus while
    // open").
    let state = opened_viewer_state(1000);
    let active_before = state.active;
    let (state, effects) = update(state, Command::ToggleActivePanel);
    assert_eq!(state.active, active_before);
    assert!(effects.is_empty());
    assert!(matches!(state.phase, UiPhase::Viewer(_)));
}

#[test]
fn viewer_search_confirm_dispatches_the_search_effect_and_keeps_the_pattern() {
    let state = opened_viewer_state(1000);
    let (state, _) = update(state, Command::ViewerSearchStart);
    let (state, _) = update(state, Command::ViewerSearchChar('a'));
    let (state, _) = update(state, Command::ViewerSearchChar('b'));
    let (state, effects) = update(state, Command::ViewerSearchConfirm);
    let UiPhase::Viewer(v) = &state.phase else { panic!("expected viewer phase") };
    let request = v.search_request.expect("a search request id must be minted and recorded");
    assert_eq!(
        effects,
        vec![Effect::RunViewerSearch { path: PathBuf::from("/left/f.txt"), start_offset: 0, pattern: b"ab".to_vec(), request }]
    );
    assert_eq!(v.search_pattern, Some(b"ab".to_vec()));
    assert_eq!(v.search_input, None, "the prompt closes once the search is dispatched");
}

#[test]
fn viewer_search_backspace_and_cancel_edit_the_in_progress_pattern() {
    let state = opened_viewer_state(1000);
    let (state, _) = update(state, Command::ViewerSearchStart);
    let (state, _) = update(state, Command::ViewerSearchChar('x'));
    let (state, _) = update(state, Command::ViewerSearchBackspace);
    let UiPhase::Viewer(v) = &state.phase else { panic!("expected viewer phase") };
    assert_eq!(v.search_input, Some(String::new()));

    let (state, _) = update(state, Command::ViewerSearchCancel);
    let UiPhase::Viewer(v) = &state.phase else { panic!("expected viewer phase") };
    assert_eq!(v.search_input, None);
}

#[test]
fn empty_search_pattern_does_not_dispatch_a_search() {
    let state = opened_viewer_state(1000);
    let (state, _) = update(state, Command::ViewerSearchStart);
    let (state, effects) = update(state, Command::ViewerSearchConfirm);
    assert!(effects.is_empty());
    let UiPhase::Viewer(v) = &state.phase else { panic!("expected viewer phase") };
    assert_eq!(v.search_pattern, None);
}

/// Run a real F7 search-confirm sequence so the returned state carries the
/// actual outstanding `search_request` id, mirroring how the TUI event loop
/// would drive it (`ViewerSearchStart` -> typed chars -> `ViewerSearchConfirm`).
fn viewer_with_pattern_confirmed(state: State, pattern: &str) -> (State, u64) {
    let (state, _) = update(state, Command::ViewerSearchStart);
    let mut state = state;
    for c in pattern.chars() {
        let (s, _) = update(state, Command::ViewerSearchChar(c));
        state = s;
    }
    let (state, _) = update(state, Command::ViewerSearchConfirm);
    let UiPhase::Viewer(v) = &state.phase else { panic!("expected viewer phase") };
    let request = v.search_request.expect("search confirm must record an outstanding request id");
    (state, request)
}

#[test]
fn viewer_search_result_moves_the_top_anchor_and_highlights_the_match() {
    let state = opened_viewer_state(1000);
    let (state, request) = viewer_with_pattern_confirmed(state, "ab");
    let (state, _) = update(state, Command::ViewerSearchResult { offset: Some(250), match_range: Some((250, 256)), request });
    let UiPhase::Viewer(v) = &state.phase else { panic!("expected viewer phase") };
    assert_eq!(v.top_offset, 250);
    assert_eq!(v.last_match, Some((250, 256)));
}

#[test]
fn viewer_search_result_with_no_match_leaves_the_top_anchor_untouched() {
    let mut state = opened_viewer_state(1000);
    if let UiPhase::Viewer(v) = &mut state.phase {
        v.top_offset = 40;
    }
    let (state, request) = viewer_with_pattern_confirmed(state, "zz");
    let (state, _) = update(state, Command::ViewerSearchResult { offset: None, match_range: None, request });
    let UiPhase::Viewer(v) = &state.phase else { panic!("expected viewer phase") };
    assert_eq!(v.top_offset, 40);
}

#[test]
fn a_stale_out_of_order_search_reply_is_dropped_not_applied() {
    // The M3-style staleness race: a first search is confirmed (minting
    // request id N), then — before its reply arrives — a second search is
    // confirmed (minting N+1) which is the one still outstanding. The first
    // search's (now stale) reply must be silently dropped rather than
    // clobbering the top offset/match the second search is still waiting
    // on to fill in.
    let mut state = opened_viewer_state(1000);
    if let UiPhase::Viewer(v) = &mut state.phase {
        v.top_offset = 5;
    }
    let (state, stale_request) = viewer_with_pattern_confirmed(state, "first");
    let (state, current_request) = viewer_with_pattern_confirmed(state, "second");
    assert_ne!(stale_request, current_request);

    // The stale reply from the first (superseded) search arrives late.
    let (state, _) =
        update(state, Command::ViewerSearchResult { offset: Some(900), match_range: Some((900, 905)), request: stale_request });
    let UiPhase::Viewer(v) = &state.phase else { panic!("expected viewer phase") };
    assert_eq!(v.top_offset, 5, "a stale reply must not move the top offset");
    assert_eq!(v.last_match, None, "a stale reply must not set a phantom match highlight");

    // The still-current search's reply, once it arrives, is applied.
    let (state, _) = update(
        state,
        Command::ViewerSearchResult { offset: Some(700), match_range: Some((700, 706)), request: current_request },
    );
    let UiPhase::Viewer(v) = &state.phase else { panic!("expected viewer phase") };
    assert_eq!(v.top_offset, 700);
    assert_eq!(v.last_match, Some((700, 706)));
}

#[test]
fn a_stale_search_reply_for_a_closed_and_reopened_viewer_session_is_dropped() {
    // A search is confirmed against one open file, then the viewer is
    // closed and a different file opened (or even the same file reopened —
    // it is a fresh session either way, per `ViewerState::new`). The first
    // session's stale reply must not be applied to the new session even
    // though `state.phase` is `UiPhase::Viewer(_)` again by the time it
    // arrives.
    let state = opened_viewer_state(1000);
    let (state, stale_request) = viewer_with_pattern_confirmed(state, "needle");

    let (state, _) = update(state, Command::ViewerClose);
    let (state, _) = update(state, Command::ViewerOpened { path: PathBuf::from("/left/other.txt"), file_len: 200 });
    let UiPhase::Viewer(v) = &state.phase else { panic!("expected viewer phase") };
    assert_eq!(v.search_request, None, "a freshly opened session has no outstanding search");

    let (state, _) =
        update(state, Command::ViewerSearchResult { offset: Some(50), match_range: Some((50, 56)), request: stale_request });
    let UiPhase::Viewer(v) = &state.phase else { panic!("expected viewer phase") };
    assert_eq!(v.path, PathBuf::from("/left/other.txt"));
    assert_eq!(v.top_offset, 0, "the stale reply from the closed session must not move the new session's top offset");
    assert_eq!(v.last_match, None, "the stale reply must not set a phantom match highlight on the new session");
}

#[test]
fn f4_from_a_panel_with_no_editor_configured_shows_the_message_and_spawns_nothing() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("report.txt", 10)];
    let (state, effects) = update(state, Command::RequestExternalEditor);
    assert!(effects.is_empty());
    assert_eq!(state.left.last_error.as_deref(), Some(crate::external_editor::NO_EDITOR_CONFIGURED_MESSAGE));
}

#[test]
fn f4_from_a_panel_with_a_blank_editor_command_is_also_treated_as_unset() {
    let mut state = test_state(UiPhase::Panels);
    state.editor = Some("   ".to_string());
    state.left.entries = vec![file_entry("report.txt", 10)];
    let (_, effects) = update(state, Command::RequestExternalEditor);
    assert!(effects.is_empty());
}

#[test]
fn f4_on_a_directory_entry_does_not_launch_the_editor() {
    let mut state = test_state(UiPhase::Panels);
    state.editor = Some("notepad".to_string());
    state.left.entries = vec![dir_entry("sub")];
    let (state, effects) = update(state, Command::RequestExternalEditor);
    assert!(effects.is_empty());
    assert_eq!(state.left.last_error, None, "a directory target is silently ignored, not an error dialog");
}

#[test]
fn f4_on_parent_dir_does_not_launch_the_editor() {
    let mut state = test_state(UiPhase::Panels);
    state.editor = Some("notepad".to_string());
    state.left.entries = vec![Entry::parent_dir()];
    let (_, effects) = update(state, Command::RequestExternalEditor);
    assert!(effects.is_empty());
}

#[test]
fn f4_launches_the_configured_editor_on_the_file_under_the_cursor() {
    let mut state = test_state(UiPhase::Panels);
    state.editor = Some("notepad".to_string());
    state.active = PanelSide::Right;
    state.right.cwd = PathBuf::from(r"C:\work");
    state.right.entries = vec![file_entry("report.txt", 10)];
    let (_, effects) = update(state, Command::RequestExternalEditor);
    match effects.as_slice() {
        [Effect::RunExternalEditor(inv, PanelSide::Right)] => {
            assert_eq!(inv.program, "notepad");
            assert_eq!(inv.file_arg, OsString::from("report.txt"));
            assert_eq!(inv.cwd, PathBuf::from(r"C:\work"));
        }
        other => panic!("expected a single RunExternalEditor effect, got {other:?}"),
    }
}

#[test]
fn f4_passes_the_original_os_string_file_name_through_without_lossy_conversion() {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let raw = std::ffi::OsStr::from_bytes(&[0x66, 0x6f, 0x80, 0x6f]).to_os_string();
        let mut state = test_state(UiPhase::Panels);
        state.editor = Some("notepad".to_string());
        state.left.entries = vec![Entry { name: raw.clone(), kind: EntryKind::File, size: 0, modified: None }];
        let (_, effects) = update(state, Command::RequestExternalEditor);
        match effects.as_slice() {
            [Effect::RunExternalEditor(inv, _)] => assert_eq!(inv.file_arg, raw),
            other => panic!("expected a single RunExternalEditor effect, got {other:?}"),
        }
    }
}

#[test]
fn external_editor_spawn_failure_surfaces_an_inline_error_without_crashing() {
    let state = test_state(UiPhase::Panels);
    let (state, effects) = update(state, Command::ExternalEditorSpawnFailed { message: "program not found".to_string() });
    assert!(effects.is_empty());
    assert_eq!(state.left.last_error.as_deref(), Some("program not found"));
    assert_eq!(state.phase, UiPhase::Panels, "still running, not crashed");
}

#[test]
fn a_subsequently_successful_f4_launch_clears_a_stale_last_error_from_an_earlier_failed_attempt() {
    let mut state = test_state(UiPhase::Panels);
    state.editor = Some("notepad".to_string());
    state.left.entries = vec![file_entry("report.txt", 10)];
    state.left.last_error = Some("program not found".to_string());

    let (state, effects) = update(state, Command::RequestExternalEditor);
    assert_eq!(
        state.left.last_error, None,
        "successfully dispatching the F4 editor spawn must clear a stale error from an earlier failed attempt"
    );
    assert!(matches!(effects.as_slice(), [Effect::RunExternalEditor(_, PanelSide::Left)]));
}

// ---------------------------------------------------------------------
// M5: git_info — generation-key staleness guard (design D3; git-info
// "Query re-issued on navigation", "Silent absence on timeout and
// stale-result discarding")
// ---------------------------------------------------------------------

#[test]
fn initial_state_issues_a_git_info_query_for_each_panel() {
    let (state, effects) = State::initial(Theme::classic(), (80, 24), 0, PathBuf::from("/l"), PathBuf::from("/r"), false);
    let left_request = state.left.git_request.expect("left panel mints a git-info request on startup");
    let right_request = state.right.git_request.expect("right panel mints a git-info request on startup");
    assert_ne!(left_request, right_request);
    assert!(effects.contains(&Effect::QueryGitInfo { panel: PanelSide::Left, path: PathBuf::from("/l"), request: left_request }));
    assert!(effects.contains(&Effect::QueryGitInfo { panel: PanelSide::Right, path: PathBuf::from("/r"), request: right_request }));
}

#[test]
fn navigating_reissues_the_git_info_query_and_clears_the_previous_directorys_info() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("sub")];
    state.left.git_info = crate::git_info::GitInfo { branch: Some("main".to_string()), ..Default::default() };
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(state.left.cwd, PathBuf::from("/left/sub"));
    assert_eq!(state.left.git_info, crate::git_info::GitInfo::none(), "the previous directory's git info is cleared, not carried over");
    let request = state.left.git_request.expect("navigating mints a fresh git-info request");
    assert!(effects.contains(&Effect::QueryGitInfo { panel: PanelSide::Left, path: PathBuf::from("/left/sub"), request }));
}

#[test]
fn a_reread_mints_a_fresh_git_info_request_even_for_the_same_path() {
    let state = test_state(UiPhase::Panels);
    let first_request = state.left.git_request;
    let (state, effects) = update(state, Command::RereadPanel(PanelSide::Left));
    let second_request = state.left.git_request.expect("re-reading mints a request id");
    assert_ne!(Some(second_request), first_request);
    assert!(effects.contains(&Effect::QueryGitInfo { panel: PanelSide::Left, path: PathBuf::from("/left"), request: second_request }));
}

#[test]
fn a_resolved_git_info_result_is_applied_in_place() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left));
    let request = state.left.git_request.unwrap();
    let info = crate::git_info::GitInfo {
        branch: Some("main".to_string()),
        statuses: std::collections::HashMap::from([(OsString::from("a.txt"), crate::git_info::FileStatus::Modified)]),
    };
    let (state, effects) =
        update(state, Command::GitInfoResolved { panel: PanelSide::Left, path: PathBuf::from("/left"), request, info: info.clone() });
    assert!(effects.is_empty());
    assert_eq!(state.left.git_info, info);
}

#[test]
fn a_git_info_result_for_a_directory_the_panel_left_is_discarded() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left));
    let request = state.left.git_request.unwrap();
    let info = crate::git_info::GitInfo { branch: Some("main".to_string()), ..Default::default() };
    let (state, _) = update(state, Command::GitInfoResolved { panel: PanelSide::Left, path: PathBuf::from("/elsewhere"), request, info });
    assert_eq!(state.left.git_info, crate::git_info::GitInfo::none(), "a result for another directory is dropped");
}

#[test]
fn a_stale_git_info_result_from_an_out_of_order_reread_is_discarded_but_the_fresher_one_applies() {
    // Mirrors `a_stale_info_result_from_an_out_of_order_reread_is_discarded_but_the_fresher_one_applies`:
    // two RereadPanel commands for the same path mint two different
    // request ids, so `path` equality alone can't tell the stale reply
    // apart from the current one — this is also how a timed-out query's
    // late reply is safely dropped (git-info "Silent absence on timeout
    // and stale-result discarding").
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left));
    let first_request = state.left.git_request.unwrap();

    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left));
    let second_request = state.left.git_request.unwrap();
    assert_ne!(first_request, second_request, "re-reading mints a fresh request id even for the same path");

    let stale_info = crate::git_info::GitInfo { branch: Some("stale-branch".to_string()), ..Default::default() };
    let (state, _) = update(
        state,
        Command::GitInfoResolved { panel: PanelSide::Left, path: PathBuf::from("/left"), request: first_request, info: stale_info },
    );
    assert_eq!(state.left.git_info, crate::git_info::GitInfo::none(), "the stale (first-request) answer is dropped");

    let fresh_info = crate::git_info::GitInfo { branch: Some("main".to_string()), ..Default::default() };
    let (state, _) = update(
        state,
        Command::GitInfoResolved {
            panel: PanelSide::Left,
            path: PathBuf::from("/left"),
            request: second_request,
            info: fresh_info.clone(),
        },
    );
    assert_eq!(state.left.git_info, fresh_info, "the current request's answer applies");
}

#[test]
fn a_timed_out_query_answered_late_with_no_info_is_silently_dropped_once_superseded() {
    // A worker-thread timeout is, from the reducer's point of view, just
    // another reply — it degrades to `GitInfo::none()` — so it goes
    // through the exact same generation-key guard as any other late
    // reply: once a fresher request is outstanding, the timed-out query's
    // eventual answer (of any content) must not clobber it.
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left));
    let timed_out_request = state.left.git_request.unwrap();

    let (mut state, _) = update(state, Command::RereadPanel(PanelSide::Left));
    let current_request = state.left.git_request.unwrap();
    state.left.git_info = crate::git_info::GitInfo { branch: Some("main".to_string()), ..Default::default() };

    let (state, _) = update(
        state,
        Command::GitInfoResolved { panel: PanelSide::Left, path: PathBuf::from("/left"), request: timed_out_request, info: GitInfo::none() },
    );
    assert_eq!(
        state.left.git_info.branch.as_deref(),
        Some("main"),
        "the timed-out query's late reply must not overwrite the current request's already-applied result"
    );
    let _ = current_request;
}

// ---------------------------------------------------------------------
// M5: F4 built-in editor
// ---------------------------------------------------------------------

use crate::editor::{EditorMove, EditorState, EntryMode, ReplacePrompt};

fn editor_with(lines: &[&str]) -> EditorState {
    let text = lines.join("\n");
    let mut bytes = text.into_bytes();
    if !lines.is_empty() {
        bytes.push(b'\n');
    }
    EditorState::from_bytes(PathBuf::from("/left/edit.txt"), &bytes)
}

/// A real on-disk file to back tests that exercise an actual
/// `EditorState::save` round trip (the `Effect::SaveEditor` reply tests),
/// rather than a bare in-memory `from_bytes` buffer whose path doesn't
/// exist.
fn editor_with_temp_file(name: &str, lines: &[&str]) -> EditorState {
    let dir = std::env::temp_dir().join(format!("filecommand-update-test-editor-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("file.txt");
    let mut text = lines.join("\n");
    if !lines.is_empty() {
        text.push('\n');
    }
    std::fs::write(&path, &text).unwrap();
    match EditorState::open(&path).unwrap() {
        crate::editor::LoadResult::Loaded(e) => e,
        crate::editor::LoadResult::TooLarge { .. } => panic!("test fixture unexpectedly exceeds the editor size cap"),
    }
}

fn editor_state(editor: EditorState) -> State {
    State { phase: UiPhase::Editor(editor), ..test_state(UiPhase::Panels) }
}

#[test]
fn f4_with_no_editor_configured_opens_the_built_in_editor() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("report.txt", 10)];
    let (state, effects) = update(state, Command::RequestEditor);
    assert_eq!(effects, vec![Effect::OpenEditor { path: PathBuf::from("/left/report.txt") }]);
    assert_eq!(state.phase, UiPhase::Panels, "phase flips only once EditorOpened comes back — opening is I/O");
}

#[test]
fn f4_with_an_external_editor_configured_takes_precedence_over_the_built_in_editor() {
    let mut state = test_state(UiPhase::Panels);
    state.editor = Some("notepad".to_string());
    state.left.entries = vec![file_entry("report.txt", 10)];
    let (_, effects) = update(state, Command::RequestEditor);
    assert!(
        matches!(effects.as_slice(), [Effect::RunExternalEditor(_, PanelSide::Left)]),
        "external editor takes precedence: no OpenEditor effect, got {effects:?}"
    );
}

#[test]
fn f4_on_a_directory_or_empty_panel_does_not_open_the_editor() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("sub")];
    let (_, effects) = update(state, Command::RequestEditor);
    assert!(effects.is_empty());

    let state = test_state(UiPhase::Panels);
    let (_, effects) = update(state, Command::RequestEditor);
    assert!(effects.is_empty());
}

#[test]
fn editor_opened_enters_the_editor_phase_with_the_loaded_buffer() {
    let state = test_state(UiPhase::Panels);
    let editor = editor_with(&["line one", "line two"]);
    let (state, effects) = update(state, Command::EditorOpened(Box::new(editor)));
    assert!(effects.is_empty());
    match state.phase {
        UiPhase::Editor(e) => assert_eq!(e.lines, vec!["line one".to_string(), "line two".to_string(), String::new()]),
        other => panic!("expected UiPhase::Editor, got {other:?}"),
    }
}

#[test]
fn editor_too_large_opens_the_viewer_instead_with_an_inline_notice() {
    let state = test_state(UiPhase::Panels);
    let (state, effects) = update(state, Command::EditorTooLarge { path: PathBuf::from("/left/huge.log"), size: 20_000_000 });
    assert_eq!(effects, vec![Effect::OpenViewer { path: PathBuf::from("/left/huge.log") }]);
    assert_eq!(state.phase, UiPhase::Panels);
    let message = state.left.last_error.expect("a notice explaining the redirect");
    assert!(message.contains("huge.log"), "{message}");
    assert!(message.contains("10 MB"), "{message}");
}

#[test]
fn editor_open_failed_surfaces_an_inline_error() {
    let state = test_state(UiPhase::Panels);
    let (state, effects) = update(state, Command::EditorOpenFailed { message: "access denied".to_string() });
    assert!(effects.is_empty());
    assert_eq!(state.phase, UiPhase::Panels);
    assert_eq!(state.left.last_error.as_deref(), Some("access denied"));
}

#[test]
fn typing_in_the_editor_inserts_and_marks_the_buffer_modified() {
    let state = editor_state(editor_with(&["abc"]));
    let (state, _) = update(state, Command::EditorChar('X'));
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert_eq!(e.lines[0], "Xabc");
    assert!(e.is_modified());
}

#[test]
fn editor_move_commands_move_the_caret() {
    let state = editor_state(editor_with(&["hello"]));
    let (state, _) = update(state, Command::EditorMove(EditorMove::Right));
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert_eq!(e.caret.col, 1);
    let (state, _) = update(state, Command::EditorMove(EditorMove::End));
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert_eq!(e.caret.col, 5);
}

#[test]
fn insert_toggles_overwrite_mode() {
    let state = editor_state(editor_with(&["abc"]));
    let (state, _) = update(state, Command::EditorToggleMode);
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert_eq!(e.mode, EntryMode::Overwrite);
}

#[test]
fn mark_cut_copy_paste_flow_through_the_reducer() {
    let state = editor_state(editor_with(&["a", "b", "c"]));
    let (state, _) = update(state, Command::EditorMark);
    let (state, _) = update(state, Command::EditorMove(EditorMove::Down));
    let (state, _) = update(state, Command::EditorCut);
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert_eq!(e.lines, vec!["c".to_string(), String::new()]);
    assert_eq!(e.clipboard, vec!["a".to_string(), "b".to_string()]);

    let (state, _) = update(state, Command::EditorPaste);
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert_eq!(e.lines, vec!["a".to_string(), "b".to_string(), "c".to_string(), String::new()]);
}

#[test]
fn undo_flows_through_the_reducer() {
    let state = editor_state(editor_with(&["abc"]));
    let (state, _) = update(state, Command::EditorMove(EditorMove::End));
    let (state, _) = update(state, Command::EditorChar('d'));
    let (state, _) = update(state, Command::EditorUndo);
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert_eq!(e.lines[0], "abc");
}

#[test]
fn search_prompt_confirm_moves_the_caret_to_the_next_match() {
    let state = editor_state(editor_with(&["needle here", "another needle"]));
    let (state, _) = update(state, Command::EditorSearchStart);
    let (state, _) = update(state, Command::EditorSearchChar('n'));
    let (state, _) = update(state, Command::EditorSearchChar('e'));
    let (state, _) = update(state, Command::EditorSearchConfirm);
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert!(e.search_prompt.is_none(), "the prompt closes on confirm");
    assert_eq!(e.caret, crate::editor::Caret { line: 1, col: 8 });
}

#[test]
fn search_prompt_backspace_and_cancel_edit_and_close_the_prompt() {
    let state = editor_state(editor_with(&["abc"]));
    let (state, _) = update(state, Command::EditorSearchStart);
    let (state, _) = update(state, Command::EditorSearchChar('x'));
    let (state, _) = update(state, Command::EditorSearchBackspace);
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert_eq!(e.search_prompt.as_deref(), Some(""));
    let (state, _) = update(state, Command::EditorSearchCancel);
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert_eq!(e.search_prompt, None);
}

#[test]
fn replace_prompt_advances_from_pattern_to_replacement_then_replaces() {
    let state = editor_state(editor_with(&["hello world"]));
    let (state, _) = update(state, Command::EditorReplaceStart);
    let (state, _) = update(state, Command::EditorReplaceChar('w'));
    let (state, _) = update(state, Command::EditorReplaceChar('o'));
    let (state, _) = update(state, Command::EditorReplaceChar('r'));
    let (state, _) = update(state, Command::EditorReplaceChar('l'));
    let (state, _) = update(state, Command::EditorReplaceChar('d'));
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert_eq!(e.replace_prompt, Some(ReplacePrompt::Pattern("world".to_string())));

    let (state, _) = update(state, Command::EditorReplaceConfirm);
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert_eq!(e.replace_prompt, Some(ReplacePrompt::Replacement { pattern: "world".to_string(), replacement: String::new() }));

    let (state, _) = update(state, Command::EditorReplaceChar('X'));
    let (state, _) = update(state, Command::EditorReplaceConfirm);
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert_eq!(e.replace_prompt, None, "the prompt closes once the replacement runs");
    assert_eq!(e.lines[0], "hello X");
    assert!(e.is_modified());
}

#[test]
fn replace_confirm_with_an_empty_pattern_cancels_rather_than_advancing() {
    let state = editor_state(editor_with(&["hello"]));
    let (state, _) = update(state, Command::EditorReplaceStart);
    let (state, _) = update(state, Command::EditorReplaceConfirm);
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert_eq!(e.replace_prompt, None);
}

#[test]
fn editor_save_dispatches_the_save_effect_and_the_reply_updates_the_saved_snapshot() {
    let mut editor = editor_with_temp_file("save-effect", &["abc"]);
    editor.type_char('X');
    assert!(editor.is_modified());
    let state = editor_state(editor.clone());
    let (state, effects) = update(state, Command::EditorSave);
    match effects.as_slice() {
        [Effect::SaveEditor { editor: e, then_quit: false }] => assert_eq!(e.lines, editor.lines),
        other => panic!("expected a single SaveEditor effect, got {other:?}"),
    }
    // The buffer is unchanged until the reply arrives — save is I/O.
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert!(e.is_modified());

    // Mirrors what the TUI's effect executor actually does: clone the
    // editor into the effect, call the real (I/O-performing) `save()` on
    // it, then reply with the post-save state.
    let mut saved = editor;
    saved.save().unwrap();
    assert!(!saved.is_modified(), "sanity: a real save clears the modified flag on the saved copy");
    let (state, effects) = update(state, Command::EditorSaved { editor: Box::new(saved), then_quit: false });
    assert!(effects.is_empty());
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert!(!e.is_modified(), "the post-save snapshot from the reply clears the modified flag");
}

#[test]
fn editor_save_failed_surfaces_an_inline_message_without_losing_the_buffer() {
    let mut editor = editor_with(&["abc"]);
    editor.type_char('X');
    let state = editor_state(editor);
    let (state, effects) = update(state, Command::EditorSaveFailed { message: "disk full".to_string() });
    assert!(effects.is_empty());
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert_eq!(e.save_error.as_deref(), Some("disk full"));
    assert!(e.is_modified(), "a failed save must not silently discard the unsaved edit");
}

#[test]
fn f10_on_an_unmodified_buffer_exits_directly() {
    let state = editor_state(editor_with(&["abc"]));
    let (state, effects) = update(state, Command::EditorRequestQuit);
    assert!(effects.is_empty());
    assert_eq!(state.phase, UiPhase::Panels);
}

#[test]
fn f10_on_a_modified_buffer_raises_the_save_on_exit_confirm() {
    let mut editor = editor_with(&["abc"]);
    editor.type_char('X');
    let state = editor_state(editor);
    let (state, effects) = update(state, Command::EditorRequestQuit);
    assert!(effects.is_empty());
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert!(e.quit_confirm);
}

#[test]
fn cancelling_the_save_on_exit_confirm_returns_to_editing() {
    let mut editor = editor_with(&["abc"]);
    editor.type_char('X');
    let state = editor_state(editor);
    let (state, _) = update(state, Command::EditorRequestQuit);
    let (state, effects) = update(state, Command::EditorCancelQuit);
    assert!(effects.is_empty());
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert!(!e.quit_confirm);
}

#[test]
fn discarding_at_the_save_on_exit_confirm_exits_without_saving() {
    let mut editor = editor_with(&["abc"]);
    editor.type_char('X');
    let state = editor_state(editor);
    let (state, _) = update(state, Command::EditorRequestQuit);
    let (state, effects) = update(state, Command::EditorConfirmQuitDiscard);
    assert!(effects.is_empty());
    assert_eq!(state.phase, UiPhase::Panels);
}

#[test]
fn confirming_save_at_the_save_on_exit_confirm_dispatches_a_save_that_quits() {
    let mut editor = editor_with_temp_file("quit-save", &["abc"]);
    editor.type_char('X');
    let state = editor_state(editor.clone());
    let (state, _) = update(state, Command::EditorRequestQuit);
    let (state, effects) = update(state, Command::EditorConfirmQuitSave);
    match effects.as_slice() {
        [Effect::SaveEditor { then_quit: true, .. }] => {}
        other => panic!("expected a SaveEditor effect with then_quit: true, got {other:?}"),
    }
    // Still in the editor until the save actually lands.
    assert!(matches!(state.phase, UiPhase::Editor(_)));

    let mut saved = editor;
    saved.save().unwrap();
    let (state, effects) = update(state, Command::EditorSaved { editor: Box::new(saved), then_quit: true });
    assert!(effects.is_empty());
    assert_eq!(state.phase, UiPhase::Panels, "a successful save-then-quit closes the editor");
}

#[test]
fn a_failed_save_during_quit_aborts_the_quit_and_keeps_editing() {
    let mut editor = editor_with(&["abc"]);
    editor.type_char('X');
    let state = editor_state(editor);
    let (state, _) = update(state, Command::EditorRequestQuit);
    let (state, effects) = update(state, Command::EditorConfirmQuitSave);
    assert!(matches!(effects.as_slice(), [Effect::SaveEditor { then_quit: true, .. }]));

    let (state, _) = update(state, Command::EditorSaveFailed { message: "disk full".to_string() });
    let UiPhase::Editor(e) = &state.phase else { panic!("expected the failed save to abort the quit and stay in the editor") };
    assert_eq!(e.save_error.as_deref(), Some("disk full"));
    assert!(!e.quit_confirm, "the confirm dialog itself closes so the user can see and act on the error");
}

// ---------------------------------------------------------------------
// M5: Tree display mode (design D7; additional-panel-modes)
// ---------------------------------------------------------------------

fn dir_child(name: &str) -> Entry {
    dir_entry(name)
}

#[test]
fn entering_tree_mode_roots_at_the_drive_and_requests_the_root_children() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\Users\demo");
    state.left.display_mode = DisplayMode::Info;
    let (state, effects) = update(state, Command::EnterTreeMode(PanelSide::Left));
    assert_eq!(state.left.display_mode, DisplayMode::Tree);
    let tree = state.left.tree.as_ref().expect("Tree mode populates tree state");
    assert_eq!(tree.nodes.len(), 1);
    assert_eq!(tree.nodes[0].path, PathBuf::from(r"C:\"));
    assert_eq!(tree.prior_mode, DisplayMode::Info, "the pre-Tree display mode is recorded for Enter to restore");
    assert!(effects.contains(&Effect::ExpandTreeNode { panel: PanelSide::Left, path: PathBuf::from(r"C:\") }));
}

#[test]
fn tree_node_expanded_splices_children_into_the_matching_panels_tree() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::EnterTreeMode(PanelSide::Left));
    let (state, _) = update(
        state,
        Command::TreeNodeExpanded { panel: PanelSide::Left, path: PathBuf::from(r"C:\"), children: vec![dir_child("alpha"), dir_child("beta")] },
    );
    let tree = state.left.tree.as_ref().unwrap();
    assert_eq!(tree.nodes.len(), 3);
    assert_eq!(tree.nodes[1].path, PathBuf::from(r"C:\alpha"));
}

#[test]
fn a_tree_node_expanded_reply_for_the_other_panel_is_dropped() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::EnterTreeMode(PanelSide::Left));
    // The right panel never entered Tree mode, so it has no tree at all —
    // a reply naming it must not panic or fabricate one.
    let (state, _) = update(
        state,
        Command::TreeNodeExpanded { panel: PanelSide::Right, path: PathBuf::from(r"C:\"), children: vec![dir_child("alpha")] },
    );
    assert!(state.right.tree.is_none());
    assert_eq!(state.left.tree.as_ref().unwrap().nodes.len(), 1, "the left panel's tree is untouched by a reply addressed elsewhere");
}

#[test]
fn moving_the_tree_cursor_relists_the_opposite_panel_at_the_highlighted_directory() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::EnterTreeMode(PanelSide::Left));
    let (state, _) = update(
        state,
        Command::TreeNodeExpanded { panel: PanelSide::Left, path: PathBuf::from(r"C:\"), children: vec![dir_child("alpha"), dir_child("beta")] },
    );
    let (state, effects) = update(state, Command::MoveCursor(CursorMove::Down(1)));
    assert_eq!(state.left.tree.as_ref().unwrap().cursor, 1);
    assert!(
        without_git_info_effects(effects).contains(&Effect::StartListing { panel: PanelSide::Right, path: PathBuf::from(r"C:\alpha") }),
        "the opposite (right) panel must be re-listed at the newly highlighted directory"
    );
    // Landing on "alpha" (not yet expanded) also requests its children.
    assert!(!state.left.tree.as_ref().unwrap().nodes[1].expanded);
}

#[test]
fn moving_onto_an_unexpanded_node_requests_its_children_but_not_an_already_expanded_one() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::EnterTreeMode(PanelSide::Left));
    let (state, _) = update(
        state,
        Command::TreeNodeExpanded { panel: PanelSide::Left, path: PathBuf::from(r"C:\"), children: vec![dir_child("alpha")] },
    );
    let (state, effects) = update(state, Command::MoveCursor(CursorMove::Down(1)));
    assert!(effects.contains(&Effect::ExpandTreeNode { panel: PanelSide::Left, path: PathBuf::from(r"C:\alpha") }));

    // Expand it, then move away and back — no second expand request for an
    // already-expanded node.
    let (state, _) = update(
        state,
        Command::TreeNodeExpanded { panel: PanelSide::Left, path: PathBuf::from(r"C:\alpha"), children: vec![] },
    );
    let (state, _) = update(state, Command::MoveCursor(CursorMove::Up(1)));
    let (_state, effects) = update(state, Command::MoveCursor(CursorMove::Down(1)));
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::ExpandTreeNode { path, .. } if path == std::path::Path::new(r"C:\alpha"))),
        "an already-expanded node must not be re-requested: {effects:?}"
    );
}

#[test]
fn enter_on_a_tree_node_restores_the_prior_mode_and_navigates_this_panel_there() {
    let mut state = test_state(UiPhase::Panels);
    state.left.display_mode = DisplayMode::Brief;
    let (state, _) = update(state, Command::EnterTreeMode(PanelSide::Left));
    let (state, _) = update(
        state,
        Command::TreeNodeExpanded { panel: PanelSide::Left, path: PathBuf::from(r"C:\"), children: vec![dir_child("alpha"), dir_child("beta")] },
    );
    let (state, _) = update(state, Command::MoveCursor(CursorMove::Down(1))); // highlight "alpha"
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(state.left.display_mode, DisplayMode::Brief, "Enter restores the pre-Tree display mode");
    assert!(state.left.tree.is_none(), "leaving Tree mode clears the tree state");
    assert!(
        without_git_info_effects(effects).contains(&Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from(r"C:\alpha") }),
        "Enter navigates *this* panel to the highlighted directory, not the opposite one"
    );
}

// ---------------------------------------------------------------------
// M5: Brief/Full/Quick view display-mode switch (design D7)
// ---------------------------------------------------------------------

#[test]
fn set_display_mode_switches_the_named_panel_and_clears_tree_state() {
    let mut state = test_state(UiPhase::Panels);
    state.right.tree = Some(crate::panel::TreeState::new(PathBuf::from(r"C:\"), DisplayMode::Full));
    state.right.display_mode = DisplayMode::Tree;
    let (state, _) = update(state, Command::SetDisplayMode { side: PanelSide::Right, mode: DisplayMode::Brief });
    assert_eq!(state.right.display_mode, DisplayMode::Brief);
    assert!(state.right.tree.is_none());
    assert_eq!(state.left.display_mode, DisplayMode::Full, "the other panel is untouched");
}

// ---------------------------------------------------------------------
// M5 review fix: a quick filter must not linger invisibly across a
// display-mode switch, since Brief/Tree/Info's renderers don't surface it
// the same way Full mode does (quick-filter "Substring narrowing as the
// pattern is typed").
// ---------------------------------------------------------------------

#[test]
fn set_display_mode_clears_an_active_quick_filter_on_the_named_panel() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("report.txt", 1), file_entry("readme.md", 2)];
    state.left.quick_filter = Some("rep".to_string());
    let (state, _) = update(state, Command::SetDisplayMode { side: PanelSide::Left, mode: DisplayMode::Brief });
    assert_eq!(state.left.display_mode, DisplayMode::Brief);
    assert_eq!(state.left.quick_filter, None, "a stale filter must not linger invisibly into Brief mode");
}

#[test]
fn set_display_mode_does_not_clear_the_opposite_panels_quick_filter() {
    let mut state = test_state(UiPhase::Panels);
    state.right.entries = vec![file_entry("report.txt", 1)];
    state.right.quick_filter = Some("rep".to_string());
    let (state, _) = update(state, Command::SetDisplayMode { side: PanelSide::Left, mode: DisplayMode::Brief });
    assert_eq!(state.right.quick_filter.as_deref(), Some("rep"), "the opposite panel's filter must be untouched");
}

#[test]
fn entering_tree_mode_clears_an_active_quick_filter() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("report.txt", 1)];
    state.left.quick_filter = Some("rep".to_string());
    let (state, _) = update(state, Command::EnterTreeMode(PanelSide::Left));
    assert_eq!(state.left.display_mode, DisplayMode::Tree);
    assert_eq!(state.left.quick_filter, None, "a stale filter must not linger invisibly into Tree mode");
}

#[test]
fn toggling_into_info_mode_clears_an_active_quick_filter() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("report.txt", 1)];
    state.left.quick_filter = Some("rep".to_string());
    let (state, _) = update(state, Command::ToggleInfoMode(PanelSide::Left));
    assert_eq!(state.left.display_mode, DisplayMode::Info);
    assert_eq!(state.left.quick_filter, None, "a stale filter must not linger invisibly into Info mode");
}

// ---------------------------------------------------------------------
// M5: Ctrl+J fuzzy jump
// ---------------------------------------------------------------------

fn history_entry(path: &str, count: u32, ms: u64) -> crate::quicksearch::FrecencyEntry {
    crate::quicksearch::FrecencyEntry { path: PathBuf::from(path), visit_count: count, last_visited_ms: ms }
}

#[test]
fn fuzzy_jump_open_and_esc_close_without_navigating() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::FuzzyJumpOpen);
    assert!(state.fuzzy_jump.is_some());
    let (state, effects) = update(state, Command::FuzzyJumpCancel);
    assert!(state.fuzzy_jump.is_none());
    assert!(effects.is_empty());
    assert_eq!(state.left.cwd, PathBuf::from("/left"), "the panel is unchanged");
}

#[test]
fn fuzzy_jump_typing_and_backspace_edit_the_pattern() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::FuzzyJumpOpen);
    let (state, _) = update(state, Command::FuzzyJumpChar('d'));
    let (state, _) = update(state, Command::FuzzyJumpChar('o'));
    assert_eq!(state.fuzzy_jump.as_ref().unwrap().pattern, "do");
    let (state, _) = update(state, Command::FuzzyJumpBackspace);
    assert_eq!(state.fuzzy_jump.as_ref().unwrap().pattern, "d");
}

#[test]
fn fuzzy_jump_confirm_navigates_the_active_panel_to_the_highlighted_directory() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Right;
    state.dir_history = vec![history_entry(r"C:\Low", 1, 0), history_entry(r"C:\High", 10, 0)];
    let (state, _) = update(state, Command::FuzzyJumpOpen);
    // No pattern typed: the full frecency-ranked list shows, most frecent
    // first, so the default (cursor 0) highlight is `C:\High`.
    let (state, effects) = update(state, Command::FuzzyJumpConfirm);
    assert!(state.fuzzy_jump.is_none(), "the dialog closes");
    assert_eq!(state.right.cwd, PathBuf::from(r"C:\High"));
    assert_eq!(state.left.cwd, PathBuf::from("/left"), "the opposite panel is unaffected");
    assert!(without_git_info_effects(effects).iter().any(|e| matches!(e, Effect::StartListing { panel: PanelSide::Right, .. })));
}

#[test]
fn fuzzy_jump_move_clamps_within_the_currently_filtered_list() {
    let mut state = test_state(UiPhase::Panels);
    state.dir_history = vec![history_entry(r"C:\A", 1, 0), history_entry(r"C:\B", 1, 0)];
    let (state, _) = update(state, Command::FuzzyJumpOpen);
    let (state, _) = update(state, Command::FuzzyJumpMove(-5));
    assert_eq!(state.fuzzy_jump.as_ref().unwrap().cursor, 0);
    let (state, _) = update(state, Command::FuzzyJumpMove(5));
    assert_eq!(state.fuzzy_jump.as_ref().unwrap().cursor, 1);
}

// ---------------------------------------------------------------------
// M5: Alt+F7 find file
// ---------------------------------------------------------------------

fn find_match(rel: &str, name: &str) -> FindMatch {
    FindMatch { relative_path: PathBuf::from(rel), entry: dir_entry_named(name) }
}

fn dir_entry_named(name: &str) -> Entry {
    Entry { name: OsString::from(name), kind: EntryKind::File, size: 0, modified: None }
}

#[test]
fn find_file_open_roots_at_the_active_panels_directory() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Right;
    state.right.cwd = PathBuf::from(r"C:\proj");
    let (state, _) = update(state, Command::FindFileOpen);
    assert_eq!(state.find_file.as_ref().unwrap().root, PathBuf::from(r"C:\proj"));
}

#[test]
fn find_file_submit_mints_a_request_and_dispatches_the_walk_effect() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\proj");
    let (state, _) = update(state, Command::FindFileOpen);
    let (state, _) = update(state, Command::FindFileChar('r'));
    let (state, effects) = update(state, Command::FindFileSubmit);
    match effects.as_slice() {
        [Effect::FindInSubtree { root, pattern, request }] => {
            assert_eq!(root, &PathBuf::from(r"C:\proj"));
            assert_eq!(pattern, "r");
            assert_eq!(state.find_file.as_ref().unwrap().request, Some(*request));
        }
        other => panic!("expected exactly one FindInSubtree effect, got {other:?}"),
    }
}

#[test]
fn find_file_matches_from_a_superseded_request_are_dropped() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::FindFileOpen);
    let (state, _) = update(state, Command::FindFileSubmit); // request 1
    let (state, _) = update(state, Command::FindFileMatch { request: 999, m: find_match("a.txt", "a.txt") });
    assert!(state.find_file.as_ref().unwrap().results.is_empty(), "a stale-request match must be dropped");
}

#[test]
fn find_file_confirm_navigates_in_place_and_seeds_the_cursor_target() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\proj");
    let (state, _) = update(state, Command::FindFileOpen);
    let (state, _) = update(state, Command::FindFileSubmit);
    let request = state.find_file.as_ref().unwrap().request.unwrap();
    let (state, _) = update(state, Command::FindFileMatch { request, m: find_match(r"sub\report.txt", "report.txt") });
    let (state, effects) = update(state, Command::FindFileConfirm);
    assert!(state.find_file.is_none(), "the dialog closes");
    assert_eq!(state.left.cwd, PathBuf::from(r"C:\proj\sub"), "navigates to the match's containing directory");
    assert_eq!(state.left.pending_cursor_target, Some(OsString::from("report.txt")));
    assert!(without_git_info_effects(effects).iter().any(|e| matches!(e, Effect::StartListing { panel: PanelSide::Left, .. })));
}

#[test]
fn find_file_confirm_with_a_root_level_match_navigates_to_the_root() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\proj");
    let (state, _) = update(state, Command::FindFileOpen);
    let (state, _) = update(state, Command::FindFileSubmit);
    let request = state.find_file.as_ref().unwrap().request.unwrap();
    let (state, _) = update(state, Command::FindFileMatch { request, m: find_match("readme.txt", "readme.txt") });
    let (state, _) = update(state, Command::FindFileConfirm);
    assert_eq!(state.left.cwd, PathBuf::from(r"C:\proj"));
}

#[test]
fn find_file_cancel_abandons_the_search_without_navigating() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::FindFileOpen);
    let (state, _) = update(state, Command::FindFileSubmit);
    let (state, effects) = update(state, Command::FindFileCancel);
    assert!(state.find_file.is_none());
    assert!(effects.is_empty());
    assert_eq!(state.left.cwd, PathBuf::from("/left"));
}

#[test]
fn listing_complete_settles_the_cursor_on_a_pending_find_file_target() {
    let mut state = test_state(UiPhase::Panels);
    state.left.pending_cursor_target = Some(OsString::from("b.txt"));
    let (state, _) = update(
        state,
        Command::ListingChunk {
            panel: PanelSide::Left,
            entries: vec![dir_entry_named("a.txt"), dir_entry_named("b.txt"), dir_entry_named("c.txt")],
        },
    );
    let (state, _) = update(state, Command::ListingComplete { panel: PanelSide::Left, total: 3 });
    assert_eq!(state.left.cursor, 1);
    assert!(state.left.pending_cursor_target.is_none(), "consumed once applied");
}

// ---------------------------------------------------------------------
// M5: F2 user menu
// ---------------------------------------------------------------------

#[test]
fn user_menu_confirm_dispatches_the_highlighted_commands_shell_passthrough() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Right;
    state.right.cwd = PathBuf::from(r"C:\Projects\app");
    state.user_menu_entries = vec![
        crate::config::UserMenuEntry { label: "A".to_string(), command: "echo a".to_string() },
        crate::config::UserMenuEntry { label: "Build".to_string(), command: "cargo build".to_string() },
    ];
    let (state, _) = update(state, Command::UserMenuOpen);
    let (state, _) = update(state, Command::UserMenuMove(1));
    let (state, effects) = update(state, Command::UserMenuConfirm);
    assert!(state.user_menu.is_none(), "the menu closes");
    match effects.as_slice() {
        [Effect::RunShellCommand(inv, side)] => {
            assert_eq!(inv.cwd, PathBuf::from(r"C:\Projects\app"));
            assert!(inv.args.iter().any(|a| a.contains("cargo build")), "{:?}", inv.args);
            assert_eq!(*side, PanelSide::Right);
        }
        other => panic!("expected exactly one RunShellCommand effect, got {other:?}"),
    }
}

#[test]
fn user_menu_esc_closes_without_running_anything() {
    let mut state = test_state(UiPhase::Panels);
    state.user_menu_entries = vec![crate::config::UserMenuEntry { label: "A".to_string(), command: "echo a".to_string() }];
    let (state, _) = update(state, Command::UserMenuOpen);
    let (state, effects) = update(state, Command::UserMenuCancel);
    assert!(state.user_menu.is_none());
    assert!(effects.is_empty());
}

#[test]
fn user_menu_move_clamps_and_confirm_on_an_empty_menu_is_a_harmless_close() {
    let state = test_state(UiPhase::Panels); // user_menu_entries is empty
    let (state, _) = update(state, Command::UserMenuOpen);
    let (state, _) = update(state, Command::UserMenuMove(5));
    assert_eq!(state.user_menu.as_ref().unwrap().cursor, 0);
    let (state, effects) = update(state, Command::UserMenuConfirm);
    assert!(state.user_menu.is_none());
    assert!(effects.is_empty(), "nothing to run on an empty menu");
}

// ---------------------------------------------------------------------
// user-menu-themes-entry: built-in Themes slot
// ---------------------------------------------------------------------

#[test]
fn user_menu_down_past_the_last_user_entry_lands_on_themes_and_clamps_there() {
    let mut state = test_state(UiPhase::Panels);
    state.user_menu_entries = vec![
        crate::config::UserMenuEntry { label: "A".to_string(), command: "echo a".to_string() },
        crate::config::UserMenuEntry { label: "B".to_string(), command: "echo b".to_string() },
    ];
    let (state, _) = update(state, Command::UserMenuOpen);
    // Two user entries (indices 0, 1) plus the built-in slot at index 2.
    let (state, _) = update(state, Command::UserMenuMove(2));
    assert_eq!(state.user_menu.as_ref().unwrap().cursor, 2, "Down from the last user entry lands on the built-in Themes slot");
    let (state, _) = update(state, Command::UserMenuMove(1));
    assert_eq!(state.user_menu.as_ref().unwrap().cursor, 2, "the built-in slot is the end of the domain: Down holds, it does not wrap");
}

#[test]
fn user_menu_confirm_on_themes_opens_the_picker_pre_highlighted_with_no_shell_effect() {
    let mut state = test_state(UiPhase::Panels);
    state.theme = Theme::terminal_green();
    state.user_menu_entries = vec![crate::config::UserMenuEntry { label: "A".to_string(), command: "echo a".to_string() }];
    let (state, _) = update(state, Command::UserMenuOpen);
    let (state, _) = update(state, Command::UserMenuMove(1)); // off the one user entry, onto the built-in slot
    assert_eq!(state.user_menu.as_ref().unwrap().cursor, 1);
    let (state, effects) = update(state, Command::UserMenuConfirm);
    assert!(state.user_menu.is_none(), "the F2 menu closes");
    let picker = state.theme_picker.expect("Confirm on the built-in slot opens the theme picker");
    let expected = crate::theme::BUILTIN_THEME_NAMES.iter().position(|n| *n == "terminal-green").unwrap();
    assert_eq!(picker.highlight, expected, "the active theme's row is pre-highlighted, same as the Options -> Themes route");
    assert!(effects.is_empty(), "no shell effect is emitted for the built-in slot");
}

#[test]
fn user_menu_confirm_on_a_user_entry_still_runs_it_via_the_shell_unaffected_by_the_built_in_slot() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Right;
    state.right.cwd = PathBuf::from(r"C:\Projects\app");
    state.user_menu_entries = vec![crate::config::UserMenuEntry { label: "Build".to_string(), command: "cargo build".to_string() }];
    let (state, _) = update(state, Command::UserMenuOpen);
    let (state, effects) = update(state, Command::UserMenuConfirm);
    assert!(state.user_menu.is_none());
    assert!(state.theme_picker.is_none(), "a user entry never opens the theme picker");
    match effects.as_slice() {
        [Effect::RunShellCommand(inv, side)] => {
            assert_eq!(inv.cwd, PathBuf::from(r"C:\Projects\app"));
            assert!(inv.args.iter().any(|a| a.contains("cargo build")), "{:?}", inv.args);
            assert_eq!(*side, PanelSide::Right);
        }
        other => panic!("expected exactly one RunShellCommand effect, got {other:?}"),
    }
}

#[test]
fn theme_picker_esc_after_f2_origin_returns_to_the_panels_without_reopening_the_user_menu() {
    let mut state = test_state(UiPhase::Panels);
    state.user_menu_entries = vec![crate::config::UserMenuEntry { label: "A".to_string(), command: "echo a".to_string() }];
    let (state, _) = update(state, Command::UserMenuOpen);
    let (state, _) = update(state, Command::UserMenuMove(1)); // onto the built-in Themes slot
    let (state, _) = update(state, Command::UserMenuConfirm);
    assert!(state.user_menu.is_none() && state.theme_picker.is_some());
    let (state, effects) = update(state, Command::ThemePickerCancel);
    assert!(state.theme_picker.is_none(), "the picker closes");
    assert!(state.user_menu.is_none(), "Esc lands on the panels, not back on the F2 menu (design D5)");
    assert!(effects.is_empty());
}

// ---------------------------------------------------------------------
// visual-themes: Options -> Themes picker
// ---------------------------------------------------------------------

#[test]
fn theme_picker_opens_from_the_options_menu_with_the_active_theme_highlighted() {
    let mut state = test_state(UiPhase::Panels);
    state.theme = Theme::terminal_green();
    let (state, _) = update(state, Command::MenuOpen);
    let (state, _) = update(state, Command::MenuHotkey('o'));
    let (state, effects) = update(state, Command::MenuActivate);
    assert!(state.menu.is_none(), "the menu overlay closes");
    let picker = state.theme_picker.expect("Options -> Themes opens the picker");
    let expected = crate::theme::BUILTIN_THEME_NAMES.iter().position(|n| *n == "terminal-green").unwrap();
    assert_eq!(picker.highlight, expected, "the active theme's row is pre-highlighted");
    assert!(effects.is_empty());
}

#[test]
fn theme_picker_up_down_moves_the_highlight_over_the_theme_list() {
    let state = test_state(UiPhase::Panels); // active theme is nc-classic (index 0)
    let (state, _) = update(state, Command::ThemePickerOpen);
    assert_eq!(state.theme_picker.as_ref().unwrap().highlight, 0);
    let (state, _) = update(state, Command::ThemePickerMove(1));
    assert_eq!(state.theme_picker.as_ref().unwrap().highlight, 1);
    let (state, _) = update(state, Command::ThemePickerMove(-1));
    assert_eq!(state.theme_picker.as_ref().unwrap().highlight, 0, "Up from the first row holds, it does not wrap");
}

#[test]
fn theme_picker_enter_applies_the_highlighted_theme_immediately_and_persists_it() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::ThemePickerOpen);
    // Move down to `yellow-storm` (index 4 of BUILTIN_THEME_NAMES).
    let target = crate::theme::BUILTIN_THEME_NAMES.iter().position(|n| *n == "yellow-storm").unwrap();
    let (state, _) = update(state, Command::ThemePickerMove(target as isize));
    let (state, effects) = update(state, Command::ThemePickerConfirm);
    assert!(state.theme_picker.is_none(), "the dialog closes");
    assert_eq!(state.theme.name, "yellow-storm", "the active theme switches in this same reducer step");
    assert_eq!(effects, vec![Effect::PersistTheme("yellow-storm".to_string())]);
}

#[test]
fn theme_picker_esc_changes_nothing() {
    let state = test_state(UiPhase::Panels); // active theme is nc-classic
    let (state, _) = update(state, Command::ThemePickerOpen);
    let (state, _) = update(state, Command::ThemePickerMove(3)); // highlight some other theme
    let (state, effects) = update(state, Command::ThemePickerCancel);
    assert!(state.theme_picker.is_none(), "the dialog closes");
    assert_eq!(state.theme.name, "nc-classic", "the active theme is untouched");
    assert!(effects.is_empty(), "nothing is persisted on cancel");
}

// ---------------------------------------------------------------------
// theme-picker-live-preview: State::render_theme()
// ---------------------------------------------------------------------

#[test]
fn render_theme_before_opening_the_picker_equals_the_active_theme() {
    let mut state = test_state(UiPhase::Panels);
    state.theme = Theme::terminal_green();
    assert_eq!(state.render_theme(), state.theme, "no picker open: render theme is the active theme");
}

#[test]
fn render_theme_while_open_tracks_the_highlighted_theme_as_it_moves() {
    let mut state = test_state(UiPhase::Panels); // active theme is nc-classic (index 0)
    state.theme = Theme::classic();
    let (state, _) = update(state, Command::ThemePickerOpen);
    // Opening is visually a no-op: highlight starts on the active theme.
    assert_eq!(state.render_theme(), state.theme, "opening previews the active theme first");

    let target = crate::theme::BUILTIN_THEME_NAMES.iter().position(|n| *n == "purple-lights").unwrap();
    let (state, _) = update(state, Command::ThemePickerMove(target as isize));
    assert_eq!(
        state.render_theme().name,
        "purple-lights",
        "moving the highlight previews the newly highlighted theme"
    );
    assert_eq!(state.theme.name, "nc-classic", "the applied theme is untouched by moving the highlight");
}

#[test]
fn render_theme_after_cancel_reverts_to_the_active_theme() {
    let state = test_state(UiPhase::Panels); // active theme is nc-classic
    let (state, _) = update(state, Command::ThemePickerOpen);
    let (state, _) = update(state, Command::ThemePickerMove(3)); // preview some other theme
    assert_ne!(state.render_theme().name, "nc-classic", "sanity: a non-active theme is previewed");
    let (state, _) = update(state, Command::ThemePickerCancel);
    assert_eq!(state.render_theme(), state.theme, "after cancel, render theme is the (unchanged) active theme");
    assert_eq!(state.theme.name, "nc-classic", "cancel never touched the active theme");
}

#[test]
fn render_theme_after_confirm_equals_the_newly_applied_theme() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::ThemePickerOpen);
    let target = crate::theme::BUILTIN_THEME_NAMES.iter().position(|n| *n == "yellow-storm").unwrap();
    let (state, _) = update(state, Command::ThemePickerMove(target as isize));
    let (state, _) = update(state, Command::ThemePickerConfirm);
    assert_eq!(state.render_theme(), state.theme, "after confirm, render theme is the applied theme");
    assert_eq!(state.render_theme().name, "yellow-storm");
}

// ---------------------------------------------------------------------
// M5: F1 Help window + About dialog
// ---------------------------------------------------------------------

#[test]
fn help_opens_with_about_filecommand_highlighted_first() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::HelpOpen);
    let help = state.help.unwrap();
    assert_eq!(help.cursor, 0);
    assert_eq!(crate::dialogs::HELP_TOPICS[help.cursor], "About FileCommand");
    assert!(help.page.is_none());
    assert!(!help.about_open);
}

#[test]
fn help_activate_on_about_opens_the_about_dialog_not_a_page() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::HelpOpen);
    let (state, _) = update(state, Command::HelpActivate);
    assert!(state.help.as_ref().unwrap().about_open);
    assert!(state.help.as_ref().unwrap().page.is_none());
}

#[test]
fn help_activate_on_a_topic_opens_its_page() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::HelpOpen);
    let (state, _) = update(state, Command::HelpMove(1)); // "Keyboard reference"
    let (state, _) = update(state, Command::HelpActivate);
    assert_eq!(state.help.as_ref().unwrap().page, Some(1));
}

#[test]
fn help_cancel_returns_a_page_to_the_list_then_closes_the_window() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::HelpOpen);
    let (state, _) = update(state, Command::HelpMove(1));
    let (state, _) = update(state, Command::HelpActivate);
    assert!(state.help.as_ref().unwrap().page.is_some());

    let (state, _) = update(state, Command::HelpCancel);
    assert!(state.help.is_some(), "Esc from a page returns to the list, not closing the window");
    assert!(state.help.as_ref().unwrap().page.is_none());
    assert_eq!(state.help.as_ref().unwrap().cursor, 1, "the highlight is preserved");

    let (state, _) = update(state, Command::HelpCancel);
    assert!(state.help.is_none(), "Esc from the list closes the window");
}

#[test]
fn help_cancel_dismisses_about_back_to_the_list_with_about_still_highlighted() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::HelpOpen);
    let (state, _) = update(state, Command::HelpActivate); // opens About
    let (state, _) = update(state, Command::HelpCancel);
    assert!(state.help.is_some());
    assert!(!state.help.as_ref().unwrap().about_open);
    assert_eq!(state.help.as_ref().unwrap().cursor, 0);
}

#[test]
fn help_move_does_not_walk_past_the_ends_of_the_topic_list() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::HelpOpen);
    let (state, _) = update(state, Command::HelpMove(-5));
    assert_eq!(state.help.as_ref().unwrap().cursor, 0);
    let last = crate::dialogs::HELP_TOPICS.len() - 1;
    let state = (0..20).fold(state, |s, _| update(s, Command::HelpMove(1)).0);
    assert_eq!(state.help.as_ref().unwrap().cursor, last);
}

// ---------------------------------------------------------------------
// M5: directory frecency recording + persistence (fuzzy-jump "Navigation
// records history")
// ---------------------------------------------------------------------

#[test]
fn navigating_into_a_directory_records_and_persists_frecency() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("sub")];
    state.clock_ms = 5_000;
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(state.dir_history, vec![history_entry("/left/sub", 1, 5_000)]);
    assert!(effects.iter().any(|e| matches!(e, Effect::PersistHistory(file) if file.directories == state.dir_history)));
}

#[test]
fn revisiting_a_directory_increments_its_frecency_entry_rather_than_duplicating() {
    let mut state = test_state(UiPhase::Panels);
    state.dir_history = vec![history_entry(r"C:\a", 1, 0)];
    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left)); // /left, a fresh entry
    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left)); // /left again
    let count = state.dir_history.iter().find(|e| e.path.as_path() == Path::new("/left")).unwrap().visit_count;
    assert_eq!(count, 2, "re-reading the same directory increments the same entry rather than adding a duplicate");
}

#[test]
fn tree_cursor_preview_of_the_opposite_panel_does_not_record_frecency() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::EnterTreeMode(PanelSide::Left));
    let (state, _) = update(
        state,
        Command::TreeNodeExpanded { panel: PanelSide::Left, path: PathBuf::from(r"C:\"), children: vec![dir_child("alpha")] },
    );
    let (state, _) = update(state, Command::MoveCursor(CursorMove::Down(1)));
    assert!(state.dir_history.is_empty(), "browsing the tree previews the opposite panel; it does not count as a visit");
}

// ---------------------------------------------------------------------
// M5 review fix: startup-warning modal (malformed usermenu.toml)
// ---------------------------------------------------------------------

#[test]
fn dismiss_startup_warning_clears_it() {
    let mut state = test_state(UiPhase::Panels);
    state.startup_warning = Some("usermenu.toml is malformed; F2 uses the default user menu".to_string());
    let (state, effects) = update(state, Command::DismissStartupWarning);
    assert_eq!(state.startup_warning, None);
    assert!(effects.is_empty());
}

#[test]
fn dismiss_startup_warning_is_a_no_op_when_nothing_is_warned() {
    let state = test_state(UiPhase::Panels);
    assert_eq!(state.startup_warning, None);
    let (state, effects) = update(state, Command::DismissStartupWarning);
    assert_eq!(state.startup_warning, None);
    assert!(effects.is_empty());
}

// ---------------------------------------------------------------------
// Panel-scrolling: reducer-level viewport reconciliation (task 1.5)
//
// `panel::tests::scroll_offset_tests` exercises `ensure_cursor_visible`
// directly at fixed row counts; these tests instead drive the reducer end
// to end so each wiring site in `update.rs` (task 1.4) is proven to
// actually call it, at term sizes chosen so `panel_viewport_rows` produces
// a known, hand-verifiable row count (visible here via `super::*`, the same
// private helper `update`'s own reducer arms use).
// ---------------------------------------------------------------------

#[test]
fn full_mode_viewport_rows_at_80x24_are_19() {
    // Every test below is built on this figure: panels_h = 24 - 2 = 22, no
    // tab strip at 1 tab (reserved = 2), body_h = 20, Full/Tree = body_h -
    // 1 (header row) = 19.
    assert_eq!(panel_viewport_rows((80, 24), DisplayMode::Full, 1), 19);
}

#[test]
fn quick_filter_narrowing_re_clamps_the_offset_through_the_reducer() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 24); // Full-mode body: 19 rows
    state.left.sort_mode = SortMode::Unsorted;
    state.left.entries = (0..30).map(|i| file_entry(&format!("e{i}"), 0)).collect();
    state.left.cursor = 25; // "e25" -- one of only three entries containing "5"
    state.left.scroll_offset = 15; // a stale offset from before filtering narrowed the list

    let (state, _) = update(state, Command::QuickFilterStart);
    let (state, _) = update(state, Command::QuickFilterChar('5'));

    let visible: Vec<String> = state.left.visible_indices().into_iter().map(|i| state.left.entries[i].name.to_string_lossy().into_owned()).collect();
    assert_eq!(visible, vec!["e5", "e15", "e25"], "only entries containing \"5\" remain");
    assert_eq!(state.left.cursor, 25, "\"e25\" stays selected — it's still visible under the filter");
    assert_eq!(state.left.scroll_offset, 2, "\"e25\" is now at visible position 2; the stale offset (15) must re-clamp to it (panel-navigation \"Quick-filter narrowing re-clamps the offset\")");
}

#[test]
fn re_sort_re_clamps_the_offset_through_the_reducer() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 24); // Full-mode body: 19 rows
    state.left.sort_mode = SortMode::Unsorted;
    // Descending by size, so "e00" (the largest) starts at position 0.
    state.left.entries = (0..30).map(|i| file_entry(&format!("e{i:02}"), 29 - i as u64)).collect();
    state.left.cursor = 0; // "e00"
    state.left.scroll_offset = 0;

    let (state, _) = update(state, Command::SetSortMode { side: PanelSide::Left, mode: SortMode::Size });

    assert_eq!(state.left.entries.last().unwrap().name, OsString::from("e00"), "ascending-by-size moves the largest entry to the very end");
    assert_eq!(state.left.cursor, 29, "the cursor re-anchors onto \"e00\"'s new position");
    assert_eq!(state.left.scroll_offset, 11, "29 + 1 - 19 = 11 (panel-navigation \"Re-sort keeps the cursor's entry in view\")");
}

#[test]
fn terminal_shrinking_re_clamps_the_offset_on_resize() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 24); // Full-mode body: 19 rows
    state.left.sort_mode = SortMode::Unsorted;
    state.left.entries = (0..30).map(|i| file_entry(&format!("e{i}"), 0)).collect();
    state.left.cursor = 18;
    state.left.scroll_offset = 0;

    let (state, _) = update(state, Command::Resize(80, 24));
    assert_eq!(state.left.scroll_offset, 0, "no shrink yet: the cursor is already inside the 19-row window");

    // 80x14: panels_h = 12, no tab strip, body_h = 10, Full rows = 9.
    assert_eq!(panel_viewport_rows((80, 14), DisplayMode::Full, 1), 9);
    let (state, _) = update(state, Command::Resize(80, 14));
    assert_eq!(state.left.scroll_offset, 10, "18 + 1 - 9 = 10 (panel-navigation \"Terminal resize re-clamps\")");
}

#[test]
fn tab_restore_re_clamps_against_the_current_viewport_through_the_reducer() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Left;
    state.term_size = (80, 24); // Full-mode body: 19 rows
    state.left.sort_mode = SortMode::Unsorted;
    state.left.entries = (0..30).map(|i| file_entry(&format!("e{i}"), 0)).collect();
    state.left.cursor = 18;
    state.left.scroll_offset = 10; // valid at 19 rows: window [10, 29)

    // Stash this as tab 1; the new (now-active) tab inherits the same
    // cursor/offset and is left untouched from here on.
    let (state, _) = update(state, Command::OpenTab);
    assert_eq!(state.left.tab_count(), 2);

    // Shrink drastically enough that the stashed tab's offset (10) will no
    // longer keep its cursor (18) in view once restored.
    let (state, _) = update(state, Command::Resize(80, 10));
    // 80x10, 2 tabs: panels_h = 8, tab strip visible (reserved = 3),
    // body_h = 5, Full rows = 4.
    assert_eq!(panel_viewport_rows((80, 10), DisplayMode::Full, 2), 4);

    let (state, _) = update(state, Command::SwitchTab(1));
    assert_eq!(state.left.cwd, PathBuf::from("/left"), "tab 1 is the originally-stashed tab");
    assert_eq!(state.left.cursor, 18, "the restored cursor position round-trips exactly");
    assert_eq!(
        state.left.scroll_offset, 15,
        "the stashed offset (10) no longer fits a 4-row window around cursor 18; 18 + 1 - 4 = 15 (panel-navigation \"Tab restore re-clamps against the current viewport\")"
    );
}

#[test]
fn streamed_chunk_keeps_the_offset_pinned_to_zero_via_the_reducer() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 24); // Full-mode body: 19 rows
    state.left.sort_mode = SortMode::Name;
    let entries: Vec<Entry> = (0..30).map(|i| file_entry(&format!("e{i:02}"), 0)).collect();

    let (state, _) = update(state, Command::ListingChunk { panel: PanelSide::Left, entries });

    assert_eq!(state.left.cursor, 0, "the cursor stays pinned to the top while the user hasn't moved it");
    assert_eq!(state.left.scroll_offset, 0, "the window stays pinned to the top right along with the cursor (panel-navigation \"Streamed listing keeps the top pinned\")");
}

#[test]
fn find_file_settle_on_listing_complete_lands_the_cursor_in_view() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 24); // Full-mode body: 19 rows
    state.left.sort_mode = SortMode::Unsorted;
    state.left.pending_cursor_target = Some(OsString::from("e25"));
    let entries: Vec<Entry> = (0..30).map(|i| file_entry(&format!("e{i}"), 0)).collect();

    let (state, _) = update(state, Command::ListingChunk { panel: PanelSide::Left, entries });
    let (state, _) = update(state, Command::ListingComplete { panel: PanelSide::Left, total: 30 });

    assert_eq!(state.left.entries[state.left.cursor].name, OsString::from("e25"), "find-file's deferred settle lands the cursor on the matched entry");
    assert_eq!(state.left.cursor, 25);
    assert_eq!(
        state.left.scroll_offset, 7,
        "25 + 1 - 19 = 7: the settled cursor lands inside the window (panel-navigation \"Scroll offset is core panel state\" -- find-file's deferred cursor settle re-clamps)"
    );
}

// ---------------------------------------------------------------------
// Panel-scrolling: Brief column-window and Tree reconciliation (task 2.3)
//
// These drive the reducer end to end, proving `reconcile_panel_viewport`'s
// Brief/Tree branches (task 2.1/2.2) are actually wired into
// `Command::MoveCursor` and `Command::TreeNodeExpanded`, on top of
// `panel::tests::brief_scroll_tests`/`tree_scroll_tests`'s pure coverage of
// the underlying clamp math.
// ---------------------------------------------------------------------

#[test]
fn brief_mode_interior_width_and_column_count_at_80x24_default_split() {
    // split 50/50 at width 80 -> left_w = 40, interior = 38 -> 3 columns
    // (byte-identical to `filecommand-tui::views::panel::render_brief_body`'s
    // `(inner_w / 12).max(1)`).
    let interior = panel_interior_width((80, 24), panel_split::DEFAULT_SPLIT_PERCENT, PanelSide::Left);
    assert_eq!(interior, 38);
    assert_eq!(brief_column_count(interior), 3);
    // Brief rows_h = the full body (no header row): panels_h = 22, no tab
    // strip at 1 tab, body_h = 20.
    assert_eq!(panel_viewport_rows((80, 24), DisplayMode::Brief, 1), 20);
}

#[test]
fn brief_mode_cursor_past_the_last_column_shifts_the_window_one_column_through_the_reducer() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 24); // Brief-mode: 20 rows_h, 3 columns (60-position window)
    state.left.display_mode = DisplayMode::Brief;
    state.left.sort_mode = SortMode::Unsorted;
    state.left.entries = (0..100).map(|i| file_entry(&format!("e{i}"), 0)).collect();
    state.left.cursor = 59; // column 2 (last of the window), row 19 -- the window's last position
    state.left.scroll_offset = 0;

    let (state, _) = update(state, Command::MoveCursor(CursorMove::Down(1)));

    assert_eq!(state.left.cursor, 60, "column 3, row 0: one step past the window's last column");
    assert_eq!(
        state.left.scroll_offset, 20,
        "shifts by exactly one column (20 positions), not further (additional-panel-modes \"Cursor past the last visible column shifts the window one column\")"
    );
    assert_eq!(state.left.scroll_offset % 20, 0, "the offset stays on a rows_h-multiple column boundary (additional-panel-modes \"Window start stays on a column boundary\")");
}

#[test]
fn brief_mode_quick_filter_re_clamps_the_column_window_through_the_reducer() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 24); // Brief-mode: 20 rows_h, 3 columns
    state.left.display_mode = DisplayMode::Brief;
    state.left.sort_mode = SortMode::Unsorted;
    state.left.entries = (0..100).map(|i| file_entry(&format!("e{i}"), 0)).collect();
    state.left.cursor = 65; // column 3
    state.left.scroll_offset = 60; // window starts at column 3, matching the cursor

    let (state, _) = update(state, Command::QuickFilterStart);
    // Only "e6" and "e65"/"e6x" family remain a small handful of matches
    // near the front of the list, forcing the (now far too large) stale
    // offset to pull back.
    let (state, _) = update(state, Command::QuickFilterChar('e'));
    let (state, _) = update(state, Command::QuickFilterChar('6'));

    let visible = state.left.visible_indices();
    assert!(!visible.is_empty());
    let pos = visible.iter().position(|&i| i == state.left.cursor).unwrap();
    let start_col = state.left.scroll_offset / 20;
    let pos_col = pos / 20;
    assert!(
        start_col <= pos_col && pos_col < start_col + 3,
        "the cursor's column ({pos_col}) must be inside the re-clamped window starting at column {start_col} (panel-navigation \"Quick-filter narrowing re-clamps the offset\", extended to Brief's column space)"
    );
    assert_eq!(state.left.scroll_offset % 20, 0, "offset stays on a column boundary after re-clamping");
}

#[test]
fn tree_mode_cursor_below_the_bottom_scrolls_the_node_window_through_the_reducer() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 24); // Tree-mode body: 19 rows (body_h - 1 header)
    assert_eq!(panel_viewport_rows((80, 24), DisplayMode::Tree, 1), 19);
    state.left.display_mode = DisplayMode::Tree;
    let mut tree = TreeState::new(PathBuf::from(r"C:\"), DisplayMode::Full);
    let children: Vec<Entry> = (0..30).map(|i| dir_entry(&format!("d{i}"))).collect();
    tree.insert_children(&PathBuf::from(r"C:\"), children); // 31 nodes total (root + 30)
    tree.cursor = 18; // last visible row of a 19-row window starting at 0
    state.left.tree = Some(tree);

    let (state, _) = update(state, Command::MoveCursor(CursorMove::Down(1)));

    let tree = state.left.tree.as_ref().unwrap();
    assert_eq!(tree.cursor, 19);
    assert_eq!(tree.scroll_offset, 1, "shifts by exactly one row, mirroring Full mode (additional-panel-modes \"Tree cursor below the bottom scrolls the nodes\")");
}

#[test]
fn tree_node_expanded_re_clamps_the_tree_offset_through_the_reducer() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 12); // a small Tree-mode body forces a small row count
    let rows = panel_viewport_rows((80, 12), DisplayMode::Tree, 1);
    state.left.display_mode = DisplayMode::Tree;
    let mut tree = TreeState::new(PathBuf::from(r"C:\"), DisplayMode::Full);
    let children: Vec<Entry> = (0..15).map(|i| dir_entry(&format!("d{i}"))).collect();
    tree.insert_children(&PathBuf::from(r"C:\"), children); // 16 nodes total (root + 15)
    tree.cursor = 12;
    tree.scroll_offset = 0; // stale at the current (small) row count: 12 is not in [0, rows)
    state.left.tree = Some(tree);

    // Expand a *different* node than the cursor's; it must not move the
    // cursor, but the node-list mutation still funnels through
    // reconciliation (additional-panel-modes "Tree mode scrolling").
    let more = vec![dir_entry("extra")];
    let (state, _) = update(state, Command::TreeNodeExpanded { panel: PanelSide::Left, path: PathBuf::from(r"C:\d0"), children: more });

    let tree = state.left.tree.as_ref().unwrap();
    assert_eq!(tree.cursor, 12, "expanding a different node doesn't move the cursor");
    assert_eq!(tree.scroll_offset, 12 + 1 - rows, "the stale offset re-clamps to the minimal-shift window around the cursor once TreeNodeExpanded re-runs reconciliation");
}

// ---------------------------------------------------------------------
// Clipboard export (clipboard-export)
// ---------------------------------------------------------------------

#[test]
fn copy_to_clipboard_uses_cursor_entry_when_nothing_explicitly_selected() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    state.left.entries = vec![file_entry("README.md", 10)];
    let (state, effects) = update(state, Command::CopyToClipboard(ClipboardPayloadKind::Paths));
    assert_eq!(
        effects,
        vec![Effect::SetClipboard(ClipboardPayload { kind: ClipboardPayloadKind::Paths, items: vec![PathBuf::from(r"C:\NORTON\README.md")] })]
    );
    assert!(state.left.clipboard_feedback.is_none(), "feedback waits for the ClipboardResult reply");
}

#[test]
fn copy_to_clipboard_uses_explicit_selection_over_cursor() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    state.left.entries = vec![file_entry("a.txt", 1), file_entry("b.txt", 2), file_entry("c.txt", 3)];
    state.left.selected.insert(OsString::from("a.txt"));
    state.left.selected.insert(OsString::from("c.txt"));
    state.left.cursor = 1; // b.txt: not selected, must not be used

    let (_, effects) = update(state, Command::CopyToClipboard(ClipboardPayloadKind::Files));
    let Some(Effect::SetClipboard(payload)) = effects.into_iter().next() else { panic!("expected a SetClipboard effect") };
    assert_eq!(payload.kind, ClipboardPayloadKind::Files);
    let mut items = payload.items;
    items.sort();
    assert_eq!(items, vec![PathBuf::from(r"C:\NORTON\a.txt"), PathBuf::from(r"C:\NORTON\c.txt")]);
}

#[test]
fn copy_to_clipboard_on_parent_dir_with_no_selection_reports_nothing_to_copy() {
    // The parent-directory pseudo-entry is never a valid clipboard source
    // (clipboard-export "Parent entry is never copied"), exactly like F5.
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![Entry::parent_dir(), file_entry("a.txt", 1)];
    state.left.cursor = 0; // ".."
    let (state, effects) = update(state, Command::CopyToClipboard(ClipboardPayloadKind::Files));
    assert!(effects.is_empty(), "nothing is written to the clipboard");
    let feedback = state.left.clipboard_feedback.expect("the mini-status reports there is nothing to copy");
    assert_eq!(feedback.message, "Nothing to copy");
    assert!(!feedback.is_error);
}

#[test]
fn copy_to_clipboard_with_empty_panel_reports_nothing_to_copy() {
    let state = test_state(UiPhase::Panels);
    let (state, effects) = update(state, Command::CopyToClipboard(ClipboardPayloadKind::Names));
    assert!(effects.is_empty());
    assert_eq!(state.left.clipboard_feedback.unwrap().message, "Nothing to copy");
}

#[test]
fn clipboard_result_ok_files_reports_the_plural_template_even_for_one() {
    let state = test_state(UiPhase::Panels);
    let payload = ClipboardPayload { kind: ClipboardPayloadKind::Files, items: vec![PathBuf::from(r"C:\NORTON\a.txt")] };
    let (state, effects) = update(state, Command::ClipboardResult { payload, fell_back_to_paths: false, result: Ok(()) });
    assert!(effects.is_empty());
    let feedback = state.left.clipboard_feedback.expect("success sets feedback");
    assert_eq!(feedback.message, "1 files copied to clipboard");
    assert!(!feedback.is_error);
}

#[test]
fn clipboard_result_ok_paths_singular_names_the_path() {
    let state = test_state(UiPhase::Panels);
    let payload = ClipboardPayload { kind: ClipboardPayloadKind::Paths, items: vec![PathBuf::from(r"C:\NORTON\README.md")] };
    let (state, _) = update(state, Command::ClipboardResult { payload, fell_back_to_paths: false, result: Ok(()) });
    assert_eq!(state.left.clipboard_feedback.unwrap().message, r"Path copied: C:\NORTON\README.md");
}

#[test]
fn clipboard_result_ok_paths_plural_counts() {
    let state = test_state(UiPhase::Panels);
    let payload = ClipboardPayload {
        kind: ClipboardPayloadKind::Paths,
        items: vec![PathBuf::from(r"C:\NORTON\a.txt"), PathBuf::from(r"C:\NORTON\b.txt")],
    };
    let (state, _) = update(state, Command::ClipboardResult { payload, fell_back_to_paths: false, result: Ok(()) });
    assert_eq!(state.left.clipboard_feedback.unwrap().message, "2 paths copied");
}

#[test]
fn clipboard_result_ok_names_counts() {
    let state = test_state(UiPhase::Panels);
    let payload = ClipboardPayload { kind: ClipboardPayloadKind::Names, items: vec![PathBuf::from("a.txt"), PathBuf::from("b.txt")] };
    let (state, _) = update(state, Command::ClipboardResult { payload, fell_back_to_paths: false, result: Ok(()) });
    assert_eq!(state.left.clipboard_feedback.unwrap().message, "2 names copied");
}

#[test]
fn clipboard_result_fallback_to_paths_names_the_platform_limitation() {
    let state = test_state(UiPhase::Panels);
    let payload = ClipboardPayload { kind: ClipboardPayloadKind::Files, items: vec![PathBuf::from(r"C:\NORTON\a.txt")] };
    let (state, _) = update(state, Command::ClipboardResult { payload, fell_back_to_paths: true, result: Ok(()) });
    let feedback = state.left.clipboard_feedback.unwrap();
    assert_eq!(feedback.message, "Paths copied (file objects unsupported here)");
    assert!(!feedback.is_error);
}

#[test]
fn clipboard_result_err_shows_the_message_in_the_error_role() {
    let state = test_state(UiPhase::Panels);
    let payload = ClipboardPayload { kind: ClipboardPayloadKind::Files, items: vec![PathBuf::from(r"C:\NORTON\a.txt")] };
    let (state, _) = update(state, Command::ClipboardResult { payload, fell_back_to_paths: false, result: Err("Clipboard busy — try again".to_string()) });
    let feedback = state.left.clipboard_feedback.unwrap();
    assert_eq!(feedback.message, "Clipboard busy — try again");
    assert!(feedback.is_error);
}

#[test]
fn clipboard_feedback_expires_via_tick_once_the_deadline_passes() {
    let mut state = test_state(UiPhase::Panels);
    state.clock_ms = 0;
    let payload = ClipboardPayload { kind: ClipboardPayloadKind::Files, items: vec![PathBuf::from("a.txt")] };
    let (state, _) = update(state, Command::ClipboardResult { payload, fell_back_to_paths: false, result: Ok(()) });
    assert!(state.left.clipboard_feedback.is_some());

    // Not yet at the deadline: still showing.
    let (state, _) = update(state, Command::Tick(CLIPBOARD_FEEDBACK_MS - 1));
    assert!(state.left.clipboard_feedback.is_some(), "feedback holds until the deadline is reached");

    // At the deadline: expired.
    let (state, _) = update(state, Command::Tick(CLIPBOARD_FEEDBACK_MS));
    assert!(state.left.clipboard_feedback.is_none(), "feedback expires once the clock reaches expires_at_ms");
}

#[test]
fn clipboard_feedback_clears_on_the_next_key_before_the_deadline() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1), file_entry("b.txt", 2)];
    state.clock_ms = 0;
    let payload = ClipboardPayload { kind: ClipboardPayloadKind::Files, items: vec![PathBuf::from("a.txt")] };
    let (state, _) = update(state, Command::ClipboardResult { payload, fell_back_to_paths: false, result: Ok(()) });
    assert!(state.left.clipboard_feedback.is_some());

    // Any other command — e.g. Down — counts as "the next key press" and
    // clears the feedback immediately, well before the ~3s deadline.
    let (state, _) = update(state, Command::MoveCursor(CursorMove::Down(1)));
    assert_eq!(state.left.cursor, 1, "the movement itself still applies");
    assert!(state.left.clipboard_feedback.is_none(), "the mini-status shows the normal display again");
}

#[test]
fn files_menu_clipboard_group_dispatches_copy_to_clipboard() {
    assert_eq!(
        menu_action_command(MenuAction::ClipboardFiles, PanelSide::Left),
        Some(Command::CopyToClipboard(ClipboardPayloadKind::Files))
    );
    assert_eq!(
        menu_action_command(MenuAction::ClipboardPaths, PanelSide::Left),
        Some(Command::CopyToClipboard(ClipboardPayloadKind::Paths))
    );
    assert_eq!(
        menu_action_command(MenuAction::ClipboardNames, PanelSide::Left),
        Some(Command::CopyToClipboard(ClipboardPayloadKind::Names))
    );
}

#[test]
fn files_menu_activate_on_copy_to_clipboard_runs_the_files_action() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::MenuOpen);
    let (mut state, _) = update(state, Command::MenuHotkey('f')); // Files
    let target = crate::menu::entries(crate::menu::MenuId::Files)
        .iter()
        .position(|e| matches!(e, MenuEntry::Item(i) if i.label == "Copy to clipboard"))
        .unwrap();
    state.menu.as_mut().unwrap().selected = target;
    let (state, effects) = update(state, Command::MenuActivate);
    assert!(state.menu.is_none(), "activating the item closes the whole menu overlay");
    assert_eq!(
        effects,
        vec![Effect::SetClipboard(ClipboardPayload { kind: ClipboardPayloadKind::Files, items: vec![PathBuf::from(r"C:\NORTON\a.txt")] })]
    );
}

#[test]
fn send_to_clipboard_action_menu_entry_targets_only_the_menus_entry() {
    // The menu's target (`notes.txt`) is copied even though a different
    // entry is selected and the cursor sits on a third one — the action
    // menu always scopes to the entry it was opened on, never the panel's
    // live selection or cursor (design D3 of `file-action-menu`).
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    state.left.entries = vec![file_entry("notes.txt", 1), file_entry("other.txt", 2)];
    state.left.selected.insert(OsString::from("other.txt"));
    let state = opened_menu_state_from(state, "notes.txt");

    let (state, effects) = update(state, Command::FileActionMenuHotkey('S'));
    assert!(state.file_action_menu.is_none(), "the hotkey closes the menu");
    assert_eq!(
        effects,
        vec![Effect::SetClipboard(ClipboardPayload { kind: ClipboardPayloadKind::Files, items: vec![PathBuf::from(r"C:\NORTON\notes.txt")] })]
    );
    assert_eq!(state.left.selected.len(), 1, "the panel's selection is untouched");
}

/// Like `opened_menu_state`, but starting from a caller-built `state`
/// (entries/selection already set up) instead of building a fresh one —
/// needed by tests that must control the panel's selection independently
/// of the menu's target entry.
fn opened_menu_state_from(mut state: State, cursor_on: &str) -> State {
    state.left.cursor = state.left.entries.iter().position(|e| e.name == cursor_on).unwrap();
    let (state, _) = update(state, Command::Enter);
    assert!(state.file_action_menu.is_some(), "setup precondition: the menu must be open");
    state
}

// ---------------------------------------------------------------------
// Mouse (mouse-basics, design D2)
// ---------------------------------------------------------------------

/// mouse-input "Click focuses and places the cursor" — "Click on the
/// inactive panel".
#[test]
fn click_entry_focuses_the_clicked_panel_and_moves_its_cursor() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Left;
    state.right.entries = vec![file_entry("notes.txt", 1), file_entry("other.txt", 2)];
    let (state, _) = update(state, Command::ClickEntry { side: PanelSide::Right, name: OsString::from("notes.txt"), mods: ClickMods::Plain });
    assert_eq!(state.active, PanelSide::Right);
    assert_eq!(state.right.cursor, 0);
}

/// mouse-input "Click on a selected entry keeps it selected" — a plain
/// click never touches the selection set, only the cursor.
#[test]
fn click_entry_plain_never_changes_selection() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1), file_entry("b.txt", 2)];
    state.left.selected.insert(OsString::from("a.txt"));
    let (state, _) = update(state, Command::ClickEntry { side: PanelSide::Left, name: OsString::from("b.txt"), mods: ClickMods::Plain });
    assert_eq!(state.left.cursor, 1);
    assert!(state.left.selected.contains(&OsString::from("a.txt")), "the existing selection survives an unrelated plain click");
    assert!(!state.left.selected.contains(&OsString::from("b.txt")), "a plain click never selects the clicked entry either");
}

/// mouse-input "Ctrl+click toggles selection" — "Toggle on".
#[test]
fn ctrl_click_toggles_selection_and_moves_the_cursor() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::ClickEntry { side: PanelSide::Left, name: OsString::from("a.txt"), mods: ClickMods::Ctrl });
    assert!(state.left.selected.contains(&OsString::from("a.txt")));
    assert_eq!(state.left.cursor, 0);

    let (state, _) = update(state, Command::ClickEntry { side: PanelSide::Left, name: OsString::from("a.txt"), mods: ClickMods::Ctrl });
    assert!(!state.left.selected.contains(&OsString::from("a.txt")), "a second Ctrl+click toggles it back off");
}

/// mouse-input "Ctrl+click toggles selection" — "Parent entry ignored".
#[test]
fn ctrl_click_on_parent_entry_never_selects_it() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![Entry { name: OsString::from(".."), kind: EntryKind::ParentDir, size: 0, modified: None }];
    let (state, _) = update(state, Command::ClickEntry { side: PanelSide::Left, name: OsString::from(".."), mods: ClickMods::Ctrl });
    assert!(state.left.selected.is_empty());
    assert_eq!(state.left.cursor, 0, "the cursor still moves to `..`");
}

/// A name the panel no longer lists is a silent no-op, not a panic — the
/// hit map can go briefly stale between a frame being drawn and the click
/// landing (e.g. a re-read completing in between).
#[test]
fn click_entry_on_a_vanished_name_is_a_no_op() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Left;
    state.right.entries = vec![file_entry("a.txt", 1)];
    let before_cursor = state.right.cursor;
    let (state, effects) = update(state, Command::ClickEntry { side: PanelSide::Right, name: OsString::from("gone.txt"), mods: ClickMods::Plain });
    assert!(effects.is_empty());
    assert_eq!(state.right.cursor, before_cursor);
    assert_eq!(state.active, PanelSide::Right, "focus still moves to the clicked side even though the name missed");
}

/// mouse-input "Click focuses and places the cursor" — clicking blank body
/// area or the title focuses without moving the cursor.
#[test]
fn focus_panel_switches_active_without_touching_the_cursor() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Left;
    state.right.entries = vec![file_entry("a.txt", 1), file_entry("b.txt", 2)];
    state.right.cursor = 1;
    let (state, effects) = update(state, Command::FocusPanel(PanelSide::Right));
    assert!(effects.is_empty());
    assert_eq!(state.active, PanelSide::Right);
    assert_eq!(state.right.cursor, 1, "the cursor is untouched");
}

/// mouse-input "Wheel moves the cursor of the panel under the pointer" —
/// "Wheel over the inactive panel": the cursor of the scrolled panel moves,
/// the active panel does not change.
#[test]
fn scroll_panel_moves_the_cursor_of_the_named_side_without_changing_focus() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Left;
    state.right.entries = (0..10).map(|i| file_entry(&format!("f{i}.txt"), 1)).collect();
    let (state, _) = update(state, Command::ScrollPanel { side: PanelSide::Right, delta: 3 });
    assert_eq!(state.active, PanelSide::Left, "the active panel never changes");
    assert_eq!(state.right.cursor, 3, "three rows per notch");
}

#[test]
fn scroll_panel_upward_never_underflows_past_the_first_row() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = (0..10).map(|i| file_entry(&format!("f{i}.txt"), 1)).collect();
    state.left.cursor = 1;
    let (state, _) = update(state, Command::ScrollPanel { side: PanelSide::Left, delta: -3 });
    assert_eq!(state.left.cursor, 0);
}

/// mouse-input "Key bar, menu bar, pull-down items, and dialog buttons are
/// clickable" — "Key bar Copy".
#[test]
fn keybar_press_five_opens_the_copy_setup_dialog() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::KeybarPress(5));
    assert!(matches!(state.phase, UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind: JobKind::Copy, .. })));
}

#[test]
fn keybar_press_ten_requests_quit_exactly_like_f10() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::KeybarPress(10));
    assert!(state.quit_confirm);
}

#[test]
fn keybar_press_out_of_range_is_a_no_op() {
    let state = test_state(UiPhase::Panels);
    let before = state.clone();
    let (state, effects) = update(state, Command::KeybarPress(0));
    assert!(effects.is_empty());
    assert_eq!(state, before);
}

/// mouse-input "Key bar, menu bar, pull-down items, and dialog buttons are
/// clickable" — a menu-title click opens that pull-down.
#[test]
fn menu_title_click_opens_the_named_pulldown() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::MenuTitleClick(MenuId::Files));
    let menu = state.menu.expect("the bar opens");
    assert_eq!(menu.active, MenuId::Files);
    assert!(menu.pulldown_open);
}

/// A menu-title click while a different pull-down is already open switches
/// to the clicked one instead of stacking overlays.
#[test]
fn menu_title_click_switches_from_an_already_open_menu() {
    let mut state = test_state(UiPhase::Panels);
    state.menu = Some(MenuState::for_menu(MenuId::Left));
    let (state, _) = update(state, Command::MenuTitleClick(MenuId::Options));
    assert_eq!(state.menu.unwrap().active, MenuId::Options);
}

/// mouse-input "Menu item activation" — clicking `Files` then `Delete`
/// starts the delete-confirmation flow exactly as F8 would.
#[test]
fn menu_item_click_activates_the_item_exactly_like_menu_activate() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    state.menu = Some(MenuState::for_menu(MenuId::Files));
    let delete_index = crate::menu::entries(MenuId::Files).iter().position(|e| matches!(e, MenuEntry::Item(i) if i.label == "Delete")).unwrap();
    let (state, _) = update(state, Command::MenuItemClick(delete_index));
    assert!(state.menu.is_none(), "activating an item closes the bar");
    assert!(matches!(state.phase, UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { .. })));
}

#[test]
fn menu_item_click_on_a_disabled_item_is_a_no_op() {
    let mut state = test_state(UiPhase::Panels);
    state.menu = Some(MenuState::for_menu(MenuId::Files));
    let disabled_index = crate::menu::entries(MenuId::Files).iter().position(|e| matches!(e, MenuEntry::Item(i) if i.label == "View")).unwrap();
    let (state, effects) = update(state, Command::MenuItemClick(disabled_index));
    assert!(state.menu.is_some(), "a disabled item never closes the bar");
    assert!(effects.is_empty());
}

#[test]
fn menu_item_click_with_no_menu_open_is_a_no_op() {
    let state = test_state(UiPhase::Panels);
    let before = state.clone();
    let (state, effects) = update(state, Command::MenuItemClick(0));
    assert!(effects.is_empty());
    assert_eq!(state, before);
}

/// mouse-input "Dialog button" — clicking `Skip All` on the conflict dialog
/// applies the choice exactly as if selected by keyboard.
#[test]
fn dialog_button_click_conflict_skip_all_sends_the_same_effect_as_the_key() {
    let progress_state = running_progress_state(JobKind::Copy, "/left", "/right");
    let (state, _) = update(progress_state, Command::JobConflict(sample_conflict()));
    assert!(matches!(state.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Conflict { .. }, .. }), "setup precondition");

    let (_, effects) = update(state.clone(), Command::FileOpConflictChoice(ConflictChoice::SkipAll));
    let (_, click_effects) = update(state, Command::DialogButtonClick(ButtonId::ConflictSkipAll));
    assert_eq!(effects, click_effects);
}

/// mouse-input "Running job accepts Cancel only" — the Cancel button click
/// signals the same cancellation the keyboard path does.
#[test]
fn dialog_button_click_progress_cancel_cancels_the_job() {
    let state = running_progress_state(JobKind::Copy, "/left", "/right");
    let (_, effects) = update(state, Command::DialogButtonClick(ButtonId::ProgressCancel));
    assert_eq!(effects, vec![Effect::CancelJob]);
}

/// Quit-dialog buttons reach the same global `quit_confirm` handling the
/// keyboard's Y/N does, regardless of what phase is underneath.
#[test]
fn dialog_button_click_quit_yes_confirms_quit() {
    let mut state = test_state(UiPhase::Panels);
    state.quit_confirm = true;
    let (state, effects) = update(state, Command::DialogButtonClick(ButtonId::QuitYes));
    assert!(effects.contains(&Effect::Quit));
    assert!(!state.quit_confirm);
}

#[test]
fn dialog_button_click_quit_no_cancels_the_quit_dialog_and_restores_context() {
    let mut state = test_state(UiPhase::Viewer(crate::viewer::ViewerState::new(PathBuf::from("f.txt"), 0)));
    state.quit_confirm = true;
    let (state, effects) = update(state, Command::DialogButtonClick(ButtonId::QuitNo));
    assert!(effects.is_empty());
    assert!(!state.quit_confirm);
    assert!(matches!(state.phase, UiPhase::Viewer(_)), "cancelling only clears the flag, restoring whatever was underneath");
}

/// `Command::OpenActionMenuAt`: files get the same single-target menu
/// `handle_enter` builds; directories get the same menu minus View, Edit,
/// and Run (file-action-menu "Directory targets and selection-scoped
/// invocation").
#[test]
fn open_action_menu_at_opens_the_menu_for_a_file() {
    let mut state = test_state(UiPhase::Panels);
    state.right.entries = vec![file_entry("notes.txt", 1)];
    let (state, _) = update(state, Command::OpenActionMenuAt { side: PanelSide::Right, name: OsString::from("notes.txt") });
    assert_eq!(state.active, PanelSide::Right);
    assert_eq!(state.right.cursor, 0);
    let menu = state.file_action_menu.expect("the menu opens for a file target");
    assert_eq!(menu.target_name, OsString::from("notes.txt"));
    assert!(!menu.selection_scoped, "not a member of any selection");
}

/// file-action-menu "Directory targets and selection-scoped invocation" —
/// "Directory menu contents".
#[test]
fn open_action_menu_at_on_a_directory_opens_the_menu_without_view_edit_or_run() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("src")];
    let (state, _) = update(state, Command::OpenActionMenuAt { side: PanelSide::Left, name: OsString::from("src") });
    assert_eq!(state.left.cursor, 0, "the cursor moves to the target");
    let menu = state.file_action_menu.expect("the menu opens for a directory target too");
    assert_eq!(
        menu.entries,
        vec![
            FileActionMenuEntry::Copy,
            FileActionMenuEntry::Rename,
            FileActionMenuEntry::Move,
            FileActionMenuEntry::Delete,
            FileActionMenuEntry::SendToClipboard,
        ]
    );
}

#[test]
fn open_action_menu_at_on_the_parent_entry_is_a_no_op() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![Entry { name: OsString::from(".."), kind: EntryKind::ParentDir, size: 0, modified: None }];
    let (state, _) = update(state, Command::OpenActionMenuAt { side: PanelSide::Left, name: OsString::from("..") });
    assert!(state.file_action_menu.is_none(), "`..` is never a valid action-menu target");
}

/// file-action-menu "Directory targets and selection-scoped invocation" —
/// "Selection-scoped delete": right-clicking a member of a multi-entry
/// selection scopes Delete to the whole selection, not just the clicked
/// entry.
#[test]
fn open_action_menu_at_on_a_selected_entry_scopes_delete_to_the_whole_selection() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1), file_entry("b.txt", 2), file_entry("c.txt", 3), file_entry("d.txt", 4)];
    state.left.selected.insert(OsString::from("a.txt"));
    state.left.selected.insert(OsString::from("b.txt"));
    state.left.selected.insert(OsString::from("c.txt"));
    let (state, _) = update(state, Command::OpenActionMenuAt { side: PanelSide::Left, name: OsString::from("b.txt") });
    let menu = state.file_action_menu.as_ref().unwrap();
    assert!(menu.selection_scoped);

    let (state, _) = update(state, Command::FileActionMenuHotkey('D'));
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { sources, .. }) => {
            assert_eq!(sources.len(), 3, "the dialog is scoped to the whole selection, naming the count");
        }
        other => panic!("expected DeleteConfirm scoped to the selection, got {other:?}"),
    }
}

/// Right-clicking an entry that is *not* selected stays scoped to that one
/// entry, exactly like the pre-existing Enter behavior.
#[test]
fn open_action_menu_at_on_an_unselected_entry_stays_single_target() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1), file_entry("b.txt", 2)];
    state.left.selected.insert(OsString::from("a.txt"));
    let (state, _) = update(state, Command::OpenActionMenuAt { side: PanelSide::Left, name: OsString::from("b.txt") });
    let menu = state.file_action_menu.as_ref().unwrap();
    assert!(!menu.selection_scoped);

    let (state, _) = update(state, Command::FileActionMenuHotkey('D'));
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { sources, .. }) => {
            assert_eq!(sources.len(), 1);
            assert_eq!(sources[0].original_name, OsString::from("b.txt"));
        }
        other => panic!("expected DeleteConfirm scoped to b.txt alone, got {other:?}"),
    }
}

/// file-action-menu "Enter stays single-target": the keyboard path never
/// scopes to the selection even when the cursor entry is itself selected.
#[test]
fn enter_on_a_selected_entry_still_opens_a_single_target_menu() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1), file_entry("b.txt", 2), file_entry("c.txt", 3)];
    state.left.selected.insert(OsString::from("a.txt"));
    state.left.selected.insert(OsString::from("b.txt"));
    state.left.selected.insert(OsString::from("c.txt"));
    state.left.cursor = 1; // b.txt, itself selected
    let (state, _) = update(state, Command::Enter);
    let menu = state.file_action_menu.as_ref().unwrap();
    assert!(!menu.selection_scoped, "Enter-key invocation SHALL remain single-target and file-only");
}

// ---------------------------------------------------------------------
// Mouse drag-and-drop (mouse-panel-drag)
// ---------------------------------------------------------------------

#[test]
fn drag_begin_on_a_selected_entry_scopes_to_the_whole_selection() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1), file_entry("b.txt", 2), file_entry("c.txt", 3)];
    state.left.selected.insert(OsString::from("a.txt"));
    state.left.selected.insert(OsString::from("c.txt"));
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy });
    let drag = state.drag.as_ref().expect("drag should have begun");
    let mut names: Vec<_> = drag.items.iter().map(|s| s.original_name.clone()).collect();
    names.sort();
    assert_eq!(names, vec![OsString::from("a.txt"), OsString::from("c.txt")]);
    assert_eq!(drag.source, PanelSide::Left);
    assert_eq!(drag.source_dir, PathBuf::from("/left"));
    assert_eq!(drag.op, JobKind::Copy);
    assert_eq!(drag.target, None);
}

#[test]
fn drag_begin_on_an_unselected_entry_drags_only_that_entry_and_leaves_selection_unchanged() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1), file_entry("b.txt", 2)];
    state.left.selected.insert(OsString::from("a.txt"));
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("b.txt"), op: JobKind::Copy });
    let drag = state.drag.as_ref().expect("drag should have begun");
    assert_eq!(drag.items.len(), 1);
    assert_eq!(drag.items[0].original_name, OsString::from("b.txt"));
    assert!(state.left.selected.contains(&OsString::from("a.txt")), "selection must be unchanged");
    assert!(!state.left.selected.contains(&OsString::from("b.txt")));
}

#[test]
fn drag_begin_never_drags_the_parent_pseudo_entry() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![Entry::parent_dir(), file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from(".."), op: JobKind::Copy });
    assert!(state.drag.is_none(), "the parent-directory pseudo-entry SHALL never be dragged");
}

#[test]
fn drag_begin_on_an_entry_that_no_longer_exists_is_a_no_op() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("gone.txt"), op: JobKind::Copy });
    assert!(state.drag.is_none());
}

#[test]
fn drag_over_stores_a_valid_target_and_the_recomputed_verb() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy });
    let (state, _) = update(state, Command::DragOver { op: JobKind::Move, target: Some(DropTarget::PanelDir(PanelSide::Right)) });
    let drag = state.drag.as_ref().expect("drag must still be in progress");
    assert_eq!(drag.op, JobKind::Move, "the verb is recomputed on every DragOver (design D2)");
    assert_eq!(drag.target, Some(DropTarget::PanelDir(PanelSide::Right)));
}

#[test]
fn drag_over_rejects_the_items_own_directory() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy });
    let (state, _) = update(state, Command::DragOver { op: JobKind::Copy, target: Some(DropTarget::PanelDir(PanelSide::Left)) });
    assert_eq!(state.drag.as_ref().unwrap().target, None, "the items' own directory is never a valid target");
}

#[test]
fn drag_over_rejects_a_dragged_directory_onto_itself_or_its_own_descendant() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("sub")];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("sub"), op: JobKind::Copy });

    // Onto itself: the `sub` row of the very panel it was dragged from.
    let (state, _) = update(
        state,
        Command::DragOver { op: JobKind::Copy, target: Some(DropTarget::SubDir { side: PanelSide::Left, name: OsString::from("sub") }) },
    );
    assert_eq!(state.drag.as_ref().unwrap().target, None, "a directory dropped onto itself must be invalid");

    // Into a listing inside itself: the other panel has since navigated
    // into a descendant of the dragged directory.
    let mut state = state;
    state.right.cwd = PathBuf::from("/left/sub/nested");
    let (state, _) = update(state, Command::DragOver { op: JobKind::Copy, target: Some(DropTarget::PanelDir(PanelSide::Right)) });
    assert_eq!(state.drag.as_ref().unwrap().target, None, "a descendant of the dragged directory must be invalid");
}

#[test]
fn drag_over_rejects_info_and_quick_view_panels() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    state.right.display_mode = DisplayMode::Info;
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy });
    let (state, _) = update(state, Command::DragOver { op: JobKind::Copy, target: Some(DropTarget::PanelDir(PanelSide::Right)) });
    assert_eq!(state.drag.as_ref().unwrap().target, None, "Info-mode panels are never valid targets");
}

#[test]
fn drag_drop_opens_the_drop_dialog_prefilled_with_the_exact_target_path() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![Entry::parent_dir(), file_entry("a.txt", 1)];
    state.right.entries = vec![Entry::parent_dir(), dir_entry("OLD")];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy });
    let (state, _) = update(
        state,
        Command::DragOver { op: JobKind::Copy, target: Some(DropTarget::SubDir { side: PanelSide::Right, name: OsString::from("OLD") }) },
    );
    let (state, effects) = update(state, Command::DragDrop { op: JobKind::Copy });
    assert!(state.drag.is_none(), "the drag ends the moment the dialog opens");
    assert!(effects.is_empty(), "opening the dialog is not itself an effect");
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, input, buttons, .. }) => {
            assert_eq!(kind, JobKind::Copy);
            assert_eq!(sources.len(), 1);
            assert_eq!(sources[0].original_name, OsString::from("a.txt"));
            assert_eq!(input, PathBuf::from("/right").join("OLD").display().to_string());
            assert_eq!(buttons, Some(DropButtons { focused: JobKind::Copy }));
        }
        other => panic!("expected a drop-initiated DestinationInput dialog, got {other:?}"),
    }
}

#[test]
fn drag_drop_on_an_invalid_target_does_nothing() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy });
    // No DragOver ever validated a target — `target` stays `None`.
    let (state, effects) = update(state, Command::DragDrop { op: JobKind::Copy });
    assert!(matches!(state.phase, UiPhase::Panels), "no dialog opens over an invalid drop");
    assert!(effects.is_empty());
    assert!(state.drag.is_none());
}

#[test]
fn drag_drop_is_cancelled_when_the_source_panel_navigated_away() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy });
    let (state, _) = update(state, Command::DragOver { op: JobKind::Copy, target: Some(DropTarget::PanelDir(PanelSide::Right)) });
    // The source panel navigates elsewhere mid-drag.
    let mut state = state;
    state.left.cwd = PathBuf::from("/left/elsewhere");
    let (state, effects) = update(state, Command::DragDrop { op: JobKind::Copy });
    assert!(matches!(state.phase, UiPhase::Panels), "the drop is cancelled, not just retargeted");
    assert!(effects.is_empty());
    assert!(state.drag.is_none());
}

/// mouse-drag "Valid drop targets" scenario "Same-panel subdirectory": a
/// subdirectory row in the *same* panel the drag started from is a valid
/// target (only the items' own directory and self/descendant drops are
/// rejected — a different sibling subdirectory of the same panel is fine).
#[test]
fn drag_over_accepts_a_subdirectory_row_in_the_same_panel_as_the_source() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![Entry::parent_dir(), file_entry("notes.txt", 1), dir_entry("src")];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("notes.txt"), op: JobKind::Copy });
    let (state, _) = update(
        state,
        Command::DragOver { op: JobKind::Copy, target: Some(DropTarget::SubDir { side: PanelSide::Left, name: OsString::from("src") }) },
    );
    assert_eq!(
        state.drag.as_ref().unwrap().target,
        Some(DropTarget::SubDir { side: PanelSide::Left, name: OsString::from("src") }),
        "a sibling subdirectory in the source's own panel must be a valid target"
    );
    let (state, effects) = update(state, Command::DragDrop { op: JobKind::Copy });
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { input, .. }) => {
            assert_eq!(input, PathBuf::from("/left").join("src").display().to_string());
        }
        other => panic!("expected the same-panel subdirectory drop dialog, got {other:?}"),
    }
    assert!(effects.is_empty());
}

/// mouse-drag "Robust against listing changes": "the target row no longer
/// resolves to a directory" — a `SubDir` target validated by an earlier
/// `DragOver` must be re-checked at `DragDrop` time, since the entry it named
/// can have changed kind (or vanished) in between.
#[test]
fn drag_drop_is_cancelled_when_the_target_row_no_longer_resolves_to_a_directory() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    state.right.entries = vec![dir_entry("OLD")];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy });
    let (state, _) = update(
        state,
        Command::DragOver { op: JobKind::Copy, target: Some(DropTarget::SubDir { side: PanelSide::Right, name: OsString::from("OLD") }) },
    );
    assert!(state.drag.as_ref().unwrap().target.is_some(), "OLD is a directory, so the target should validate");

    // Between the last `DragOver` and the button-up, `OLD` stops being a
    // directory (e.g. a concurrent re-read replaced it with a same-named
    // file).
    let mut state = state;
    state.right.entries = vec![file_entry("OLD", 4)];
    let (state, effects) = update(state, Command::DragDrop { op: JobKind::Copy });
    assert!(matches!(state.phase, UiPhase::Panels), "the drop is cancelled when the target no longer resolves to a directory");
    assert!(effects.is_empty());
    assert!(state.drag.is_none());
}

/// mouse-drag "Robust against listing changes": the same re-validation also
/// cancels the drop outright when the target row disappears entirely rather
/// than merely changing kind.
#[test]
fn drag_drop_is_cancelled_when_the_target_row_has_vanished() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    state.right.entries = vec![dir_entry("OLD")];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy });
    let (state, _) = update(
        state,
        Command::DragOver { op: JobKind::Copy, target: Some(DropTarget::SubDir { side: PanelSide::Right, name: OsString::from("OLD") }) },
    );
    let mut state = state;
    state.right.entries = vec![]; // OLD is gone by the time the button comes up.
    let (state, effects) = update(state, Command::DragDrop { op: JobKind::Copy });
    assert!(matches!(state.phase, UiPhase::Panels), "the drop is cancelled when the target row no longer exists");
    assert!(effects.is_empty());
    assert!(state.drag.is_none());
}

/// mouse-drag + operation-dialogs: a Move-proposing drag never runs the job
/// directly on drop — it only ever opens the drop dialog with `[ Move ]`
/// focused, and the job starts strictly from an explicit
/// `FileOpConfirmAs`/`FileOpConfirm` on that dialog (a button click, or Enter
/// while it's focused). This is the named-test complement to the
/// `drag_proptests` invariant that no drag-lifecycle command ever emits
/// `Effect::RunJob` directly.
#[test]
fn a_move_drag_drop_opens_the_dialog_and_never_runs_the_job_directly() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    // Right-button drag (design D1): proposes Move.
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Move });
    let (state, _) = update(state, Command::DragOver { op: JobKind::Move, target: Some(DropTarget::PanelDir(PanelSide::Right)) });
    let (state, drop_effects) = update(state, Command::DragDrop { op: JobKind::Move });
    assert!(drop_effects.is_empty(), "DragDrop must never itself emit an effect, let alone RunJob");
    match &state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, buttons, .. }) => {
            assert_eq!(*kind, JobKind::Move);
            assert_eq!(*buttons, Some(DropButtons { focused: JobKind::Move }), "the dialog must open with [ Move ] focused, not run yet");
        }
        other => panic!("expected the drop dialog with Move focused, got {other:?}"),
    }
    // Only now, via the dialog's own confirm, does the job actually start.
    let (state, effects) = update(state, Command::FileOpConfirm);
    assert!(matches!(state.phase, UiPhase::FileOpRunning { .. }));
    match effects.as_slice() {
        [Effect::RunJob(job)] => assert_eq!(job.kind, JobKind::Move),
        other => panic!("expected exactly one RunJob(Move) from the dialog confirm, got {other:?}"),
    }
}

#[test]
fn drag_cancel_clears_the_drag_with_no_effect() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy });
    assert!(state.drag.is_some());
    let (state, effects) = update(state, Command::DragCancel);
    assert!(state.drag.is_none());
    assert!(effects.is_empty());
    assert!(matches!(state.phase, UiPhase::Panels));
}

/// mouse-drag "Cancel and phase-change clear the drag": "Pressing Esc during
/// a drag SHALL cancel it" (tasks.md 2.4). `input::map_panel_key` maps Esc to
/// `Command::DragCancel` while `state.drag` is `Some` (see
/// `filecommand-tui/src/input/mod.rs::map_panel_key`'s own test coverage) —
/// this closes the loop from the core side: a `DragDrop` that arrives *after*
/// the cancel (exactly what `MouseTracker` sends on the button-up that
/// follows a cancelled drag, since it only learns the drag ended once this
/// event lands) finds `state.drag` already `None` and is a pure no-op, so
/// "no dialog opens and nothing changes" holds end to end.
#[test]
fn a_drag_drop_that_arrives_after_esc_cancelled_the_drag_is_a_no_op() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy });
    let (state, _) = update(state, Command::DragOver { op: JobKind::Copy, target: Some(DropTarget::PanelDir(PanelSide::Right)) });
    let (state, _) = update(state, Command::DragCancel);
    assert!(state.drag.is_none(), "Esc must clear the drag immediately");

    // The button-up event the TUI's `MouseTracker` still has queued arrives
    // after the cancel — `resolve_drag_release` cannot know the drag was
    // cancelled and sends `DragDrop` regardless (mouse-panel-drag input
    // stage); core must not resurrect a dialog from it.
    let (state, effects) = update(state, Command::DragDrop { op: JobKind::Copy });
    assert!(matches!(state.phase, UiPhase::Panels), "no dialog opens from a drop after Esc cancelled the drag");
    assert!(effects.is_empty());
    assert!(state.drag.is_none());
}

#[test]
fn keyboard_copy_dialog_still_has_no_button_row() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::RequestCopy);
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { buttons, .. }) => {
            assert_eq!(buttons, None, "the keyboard F5 dialog has no button row");
        }
        other => panic!("expected DestinationInput, got {other:?}"),
    }
}

/// operation-dialogs "Switching the verb in the dialog": clicking `[ Move ]`
/// on a dialog that opened proposing Copy starts a Move job, without first
/// needing to change which button is focused.
#[test]
fn file_op_confirm_as_overrides_the_dialogs_opened_verb() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy });
    let (state, _) = update(state, Command::DragOver { op: JobKind::Copy, target: Some(DropTarget::PanelDir(PanelSide::Right)) });
    let (state, _) = update(state, Command::DragDrop { op: JobKind::Copy });
    match &state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, buttons, .. }) => {
            assert_eq!(*kind, JobKind::Copy);
            assert_eq!(*buttons, Some(DropButtons { focused: JobKind::Copy }));
        }
        other => panic!("expected drop dialog, got {other:?}"),
    }
    let (state, effects) = update(state, Command::FileOpConfirmAs(JobKind::Move));
    assert!(matches!(state.phase, UiPhase::FileOpRunning { .. }));
    match effects.as_slice() {
        [Effect::RunJob(job)] => assert_eq!(job.kind, JobKind::Move),
        other => panic!("expected exactly one RunJob(Move), got {other:?}"),
    }
}

/// `ButtonId::DropDialogCopy`/`DropDialogMove`/`DropDialogCancel` route
/// exactly like their keyboard equivalents via `button_command`.
#[test]
fn drop_dialog_button_ids_route_through_button_command() {
    assert_eq!(button_command(ButtonId::DropDialogCopy), Some(Command::FileOpConfirmAs(JobKind::Copy)));
    assert_eq!(button_command(ButtonId::DropDialogMove), Some(Command::FileOpConfirmAs(JobKind::Move)));
    assert_eq!(button_command(ButtonId::DropDialogCancel), Some(Command::FileOpCancel));
}

#[test]
fn drag_clears_when_the_f9_menu_opens() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy });
    assert!(state.drag.is_some());
    let (state, _) = update(state, Command::MenuOpen);
    assert!(state.drag.is_none(), "opening an overlay must clear an in-progress drag");
}

#[test]
fn drag_clears_on_quit_request() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy });
    let (state, _) = update(state, Command::RequestQuit);
    assert!(state.drag.is_none());
}

#[test]
fn drag_clears_on_listing_failure() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy });
    let (state, _) = update(state, Command::ListingFailed { panel: PanelSide::Right, message: "boom".to_string() });
    assert!(state.drag.is_none());
}

#[test]
fn drag_clears_on_resize_below_the_minimum() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy });
    let (state, _) = update(state, Command::Resize(10, 5));
    assert!(matches!(state.phase, UiPhase::Placeholder));
    assert!(state.drag.is_none());
}

mod drag_proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_side() -> impl Strategy<Value = PanelSide> {
        prop_oneof![Just(PanelSide::Left), Just(PanelSide::Right)]
    }

    fn arb_verb() -> impl Strategy<Value = JobKind> {
        prop_oneof![Just(JobKind::Copy), Just(JobKind::Move)]
    }

    fn arb_name() -> impl Strategy<Value = OsString> {
        prop_oneof![Just(OsString::from("a.txt")), Just(OsString::from("sub")), Just(OsString::from(".."))].prop_map(|n| n)
    }

    fn arb_drop_target() -> impl Strategy<Value = DropTarget> {
        prop_oneof![
            arb_side().prop_map(DropTarget::PanelDir),
            (arb_side(), arb_name()).prop_map(|(side, name)| DropTarget::SubDir { side, name }),
            (arb_side(), arb_name()).prop_map(|(side, name)| DropTarget::TreeNode { side, path: PathBuf::from(name) }),
            (arb_side(), 0usize..3).prop_map(|(side, index)| DropTarget::Tab { side, index }),
        ]
    }

    fn arb_command() -> impl Strategy<Value = Command> {
        prop_oneof![
            (arb_side(), arb_name(), arb_verb()).prop_map(|(side, name, op)| Command::DragBegin { side, name, op }),
            (arb_verb(), prop::option::of(arb_drop_target())).prop_map(|(op, target)| Command::DragOver { op, target }),
            arb_verb().prop_map(|op| Command::DragDrop { op }),
            Just(Command::DragCancel),
            (arb_side(), arb_name()).prop_map(|(side, name)| Command::ClickEntry { side, name, mods: ClickMods::Plain }),
            Just(Command::ToggleSelectAtCursor),
            Just(Command::RequestCopy),
            Just(Command::RequestMove),
            // `ConfirmQuit`/`CancelQuit` are deliberately excluded: core
            // assumes (via the TUI's mode-gating table, not enforced here)
            // that they only ever arrive while `state.quit_confirm` is
            // already `true` — `RequestQuit` alone already exercises the
            // relevant drag-clearing path (see `drag_clears_on_quit_request`
            // above).
            Just(Command::RequestQuit),
            Just(Command::MenuOpen),
            Just(Command::MenuClose),
            Just(Command::FileOpConfirm),
            arb_verb().prop_map(Command::FileOpConfirmAs),
            Just(Command::FileOpCancel),
            arb_side().prop_map(|panel| Command::ListingFailed { panel, message: "boom".to_string() }),
            (10u16..200, 4u16..80).prop_map(|(w, h)| Command::Resize(w, h)),
        ]
    }

    fn seeded_state() -> State {
        let mut state = test_state(UiPhase::Panels);
        state.left.entries = vec![Entry::parent_dir(), dir_entry("sub"), file_entry("a.txt", 1)];
        state.right.entries = vec![Entry::parent_dir(), dir_entry("sub"), file_entry("a.txt", 1)];
        state
    }

    fn any_overlay_open(state: &State) -> bool {
        state.menu.is_some()
            || state.drive_select.is_some()
            || state.fuzzy_jump.is_some()
            || state.find_file.is_some()
            || state.user_menu.is_some()
            || state.theme_picker.is_some()
            || state.file_action_menu.is_some()
            || state.help.is_some()
            || state.startup_warning.is_some()
            || state.quit_confirm
    }

    proptest! {
        /// mouse-drag "Cancel and phase-change clear the drag", property-
        /// tested over random command interleavings (design D5): `drag` is
        /// never `Some` outside `UiPhase::Panels` with no overlay open, and
        /// none of `DragBegin`/`DragOver`/`DragDrop`/`DragCancel` ever
        /// directly emits `Effect::RunJob` — a job only ever starts through
        /// the dialog's own `FileOpConfirm`/`FileOpConfirmAs`.
        #[test]
        fn drag_invariants_hold_over_random_command_interleavings(cmds in prop::collection::vec(arb_command(), 0..25)) {
            let mut state = seeded_state();
            for cmd in cmds {
                let is_drag_lifecycle_cmd =
                    matches!(cmd, Command::DragBegin { .. } | Command::DragOver { .. } | Command::DragDrop { .. } | Command::DragCancel);
                let (next_state, effects) = update(state, cmd);
                state = next_state;
                if is_drag_lifecycle_cmd {
                    prop_assert!(
                        !effects.iter().any(|e| matches!(e, Effect::RunJob(_))),
                        "DragBegin/DragOver/DragDrop/DragCancel must never directly emit RunJob"
                    );
                }
                if state.drag.is_some() {
                    prop_assert!(matches!(state.phase, UiPhase::Panels), "drag survived outside UiPhase::Panels");
                    prop_assert!(!any_overlay_open(&state), "drag survived with an overlay open");
                }
            }
        }
    }
}
