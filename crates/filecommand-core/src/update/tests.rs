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
fn quick_search_backspace_shrinks_and_stays_active_when_emptied() {
    // type-ahead-jump "Backspace on a single-character pattern": the
    // pattern becomes empty but type-ahead mode itself stays active (only
    // Esc or a movement key exits it) and the cursor holds its position
    // rather than re-jumping against an empty pattern.
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
    assert_eq!(state.quick_search, None, "Esc is still the way to actually exit type-ahead");
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
