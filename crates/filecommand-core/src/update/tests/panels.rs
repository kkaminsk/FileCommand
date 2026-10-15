use super::*;

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
    assert_eq!(without_git_info_effects(effects), vec![start_listing(&state, PanelSide::Left, "/left/sub")]);
    // The panel's own entries were reset locally — no filesystem was touched.
    assert!(state.left.entries.is_empty());
    assert!(matches!(state.left.progress, crate::panel::ListingProgress::Streaming { count: 0 }));
}

#[test]
fn worker_events_reenter_through_update() {
    let state = test_state(UiPhase::Panels);
    let entries = vec![file_entry("a", 1)];
    let (state, effects) = listing_chunk(state, PanelSide::Left, entries.clone());
    assert!(effects.is_empty());
    assert_eq!(state.left.entries, entries);
    let (state, effects) = listing_complete(state, PanelSide::Left, 1);
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
    assert_eq!(without_git_info_effects(effects), vec![start_listing(&state, PanelSide::Left, "/a")]);
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
    assert_eq!(state.command_line.text(), "dir", "the typed buffer must survive RequestQuit untouched");
    let (state, _) = update(state, Command::CancelQuit);
    assert_eq!(state, before, "cancel must restore the typed command line bit-for-bit");
}

#[test]
fn request_quit_opens_under_an_active_quick_filter_and_cancel_restores_it() {
    let mut before = test_state(UiPhase::Panels);
    before.left.set_quick_filter(Some("rep".to_string()));
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
    let (before, _) = update(
        with_pending_open(test_state(UiPhase::Panels), 3),
        Command::ViewerOpened { request: 3, path: PathBuf::from("/left/big.log"), file_len: 5_000_000_000 },
    );
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
    let (state, _) = update(state, Command::Tick { now_ms: 1_799, epoch_ms: 1_799 });
    assert!(matches!(state.phase, UiPhase::Splash { .. }));
    let (state, _) = update(state, Command::Tick { now_ms: 1_800, epoch_ms: 1_800 });
    assert_eq!(state.phase, UiPhase::Panels);
}

#[test]
fn placeholder_below_min_and_grows_back_to_panels_never_splash() {
    // resize-preserves-session-state design D1/D2: "too small" is the
    // `undersized` flag beside the phase; the splash is dismissed for good.
    let state = test_state(UiPhase::Splash { started_at_ms: 0 });
    let (state, _) = update(state, Command::Resize(40, 10));
    assert!(state.undersized);
    assert_eq!(state.phase, UiPhase::Panels);
    let (state, _) = update(state, Command::Resize(80, 24));
    assert!(!state.undersized);
    assert_eq!(state.phase, UiPhase::Panels);
}

#[test]
fn too_small_boundary_at_59x16_is_placeholder() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::Resize(59, 16));
    assert!(state.undersized);
    assert_eq!(state.phase, UiPhase::Panels);
}

#[test]
fn too_small_boundary_at_60x15_is_placeholder() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::Resize(60, 15));
    assert!(state.undersized);
    assert_eq!(state.phase, UiPhase::Panels);
}

#[test]
fn too_small_boundary_at_60x16_is_panels() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::Resize(60, 16));
    assert!(!state.undersized);
    assert_eq!(state.phase, UiPhase::Panels);
}

#[test]
fn splash_skipped_when_starting_below_60x16_floor() {
    let (state, _) = State::initial(Theme::classic(), (59, 16), 0, 0, PathBuf::from("/l"), PathBuf::from("/r"), true);
    assert!(state.undersized);
    assert_eq!(state.phase, UiPhase::Panels);
}

#[test]
fn splash_starts_normally_at_exactly_60x16() {
    let (state, _) = State::initial(Theme::classic(), (60, 16), 0, 0, PathBuf::from("/l"), PathBuf::from("/r"), true);
    assert_eq!(state.phase, UiPhase::Splash { started_at_ms: 0 });
}

#[test]
fn placeholder_replaces_splash_when_resized_below_new_floor_mid_splash() {
    let state = test_state(UiPhase::Splash { started_at_ms: 0 });
    let (state, _) = update(state, Command::Resize(59, 16));
    assert!(state.undersized);
    assert_eq!(state.phase, UiPhase::Panels);
    // Enlarging back never returns to Splash, only Panels.
    let (state, _) = update(state, Command::Resize(60, 16));
    assert!(!state.undersized);
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
    let (state, _) = listing_chunk(state, PanelSide::Left, entries.clone());
    assert_eq!(state.left.entries, entries);
    assert!(matches!(state.phase, UiPhase::Splash { .. }));
}

#[test]
fn initial_builds_start_listing_effects_for_both_panels() {
    let (state, effects) = State::initial(Theme::classic(), (80, 24), 0, 0, PathBuf::from("/l"), PathBuf::from("/r"), true);
    assert_eq!(state.phase, UiPhase::Splash { started_at_ms: 0 });
    assert_eq!(without_git_info_effects(effects), vec![start_listing(&state, PanelSide::Left, "/l"), start_listing(&state, PanelSide::Right, "/r"),]);
}

#[test]
fn initial_below_min_size_skips_straight_to_placeholder() {
    let (state, _) = State::initial(Theme::classic(), (40, 10), 0, 0, PathBuf::from("/l"), PathBuf::from("/r"), true);
    assert!(state.undersized);
    assert_eq!(state.phase, UiPhase::Panels);
}

#[test]
fn initial_without_splash_starts_at_panels() {
    let (state, _) = State::initial(Theme::classic(), (80, 24), 0, 0, PathBuf::from("/l"), PathBuf::from("/r"), false);
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
            assert_eq!(input.text(), PathBuf::from("/right").display().to_string());
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
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Backspace)); // backspace on prefilled input
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Insert('X')));
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
    assert_eq!(state.command_line.text(), "dir");
    assert_eq!(state.left.cursor, 1, "typing must not move the panel cursor");
}

#[test]
fn backspace_edits_the_buffer_and_clears_the_history_cursor() {
    let mut state = test_state(UiPhase::Panels);
    state.history = vec!["old".to_string()];
    let state = type_line(state, "dir");
    let (state, _) = update(state, Command::CommandLineHistoryPrev);
    assert_eq!(state.command_line.text(), "old");
    assert_eq!(state.history_cursor, Some(0));

    let (state, _) = update(state, Command::TextEdit(TextEditOp::Backspace));
    assert_eq!(state.command_line.text(), "ol");
    assert_eq!(state.history_cursor, None, "editing leaves history recall");
}

#[test]
fn command_line_clear_command_still_clears_the_buffer() {
    // `Command::CommandLineClear` behaves as a plain "clear the buffer"
    // reducer action. Esc no longer dispatches it (`input::map_panel_key`
    // maps panel-level Esc to `RequestQuit` unconditionally); the shipped
    // trigger is the clear-line chord, Ctrl+Y by default (command-line
    // "Clear the command line with a dedicated key").
    let state = type_line(test_state(UiPhase::Panels), "dir");
    let (state, _) = update(state, Command::CommandLineClear);
    assert!(state.command_line.is_empty());
    assert_eq!(state.history_cursor, None);
}

#[test]
fn command_line_clear_ends_history_recall_and_hands_up_down_back_to_the_panel() {
    // command-line "Clear the command line with a dedicated key" scenario
    // "Clearing ends history recall": after the clear the buffer is empty,
    // so the input layer routes Up/Down to the panel cursor again — the
    // same release backspacing to empty performs (command-line "Command
    // history navigation"; see input/tests.rs `ctrl_y_clears_a_composed_line`).
    let mut state = test_state(UiPhase::Panels);
    state.history = vec!["old".to_string()];
    state.left.entries = vec![file_entry("a", 1), file_entry("b", 2)];
    let state = type_line(state, "dir");
    let (state, _) = update(state, Command::CommandLineHistoryPrev);
    assert_eq!(state.command_line.text(), "old");
    assert_eq!(state.history_cursor, Some(0));

    let (state, _) = update(state, Command::CommandLineClear);
    assert!(state.command_line.is_empty());
    assert_eq!(state.history_cursor, None, "clearing ends the recall");

    // With the buffer empty, the mapper sends cursor moves for Up/Down
    // again — the panel cursor must be free to move.
    let (state, _) = update(state, Command::MoveCursor(CursorMove::Down(1)));
    assert_eq!(state.left.cursor, 1, "the next Up/Down belongs to the panel, not to history");
    assert!(state.command_line.is_empty(), "no history entry was recalled");
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
    assert_eq!(state.command_line.text(), "dir");
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

    let (state, _) = update(state, Command::QuickFilterStart(PanelSide::Left));
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
    state.left.set_quick_filter(Some("rr".to_string()));
    let (state, _) = update(state, Command::QuickSearchStart('b'));
    assert_eq!(
        state.left.entries[state.left.cursor].name.to_string_lossy(),
        "berry",
        "the jump must only search entries the active filter leaves visible"
    );
}

// ---------------------------------------------------------------------
// filter-selection-safety: no current entry under an empty filter (R-17;
// quick-filter "Cursor and mini-status behavior under an active filter")
// ---------------------------------------------------------------------

/// A drive-root-style listing (no `..`) whose quick filter matches nothing,
/// with nothing selected: the panel has no current entry.
fn filtered_out_root_state() -> State {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from("/");
    state.left.entries = vec![file_entry("pagefile.sys", 1), file_entry("notes.txt", 1)];
    state.left.set_quick_filter(Some("zzzz".to_string()));
    state
}

/// quick-filter "Cursor-targeted operations do nothing while no entry is
/// visible" (tasks.md 4.3).
#[test]
fn cursor_targeted_commands_do_nothing_while_no_entry_is_visible() {
    for cmd in [
        Command::RequestDelete,
        Command::RequestCopy,
        Command::RequestMove,
        Command::RequestViewer,
        Command::RequestEditor,
        Command::Enter,
        Command::PasteCursorName,
    ] {
        let state = filtered_out_root_state();
        let command_line_before = state.command_line.clone();
        let (state, effects) = update(state, cmd.clone());
        assert!(effects.is_empty(), "{cmd:?} must emit no effects");
        assert_eq!(state.phase, UiPhase::Panels, "{cmd:?} must not change the phase");
        assert!(state.file_action_menu.is_none(), "{cmd:?} must not open the file-action menu");
        assert_eq!(state.command_line, command_line_before, "{cmd:?} must not touch the command line");
    }
}

/// quick-filter "Ins does nothing while no entry is visible" (tasks.md 4.3).
#[test]
fn ins_does_nothing_while_no_entry_is_visible() {
    let state = filtered_out_root_state();
    let (state, effects) = update(state, Command::ToggleSelectAtCursor);
    assert!(effects.is_empty());
    assert!(state.left.selected.is_empty(), "Ins must not toggle the hidden cursor entry");
}

/// quick-filter "Widening the pattern restores a current entry"
/// (tasks.md 4.4).
#[test]
fn widening_the_pattern_restores_cursor_targeted_commands() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from("/");
    state.left.entries = vec![file_entry("aaa.txt", 1), file_entry("notes.txt", 1)];
    let (state, _) = update(state, Command::QuickFilterStart(PanelSide::Left));
    let state = "notesz".chars().fold(state, |s, c| update(s, Command::QuickFilterChar(c)).0);
    assert!(state.left.selected().is_none(), "setup precondition: nothing visible");

    let (state, _) = update(state, Command::QuickFilterBackspace); // "notes"
    let (state, effects) = update(state, Command::RequestViewer);
    assert_eq!(effects, vec![Effect::OpenViewer { request: 3, path: PathBuf::from("/notes.txt") }]);
    assert_eq!(state.phase, UiPhase::Panels, "the viewer opens only once ViewerOpened comes back");
}

// ---------------------------------------------------------------------
// Panel tabs (Ctrl+T / Ctrl+W / Alt+1..9), reducer-level (task 15.5)
// ---------------------------------------------------------------------

#[test]
fn tab_commands_are_scoped_to_the_active_panel() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Left;

    let (state, _) = update(state, Command::OpenTab(PanelSide::Left));
    assert_eq!(state.left.tab_count(), 2);
    assert_eq!(state.right.tab_count(), 1, "the opposite panel's tabs must be untouched");

    let (mut state, _) = update(state, Command::CloseTab(PanelSide::Left));
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
// line-editing-in-text-fields: the shared LineEdit backing (reducer-level,
// tasks.md 2.5)
// ---------------------------------------------------------------------

#[test]
fn text_edit_inserts_at_the_caret_mid_command_line() {
    // command-line "Caret editing mid-line": `cd srv`, Left x2, `c` ->
    // `cd scrv`, with the panel cursor untouched.
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a", 1), file_entry("b", 2)];
    state.left.cursor = 1;
    let state = type_line(state, "cd srv");
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Left));
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Left));
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Insert('c')));
    assert_eq!(state.command_line.text(), "cd scrv");
    assert_eq!(state.command_line.caret(), 5, "the caret sits after the inserted c");
    assert_eq!(state.left.cursor, 1, "caret editing never moves the panel cursor");
}

#[test]
fn caret_moves_are_the_only_non_mutating_edits() {
    // A pure caret move leaves history recall alone; the next text change
    // ends it, exactly as typing always has.
    let mut state = test_state(UiPhase::Panels);
    state.history = vec!["old".to_string()];
    let state = type_line(state, "x");
    let (state, _) = update(state, Command::CommandLineHistoryPrev);
    assert_eq!(state.history_cursor, Some(0));
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Left));
    assert_eq!(state.history_cursor, Some(0), "a caret move does not end recall");
    let (state, _) = update(state, Command::TextEdit(TextEditOp::DeleteWordBack));
    assert_eq!(state.history_cursor, None, "a text-mutating edit ends recall");
    assert_eq!(state.command_line.text(), "d", "delete-word-back from mid-`old` removes `ol`");
}

#[test]
fn home_still_moves_the_panel_cursor_mid_composition() {
    // command-line "Home still moves the panel cursor while composing":
    // Home is a panel key even with text on the line, and the buffer and
    // its caret are unchanged by it.
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a", 1), file_entry("b", 2)];
    state.left.cursor = 1;
    let state = type_line(state, "cd docs");
    let (state, _) = update(state, Command::MoveCursor(CursorMove::Home));
    assert_eq!(state.left.cursor, 0, "Home jumps the panel cursor to the first entry");
    assert_eq!(state.command_line.text(), "cd docs");
    assert_eq!(state.command_line.caret(), 7, "the caret is untouched");
}

#[test]
fn history_recall_leaves_the_caret_at_the_end() {
    let mut state = test_state(UiPhase::Panels);
    state.history = vec!["cd somewhere".to_string()];
    let state = type_line(state, "x");
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Home));
    let (state, _) = update(state, Command::CommandLineHistoryPrev);
    assert_eq!(state.command_line.text(), "cd somewhere");
    assert_eq!(state.command_line.caret(), state.command_line.len_chars(), "recall lands the caret at the end");
}

#[test]
fn paste_flattens_line_breaks_and_inserts_at_the_caret() {
    // text-field-editing "Multi-line clipboard text is flattened": the two
    // clipboard lines land joined by one space, and the panel cursor does
    // not move.
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a", 1), file_entry("b", 2)];
    state.left.cursor = 1;
    let state = type_line(state, "cd ");
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Paste("one\r\ntwo".to_string())));
    assert_eq!(state.command_line.text(), "cd one two");
    assert_eq!(state.command_line.caret(), state.command_line.len_chars());
    assert_eq!(state.left.cursor, 1, "pasting never moves the panel cursor");
}

#[test]
fn request_paste_asks_the_tui_for_clipboard_text() {
    // design D4: the key map emits `RequestPaste`; the reducer turns it
    // into the clipboard-read effect whose answer comes back as
    // `TextEdit(Paste(..))`.
    let state = type_line(test_state(UiPhase::Panels), "x");
    let (state, effects) = update(state, Command::RequestPaste);
    assert_eq!(effects, vec![Effect::ReadClipboardText]);
    assert_eq!(state.command_line.text(), "x", "nothing changes until the clipboard text arrives");
}

#[test]
fn paste_cursor_entry_after_caret_editing_lands_at_the_end() {
    // command-line "Paste after caret editing lands at the end".
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("old.log", 1)];
    let state = type_line(state, "del ");
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Home));
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Right));
    let (state, _) = update(state, Command::PasteCursorName);
    assert_eq!(state.command_line.text(), "del old.log");
    assert_eq!(state.command_line.caret(), state.command_line.len_chars());
}

#[test]
fn destination_field_edits_in_the_middle() {
    // operation-dialogs "Editing the pre-filled destination in place":
    // `D:\backup`, Home, Delete, `E` -> `E:\backup`.
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    state.right.cwd = PathBuf::from(r"D:\backup");
    let (state, _) = update(state, Command::RequestCopy);
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Home));
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Delete));
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Insert('E')));
    match &state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { input, .. }) => {
            assert_eq!(input.text(), r"E:\backup");
        }
        other => panic!("expected FileOpSetup::DestinationInput, got {other:?}"),
    }
}

#[test]
fn destination_field_word_keys_and_paste_work() {
    // text-field-editing "Word delete" over the pre-filled path, then a
    // paste flattened into the field at the caret.
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    state.right.cwd = PathBuf::from(r"D:\backup\2026");
    let (state, _) = update(state, Command::RequestCopy);
    let (state, _) = update(state, Command::TextEdit(TextEditOp::DeleteWordBack));
    match &state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { input, .. }) => assert_eq!(input.text(), r"D:\backup\"),
        other => panic!("expected FileOpSetup::DestinationInput, got {other:?}"),
    }
    let (state, effects) = update(state, Command::RequestPaste);
    assert_eq!(effects, vec![Effect::ReadClipboardText]);
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Paste("one\ntwo".to_string())));
    match &state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { input, .. }) => {
            assert_eq!(input.text(), r"D:\backup\one two");
            assert_eq!(input.caret(), input.len_chars(), "the paste leaves the caret at the end");
        }
        other => panic!("expected FileOpSetup::DestinationInput, got {other:?}"),
    }
}

#[test]
fn pre_filled_destination_accepts_unchanged_with_enter() {
    // text-field-editing "Pre-filled field accepts unchanged with Enter":
    // the caret sits at the end of the pre-fill, so no editing key is
    // needed before accepting exactly as before this capability existed.
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::RequestCopy);
    match &state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { input, .. }) => {
            assert_eq!(input.text(), "/right");
            assert_eq!(input.caret(), input.len_chars(), "a pre-filled field opens with the caret at the end");
        }
        other => panic!("expected FileOpSetup::DestinationInput, got {other:?}"),
    }
    let (_, effects) = update(state, Command::FileOpConfirm);
    assert!(matches!(effects.as_slice(), [Effect::RunJob(Job { kind: JobKind::Copy, .. })]));
}

#[test]
fn rename_edits_the_stem_without_retyping_the_extension() {
    // text-field-editing "Insert in the middle of a pre-filled name":
    // `draft.txt` -> `drift.txt` via Left, Backspace, `i`. (The spec
    // scenario's "Left five times" is off by one — from the end of the
    // 9-char name the `a` is six Lefts away; the stated result `drift.txt`
    // is what the reducer must produce.)
    let state = opened_menu_state(&["draft.txt"], "draft.txt");
    let (state, _) = update(state, Command::FileActionMenuHotkey('R'));
    match &state.phase {
        UiPhase::FileOpSetup(FileOpSetup::RenameInput { input, .. }) => {
            assert_eq!(input.text(), "draft.txt");
            assert_eq!(input.caret(), input.len_chars(), "the rename field opens with the caret at the end");
        }
        other => panic!("expected FileOpSetup::RenameInput, got {other:?}"),
    }
    let state = (0..6).fold(state, |s, _| update(s, Command::TextEdit(TextEditOp::Left)).0);
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Backspace));
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Insert('i')));
    match &state.phase {
        UiPhase::FileOpSetup(FileOpSetup::RenameInput { input, .. }) => {
            assert_eq!(input.text(), "drift.txt");
            assert_eq!(input.caret(), 3, "the caret sits after the inserted i");
        }
        other => panic!("expected FileOpSetup::RenameInput, got {other:?}"),
    }
}

#[test]
fn conflict_rename_field_edits_in_the_middle() {
    // The overwrite-conflict Rename field is the same shared text field:
    // Home, then typing, prepends to the composed name.
    let state = running_progress_state(JobKind::Copy, "/left", "/right");
    let (state, _) = update(state, Command::JobConflict(sample_conflict()));
    let (state, _) = update(state, Command::FileOpBeginRename);
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Insert('b')));
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Home));
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Insert('a')));
    let (_state, effects) = update(state, Command::FileOpConfirm);
    assert_eq!(effects, vec![Effect::SendConflictReply(ConflictChoice::Rename(OsString::from("ab")))]);
}

#[test]
fn find_file_pattern_edits_mid_string_until_submitted() {
    // find-file "Pattern is editable mid-string": `reprt`, Left x2, `o` ->
    // `report`; once submitted, editing keys stop reaching the pattern.
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::FindFileOpen);
    let state = "reprt".chars().fold(state, |s, c| update(s, Command::TextEdit(TextEditOp::Insert(c))).0);
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Left));
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Left));
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Insert('o')));
    assert_eq!(state.find_file.as_ref().unwrap().pattern.text(), "report");
    let (state, _) = update(state, Command::FindFileSubmit);
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Insert('x')));
    assert_eq!(state.find_file.as_ref().unwrap().pattern.text(), "report", "the pattern is fixed once the walk is in flight");
}

#[test]
fn fuzzy_jump_pattern_edited_mid_string_renarrows() {
    // fuzzy-jump "Pattern edited mid-string re-narrows the list": `prj`,
    // Left, `o` -> `proj`; a pure caret move keeps the highlight, the text
    // change resets it to the top of the newly narrowed list.
    let mut state = test_state(UiPhase::Panels);
    state.dir_history = vec![history_entry(r"C:\proj", 1, 0), history_entry(r"C:\prj-other", 1, 0)];
    let (state, _) = update(state, Command::FuzzyJumpOpen);
    let state = "prj".chars().fold(state, |s, c| update(s, Command::TextEdit(TextEditOp::Insert(c))).0);
    let (state, _) = update(state, Command::FuzzyJumpMove(1));
    assert_eq!(state.fuzzy_jump.as_ref().unwrap().cursor, 1);
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Left));
    assert_eq!(state.fuzzy_jump.as_ref().unwrap().cursor, 1, "a caret move never re-narrows");
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Insert('o')));
    let fj = state.fuzzy_jump.as_ref().unwrap();
    assert_eq!(fj.pattern.text(), "proj");
    assert_eq!(fj.cursor, 0, "the list re-narrows against `proj` from the top");
}

// `running_a_command_records_history_clears_the_buffer_and_persists` and
// `running_a_command_uses_the_configured_shell` used to assert that
// arbitrary typed text (`dir`, `Get-ChildItem`) reached the shell — no
// longer true now that the command line only recognizes `cd`/`del`/`rmdir`
// (command-line "Command-line builtin whitelist"). The buffer-clearing
// assertion moves to the rejection path below; `shell::build_command`
// respecting the configured shell is still exercised, just through the F2
// user menu, one of its two remaining (unaffected) call sites.
#[test]
fn an_unrecognized_command_clears_the_buffer_and_is_rejected_without_touching_history() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    let state = type_line(state, "dir");
    let (state, effects) = update(state, Command::Enter);

    assert!(state.command_line.is_empty(), "the buffer is cleared even for a rejected line");
    assert!(state.history.is_empty(), "a rejected command is not recorded in history");
    assert!(effects.is_empty(), "nothing is spawned and nothing is persisted, got {effects:?}");
    assert_eq!(state.left.last_error.as_deref(), Some("'dir' is not a recognized command"));
    assert_eq!(state.prompt(), format!("{}>", PathBuf::from(r"C:\NORTON").display()), "the panel itself is untouched");
}

#[test]
fn f2_user_menu_entry_uses_the_configured_shell() {
    let mut state = test_state(UiPhase::Panels);
    state.shell.shell = Some("powershell".to_string());
    state.user_menu_entries = vec![crate::config::UserMenuEntry { label: "List".to_string(), command: "Get-ChildItem".to_string() }];
    let (state, _) = update(state, Command::UserMenuOpen);
    let (_, effects) = update(state, Command::UserMenuConfirm);
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
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(without_git_info_effects(effects), vec![start_listing(&state, PanelSide::Left, "/left/sub")]);
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
    assert_eq!(menu.entries[1], FileActionMenuEntry::Open, "Open follows Run for an executable (open-with-associated-app design D2)");
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
    assert_eq!(menu.entries[0], FileActionMenuEntry::Open, "non-executable: Open is listed first (open-with-associated-app design D2)");
    assert_eq!(menu.selected(), FileActionMenuEntry::Open, "Open is highlighted by default");
}

// ---------------------------------------------------------------------
// relative-destination-resolution: the shared resolver's new contract
// (command-line "cd, del and rmdir targets resolve to a normalized
// absolute path") — the same rules the destination input dialog resolves
// with (operation-dialogs "Relative destinations resolve against the
// source panel directory")
// ---------------------------------------------------------------------

/// The resolver's form table (design D2): every result is absolute and
/// free of `.`/`..` components whenever `cwd` is, and a drive-relative
/// form never depends on the process's per-drive current directory.
#[cfg(windows)]
#[test]
fn resolve_cd_target_normalizes_and_resolves_drive_relative_forms() {
    assert_eq!(resolve_cd_target(Path::new(r"C:\work\proj"), r"..\archive"), Some(PathBuf::from(r"C:\work\archive")));
    assert_eq!(resolve_cd_target(Path::new(r"C:\work"), "backup"), Some(PathBuf::from(r"C:\work\backup")));
    // A drive-rooted path keeps the panel's drive.
    assert_eq!(resolve_cd_target(Path::new(r"C:\work"), r"\archive"), Some(PathBuf::from(r"C:\archive")));
    // A drive-relative form resolves against the panel's directory when it
    // is on that drive (compared case-insensitively), and against the
    // drive's root otherwise.
    assert_eq!(resolve_cd_target(Path::new(r"D:\work"), "D:old"), Some(PathBuf::from(r"D:\work\old")));
    assert_eq!(resolve_cd_target(Path::new(r"d:\work"), "D:old"), Some(PathBuf::from(r"d:\work\old")));
    assert_eq!(resolve_cd_target(Path::new(r"C:\work"), "D:old"), Some(PathBuf::from(r"D:\old")));
    // UNC and absolute paths pass through unchanged; an absolute path
    // carrying `..` is normalized rather than kept as typed.
    assert_eq!(resolve_cd_target(Path::new(r"C:\work"), r"\\server\share\dest"), Some(PathBuf::from(r"\\server\share\dest")));
    assert_eq!(resolve_cd_target(Path::new(r"C:\work"), r"E:\backup"), Some(PathBuf::from(r"E:\backup")));
    assert_eq!(resolve_cd_target(Path::new(r"C:\work"), r"C:\a\..\b"), Some(PathBuf::from(r"C:\b")));
    // `..` at a drive root still resolves to nothing.
    assert_eq!(resolve_cd_target(Path::new(r"C:\"), ".."), None);
}

/// The normalization half of the new contract is platform-independent (the
/// Windows-only drive forms are covered by the test above).
#[test]
fn resolve_cd_target_removes_dot_and_dotdot_components() {
    assert_eq!(resolve_cd_target(Path::new("/a/b"), "../c"), Some(PathBuf::from("/a/c")));
    assert_eq!(resolve_cd_target(Path::new("/a/b"), "./c"), Some(PathBuf::from("/a/b/c")));
    assert_eq!(resolve_cd_target(Path::new("/a/b"), "c/../../d"), Some(PathBuf::from("/a/d")));
}

/// command-line "cd with a parent-relative path lands on the normalized
/// directory": `cd ..\x` navigates to the *normalized* path, so both the
/// probe and the `StartListing` it begins carry no `..` component (a
/// verbatim-prefixed path with one fails with OS error 123). The resolved
/// parent is never the panel's own directory, so the check always goes
/// through the asynchronous probe (offload-blocking-io design D1).
#[cfg(windows)]
#[test]
fn cd_with_a_parent_relative_path_lists_the_normalized_directory() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\work\proj");
    let state = type_line(state, r"cd ..\x");
    let (state, effects) = update(state, Command::Enter);
    let expected = PathBuf::from(r"C:\work\x");
    assert_eq!(effects, vec![Effect::ProbePath { request: 3, path: expected.clone() }], "the probe takes the normalized path");
    let (state, effects) = update(state, Command::PathProbed { request: 3, kind: ProbeKind::Dir });
    assert_eq!(state.left.cwd, expected, "the panel lands on the normalized directory");
    assert!(without_git_info_effects(effects).contains(&start_listing(&state, PanelSide::Left, expected)), "the listing targets the resolved path");
}

/// command-line "cd with a drive-relative path resolves against the panel
/// or the drive root".
#[cfg(windows)]
#[test]
fn cd_with_a_drive_relative_path_resolves_against_the_panel_or_the_drive_root() {
    // On the target's own drive the panel directory stands in for the
    // drive's current directory: `D:old` names the listed entry `old`.
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"D:\work");
    state.left.entries = vec![dir_entry("old")];
    let state = type_line(state, "cd D:old");
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(state.left.cwd, PathBuf::from(r"D:\work\old"));
    let effects = without_git_info_effects(effects);
    assert!(
        effects.contains(&start_listing(&state, PanelSide::Left, r"D:\work\old")),
        "the listing targets the panel-relative resolution, got {effects:?}"
    );

    // On any other drive there is no per-drive current directory to lean
    // on, so the target resolves against the drive's root: the probe is
    // issued for exactly the resolved path, and a `Missing` reply names it.
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\work");
    let state = type_line(state, "cd D:old");
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(effects, vec![Effect::ProbePath { request: 3, path: PathBuf::from(r"D:\old") }], "the probe takes the drive-root resolution");
    let (state, effects) = update(state, Command::PathProbed { request: 3, kind: ProbeKind::Missing });
    assert!(effects.is_empty(), "a nonexistent target starts no listing, got {effects:?}");
    let expected = format!("{} not found", PathBuf::from(r"D:\old").display());
    assert_eq!(state.left.last_error.as_deref(), Some(expected.as_str()));
}

/// command-line "del with a parent-relative path names the resolved file":
/// the `..` resolves against the panel's directory, so the probe and the
/// delete-confirmation dialog it opens are for the file the path actually
/// points at.
#[cfg(windows)]
#[test]
fn del_with_a_parent_relative_path_names_the_resolved_file() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\work\proj");
    let state = type_line(state, r"del ..\old.txt");
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(effects, vec![Effect::ProbePath { request: 3, path: PathBuf::from(r"C:\work\old.txt") }]);
    let (state, effects) = update(state, Command::PathProbed { request: 3, kind: ProbeKind::File });
    assert!(effects.is_empty(), "nothing is deleted until the dialog is accepted, got {effects:?}");
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { ref sources, .. }) => {
            assert_eq!(sources.len(), 1);
            assert_eq!(sources[0].path, PathBuf::from(r"C:\work\old.txt"), "the dialog names the resolved, normalized path");
            assert_eq!(sources[0].original_name, OsString::from("old.txt"));
            assert!(!sources[0].is_dir);
        }
        other => panic!("expected the delete-confirmation dialog, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// command-line-builtin-whitelist: `del`/`rmdir` typed builtins
// ---------------------------------------------------------------------

#[test]
fn del_on_a_file_opens_the_delete_confirm_dialog_naming_it() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    state.left.entries = vec![file_entry("notes.txt", 1)];
    let state = type_line(state, "del notes.txt");
    let (state, effects) = update(state, Command::Enter);
    assert!(effects.is_empty(), "nothing is deleted until the dialog is accepted, got {effects:?}");
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { ref sources, needs_second_confirm, confirmed_once, .. }) => {
            assert_eq!(sources.len(), 1);
            assert_eq!(sources[0].original_name, OsString::from("notes.txt"));
            assert_eq!(sources[0].path, PathBuf::from(r"C:\NORTON\notes.txt"));
            assert!(!sources[0].is_dir);
            assert!(!needs_second_confirm, "a single file needs only one confirmation");
            assert!(!confirmed_once);
        }
        other => panic!("expected the delete-confirm dialog to open, got {other:?}"),
    }
    assert!(state.history.is_empty(), "`del` is not recorded in command history");
}

#[test]
fn rmdir_on_a_directory_opens_the_delete_confirm_dialog_and_needs_second_confirmation() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    state.left.entries = vec![dir_entry("docs")];
    let state = type_line(state, "rmdir docs");
    let (state, effects) = update(state, Command::Enter);
    assert!(effects.is_empty());
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { ref sources, needs_second_confirm, confirmed_once, .. }) => {
            assert_eq!(sources.len(), 1);
            assert_eq!(sources[0].original_name, OsString::from("docs"));
            assert!(sources[0].is_dir);
            assert!(needs_second_confirm, "a directory requires the existing second confirmation");
            assert!(!confirmed_once);
        }
        other => panic!("expected the delete-confirm dialog to open, got {other:?}"),
    }

    let (state, effects) = update(state, Command::FileOpConfirm);
    assert!(effects.is_empty(), "first confirm just arms the second confirmation");
    let (_, effects) = update(state, Command::FileOpConfirm);
    assert!(matches!(effects.as_slice(), [Effect::RunJob(Job { kind: JobKind::Delete, .. })]), "nothing was deleted until the second confirm");
}

#[test]
fn del_rejects_a_directory_target() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    state.left.entries = vec![dir_entry("docs")];
    let state = type_line(state, "del docs");
    let (state, effects) = update(state, Command::Enter);
    assert!(effects.is_empty());
    assert_eq!(state.phase, UiPhase::Panels, "no dialog opens");
    let expected = format!("{} is a directory", PathBuf::from(r"C:\NORTON\docs").display());
    assert_eq!(state.left.last_error.as_deref(), Some(expected.as_str()));
}

#[test]
fn rmdir_rejects_a_file_target() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    state.left.entries = vec![file_entry("notes.txt", 1)];
    let state = type_line(state, "rmdir notes.txt");
    let (state, effects) = update(state, Command::Enter);
    assert!(effects.is_empty());
    assert_eq!(state.phase, UiPhase::Panels, "no dialog opens");
    let expected = format!("{} is not a directory", PathBuf::from(r"C:\NORTON\notes.txt").display());
    assert_eq!(state.left.last_error.as_deref(), Some(expected.as_str()));
}

#[test]
fn del_and_rmdir_reject_dot_targets() {
    for verb in ["del", "rmdir"] {
        for target in [".", "..", r".\", r"..\"] {
            let mut state = test_state(UiPhase::Panels);
            state.left.cwd = PathBuf::from(r"C:\NORTON");
            state.left.entries = vec![dir_entry("docs")];
            let state = type_line(state, &format!("{verb} {target}"));
            let (state, effects) = update(state, Command::Enter);
            assert!(effects.is_empty(), "{verb} {target}: no dialog and no effects, got {effects:?}");
            assert_eq!(state.phase, UiPhase::Panels, "{verb} {target}: the delete-confirm dialog must never open on the current or parent directory");
            let expected = format!("{verb}: invalid target {target}");
            assert_eq!(state.left.last_error.as_deref(), Some(expected.as_str()), "{verb} {target}: invalid-target error");
        }
    }
}

#[test]
fn del_dotdot_sibling_is_not_caught_by_the_dot_target_guard() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    let state = type_line(state, r"del ..\sibling.txt");
    let (state, effects) = update(state, Command::Enter);
    assert!(
        matches!(effects.as_slice(), [Effect::ProbePath { .. }]),
        "a multi-component relative target follows the ordinary probe path, got {effects:?}"
    );
    assert_eq!(state.phase, UiPhase::Panels);
    let (state, effects) = update(state, Command::PathProbed { request: 3, kind: ProbeKind::Missing });
    assert!(effects.is_empty());
    assert_eq!(state.phase, UiPhase::Panels);
    let err = state.left.last_error.unwrap();
    assert!(err.ends_with("not found"), "a multi-component relative target follows the ordinary not-found path, got: {err}");
}

/// command-line "del/rmdir on a nonexistent target is rejected": the
/// unlisted target is probed, and the `Missing` reply rejects it with no
/// dialog.
#[test]
fn del_and_rmdir_on_a_nonexistent_target_are_rejected() {
    for verb in ["del", "rmdir"] {
        let mut state = test_state(UiPhase::Panels);
        state.left.cwd = PathBuf::from(r"C:\NORTON");
        let state = type_line(state, &format!(r"{verb} \NOSUCHFILE.TXT"));
        let (state, effects) = update(state, Command::Enter);
        assert_eq!(
            effects,
            vec![Effect::ProbePath { request: 3, path: PathBuf::from(r"C:\NOSUCHFILE.TXT") }],
            "{verb}: the unlisted target is probed"
        );
        assert_eq!(state.phase, UiPhase::Panels, "{verb}: no dialog opens before the reply");
        assert_eq!(
            state.command_probe.as_ref().map(|p| p.intent.clone()),
            Some(ProbeIntent::Delete { want_dir: verb == "rmdir" }),
            "{verb}: the pending probe records the verb"
        );
        let (state, effects) = update(state, Command::PathProbed { request: 3, kind: ProbeKind::Missing });
        assert!(effects.is_empty(), "{verb}: nothing spawns and no dialog opens, got {effects:?}");
        assert_eq!(state.phase, UiPhase::Panels, "{verb}: no dialog opens");
        let expected = format!("{} not found", PathBuf::from(r"C:\NOSUCHFILE.TXT").display());
        assert_eq!(state.left.last_error.as_deref(), Some(expected.as_str()), "{verb}: the panel shows a not-found error");
    }
}

/// command-line "del on an unlisted target on a slow share does not freeze
/// the UI": the probe reply of the matching type opens the dialog naming
/// the target, and a mismatched type gives the verb's own rejection.
#[test]
fn del_and_rmdir_on_an_unlisted_target_open_the_dialog_on_a_type_match() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    let state = type_line(state, r"del \\nas\share\old.log");
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(effects, vec![Effect::ProbePath { request: 3, path: PathBuf::from(r"\\nas\share\old.log") }]);
    assert_eq!(state.left.pending_note.as_deref(), Some(r"Checking \\nas\share\old.log…"));
    let (state, effects) = update(state, Command::PathProbed { request: 3, kind: ProbeKind::File });
    assert!(effects.is_empty(), "nothing is deleted until the dialog is accepted");
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { ref sources, needs_second_confirm, .. }) => {
            assert_eq!(sources.len(), 1);
            assert_eq!(sources[0].original_name, OsString::from("old.log"));
            assert_eq!(sources[0].path, PathBuf::from(r"\\nas\share\old.log"));
            assert!(!sources[0].is_dir);
            assert!(!needs_second_confirm);
        }
        other => panic!("expected the delete-confirmation dialog, got {other:?}"),
    }
    assert_eq!(state.left.pending_note, None);

    // `rmdir` on a directory reply: the dialog, with its second confirmation.
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    let state = type_line(state, r"rmdir \\nas\share\tmp");
    let (state, _) = update(state, Command::Enter);
    let (state, _) = update(state, Command::PathProbed { request: 3, kind: ProbeKind::Dir });
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { ref sources, needs_second_confirm, .. }) => {
            assert_eq!(sources[0].original_name, OsString::from("tmp"));
            assert!(sources[0].is_dir);
            assert!(needs_second_confirm);
        }
        other => panic!("expected the delete-confirmation dialog, got {other:?}"),
    }

    // Type mismatches give the verb's own rejection and open nothing.
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    let state = type_line(state, r"del \\nas\share\tmp");
    let (state, _) = update(state, Command::Enter);
    let (state, _) = update(state, Command::PathProbed { request: 3, kind: ProbeKind::Dir });
    assert_eq!(state.phase, UiPhase::Panels);
    assert_eq!(state.left.last_error.as_deref(), Some(format!("{} is a directory", PathBuf::from(r"\\nas\share\tmp").display()).as_str()));

    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    let state = type_line(state, r"rmdir \\nas\share\old.log");
    let (state, _) = update(state, Command::Enter);
    let (state, _) = update(state, Command::PathProbed { request: 3, kind: ProbeKind::File });
    assert_eq!(state.phase, UiPhase::Panels);
    assert_eq!(state.left.last_error.as_deref(), Some(format!("{} is not a directory", PathBuf::from(r"\\nas\share\old.log").display()).as_str()));
}

/// command-line "A late answer does not open the dialog over a menu": the
/// panels no longer own input, so the reply is discarded — no dialog, no
/// error — and the pending entry is cleared.
#[test]
fn a_delete_probe_reply_after_the_menu_opened_opens_no_dialog() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    let state = type_line(state, r"rmdir \\nas\share\tmp");
    let (state, _) = update(state, Command::Enter);
    let (state, _) = update(state, Command::MenuOpen);
    assert!(state.menu.is_some());
    let (state, effects) = update(state, Command::PathProbed { request: 3, kind: ProbeKind::Dir });
    assert!(effects.is_empty());
    assert!(state.menu.is_some(), "the menu stays open");
    assert_eq!(state.phase, UiPhase::Panels, "the delete-confirmation dialog does not open");
    assert_eq!(state.left.last_error, None, "no error either: the reply is simply dropped");
    assert_eq!(state.command_probe, None);
    assert_eq!(state.left.pending_note, None);
}

/// A `del`/`rmdir` reply superseded by a newer command-line check, or one
/// arriving after the panel navigated, is ignored.
#[test]
fn a_delete_probe_reply_that_is_stale_or_whose_panel_navigated_is_ignored() {
    // Superseded by a newer probe.
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    let state = type_line(state, r"del \\nas\share\a.log");
    let (state, _) = update(state, Command::Enter);
    let state = type_line(state, r"del \\nas\share\b.log");
    let (state, _) = update(state, Command::Enter);
    let before = state.clone();
    let (state, effects) = update(state, Command::PathProbed { request: 3, kind: ProbeKind::File });
    assert!(effects.is_empty());
    assert_eq!(state, before, "the stale reply changes nothing");

    // The panel navigated meanwhile.
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    state.left.entries = vec![dir_entry("docs")];
    let state = type_line(state, r"del \\nas\share\a.log");
    let (state, _) = update(state, Command::Enter);
    let (state, _) = update(state, Command::Enter); // into `docs`
    let (state, effects) = update(state, Command::PathProbed { request: 3, kind: ProbeKind::File });
    assert!(effects.is_empty());
    assert_eq!(state.phase, UiPhase::Panels, "no dialog opens for a panel that moved on");
    assert_eq!(state.left.last_error, None);
    assert_eq!(state.command_probe, None);
}

#[test]
fn del_and_rmdir_verbs_are_case_insensitive() {
    assert_eq!(parse_del("DEL notes.txt"), Some("notes.txt".to_string()));
    assert_eq!(parse_rmdir("RmDir docs"), Some("docs".to_string()));
    assert_eq!(parse_del("delete notes.txt"), None, "`delete` must not falsely match the `del` verb");
    assert_eq!(parse_rmdir("rd docs"), None, "`rd` is not an alias for `rmdir` (design.md non-goal)");
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
    assert_eq!(without_git_info_effects(effects), vec![start_listing(&state, PanelSide::Left, "/left")]);
    assert!(state.left.entries.is_empty(), "a re-read discards the current entries and streams fresh ones");
    assert!(state.left.progress.is_streaming());
}

#[test]
fn swap_panels_exchanges_everything_and_keeps_focus_and_split() {
    // Spec scenario "Swap keeps focus on the same side".
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from("C:\\a");
    state.right.cwd = PathBuf::from("D:\\b");
    state.left.entries = vec![file_entry("a.txt", 1)];
    state.right.entries = vec![file_entry("b.txt", 2), file_entry("c.txt", 3)];
    state.right.cursor = 1;
    state.right.scroll_offset = 0;
    state.right.selected.insert(OsString::from("b.txt"));
    state.left.sort_mode = SortMode::Size;
    state.split_percent = 60;
    complete_panel_listings(&mut state);

    let (state, effects) = update(state, Command::SwapPanels);
    assert_eq!(state.left.cwd, PathBuf::from("D:\\b"));
    assert_eq!(state.right.cwd, PathBuf::from("C:\\a"));
    assert_eq!(state.left.entries.len(), 2, "the listing moved with the panel");
    assert_eq!(state.left.cursor, 1, "the cursor moved with the panel");
    assert!(state.left.selected.contains(&OsString::from("b.txt")), "the selection moved with the panel");
    assert_eq!(state.right.sort_mode, SortMode::Size, "the sort mode moved with the panel");
    assert_eq!(state.active, PanelSide::Left, "the active side is unchanged");
    assert_eq!(state.split_percent, 60, "the split is unchanged");
    assert!(state.prompt().starts_with("D:\\b>"), "the command-line prompt follows: {}", state.prompt());
    // No directory is re-read (design D3); both sides' git queries are
    // re-issued with fresh ids so pre-swap replies drop as stale.
    assert!(effects.iter().all(|e| !matches!(e, Effect::StartListing { .. })), "no re-read: {effects:?}");
    assert!(effects.iter().any(|e| matches!(e, Effect::QueryGitInfo { panel: PanelSide::Left, path, .. } if *path == Path::new("D:\\b"))));
    assert!(effects.iter().any(|e| matches!(e, Effect::QueryGitInfo { panel: PanelSide::Right, path, .. } if *path == Path::new("C:\\a"))));
}

#[test]
fn swap_panels_carries_tabs_and_selection() {
    // Spec scenario "Swap carries tabs and selection".
    let mut state = test_state(UiPhase::Panels);
    state.right.open_tab();
    state.right.open_tab();
    state.right.selected.insert(OsString::from("x"));
    state.right.selected.insert(OsString::from("y"));
    complete_panel_listings(&mut state);

    let (state, _) = update(state, Command::SwapPanels);
    assert_eq!(state.left.tab_count(), 3, "the tabs moved to the left panel");
    assert_eq!(state.right.tab_count(), 1);
    assert_eq!(state.left.selected.len(), 2, "the selection moved with them");
}

#[test]
fn swap_panels_is_refused_while_either_listing_is_streaming() {
    // Spec scenario "Swap refused while reading".
    for streaming_side in [PanelSide::Left, PanelSide::Right] {
        let mut state = test_state(UiPhase::Panels);
        complete_panel_listings(&mut state);
        state.panel_mut(streaming_side).progress = crate::panel::ListingProgress::Streaming { count: 0 };
        let before = state.clone();
        let (state, effects) = update(state, Command::SwapPanels);
        assert!(effects.is_empty());
        assert_eq!(state, before, "a streaming panel ({streaming_side:?}) refuses the swap");
    }
}

#[test]
fn swap_panels_is_ignored_during_a_drag() {
    let mut state = test_state(UiPhase::Panels);
    complete_panel_listings(&mut state);
    state.drag = Some(DragState {
        source: PanelSide::Left,
        source_dir: state.left.cwd.clone(),
        items: vec![SourceItem { original_name: OsString::from("a.txt"), path: PathBuf::from("/left/a.txt"), is_dir: false }],
        op: JobKind::Copy,
        target: None,
    });
    let (state, effects) = update(state, Command::SwapPanels);
    assert!(effects.is_empty());
    assert_eq!(state.left.cwd, PathBuf::from("/left"), "nothing swapped");
}

#[test]
fn swap_panels_re_issues_info_and_git_queries_with_fresh_ids() {
    // Spec scenario "Git info follows the swap" + design D3: pending queries
    // are re-issued under fresh request ids, so a reply from before the swap
    // is discarded as stale.
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from("C:\\repo");
    state.left.display_mode = DisplayMode::Info;
    state.left.info_request = Some(10);
    state.left.git_request = Some(11);
    state.right.git_request = Some(12);
    state.request_seq = 100;
    complete_panel_listings(&mut state);

    let (state, effects) = update(state, Command::SwapPanels);
    // The left panel (the old right, still in Info? no — Info moved right):
    // Info mode moved with the panel, so the *right* panel is now in Info.
    assert_eq!(state.right.display_mode, DisplayMode::Info, "display mode moved with the panel");
    let info_query = effects.iter().find_map(|e| match e {
        Effect::QueryInfo { panel, request, .. } => Some((*panel, *request)),
        _ => None,
    });
    assert_eq!(info_query.map(|(p, _)| p), Some(PanelSide::Right), "QueryInfo is re-issued for the side now in Info mode");
    let fresh: Vec<u64> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::QueryInfo { request, .. } | Effect::QueryGitInfo { request, .. } => Some(*request),
            _ => None,
        })
        .collect();
    assert_eq!(fresh.len(), 3, "one Info query (right) plus two git queries");
    assert!(fresh.iter().all(|id| *id > 100), "every re-issued query carries a fresh id: {fresh:?}");
    assert!(state.right.info_request > Some(100) && state.left.git_request > Some(100) && state.right.git_request > Some(100));

    // A reply to a pre-swap request id is stale and dropped.
    let (state2, _) = update(
        state.clone(),
        Command::GitInfoResolved {
            panel: PanelSide::Right,
            path: PathBuf::from("C:\\repo"),
            request: 11,
            info: GitInfo::none(),
            timed_out_root: None,
        },
    );
    assert_eq!(state2, state, "a pre-swap git reply never applies");
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
    let (state, effects) =
        update(state, Command::InfoResolved { panel: PanelSide::Left, path: PathBuf::from("/left"), request, values: values.clone() });
    assert!(effects.is_empty());
    assert_eq!(state.left.info, values);
}

#[test]
fn an_info_result_for_a_directory_the_panel_left_is_discarded() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::ToggleInfoMode(PanelSide::Left));
    let request = state.left.info_request.unwrap();
    let values = InfoValues { file_count: Some(12), ..InfoValues::default() };
    let (state, _) = update(state, Command::InfoResolved { panel: PanelSide::Left, path: PathBuf::from("/elsewhere"), request, values });
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
    let (state, _) =
        update(state, Command::InfoResolved { panel: PanelSide::Left, path: PathBuf::from("/left"), request: first_request, values: stale_values });
    assert_eq!(state.left.info, InfoValues::default(), "the stale (first-request) answer is dropped");

    let fresh_values = InfoValues { file_count: Some(7), ..InfoValues::default() };
    let (state, _) = update(
        state,
        Command::InfoResolved { panel: PanelSide::Left, path: PathBuf::from("/left"), request: second_request, values: fresh_values.clone() },
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
    let (state, _) =
        update(state, Command::TreeNodeExpanded { panel: PanelSide::Right, path: PathBuf::from(r"C:\"), children: vec![dir_child("alpha")] });
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
        without_git_info_effects(effects).contains(&start_listing(&state, PanelSide::Right, r"C:\alpha")),
        "the opposite (right) panel must be re-listed at the newly highlighted directory"
    );
    // Landing on "alpha" (not yet expanded) also requests its children.
    assert!(!state.left.tree.as_ref().unwrap().nodes[1].expanded);
}

#[test]
fn moving_onto_an_unexpanded_node_requests_its_children_but_not_an_already_expanded_one() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::EnterTreeMode(PanelSide::Left));
    let (state, _) =
        update(state, Command::TreeNodeExpanded { panel: PanelSide::Left, path: PathBuf::from(r"C:\"), children: vec![dir_child("alpha")] });
    let (state, effects) = update(state, Command::MoveCursor(CursorMove::Down(1)));
    assert!(effects.contains(&Effect::ExpandTreeNode { panel: PanelSide::Left, path: PathBuf::from(r"C:\alpha") }));

    // Expand it, then move away and back — no second expand request for an
    // already-expanded node.
    let (state, _) = update(state, Command::TreeNodeExpanded { panel: PanelSide::Left, path: PathBuf::from(r"C:\alpha"), children: vec![] });
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
        without_git_info_effects(effects).contains(&start_listing(&state, PanelSide::Left, r"C:\alpha")),
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
    state.left.set_quick_filter(Some("rep".to_string()));
    let (state, _) = update(state, Command::SetDisplayMode { side: PanelSide::Left, mode: DisplayMode::Brief });
    assert_eq!(state.left.display_mode, DisplayMode::Brief);
    assert_eq!(state.left.quick_filter, None, "a stale filter must not linger invisibly into Brief mode");
}

#[test]
fn set_display_mode_does_not_clear_the_opposite_panels_quick_filter() {
    let mut state = test_state(UiPhase::Panels);
    state.right.entries = vec![file_entry("report.txt", 1)];
    state.right.set_quick_filter(Some("rep".to_string()));
    let (state, _) = update(state, Command::SetDisplayMode { side: PanelSide::Left, mode: DisplayMode::Brief });
    assert_eq!(state.right.quick_filter.as_deref(), Some("rep"), "the opposite panel's filter must be untouched");
}

#[test]
fn entering_tree_mode_clears_an_active_quick_filter() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("report.txt", 1)];
    state.left.set_quick_filter(Some("rep".to_string()));
    let (state, _) = update(state, Command::EnterTreeMode(PanelSide::Left));
    assert_eq!(state.left.display_mode, DisplayMode::Tree);
    assert_eq!(state.left.quick_filter, None, "a stale filter must not linger invisibly into Tree mode");
}

#[test]
fn toggling_into_info_mode_clears_an_active_quick_filter() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("report.txt", 1)];
    state.left.set_quick_filter(Some("rep".to_string()));
    let (state, _) = update(state, Command::ToggleInfoMode(PanelSide::Left));
    assert_eq!(state.left.display_mode, DisplayMode::Info);
    assert_eq!(state.left.quick_filter, None, "a stale filter must not linger invisibly into Info mode");
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
    state.left.entries = vec![Entry::new(OsString::from(".."), EntryKind::ParentDir, 0, None, false, false)];
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
fn menu_item_click_on_a_separator_is_a_no_op() {
    // No shipped item is disabled any more (pulldown-menu-completion design
    // D5), so the "unselectable row" click case is pinned via a separator —
    // the disabled-item guard itself is covered by menu.rs's local-fixture
    // tests.
    let mut state = test_state(UiPhase::Panels);
    state.menu = Some(MenuState::for_menu(MenuId::Files));
    let separator_index = crate::menu::entries(MenuId::Files).iter().position(|e| matches!(e, MenuEntry::Separator)).unwrap();
    let (state, effects) = update(state, Command::MenuItemClick(separator_index));
    assert!(state.menu.is_some(), "an unselectable row never closes the bar");
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

/// operation-dialogs "Overwrite-conflict dialog" scenario "The Cancel
/// control and hotkey match Esc" (process-lifecycle-hardening task 4.5): a
/// Cancel answer — by key or by the `(C)ancel` control — is forwarded to
/// the job as `SendConflictReply(Cancel)` and the dialog returns to
/// Progress until `JobDone` arrives, exactly like every other answer.
#[test]
fn conflict_cancel_answer_is_forwarded_and_returns_the_dialog_to_progress() {
    let progress_state = running_progress_state(JobKind::Copy, "/left", "/right");
    let (state, _) = update(progress_state, Command::JobConflict(sample_conflict()));
    assert!(matches!(state.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Conflict { .. }, .. }), "setup precondition");

    let (after_key, effects) = update(state.clone(), Command::FileOpConflictChoice(ConflictChoice::Cancel));
    assert_eq!(effects, vec![Effect::SendConflictReply(ConflictChoice::Cancel)]);
    assert!(
        matches!(after_key.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Progress { .. }, .. }),
        "back to Progress until JobDone: {:?}",
        after_key.phase
    );

    let (after_click, click_effects) = update(state, Command::DialogButtonClick(ButtonId::ConflictCancel));
    assert_eq!(click_effects, effects, "the control routes to the same command as the key");
    assert_eq!(after_click.phase, after_key.phase);
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
            FileActionMenuEntry::Open,
            FileActionMenuEntry::SendToClipboard,
            FileActionMenuEntry::Copy,
            FileActionMenuEntry::Rename,
            FileActionMenuEntry::Move,
            FileActionMenuEntry::Delete,
        ]
    );
    assert_eq!(menu.selected(), FileActionMenuEntry::Open, "Open is highlighted first for a directory target");
}

#[test]
fn open_action_menu_at_on_the_parent_entry_is_a_no_op() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![Entry::new(OsString::from(".."), EntryKind::ParentDir, 0, None, false, false)];
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
// resize-preserves-session-state: shrinking below the 60x16 floor raises
// `State::undersized` and leaves the phase and every overlay intact
// (application-shell "Resize handling with 60x16 hard floor and
// placeholder"; design D1-D7)
// ---------------------------------------------------------------------

/// Task 4.1: shrink to 50x12, then enlarge back to 80x24. The phase and
/// every overlay field — in fact the whole `State` apart from `term_size`
/// and `undersized`, which are restored too — must be equal before and
/// after, with `undersized` set in between. The input is first normalised
/// to 80x24 so the comparison is against a reconciled 80x24 state.
fn assert_survives_shrink_and_enlarge(state: State) -> State {
    let (state, _) = update(state, Command::Resize(80, 24));
    let before = state.clone();

    let (shrunk, effects) = update(state, Command::Resize(50, 12));
    assert!(effects.is_empty(), "a resize emits no effects");
    assert!(shrunk.undersized, "50x12 is below the floor");
    assert_eq!(shrunk.term_size, (50, 12));
    assert_eq!(shrunk.phase, before.phase, "the shrink must not touch the phase");
    assert_eq!(shrunk.menu, before.menu);
    assert_eq!(shrunk.drive_select, before.drive_select);
    assert_eq!(shrunk.fuzzy_jump, before.fuzzy_jump);
    assert_eq!(shrunk.find_file, before.find_file);
    assert_eq!(shrunk.user_menu, before.user_menu);
    assert_eq!(shrunk.history_dialog, before.history_dialog);
    assert_eq!(shrunk.theme_picker, before.theme_picker);
    assert_eq!(shrunk.file_action_menu, before.file_action_menu);
    assert_eq!(shrunk.help, before.help);
    assert_eq!(shrunk.quick_search, before.quick_search);
    assert_eq!(shrunk.command_line, before.command_line);
    assert_eq!(shrunk.left.quick_filter, before.left.quick_filter);

    let (restored, effects) = update(shrunk, Command::Resize(80, 24));
    assert!(effects.is_empty());
    assert!(!restored.undersized);
    assert_eq!(restored, before, "enlarging must restore the state exactly as it was");
    restored
}

fn twenty_line_editor() -> EditorState {
    let text: String = (1..=20).map(|n| format!("line {n}\n")).collect();
    EditorState::from_bytes(PathBuf::from("/left/notes.txt"), text.as_bytes()).expect("valid UTF-8 fixture")
}

/// Task 4.2: the editor with a modified buffer, the caret on line 12 and
/// overwrite mode (application-shell "Unsaved editor buffer survives a
/// shrink").
#[test]
fn undersized_preserves_a_modified_editor_buffer_caret_and_mode() {
    let mut state = editor_state(twenty_line_editor());
    state.term_size = (80, 24);
    let (state, _) = update(state, Command::EditorChar('x'));
    let mut state = state;
    for _ in 0..12 {
        let (s, _) = update(state, Command::EditorMove(EditorMove::Down));
        state = s;
    }
    let (state, _) = update(state, Command::EditorToggleMode);
    let UiPhase::Editor(before) = &state.phase else { panic!("expected the editor") };
    assert!(before.is_modified());
    assert_eq!(before.caret.line, 12);
    assert_eq!(before.mode, crate::editor::EntryMode::Overwrite);
    let before = before.clone();

    let restored = assert_survives_shrink_and_enlarge(state);
    let UiPhase::Editor(after) = &restored.phase else { panic!("expected the editor") };
    assert_eq!(*after, before);
    assert!(after.is_modified(), "the modified indicator survives");
    assert_eq!(after.caret.line, 12);
    assert_eq!(after.mode, crate::editor::EntryMode::Overwrite);
    assert!(after.lines[0].starts_with('x'), "the unsaved edit is still in the buffer");
}

/// Task 4.2: the viewer in hex mode at a non-zero offset (application-shell
/// "Viewer position and mode survive a shrink").
#[test]
fn undersized_preserves_the_viewer_mode_and_offset() {
    let mut viewer = ViewerState::new(PathBuf::from("/left/big.bin"), 1 << 20);
    viewer.mode = crate::viewer::ViewMode::Hex;
    viewer.top_offset = 0x4000;
    let state = test_state(UiPhase::Viewer(viewer.clone()));
    let restored = assert_survives_shrink_and_enlarge(state);
    assert_eq!(restored.phase, UiPhase::Viewer(viewer));
}

/// Task 4.2: the F5 destination dialog with typed text (application-shell
/// "Open dialog and typed text survive a shrink").
#[test]
fn undersized_preserves_the_destination_dialog_and_its_text() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::RequestCopy);
    let mut state = state;
    for c in r"D:\backup".chars() {
        let (s, _) = update(state, Command::TextEdit(TextEditOp::Insert(c)));
        state = s;
    }
    // The dialog opens pre-filled with the other panel's directory; the
    // typed text lands after it.
    let typed = match &state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { input, .. }) => {
            assert!(input.text().ends_with(r"D:\backup"), "typed text landed: {:?}", input.text());
            input.clone()
        }
        other => panic!("expected the destination dialog, got {other:?}"),
    };
    let restored = assert_survives_shrink_and_enlarge(state);
    match &restored.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { input, .. }) => assert_eq!(*input, typed, "text and caret are where they were"),
        other => panic!("expected the destination dialog, got {other:?}"),
    }
}

/// Task 4.2: the F8 delete confirmation.
#[test]
fn undersized_preserves_the_delete_confirmation() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::RequestDelete);
    assert!(matches!(state.phase, UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { .. })));
    let restored = assert_survives_shrink_and_enlarge(state);
    assert!(matches!(restored.phase, UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { .. })));
}

/// Task 4.2: a running job with each of its dialogs (application-shell
/// "Running job keeps reporting progress while undersized").
#[test]
fn undersized_preserves_a_running_job_and_each_of_its_dialogs() {
    // Progress.
    let progress = running_progress_state(JobKind::Copy, "/left", "/right");
    let restored = assert_survives_shrink_and_enlarge(progress.clone());
    assert!(matches!(restored.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Progress { .. }, .. }));

    // Conflict, without a rename in progress.
    let (conflict, _) = update(progress.clone(), Command::JobConflict(sample_conflict()));
    assert!(matches!(conflict.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Conflict { rename_input: None, .. }, .. }));
    let restored = assert_survives_shrink_and_enlarge(conflict.clone());
    assert!(matches!(restored.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Conflict { rename_input: None, .. }, .. }));

    // Conflict, with a rename being typed.
    let (renaming, _) = update(conflict, Command::FileOpBeginRename);
    let (renaming, _) = update(renaming, Command::TextEdit(TextEditOp::Insert('b')));
    assert!(matches!(renaming.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Conflict { rename_input: Some(_), .. }, .. }));
    let restored = assert_survives_shrink_and_enlarge(renaming);
    match &restored.phase {
        UiPhase::FileOpRunning { dialog: RunningDialog::Conflict { rename_input: Some(input), .. }, .. } => assert_eq!(input.text(), "b"),
        other => panic!("expected the conflict dialog mid-rename, got {other:?}"),
    }

    // Error.
    let info = ErrorInfo { path: PathBuf::from("/left/a.txt"), message: "permission denied".to_string() };
    let (error, _) = update(progress, Command::JobError(info));
    assert!(matches!(error.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Error { .. }, .. }));
    let restored = assert_survives_shrink_and_enlarge(error);
    assert!(matches!(restored.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Error { .. }, .. }));
}

/// Task 4.2: the end-of-job summary.
#[test]
fn undersized_preserves_the_end_of_job_summary() {
    let skipped = vec![SkippedItem::skipped(PathBuf::from("/left/a.txt"), "exists".to_string())];
    let state = test_state(UiPhase::FileOpSummary(skipped.clone()));
    let restored = assert_survives_shrink_and_enlarge(state);
    assert_eq!(restored.phase, UiPhase::FileOpSummary(skipped));
}

/// One way of opening an overlay beside the `Panels` phase, for task 4.3.
type OpenOverlay = Box<dyn Fn(&mut State)>;

/// Task 4.3: every overlay that lives beside the `Panels` phase.
#[test]
fn undersized_preserves_every_overlay_over_the_panels() {
    let overlays: Vec<(&str, OpenOverlay)> = vec![
        ("F9 menu", Box::new(|s| s.menu = Some(MenuState::opened()))),
        ("drive select", Box::new(|s| s.drive_select = Some(DriveSelect::new(PanelSide::Left, vec!['C', 'D'], Some('C'))))),
        ("fuzzy jump", Box::new(|s| s.fuzzy_jump = Some(FuzzyJumpState::new()))),
        ("find file", Box::new(|s| s.find_file = Some(FindFileState::new(PathBuf::from("/left"))))),
        ("user menu", Box::new(|s| s.user_menu = Some(UserMenuState::new()))),
        ("history dialog", Box::new(|s| s.history_dialog = Some(HistoryDialogState::new()))),
        ("theme picker", Box::new(|s| s.theme_picker = Some(ThemePickerState::open("nc-classic")))),
        ("file-action menu", Box::new(|s| s.file_action_menu = Some(FileActionMenuState::new(OsString::from("a.txt"), false)))),
        ("help", Box::new(|s| s.help = Some(HelpState::new()))),
        ("quick filter", Box::new(|s| s.left.set_quick_filter(Some("rep".to_string())))),
        ("type-ahead", Box::new(|s| s.quick_search = Some("no".to_string()))),
        ("command-line text", Box::new(|s| s.command_line = LineEdit::from("dir /w"))),
    ];
    for (name, open) in overlays {
        let mut state = test_state(UiPhase::Panels);
        state.left.entries = vec![file_entry("a.txt", 1), file_entry("notes.txt", 2)];
        open(&mut state);
        let restored = assert_survives_shrink_and_enlarge(state.clone());
        assert_eq!(restored.phase, UiPhase::Panels, "{name}");
    }
}

/// Task 4.4: the splash is the one exception — it is dismissed by the shrink
/// and does not return (startup-splash "Shrinking below the floor
/// mid-splash", "Below the floor at startup").
#[test]
fn undersized_dismisses_the_splash_for_good() {
    let state = test_state(UiPhase::Splash { started_at_ms: 0 });
    let (state, _) = update(state, Command::Resize(50, 12));
    assert!(state.undersized);
    assert_eq!(state.phase, UiPhase::Panels);
    let (state, _) = update(state, Command::Resize(80, 24));
    assert!(!state.undersized);
    assert_eq!(state.phase, UiPhase::Panels, "enlarging reveals the panels, never the splash");

    let (state, _) = State::initial(Theme::classic(), (50, 12), 0, 0, PathBuf::from("/l"), PathBuf::from("/r"), true);
    assert!(state.undersized);
    assert_eq!(state.phase, UiPhase::Panels, "a start below the floor skips the splash");
}

fn shrunk(state: State) -> State {
    let (state, _) = update(state, Command::Resize(50, 12));
    assert!(state.undersized);
    state
}

/// Task 4.5: job replies keep being applied to the hidden running dialog
/// (application-shell "Running job keeps reporting progress while
/// undersized", "Conflict raised while undersized waits for an answer",
/// "Job finishing while undersized still refreshes the panels").
#[test]
fn undersized_still_applies_job_progress_conflict_and_error() {
    let state = shrunk(running_progress_state(JobKind::Copy, "/left", "/right"));

    let info = ProgressInfo { files_done: 2, files_total: 3, bytes_done: 20, bytes_total: 30, current_file: OsString::from("b.txt") };
    let (state, effects) = update(state, Command::JobProgress(info.clone()));
    assert!(effects.is_empty());
    assert!(state.undersized);
    match &state.phase {
        UiPhase::FileOpRunning { dialog: RunningDialog::Progress { progress, .. }, .. } => assert_eq!(*progress, info),
        other => panic!("expected the hidden progress dialog to update, got {other:?}"),
    }

    let (conflict, effects) = update(state.clone(), Command::JobConflict(sample_conflict()));
    assert!(effects.is_empty(), "no default answer is sent: the job waits for the user");
    assert!(conflict.undersized);
    assert!(matches!(conflict.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Conflict { .. }, .. }));
    // Enlarging shows the conflict dialog, and answering it resumes the job.
    let (conflict, _) = update(conflict, Command::Resize(80, 24));
    assert!(matches!(conflict.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Conflict { .. }, .. }));
    let (_, effects) = update(conflict, Command::FileOpConflictChoice(ConflictChoice::Overwrite));
    assert_eq!(effects, vec![Effect::SendConflictReply(ConflictChoice::Overwrite)]);

    let info = ErrorInfo { path: PathBuf::from("/left/a.txt"), message: "permission denied".to_string() };
    let (error, effects) = update(state, Command::JobError(info));
    assert!(effects.is_empty());
    assert!(error.undersized);
    assert!(matches!(error.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Error { .. }, .. }));
}

#[test]
fn undersized_job_done_still_rereads_the_panels() {
    let state = shrunk(running_progress_state(JobKind::Copy, "/left", "/right"));
    let (state, effects) = update(
        state,
        Command::JobDone {
            outcome: JobOutcome::Completed { skipped: vec![] },
            source_dir: PathBuf::from("/left"),
            dest_dir: PathBuf::from("/right"),
        },
    );
    assert!(state.undersized, "the placeholder stays up");
    assert_eq!(state.phase, UiPhase::Panels);
    assert_eq!(
        without_git_info_effects(effects),
        vec![start_listing(&state, PanelSide::Left, "/left"), start_listing(&state, PanelSide::Right, "/right"),]
    );

    let state = shrunk(running_progress_state(JobKind::Copy, "/left", "/right"));
    let skipped = vec![SkippedItem::skipped(PathBuf::from("/left/a.txt"), "exists".to_string())];
    let (state, effects) = update(
        state,
        Command::JobDone {
            outcome: JobOutcome::Completed { skipped: skipped.clone() },
            source_dir: PathBuf::from("/left"),
            dest_dir: PathBuf::from("/right"),
        },
    );
    assert!(state.undersized);
    assert_eq!(state.phase, UiPhase::FileOpSummary(skipped), "the summary waits under the placeholder");
    assert!(without_git_info_effects(effects).iter().all(|e| matches!(e, Effect::StartListing { .. })));
}

/// Task 4.6: replies to requests made before the shrink are applied to the
/// hidden screen.
#[test]
fn undersized_still_applies_editor_saved_viewer_opened_and_editor_opened() {
    // EditorSaved: the saved buffer replaces the hidden one, and the caret
    // is not re-scrolled against the sub-floor viewport (design D6).
    let mut state = editor_state(twenty_line_editor());
    state.term_size = (80, 24);
    let mut state = state;
    for _ in 0..12 {
        let (s, _) = update(state, Command::EditorMove(EditorMove::Down));
        state = s;
    }
    let (state, _) = update(state, Command::EditorChar('x'));
    let state = shrunk(state);
    let mut saved = twenty_line_editor();
    for _ in 0..12 {
        saved.move_down();
    }
    let (state, effects) = update(state, Command::EditorSaved { editor: Box::new(saved), then_quit: false });
    assert!(effects.is_empty());
    assert!(state.undersized);
    match &state.phase {
        UiPhase::Editor(editor) => {
            assert!(!editor.is_modified(), "the saved buffer replaced the hidden one");
            assert_eq!(editor.caret.line, 12);
            assert_eq!(editor.top_line, 0, "no re-scroll against a 10-row window while undersized");
        }
        other => panic!("expected the hidden editor, got {other:?}"),
    }
    // A failed save likewise lands on the hidden editor without a re-scroll.
    let (state, _) = update(state, Command::EditorSaveFailed { message: "disk full".to_string() });
    match &state.phase {
        UiPhase::Editor(editor) => {
            assert_eq!(editor.save_error.as_deref(), Some("disk full"));
            assert_eq!(editor.top_line, 0);
        }
        other => panic!("expected the hidden editor, got {other:?}"),
    }

    // ViewerOpened over hidden panels: being undersized is not "leaving the
    // panels", so the reply is still current (offload-blocking-io D4).
    let state = with_pending_open(shrunk(test_state(UiPhase::Panels)), 3);
    let (state, _) = update(state, Command::ViewerOpened { request: 3, path: PathBuf::from("/left/a.txt"), file_len: 10 });
    assert!(state.undersized);
    assert!(matches!(state.phase, UiPhase::Viewer(_)), "the viewer opens under the placeholder");

    // EditorOpened over hidden panels.
    let state = with_pending_open(shrunk(test_state(UiPhase::Panels)), 3);
    let (state, _) = update(state, Command::EditorOpened { request: 3, editor: Box::new(twenty_line_editor()) });
    assert!(state.undersized);
    assert!(matches!(state.phase, UiPhase::Editor(_)), "the editor opens under the placeholder");
}

/// Design D6: the first resize back above the floor re-scrolls a hidden
/// editor so its caret is visible at the restored size; the caret itself
/// never moves.
#[test]
fn enlarging_rescrolls_the_hidden_editor_to_its_caret() {
    let text: String = (1..=40).map(|n| format!("line {n}\n")).collect();
    let mut editor = EditorState::from_bytes(PathBuf::from("/left/notes.txt"), text.as_bytes()).expect("valid UTF-8 fixture");
    for _ in 0..30 {
        editor.move_down();
    }
    assert_eq!((editor.caret.line, editor.top_line), (30, 0), "built unreconciled on purpose");
    let mut state = editor_state(editor);
    state.term_size = (80, 24);
    let state = shrunk(state);
    match &state.phase {
        UiPhase::Editor(editor) => assert_eq!(editor.top_line, 0, "the shrink touches nothing"),
        other => panic!("expected the editor, got {other:?}"),
    }
    let (state, _) = update(state, Command::Resize(80, 24));
    match &state.phase {
        UiPhase::Editor(editor) => {
            assert_eq!(editor.caret.line, 30, "the caret never moves");
            assert!(editor.top_line > 0 && editor.top_line <= 30, "the caret is scrolled into the 22-row body, got top_line {}", editor.top_line);
            assert!(editor.caret.line < editor.top_line + 22);
        }
        other => panic!("expected the editor, got {other:?}"),
    }
}

/// Task 4.7: quitting from the placeholder over a running job cancels the
/// job first, as it does at full size (design D4). Before this change the
/// placeholder phase had replaced `FileOpRunning`, so only `Quit` was sent.
#[test]
fn undersized_confirm_quit_over_a_running_job_cancels_it() {
    let state = shrunk(running_progress_state(JobKind::Copy, "/left", "/right"));
    let (state, _) = update(state, Command::RequestQuit);
    assert!(state.quit_confirm);
    let (_, effects) = update(state, Command::ConfirmQuit);
    assert_eq!(effects, vec![Effect::CancelJob, Effect::Quit]);
}

/// Task 4.8 is `drag_clears_on_resize_below_the_minimum` above; this pins
/// the other half of design D6 — the drag cannot be started under the
/// placeholder either.
#[test]
fn undersized_refuses_a_new_drag() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let state = shrunk(state);
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy });
    assert!(state.drag.is_none());
}
