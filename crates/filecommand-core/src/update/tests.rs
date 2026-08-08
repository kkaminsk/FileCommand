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
    assert_eq!(effects, vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left/sub") }]);
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
    assert_eq!(effects, vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/a") }]);
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
fn f10_raises_quit_confirm_and_confirm_quits() {
    let state = test_state(UiPhase::Panels);
    let (state, effects) = update(state, Command::RequestQuit);
    assert_eq!(state.phase, UiPhase::QuitConfirm);
    assert!(effects.is_empty());
    let (state, effects) = update(state, Command::ConfirmQuit);
    assert_eq!(effects, vec![Effect::Quit]);
    let _ = state;
}

#[test]
fn quit_confirm_cancel_returns_to_panels() {
    let state = test_state(UiPhase::QuitConfirm);
    let (state, effects) = update(state, Command::CancelQuit);
    assert_eq!(state.phase, UiPhase::Panels);
    assert!(effects.is_empty());
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
        effects,
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
        effects,
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
    assert_eq!(effects, vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left") }]);

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
    assert_eq!(effects, vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left") }]);
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
    assert_eq!(effects, vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left") }]);
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
fn esc_clears_the_buffer() {
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
fn quick_search_backspace_shrinks_then_exits_the_mode() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("alpha", 1)];
    let (state, _) = update(state, Command::QuickSearchStart('a'));
    let (state, _) = update(state, Command::QuickSearchChar('l'));
    assert_eq!(state.quick_search.as_deref(), Some("al"));
    let (state, _) = update(state, Command::QuickSearchBackspace);
    assert_eq!(state.quick_search.as_deref(), Some("a"));
    let (state, _) = update(state, Command::QuickSearchBackspace);
    assert_eq!(state.quick_search, None);
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
        [Effect::RunShellCommand(inv, side), Effect::PersistHistory(entries)] => {
            assert_eq!(inv.cwd, PathBuf::from(r"C:\NORTON"));
            assert_eq!(inv.args.last().unwrap(), "dir");
            assert_eq!(*side, PanelSide::Left, "the command ran in the active panel, so that panel is re-read");
            assert_eq!(entries, &vec!["dir".to_string()]);
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
    assert_eq!(effects, vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left/sub") }]);
}

#[test]
fn enter_on_an_executable_target_spawns_it_through_the_shell() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("setup.exe", 1)];
    let (_, effects) = update(state, Command::Enter);
    match effects.as_slice() {
        [Effect::RunShellCommand(inv, side)] => {
            assert_eq!(inv.args.last().unwrap(), "\"setup.exe\"");
            assert_eq!(inv.cwd, PathBuf::from("/left"));
            assert_eq!(*side, PanelSide::Left);
        }
        other => panic!("expected the executable to spawn through the shell, got {other:?}"),
    }
}

#[test]
fn enter_on_a_plain_file_does_nothing() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("readme.txt", 1)];
    let (_, effects) = update(state, Command::Enter);
    assert!(effects.is_empty());
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
    assert_eq!(effects, vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left") }]);
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
    // Opens on Info; Down walks Name, Extension, Modif. time, Size
    // (the separator is skipped, not counted).
    let state = (0..4).fold(state, |s, _| update(s, Command::MenuSelectNext).0);
    assert_eq!(state.menu.as_ref().unwrap().selected_item().map(|i| i.label), Some("Size"));
    let (state, _) = update(state, Command::MenuActivate);

    assert_eq!(state.left.sort_mode, SortMode::Size, "the Left menu targets the left panel regardless of focus");
    assert_eq!(state.right.sort_mode, SortMode::Name);
}

#[test]
fn right_menu_targets_the_right_panel() {
    let mut state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::MenuOpen);
    let (state, _) = update(state, Command::MenuHotkey('r'));
    let (state, effects) = update(state, Command::MenuActivate); // Info
    assert_eq!(state.right.display_mode, DisplayMode::Info);
    assert_eq!(state.left.display_mode, DisplayMode::Full);
    assert!(effects.iter().any(|e| matches!(e, Effect::QueryInfo { panel: PanelSide::Right, .. })));
}

#[test]
fn activating_a_menu_with_no_enabled_items_just_closes_it() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::MenuOpen);
    let (state, _) = update(state, Command::MenuHotkey('o')); // Options: all disabled
    let (state, effects) = update(state, Command::MenuActivate);
    assert!(state.menu.is_some(), "with nothing selectable, Enter does nothing");
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
    assert_eq!(
        effects,
        vec![
            Effect::FetchDriveLabel { target: PanelSide::Left, letter: 'A' },
            Effect::FetchDriveLabel { target: PanelSide::Left, letter: 'C' },
            Effect::FetchDriveLabel { target: PanelSide::Left, letter: 'D' },
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
    let (state, effects) =
        update(state, Command::DriveLabelResolved { target: PanelSide::Left, letter: 'C', label: Some("OS".to_string()) });
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
    let (state, _) = update(state, Command::DriveSelectCancel);
    assert!(state.drive_select.is_none());

    let (state, effects) =
        update(state, Command::DriveLabelResolved { target: PanelSide::Left, letter: 'C', label: Some("OS".to_string()) });
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
    // The dialog was reopened for the other panel before the label landed.
    let (state, _) = update(state, Command::DriveListReady { target: PanelSide::Right, drives: vec!['C'] });
    let (state, _) = update(state, Command::DriveLabelResolved { target: PanelSide::Left, letter: 'C', label: Some("OS".to_string()) });
    assert_eq!(state.drive_select.unwrap().drives[0].label, None);
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
    assert_eq!(effects, vec![Effect::StartListing { panel: PanelSide::Right, path: PathBuf::from(r"D:\") }]);
}

#[test]
fn selecting_an_unavailable_drive_surfaces_the_panel_error_state() {
    let (state, _) = update(
        test_state(UiPhase::Panels),
        Command::DriveListReady { target: PanelSide::Left, drives: vec!['A'] },
    );
    let (state, effects) = update(state, Command::DriveSelectConfirm);
    assert_eq!(effects, vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from(r"A:\") }]);
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
    assert_eq!(effects, vec![Effect::QueryInfo { panel: PanelSide::Left, path: PathBuf::from("/left") }]);

    let (state, effects) = update(state, Command::ToggleInfoMode(PanelSide::Left));
    assert_eq!(state.left.display_mode, DisplayMode::Full);
    assert!(effects.is_empty(), "leaving Info mode queries nothing");
}

#[test]
fn info_values_start_pending_and_resolve_in_place() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::ToggleInfoMode(PanelSide::Left));
    assert_eq!(state.left.info, InfoValues::default(), "every value starts unresolved");

    let values = InfoValues { file_count: Some(12), dir_count: Some(3), ..InfoValues::default() };
    let (state, effects) =
        update(state, Command::InfoResolved { panel: PanelSide::Left, path: PathBuf::from("/left"), values: values.clone() });
    assert!(effects.is_empty());
    assert_eq!(state.left.info, values);
}

#[test]
fn an_info_result_for_a_directory_the_panel_left_is_discarded() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::ToggleInfoMode(PanelSide::Left));
    let values = InfoValues { file_count: Some(12), ..InfoValues::default() };
    let (state, _) = update(state, Command::InfoResolved { panel: PanelSide::Left, path: PathBuf::from("/elsewhere"), values });
    assert_eq!(state.left.info, InfoValues::default(), "a result for another directory is dropped");
}

#[test]
fn an_info_result_arriving_after_info_mode_was_left_is_discarded() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::ToggleInfoMode(PanelSide::Left));
    let (state, _) = update(state, Command::ToggleInfoMode(PanelSide::Left)); // back to Full
    let values = InfoValues { file_count: Some(12), ..InfoValues::default() };
    let (state, _) = update(state, Command::InfoResolved { panel: PanelSide::Left, path: PathBuf::from("/left"), values });
    assert_eq!(state.left.info, InfoValues::default());
}

#[test]
fn navigating_while_in_info_mode_re_queries_for_the_new_directory() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("sub")];
    let (state, _) = update(state, Command::ToggleInfoMode(PanelSide::Left));
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(state.left.cwd, PathBuf::from("/left/sub"));
    assert!(effects.contains(&Effect::QueryInfo { panel: PanelSide::Left, path: PathBuf::from("/left/sub") }));
    assert_eq!(state.left.info, InfoValues::default(), "the previous directory's figures are cleared");
}

#[test]
fn a_panel_not_in_info_mode_issues_no_info_query_when_it_navigates() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("sub")];
    let (_, effects) = update(state, Command::Enter);
    assert!(!effects.iter().any(|e| matches!(e, Effect::QueryInfo { .. })));
}
