use super::*;

// ---------------------------------------------------------------------
// relative-destination-resolution: the F5/F6/F7 destination resolves
// against the source panel's directory (operation-dialogs "Relative
// destinations resolve against the source panel directory"). The drive
// forms are Windows-only `Path` semantics, so these tests are too.
// ---------------------------------------------------------------------

/// Backspace the destination/name field's current text away and type
/// `replacement` — the same edits a user retyping the field makes.
#[cfg(windows)]
fn replace_destination_text(state: State, replacement: &str) -> State {
    let len = match &state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { input, .. }) => input.len_chars(),
        other => panic!("expected the destination dialog, got {other:?}"),
    };
    let state = (0..len).fold(state, |s, _| update(s, Command::TextEdit(TextEditOp::Backspace)).0);
    replacement.chars().fold(state, |s, c| update(s, Command::TextEdit(TextEditOp::Insert(c))).0)
}

/// Drive F6 from a left panel at `source_cwd` holding one file, retype the
/// destination field as `text`, and confirm with Enter.
#[cfg(windows)]
fn confirm_move_with_destination(source_cwd: &str, text: &str) -> (State, Vec<Effect>) {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(source_cwd);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::RequestMove);
    let state = replace_destination_text(state, text);
    update(state, Command::FileOpConfirm)
}

/// Scenario "A relative name lands inside the source panel's directory".
#[cfg(windows)]
#[test]
fn a_relative_destination_lands_inside_the_source_panels_directory() {
    let (_, effects) = confirm_move_with_destination(r"C:\work", "backup");
    match effects.as_slice() {
        [Effect::RunJob(job)] => {
            assert_eq!(job.kind, JobKind::Move);
            assert_eq!(job.dest_dir, PathBuf::from(r"C:\work\backup"), "relative to the source panel, not the launch directory");
        }
        other => panic!("expected exactly one RunJob, got {other:?}"),
    }
}

/// Scenario "A parent-relative path is resolved and normalized": the job
/// carries the normalized path, so the completion re-read finds the panel
/// showing it (`panels_matching` compares by exact `PathBuf` equality).
#[cfg(windows)]
#[test]
fn a_parent_relative_destination_is_resolved_and_normalized() {
    let (state, effects) = confirm_move_with_destination(r"C:\work\proj", r"..\archive");
    match effects.as_slice() {
        [Effect::RunJob(job)] => assert_eq!(job.dest_dir, PathBuf::from(r"C:\work\archive")),
        other => panic!("expected exactly one RunJob, got {other:?}"),
    }
    let mut state = state;
    state.right.cwd = PathBuf::from(r"C:\work\archive");
    let (state, effects) = update(
        state,
        Command::JobDone {
            outcome: JobOutcome::Completed { skipped: vec![] },
            source_dir: PathBuf::from(r"C:\work\proj"),
            dest_dir: PathBuf::from(r"C:\work\archive"),
        },
    );
    assert!(
        effects.contains(&start_listing(&state, PanelSide::Right, r"C:\work\archive")),
        "the panel showing the resolved destination re-reads on completion, got {effects:?}"
    );
}

/// Scenarios "A bare drive letter means that drive's root" and "A
/// drive-rooted path keeps the source panel's drive".
#[cfg(windows)]
#[test]
fn bare_drive_letter_and_drive_rooted_destinations_resolve() {
    let (_, effects) = confirm_move_with_destination(r"C:\work", "D:");
    match effects.as_slice() {
        [Effect::RunJob(job)] => assert_eq!(job.dest_dir, PathBuf::from(r"D:\")),
        other => panic!("expected exactly one RunJob, got {other:?}"),
    }
    let (_, effects) = confirm_move_with_destination(r"C:\work", r"\archive");
    match effects.as_slice() {
        [Effect::RunJob(job)] => assert_eq!(job.dest_dir, PathBuf::from(r"C:\archive")),
        other => panic!("expected exactly one RunJob, got {other:?}"),
    }
}

/// Scenario "A drive-relative path resolves against the source panel or
/// the drive root".
#[cfg(windows)]
#[test]
fn a_drive_relative_destination_resolves_against_the_source_panel_or_the_drive_root() {
    let (_, effects) = confirm_move_with_destination(r"D:\work", "D:old");
    match effects.as_slice() {
        [Effect::RunJob(job)] => assert_eq!(job.dest_dir, PathBuf::from(r"D:\work\old")),
        other => panic!("expected exactly one RunJob, got {other:?}"),
    }
    let (_, effects) = confirm_move_with_destination(r"C:\work", "D:old");
    match effects.as_slice() {
        [Effect::RunJob(job)] => assert_eq!(job.dest_dir, PathBuf::from(r"D:\old")),
        other => panic!("expected exactly one RunJob, got {other:?}"),
    }
}

/// Scenario "UNC and absolute destinations are used unchanged".
#[cfg(windows)]
#[test]
fn unc_and_absolute_destinations_are_used_unchanged() {
    let (_, effects) = confirm_move_with_destination(r"C:\work", r"\\server\share\dest");
    match effects.as_slice() {
        [Effect::RunJob(job)] => assert_eq!(job.dest_dir, PathBuf::from(r"\\server\share\dest")),
        other => panic!("expected exactly one RunJob, got {other:?}"),
    }
    let (_, effects) = confirm_move_with_destination(r"C:\work", r"E:\backup");
    match effects.as_slice() {
        [Effect::RunJob(job)] => assert_eq!(job.dest_dir, PathBuf::from(r"E:\backup")),
        other => panic!("expected exactly one RunJob, got {other:?}"),
    }
}

/// Scenario "An edited drop destination resolves against the drag's source
/// panel": the drag begins on the *inactive* right panel, so `sub` is
/// relative to `D:\src`, not to the panel it was dropped on.
#[cfg(windows)]
#[test]
fn an_edited_drop_destination_resolves_against_the_drags_source_panel() {
    let mut state = test_state(UiPhase::Panels);
    state.right.cwd = PathBuf::from(r"D:\src");
    state.right.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Right, name: OsString::from("a.txt"), op: JobKind::Copy });
    let (state, _) = update(state, Command::DragOver { op: JobKind::Copy, target: Some(DropTarget::PanelDir(PanelSide::Left)) });
    let (state, _) = update(state, Command::DragDrop { op: JobKind::Copy });
    assert!(
        matches!(state.phase, UiPhase::FileOpSetup(FileOpSetup::DestinationInput { buttons: Some(_), .. })),
        "setup precondition: the drop dialog opened"
    );
    let state = replace_destination_text(state, "sub");
    let (_, effects) = update(state, Command::FileOpConfirmAs(JobKind::Copy));
    match effects.as_slice() {
        [Effect::RunJob(job)] => {
            assert_eq!(job.kind, JobKind::Copy);
            assert_eq!(job.dest_dir, PathBuf::from(r"D:\src\sub"), "resolved against the panel the drag started from");
        }
        other => panic!("expected exactly one RunJob, got {other:?}"),
    }
}

/// Scenario "A make-directory name with a parent path creates it where it
/// points": the job carries the resolved parent and the bare name, so a
/// panel showing that parent re-reads on completion.
#[cfg(windows)]
#[test]
fn a_make_directory_name_with_a_parent_path_creates_it_where_it_points() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\work\proj");
    state.right.cwd = PathBuf::from(r"C:\work");
    let (state, _) = update(state, Command::RequestMkdir);
    let state = replace_destination_text(state, r"..\shared");
    let (state, effects) = update(state, Command::FileOpConfirm);
    match effects.as_slice() {
        [Effect::RunJob(job)] => {
            assert_eq!(job.kind, JobKind::Mkdir);
            assert_eq!(job.dest_dir, PathBuf::from(r"C:\work"));
            assert_eq!(job.new_dir_name, Some(OsString::from("shared")));
        }
        other => panic!("expected exactly one RunJob, got {other:?}"),
    }
    let (state, effects) = update(
        state,
        Command::JobDone {
            outcome: JobOutcome::Completed { skipped: vec![] },
            source_dir: PathBuf::from(r"C:\work\proj"),
            dest_dir: PathBuf::from(r"C:\work"),
        },
    );
    assert!(
        effects.contains(&start_listing(&state, PanelSide::Right, r"C:\work")),
        "a panel showing the new directory's parent re-reads on completion, got {effects:?}"
    );
}

/// A plain F7 name builds exactly the job F7 has always built: the panel's
/// own directory as `dest_dir`, the bare name as `new_dir_name` (design D3).
#[cfg(windows)]
#[test]
fn a_plain_make_directory_name_builds_the_unchanged_job() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\work");
    let (state, _) = update(state, Command::RequestMkdir);
    let state = replace_destination_text(state, "sub");
    let (_, effects) = update(state, Command::FileOpConfirm);
    match effects.as_slice() {
        [Effect::RunJob(job)] => assert_eq!(
            job,
            &Job {
                kind: JobKind::Mkdir,
                sources: vec![],
                source_dir: PathBuf::from(r"C:\work"),
                dest_dir: PathBuf::from(r"C:\work"),
                new_dir_name: Some(OsString::from("sub")),
            },
            "a plain name yields exactly today's job"
        ),
        other => panic!("expected exactly one RunJob, got {other:?}"),
    }
}

/// Design D3: a make-directory name that resolves to something with no
/// final component (a drive root) names nothing to create — the dialog
/// stays open, exactly as for an empty field.
#[cfg(windows)]
#[test]
fn a_make_directory_name_resolving_to_a_drive_root_keeps_the_dialog_open() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\work");
    let (state, _) = update(state, Command::RequestMkdir);
    let state = replace_destination_text(state, "D:");
    let (state, effects) = update(state, Command::FileOpConfirm);
    assert!(effects.is_empty(), "no operation starts, got {effects:?}");
    assert!(matches!(state.phase, UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind: JobKind::Mkdir, .. })));
}

/// Scenario "An unresolvable destination keeps the dialog open": `..` at a
/// drive root resolves to nothing, so the dialog stays open with the text
/// intact and no operation starts.
#[cfg(windows)]
#[test]
fn an_unresolvable_destination_keeps_the_dialog_open() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\");
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::RequestMove);
    let state = replace_destination_text(state, "..");
    let (state, effects) = update(state, Command::FileOpConfirm);
    assert!(effects.is_empty(), "no operation starts, got {effects:?}");
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { ref input, .. }) => {
            assert_eq!(input.text(), "..", "the field keeps the entered text");
        }
        other => panic!("expected the dialog to stay open, got {other:?}"),
    }
}

#[test]
fn delete_confirm_requires_second_confirmation_for_a_directory() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("sub")];
    let (state, effects) = update(state, Command::RequestDelete);
    assert!(effects.is_empty());
    assert!(matches!(state.phase, UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { needs_second_confirm: true, confirmed_once: false, .. })));

    let (state, effects) = update(state, Command::FileOpConfirm);
    assert!(effects.is_empty(), "first confirm just arms the second confirmation");
    assert!(matches!(state.phase, UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { needs_second_confirm: true, confirmed_once: true, .. })));

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
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Insert('b')));
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Insert('.')));
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Insert('c')));
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
        Command::JobDone {
            outcome: JobOutcome::Completed { skipped: vec![] },
            source_dir: PathBuf::from("/left"),
            dest_dir: PathBuf::from("/right"),
        },
    );
    assert_eq!(state.phase, UiPhase::Panels);
    assert_eq!(
        without_git_info_effects(effects),
        vec![start_listing(&state, PanelSide::Left, "/left"), start_listing(&state, PanelSide::Right, "/right"),]
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
    let skipped = vec![SkippedItem::skipped(PathBuf::from("/left/a.txt"), "denied".to_string())];
    let (state, effects) = update(
        state,
        Command::JobDone {
            outcome: JobOutcome::Completed { skipped: skipped.clone() },
            source_dir: PathBuf::from("/left"),
            dest_dir: PathBuf::from("/left"),
        },
    );
    assert_eq!(state.phase, UiPhase::FileOpSummary(skipped));
    assert_eq!(without_git_info_effects(effects), vec![start_listing(&state, PanelSide::Left, "/left")]);

    let (state, _) = update(state, Command::FileOpConfirm);
    assert_eq!(state.phase, UiPhase::Panels);
}

/// application-shell "Panic hook restores the terminal before reporting"
/// scenario "A panicked job ends and closes its dialog"
/// (process-lifecycle-hardening task 5.5): the worker's `catch_unwind`
/// reply is a `Cancelled` outcome carrying one internal-error skipped item
/// (design D6). The reducer needs no special case — the item takes the
/// job to the summary, which names the error, and the panels re-read.
#[test]
fn job_done_cancelled_with_an_internal_error_item_shows_the_summary_and_rereads_panels() {
    let mut state = test_state(UiPhase::FileOpRunning {
        source_dir: PathBuf::from("/left"),
        dest_dir: PathBuf::from("/right"),
        dialog: RunningDialog::Progress { kind: JobKind::Copy, progress: ProgressInfo::starting(3, 30) },
    });
    state.left.cwd = PathBuf::from("/left");
    state.right.cwd = PathBuf::from("/right");
    let skipped = vec![SkippedItem::skipped(PathBuf::from("/left"), "internal error: index out of bounds".to_string())];
    let (state, effects) = update(
        state,
        Command::JobDone {
            outcome: JobOutcome::Cancelled { skipped: skipped.clone() },
            source_dir: PathBuf::from("/left"),
            dest_dir: PathBuf::from("/right"),
        },
    );
    assert_eq!(state.phase, UiPhase::FileOpSummary(skipped), "the progress dialog is gone and the summary names the internal error");
    assert_eq!(
        without_git_info_effects(effects),
        vec![start_listing(&state, PanelSide::Left, "/left"), start_listing(&state, PanelSide::Right, "/right"),],
        "both affected panels re-read"
    );

    // "a new file operation can be started afterwards": dismissing the
    // summary returns to the panels.
    let (state, _) = update(state, Command::FileOpConfirm);
    assert_eq!(state.phase, UiPhase::Panels);
}

/// application-shell scenario "A panicked listing ends as a failed listing"
/// (process-lifecycle-hardening task 5.5): a `ListingFailed` that arrives
/// after some chunks have streamed in clears the `Streaming` state — the
/// panel stops showing `Reading… N` and shows the error instead.
#[test]
fn listing_failed_after_chunks_clears_the_streaming_state() {
    let mut state = test_state(UiPhase::Panels);
    state.left.progress = crate::panel::ListingProgress::Streaming { count: 0 };
    let (state, _) =
        update(state, Command::ListingChunk { panel: PanelSide::Left, entries: vec![file_entry("a.txt", 1), file_entry("b.txt", 2)], request: 1 });
    assert!(state.left.progress.is_streaming(), "setup precondition: still reading after the first chunk");

    let (state, _) =
        update(state, Command::ListingFailed { panel: PanelSide::Left, message: "Internal error while reading /left: boom".to_string(), request: 1 });
    assert!(!state.left.progress.is_streaming(), "no longer `Reading… N`: {:?}", state.left.progress);
    assert_eq!(state.left.progress.count(), state.left.entries.len(), "the entries that did arrive are kept and counted");
    assert_eq!(state.left.last_error.as_deref(), Some("Internal error while reading /left: boom"));
}

/// operation-dialogs "End-of-job skipped-files summary" scenario "Summary
/// shown for warnings alone" (error-diagnostics): a `JobDone` whose
/// `skipped` vector holds only warnings still moves to the summary phase
/// and still emits the panel re-reads.
#[test]
fn job_done_with_only_warnings_shows_summary_and_rereads_panels() {
    let mut state = test_state(UiPhase::FileOpRunning {
        source_dir: PathBuf::from("/left"),
        dest_dir: PathBuf::from("/right"),
        dialog: RunningDialog::Progress { kind: JobKind::Copy, progress: ProgressInfo::starting(1, 10) },
    });
    state.left.cwd = PathBuf::from("/left");
    state.right.cwd = PathBuf::from("/right");
    let warnings = vec![SkippedItem::warning(PathBuf::from("/right/report.docx"), "modification time not restored: denied".to_string())];
    let (state, effects) = update(
        state,
        Command::JobDone {
            outcome: JobOutcome::Completed { skipped: warnings.clone() },
            source_dir: PathBuf::from("/left"),
            dest_dir: PathBuf::from("/right"),
        },
    );
    assert_eq!(state.phase, UiPhase::FileOpSummary(warnings));
    assert_eq!(
        without_git_info_effects(effects),
        vec![start_listing(&state, PanelSide::Left, "/left"), start_listing(&state, PanelSide::Right, "/right"),]
    );
}

/// The unchanged half of "No summary when nothing was skipped": a job with
/// neither skips nor warnings returns to the panels with no summary.
#[test]
fn job_done_with_no_skips_and_no_warnings_shows_no_summary() {
    let state = running_progress_state(JobKind::Copy, "/left", "/right");
    let (state, _) = update(
        state,
        Command::JobDone {
            outcome: JobOutcome::Completed { skipped: vec![] },
            source_dir: PathBuf::from("/left"),
            dest_dir: PathBuf::from("/right"),
        },
    );
    assert_eq!(state.phase, UiPhase::Panels, "no summary when nothing was skipped and no warnings were recorded");
    assert!(!matches!(state.phase, UiPhase::FileOpSummary(_)));
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
    let (state, effects) = update(
        state,
        Command::JobDone {
            outcome: JobOutcome::Completed { skipped: vec![] },
            source_dir: PathBuf::from("/left"),
            dest_dir: PathBuf::from("/somewhere/else"),
        },
    );
    assert_eq!(without_git_info_effects(effects), vec![start_listing(&state, PanelSide::Left, "/left")]);
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
    state.term_size = (80, 24); // Full-mode body: 19 rows
    state.left.cwd = PathBuf::from("/shared");
    state.right.cwd = PathBuf::from("/shared");
    // Both panels sit partway down the same 30-entry listing; the delete
    // removes "e10".
    for side in [PanelSide::Left, PanelSide::Right] {
        let p = state.panel_mut(side);
        p.sort_mode = SortMode::Unsorted;
        p.entries = (0..30).map(|i| file_entry(&format!("e{i:02}"), 0)).collect();
        p.progress = crate::panel::ListingProgress::Complete { count: 30 };
        p.cursor_user_moved = true;
    }
    state.left.cursor = 10; // on the deleted "e10"
    state.left.scroll_offset = 5;
    state.right.cursor = 20; // on "e20", which survives
    state.right.scroll_offset = 8;

    let (mut state, effects) = update(
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
        vec![start_listing(&state, PanelSide::Left, "/shared"), start_listing(&state, PanelSide::Right, "/shared"),]
    );

    // Each panel's re-read streams back the same 29 survivors; each keeps
    // its own place (file-operations "Post-delete re-read keeps the user's
    // place" — on *both* panels browsing the affected directory).
    let survivors: Vec<Entry> = (0..30).filter(|i| *i != 10).map(|i| file_entry(&format!("e{i:02}"), 0)).collect();
    for side in [PanelSide::Left, PanelSide::Right] {
        let (next, _) = listing_chunk(state, side, survivors.clone());
        state = next;
        let (next, _) = listing_complete(state, side, 29);
        state = next;
    }

    assert_eq!(
        state.left.entries[state.left.cursor].name,
        OsString::from("e11"),
        "the active panel's cursor lands on the entry that followed the deleted one"
    );
    assert_eq!(state.left.cursor, 10);
    assert_eq!(state.left.scroll_offset, 5, "the active panel's window stays put");
    assert_eq!(state.right.entries[state.right.cursor].name, OsString::from("e20"), "the inactive panel's cursor stays on its own (surviving) entry");
    assert_eq!(state.right.cursor, 19, "e20 shifted up one row in the 29-entry list");
    assert_eq!(state.right.scroll_offset, 8, "the inactive panel's window stays put too");
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
        Command::JobDone { outcome: JobOutcome::Completed { skipped: vec![] }, source_dir: PathBuf::from("/left"), dest_dir: PathBuf::from("/left") },
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
    let (state, _) = listing_complete(state, PanelSide::Left, 1);
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
    assert_eq!(without_git_info_effects(effects), vec![start_listing(&state, PanelSide::Left, "/left")]);
    assert!(state.left.last_error.is_none());
}

// ---------------------------------------------------------------------
// filter-selection-safety: hidden-selection disclosure (R-17; selection
// "Operations disclose selected entries hidden by the quick filter")
// ---------------------------------------------------------------------

/// Five selected entries, two of them (`x.log`, `y.log`) hidden by the
/// `.txt` quick filter.
fn five_selected_two_hidden_state() -> State {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1), file_entry("b.txt", 1), file_entry("c.txt", 1), file_entry("x.log", 1), file_entry("y.log", 1)];
    for name in ["a.txt", "b.txt", "c.txt", "x.log", "y.log"] {
        state.left.selected.insert(OsString::from(name));
    }
    state.left.set_quick_filter(Some(".txt".to_string()));
    state
}

/// selection "Delete confirmation states the hidden count" (tasks.md 4.5).
#[test]
fn delete_confirm_discloses_selected_entries_hidden_by_the_filter() {
    let state = five_selected_two_hidden_state();
    let (state, effects) = update(state, Command::RequestDelete);
    assert!(effects.is_empty());
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { ref sources, hidden_by_filter, .. }) => {
            assert_eq!(sources.len(), 5);
            assert_eq!(hidden_by_filter, 2, "x.log and y.log are selected but hidden by the filter");
        }
        other => panic!("expected FileOpSetup::DeleteConfirm, got {other:?}"),
    }
    // Confirming still deletes all five (user decision: the operation
    // reaches the whole selection; the dialog only discloses).
    let (state, effects) = update(state, Command::FileOpConfirm);
    match effects.as_slice() {
        [Effect::RunJob(job)] => assert_eq!(job.sources.len(), 5),
        other => panic!("expected one RunJob over all five sources, got {other:?}"),
    }
    assert!(matches!(state.phase, UiPhase::FileOpRunning { .. }));
}

/// selection "Destination dialog states the hidden count" (tasks.md 4.5).
#[test]
fn destination_dialog_discloses_selected_entries_hidden_by_the_filter() {
    let state = five_selected_two_hidden_state();
    let (state, _) = update(state, Command::RequestCopy);
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, ref sources, hidden_by_filter, .. }) => {
            assert_eq!(kind, JobKind::Copy);
            assert_eq!(sources.len(), 5);
            assert_eq!(hidden_by_filter, 2);
        }
        other => panic!("expected FileOpSetup::DestinationInput, got {other:?}"),
    }
    // Accepting the destination copies all five.
    let (_, effects) = update(state, Command::FileOpConfirm);
    match effects.as_slice() {
        [Effect::RunJob(job)] => assert_eq!(job.sources.len(), 5),
        other => panic!("expected one RunJob over all five sources, got {other:?}"),
    }
}

/// selection "No hidden selection leaves the dialogs unchanged"
/// (tasks.md 4.5).
#[test]
fn no_filter_means_no_hidden_selection_disclosure() {
    let mut state = five_selected_two_hidden_state();
    state.left.quick_filter = None;
    let (state, _) = update(state, Command::RequestDelete);
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { hidden_by_filter, .. }) => assert_eq!(hidden_by_filter, 0),
        other => panic!("expected FileOpSetup::DeleteConfirm, got {other:?}"),
    }
    let mut state = five_selected_two_hidden_state();
    state.left.quick_filter = None;
    let (state, _) = update(state, Command::RequestCopy);
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { hidden_by_filter, .. }) => assert_eq!(hidden_by_filter, 0),
        other => panic!("expected FileOpSetup::DestinationInput, got {other:?}"),
    }
}

/// A drop begun on the *inactive* panel computes the count against that
/// (source) panel's own filter and selection (design D2; tasks.md 4.5).
#[test]
fn a_drop_computes_the_hidden_count_against_the_source_panel() {
    let mut state = test_state(UiPhase::Panels);
    state.right.entries = vec![file_entry("a.txt", 1), file_entry("b.txt", 1), file_entry("x.log", 1)];
    for name in ["a.txt", "b.txt", "x.log"] {
        state.right.selected.insert(OsString::from(name));
    }
    state.right.set_quick_filter(Some(".txt".to_string())); // hides x.log
    state.left.entries = vec![dir_entry("dest")];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Right, name: OsString::from("a.txt"), op: JobKind::Copy });
    let (state, _) = update(
        state,
        Command::DragOver { op: JobKind::Copy, target: Some(DropTarget::SubDir { side: PanelSide::Left, name: OsString::from("dest") }) },
    );
    let (state, _) = update(state, Command::DragDrop { op: JobKind::Copy });
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { ref sources, hidden_by_filter, .. }) => {
            assert_eq!(sources.len(), 3);
            assert_eq!(hidden_by_filter, 1, "the count comes from the source panel's filter and selection");
        }
        other => panic!("expected the drop dialog, got {other:?}"),
    }
}

/// A right-clicked, selection-scoped file-action-menu Delete reports the
/// same count F8 does; the Enter-opened (single-target) menu reports 0
/// (design D2; tasks.md 4.5).
#[test]
fn a_selection_scoped_menu_delete_discloses_the_hidden_count() {
    let state = five_selected_two_hidden_state();
    let (state, _) = update(state, Command::OpenActionMenuAt { side: PanelSide::Left, name: OsString::from("a.txt") });
    assert!(state.file_action_menu.as_ref().unwrap().selection_scoped, "setup precondition: right-click on a selected entry");
    let (state, _) = update(state, Command::FileActionMenuHotkey('D'));
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { ref sources, hidden_by_filter, .. }) => {
            assert_eq!(sources.len(), 5);
            assert_eq!(hidden_by_filter, 2);
        }
        other => panic!("expected FileOpSetup::DeleteConfirm, got {other:?}"),
    }
}

#[test]
fn an_enter_opened_menu_delete_is_single_target_with_no_disclosure() {
    let mut state = five_selected_two_hidden_state();
    state.left.cursor = 0; // a.txt, visible under the filter and itself selected
    let (state, _) = update(state, Command::Enter);
    assert!(state.file_action_menu.is_some());
    let (state, _) = update(state, Command::FileActionMenuHotkey('D'));
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { ref sources, hidden_by_filter, .. }) => {
            assert_eq!(sources.len(), 1, "the Enter-opened menu stays single-target");
            assert_eq!(hidden_by_filter, 0, "its one target is a visible entry");
        }
        other => panic!("expected FileOpSetup::DeleteConfirm, got {other:?}"),
    }
}

/// A command-line `del` of an unselected entry the filter hides reports 0:
/// the typed target is not a selected entry of the panel, so the "selected
/// … hidden by filter" wording would not be true of it (design D2;
/// tasks.md 4.5).
#[test]
fn a_command_line_del_of_an_unselected_hidden_entry_reports_zero() {
    let mut state = five_selected_two_hidden_state();
    state.left.entries.push(file_entry("zz.log", 1)); // hidden by the ".txt" filter, not selected
    let state = type_line(state, "del zz.log");
    let (state, effects) = update(state, Command::Enter);
    assert!(effects.is_empty());
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { ref sources, hidden_by_filter, .. }) => {
            assert_eq!(sources.len(), 1);
            assert_eq!(hidden_by_filter, 0);
        }
        other => panic!("expected FileOpSetup::DeleteConfirm, got {other:?}"),
    }
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
            assert_eq!(input.text(), "draft.txt");
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
    let state = (0.."draft.txt".len()).fold(state, |s, _| update(s, Command::TextEdit(TextEditOp::Backspace)).0);
    let state = "final.txt".chars().fold(state, |s, c| update(s, Command::TextEdit(TextEditOp::Insert(c))).0);
    match &state.phase {
        UiPhase::FileOpSetup(FileOpSetup::RenameInput { input, .. }) => assert_eq!(input.text(), "final.txt"),
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
    let state = (0.."readme.md".len()).fold(state, |s, _| update(s, Command::TextEdit(TextEditOp::Backspace)).0);
    let state = "README.md".chars().fold(state, |s, c| update(s, Command::TextEdit(TextEditOp::Insert(c))).0);
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
        effects
            .iter()
            .any(|e| matches!(e, Effect::StartListing { panel: PanelSide::Left, path, .. } if path.as_path() == std::path::Path::new("/left"))),
        "the panel showing the renamed entry's directory is re-read, got {effects:?}"
    );
}

#[test]
fn ctrl_enter_pastes_the_cursor_filename_and_ctrl_bracket_the_full_path() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    state.left.entries = vec![file_entry("README.md", 1)];

    let (name_state, _) = update(state.clone(), Command::PasteCursorName);
    assert_eq!(name_state.command_line.text(), "README.md");

    let (path_state, _) = update(state, Command::PasteCursorPath);
    assert_eq!(path_state.command_line.text(), PathBuf::from(r"C:\NORTON").join("README.md").display().to_string());
}

#[test]
fn pasting_onto_a_non_empty_buffer_separates_with_a_space() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let state = type_line(state, "type");
    let (state, _) = update(state, Command::PasteCursorName);
    assert_eq!(state.command_line.text(), "type a.txt");
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
    // The new synchronous existence check (command-line "cd navigates the
    // active panel or rejects a nonexistent target") is satisfied here via
    // the panel's own listing rather than real I/O — `sub` is a
    // currently-visible directory entry.
    state.left.entries = vec![dir_entry("sub")];
    let state = type_line(state, "cd sub");
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(state.left.cwd, PathBuf::from("/left/sub"));
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::RunShellCommand(..))),
        "`cd` in a fresh child would be a no-op, so it must not reach the shell"
    );
    assert!(effects.contains(&start_listing(&state, PanelSide::Left, "/left/sub")));
    // command-line "A listed subdirectory needs no filesystem check": the
    // listing answers in the Enter itself, with no probe round trip.
    assert!(!effects.iter().any(|e| matches!(e, Effect::ProbePath { .. })), "the listed-entry fast path emits no probe: {effects:?}");
    assert_eq!(state.command_probe, None);
    assert_eq!(state.history, vec!["cd sub".to_string()], "`cd` is still a history entry");
}

/// application-shell "A command-line target outside the listing produces a
/// probe request, not a disk read" and command-line "cd to a nonexistent
/// directory is rejected without navigating": the reducer emits the probe
/// and leaves the panel alone — the outcome cannot depend on the test
/// machine's disk — and the `Missing` reply then rejects the command exactly
/// as the synchronous check used to.
#[test]
fn cd_to_a_nonexistent_directory_is_rejected_without_navigating() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    state.left.last_error = Some("an older error".to_string());
    let state = type_line(state, r"cd \NOSUCHDIR");
    let (state, effects) = update(state, Command::Enter);
    let expected_path = PathBuf::from(r"C:\NOSUCHDIR");
    assert_eq!(effects, vec![Effect::ProbePath { request: 3, path: expected_path.clone() }], "the check is a probe request, not a disk read");
    assert_eq!(state.left.cwd, PathBuf::from(r"C:\NORTON"), "the active panel's cwd is left completely unchanged");
    assert_eq!(
        state.command_probe,
        Some(PendingProbe {
            request: 3,
            side: PanelSide::Left,
            cwd: PathBuf::from(r"C:\NORTON"),
            path: expected_path.clone(),
            intent: ProbeIntent::Cd { text: r"cd \NOSUCHDIR".to_string() }
        })
    );
    assert_eq!(state.left.pending_note.as_deref(), Some(format!("Checking {}…", expected_path.display()).as_str()));
    assert_eq!(state.left.last_error, None, "a new command supersedes an old error (design D3)");

    let (state, effects) = update(state, Command::PathProbed { request: 3, kind: ProbeKind::Missing });
    assert_eq!(state.left.cwd, PathBuf::from(r"C:\NORTON"));
    assert!(effects.is_empty(), "no listing read is attempted, got {effects:?}");
    let expected = format!("{} not found", expected_path.display());
    assert_eq!(state.left.last_error.as_deref(), Some(expected.as_str()), "the panel shows an error indicating the target was not found");
    assert!(state.history.is_empty(), "a rejected cd is not recorded in history");
    assert_eq!(state.command_probe, None, "the pending entry is cleared");
    assert_eq!(state.left.pending_note, None, "and so is the note");
}

/// application-shell "The probe reply completes the command": a `Dir`
/// reply navigates and records history exactly as a synchronous check
/// would have.
#[test]
fn a_dir_probe_reply_navigates_and_records_history() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    let state = type_line(state, r"cd \\nas\share");
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(effects, vec![Effect::ProbePath { request: 3, path: PathBuf::from(r"\\nas\share") }]);
    assert_eq!(state.left.pending_note.as_deref(), Some(r"Checking \\nas\share…"));

    let (state, effects) = update(state, Command::PathProbed { request: 3, kind: ProbeKind::Dir });
    assert_eq!(state.left.cwd, PathBuf::from(r"\\nas\share"), "the panel navigates to the probed directory");
    assert!(effects.contains(&start_listing(&state, PanelSide::Left, r"\\nas\share")), "{effects:?}");
    assert_eq!(state.history, vec![r"cd \\nas\share".to_string()], "the line is recorded in command history");
    assert!(effects.iter().any(|e| matches!(e, Effect::PersistHistory(f) if f.commands == state.history)), "history is persisted once: {effects:?}");
    assert_eq!(state.command_probe, None);
    assert_eq!(state.left.pending_note, None);
}

/// A `File` reply gives the same rejection the listed-entry path gives for
/// a listed file.
#[test]
fn a_file_probe_reply_rejects_cd_as_not_a_directory() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    let state = type_line(state, r"cd \tools\readme.txt");
    let (state, _) = update(state, Command::Enter);
    let (state, effects) = update(state, Command::PathProbed { request: 3, kind: ProbeKind::File });
    assert!(effects.is_empty());
    assert_eq!(state.left.cwd, PathBuf::from(r"C:\NORTON"));
    let expected = format!("{} is not a directory", PathBuf::from(r"C:\tools\readme.txt").display());
    assert_eq!(state.left.last_error.as_deref(), Some(expected.as_str()));
    assert!(state.history.is_empty());
}

/// application-shell "A superseded probe reply is discarded": a second `cd`
/// replaces the pending probe, so the first reply changes nothing — no
/// navigation and no error.
#[test]
fn a_probe_reply_superseded_by_a_newer_cd_is_discarded() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    let state = type_line(state, r"cd \\slow\share");
    let (state, _) = update(state, Command::Enter);
    let state = type_line(state, r"cd \\fast\share");
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(effects, vec![Effect::ProbePath { request: 4, path: PathBuf::from(r"\\fast\share") }]);
    assert_eq!(state.left.pending_note.as_deref(), Some(r"Checking \\fast\share…"), "the newer check owns the note");

    let before = state.clone();
    let (state, effects) = update(state, Command::PathProbed { request: 3, kind: ProbeKind::Dir });
    assert!(effects.is_empty(), "the stale reply starts nothing: {effects:?}");
    assert_eq!(state, before, "the stale reply changes nothing — the newer check is still outstanding with its note");

    let (state, _) = update(state, Command::PathProbed { request: 4, kind: ProbeKind::Dir });
    assert_eq!(state.left.cwd, PathBuf::from(r"\\fast\share"), "the current reply still applies");
}

/// command-line "A late answer after navigating elsewhere is discarded":
/// the panel no longer shows the directory the command was entered for.
#[test]
fn a_probe_reply_after_the_panel_navigated_is_discarded() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    state.left.entries = vec![dir_entry("docs")];
    let state = type_line(state, r"cd \\slow-server\share");
    let (state, _) = update(state, Command::Enter);
    assert!(state.command_probe.is_some());
    // Enter on the listed `docs` directory navigates the panel meanwhile.
    let (state, _) = update(state, Command::Enter);
    assert_eq!(state.left.cwd, PathBuf::from(r"C:\NORTON\docs"));
    assert_eq!(state.left.pending_note, None, "a new listing takes the note down");

    let (state, effects) = update(state, Command::PathProbed { request: 3, kind: ProbeKind::Missing });
    assert!(effects.is_empty());
    assert_eq!(state.left.cwd, PathBuf::from(r"C:\NORTON\docs"), "the late answer does not navigate");
    assert_eq!(state.left.last_error, None, "and sets no error");
    assert_eq!(state.command_probe, None, "the stale entry is cleared");
}

#[test]
fn cd_to_a_file_is_rejected() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    state.left.entries = vec![file_entry("readme.txt", 1)];
    let state = type_line(state, "cd readme.txt");
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(state.left.cwd, PathBuf::from(r"C:\NORTON"), "the active panel's cwd is left unchanged");
    assert!(effects.is_empty(), "no listing read is attempted, got {effects:?}");
    let expected = format!("{} is not a directory", PathBuf::from(r"C:\NORTON\readme.txt").display());
    assert_eq!(state.left.last_error.as_deref(), Some(expected.as_str()));
}

#[test]
fn cd_accepts_a_manually_entered_unc_path() {
    // A manually-typed UNC path resolves as absolute rather than being
    // joined onto the panel's cwd (this is `dispatch_cd`'s pure path
    // arithmetic, shared with `del`/`rmdir`); a fabricated share can't
    // additionally satisfy the new existence check in a unit test (no real
    // network I/O here), so that half is covered separately by the
    // synchronous-rejection and race-scenario tests below.
    assert_eq!(resolve_cd_target(Path::new(r"C:\NORTON"), r"\\server\share"), Some(PathBuf::from(r"\\server\share")));
}

#[test]
fn a_cd_target_that_fails_asynchronously_after_passing_the_sync_check_surfaces_the_panel_error_state() {
    // Different from `cd_to_a_nonexistent_directory_is_rejected_without_
    // navigating`: this is the race where the target existed at
    // synchronous-check time (so the check passed and a listing began) but
    // became unreachable before the read completed — the read itself runs
    // on a worker thread, and its failure comes back through the same
    // event path as any other listing error (design.md "a race where a
    // target existed at check time but became unreachable before the read
    // completed").
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("share")];
    let state = type_line(state, "cd share");
    let (state, effects) = update(state, Command::Enter);
    assert!(effects.iter().any(|e| matches!(e, Effect::StartListing { .. })), "the sync check passed, so a listing began");
    let (state, _) = listing_failed(state, PanelSide::Left, "network path not found".into());
    assert_eq!(state.left.last_error.as_deref(), Some("network path not found"));
}

/// history-persistence-hardening design D4: a successful `cd` emits exactly
/// one `Effect::PersistHistory` (the duplicate from `dispatch_cd` is gone),
/// and that one snapshot already holds the new command and the new visit.
#[test]
fn a_successful_cd_emits_exactly_one_persist_history_holding_the_command_and_the_visit() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("share")];
    state.epoch_ms = 1_787_000_000_000;
    let state = type_line(state, "cd share");
    let (state, effects) = update(state, Command::Enter);
    let persists: Vec<&config::HistoryFile> = effects.iter().filter_map(|e| if let Effect::PersistHistory(f) = e { Some(f) } else { None }).collect();
    assert_eq!(persists.len(), 1, "exactly one persist, got {effects:?}");
    assert_eq!(persists[0].commands, vec!["cd share".to_string()]);
    assert_eq!(persists[0].directories, vec![history_entry("/left/share", 1, 1_787_000_000_000)]);
    assert_eq!(persists[0].commands, state.history);
    assert_eq!(persists[0].directories, state.dir_history);
}

#[test]
fn cd_dotdot_and_bare_drive_letters_resolve() {
    assert_eq!(resolve_cd_target(Path::new("/a/b"), ".."), Some(PathBuf::from("/a")));
    assert_eq!(resolve_cd_target(Path::new("/a/b"), "."), Some(PathBuf::from("/a/b")));
    assert_eq!(resolve_cd_target(Path::new("/a"), "D:"), Some(PathBuf::from(r"D:\")));
    assert_eq!(parse_cd("dir"), None);
    assert_eq!(parse_cd("cd  \"C:\\Program Files\" "), Some(r"C:\Program Files".to_string()));
    assert_eq!(parse_cd("CD sub"), Some("sub".to_string()), "builtin verbs are case-insensitive");
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
    let payload =
        ClipboardPayload { kind: ClipboardPayloadKind::Paths, items: vec![PathBuf::from(r"C:\NORTON\a.txt"), PathBuf::from(r"C:\NORTON\b.txt")] };
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
    let (state, _) =
        update(state, Command::ClipboardResult { payload, fell_back_to_paths: false, result: Err("Clipboard busy — try again".to_string()) });
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
    let (state, _) = update(state, Command::Tick { now_ms: CLIPBOARD_FEEDBACK_MS - 1, epoch_ms: 0 });
    assert!(state.left.clipboard_feedback.is_some(), "feedback holds until the deadline is reached");

    // At the deadline: expired.
    let (state, _) = update(state, Command::Tick { now_ms: CLIPBOARD_FEEDBACK_MS, epoch_ms: 0 });
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
    assert_eq!(menu_action_command(MenuAction::ClipboardFiles, PanelSide::Left), Some(Command::CopyToClipboard(ClipboardPayloadKind::Files)));
    assert_eq!(menu_action_command(MenuAction::ClipboardPaths, PanelSide::Left), Some(Command::CopyToClipboard(ClipboardPayloadKind::Paths)));
    assert_eq!(menu_action_command(MenuAction::ClipboardNames, PanelSide::Left), Some(Command::CopyToClipboard(ClipboardPayloadKind::Names)));
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
            assert_eq!(input.text(), PathBuf::from("/right").join("OLD").display().to_string());
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
            assert_eq!(input.text(), PathBuf::from("/left").join("src").display().to_string());
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
    let (state, _) = listing_failed(state, PanelSide::Right, "boom".to_string());
    assert!(state.drag.is_none());
}

#[test]
fn drag_clears_on_resize_below_the_minimum() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy });
    let (state, _) = update(state, Command::Resize(10, 5));
    assert!(state.undersized);
    assert_eq!(state.phase, UiPhase::Panels, "the panels survive the shrink; only the drag is cleared");
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
            // Both the seeded panels' current listing ids (1 and 2, from
            // `test_state`) and a superseded one: an accepted failure clears
            // the drag, a stale one is dropped — neither may break the
            // invariant (listing-request-guard design D2).
            (arb_side(), prop_oneof![Just(1u64), Just(2u64), Just(999u64)]).prop_map(|(panel, request)| Command::ListingFailed {
                panel,
                message: "boom".to_string(),
                request
            }),
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
