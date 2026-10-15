use super::*;

// ---------------------------------------------------------------------
// M4: F3 viewer & F4 external editor
// ---------------------------------------------------------------------

#[test]
fn f3_on_a_file_dispatches_open_viewer_without_touching_the_filesystem() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("notes.txt", 42)];
    let (state, effects) = update(state, Command::RequestViewer);
    assert_eq!(effects, vec![Effect::OpenViewer { request: 3, path: PathBuf::from("/left/notes.txt") }]);
    // Nothing changed yet — the phase flips only once `ViewerOpened` comes
    // back, since opening is I/O and `update` performs none itself.
    assert_eq!(state.phase, UiPhase::Panels);
    assert_eq!(state.pending_open, Some(PendingOpen { request: 3, side: PanelSide::Left, kind: PendingOpenKind::Viewer }));
    assert_eq!(state.left.pending_note.as_deref(), Some("Opening notes.txt…"), "the panel announces the outstanding open");
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
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("big.log", 5_000_000_000)];
    let (state, _) = update(state, Command::RequestViewer);
    let (state, effects) = update(state, Command::ViewerOpened { request: 3, path: PathBuf::from("/left/big.log"), file_len: 5_000_000_000 });
    assert!(effects.is_empty());
    assert_eq!(state.pending_open, None, "the applied reply clears the pending open");
    assert_eq!(state.left.pending_note, None, "and takes the `Opening…` note down");
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
    let state = with_pending_open(test_state(UiPhase::Panels), 3);
    let (state, effects) = update(state, Command::ViewerOpenFailed { request: 3, message: "access denied".to_string() });
    assert!(effects.is_empty());
    assert_eq!(state.phase, UiPhase::Panels);
    assert_eq!(state.left.last_error.as_deref(), Some("access denied"));
    assert_eq!(state.pending_open, None);
}

#[test]
fn a_subsequently_successful_f3_open_clears_a_stale_last_error_from_an_earlier_failed_attempt() {
    let mut state = test_state(UiPhase::Panels);
    let (state_after_failure, _) =
        update(with_pending_open(state.clone(), 3), Command::ViewerOpenFailed { request: 3, message: "access denied".to_string() });
    assert_eq!(state_after_failure.left.last_error.as_deref(), Some("access denied"));

    state.left.last_error = Some("access denied".to_string());
    let (state, _) = update(with_pending_open(state, 3), Command::ViewerOpened { request: 3, path: PathBuf::from("/left/notes.txt"), file_len: 10 });
    assert_eq!(state.left.last_error, None, "a successful F3 open must clear a stale error left by an earlier failed attempt");
    assert!(matches!(state.phase, UiPhase::Viewer(_)));
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
    assert_eq!(effects, vec![Effect::RunViewerSearch { path: PathBuf::from("/left/f.txt"), start_offset: 0, pattern: b"ab".to_vec(), request }]);
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
    let (state, _) = update(state, Command::ViewerSearchResult { offset: Some(900), match_range: Some((900, 905)), request: stale_request });
    let UiPhase::Viewer(v) = &state.phase else { panic!("expected viewer phase") };
    assert_eq!(v.top_offset, 5, "a stale reply must not move the top offset");
    assert_eq!(v.last_match, None, "a stale reply must not set a phantom match highlight");

    // The still-current search's reply, once it arrives, is applied.
    let (state, _) = update(state, Command::ViewerSearchResult { offset: Some(700), match_range: Some((700, 706)), request: current_request });
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
    let (state, _) =
        update(with_pending_open(state, 50), Command::ViewerOpened { request: 50, path: PathBuf::from("/left/other.txt"), file_len: 200 });
    let UiPhase::Viewer(v) = &state.phase else { panic!("expected viewer phase") };
    assert_eq!(v.search_request, None, "a freshly opened session has no outstanding search");

    let (state, _) = update(state, Command::ViewerSearchResult { offset: Some(50), match_range: Some((50, 56)), request: stale_request });
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
        state.left.entries = vec![Entry::new(raw.clone(), EntryKind::File, 0, None, false, false)];
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
    assert_eq!(state.left.last_error, None, "successfully dispatching the F4 editor spawn must clear a stale error from an earlier failed attempt");
    assert!(matches!(effects.as_slice(), [Effect::RunExternalEditor(_, PanelSide::Left)]));
}

// ---------------------------------------------------------------------
// offload-blocking-io: open replies are gated on the pending request
// (application-shell "Slow storage never blocks the event loop"; design D4)
// ---------------------------------------------------------------------

/// application-shell "A newer F3 supersedes an outstanding open": the late
/// answer for the first file is discarded without touching the newer
/// pending open, and the newer answer still applies.
#[test]
fn an_open_reply_with_a_stale_request_id_is_discarded() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("slow.log", 10), file_entry("fast.txt", 10)];
    let (state, effects) = update(state, Command::RequestViewer);
    assert_eq!(effects, vec![Effect::OpenViewer { request: 3, path: PathBuf::from("/left/slow.log") }]);
    let (state, _) = update(state, Command::MoveCursor(CursorMove::Down(1)));
    let (state, effects) = update(state, Command::RequestViewer);
    assert_eq!(effects, vec![Effect::OpenViewer { request: 4, path: PathBuf::from("/left/fast.txt") }]);
    assert_eq!(state.left.pending_note.as_deref(), Some("Opening fast.txt…"), "the newer request owns the note");

    let before = state.clone();
    let (state, effects) = update(state, Command::ViewerOpened { request: 3, path: PathBuf::from("/left/slow.log"), file_len: 10 });
    assert!(effects.is_empty());
    assert_eq!(state, before, "the stale reply changes nothing, and the newer pending open is untouched");
    let (state, effects) = update(state, Command::ViewerOpenFailed { request: 3, message: "late failure".to_string() });
    assert!(effects.is_empty());
    assert_eq!(state, before, "a stale failure lands nowhere either");

    let (state, _) = update(state, Command::ViewerOpened { request: 4, path: PathBuf::from("/left/fast.txt"), file_len: 10 });
    match &state.phase {
        UiPhase::Viewer(v) => assert_eq!(v.path, PathBuf::from("/left/fast.txt"), "the viewer opens the newer file"),
        other => panic!("expected the viewer, got {other:?}"),
    }
    assert_eq!(state.pending_open, None);
}

/// application-shell "A late open does not pull the user out of a menu":
/// the reply arriving with the F9 menu open is discarded, the menu stays,
/// and the pending state is cleared.
#[test]
fn an_open_reply_arriving_with_the_menu_open_is_discarded() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("notes.txt", 10)];
    let (state, _) = update(state, Command::RequestEditor);
    let (state, _) = update(state, Command::MenuOpen);
    assert!(state.menu.is_some());
    let (state, effects) = update(state, Command::EditorOpened { request: 3, editor: Box::new(editor_with(&["abc"])) });
    assert!(effects.is_empty());
    assert!(state.menu.is_some(), "the menu stays open");
    assert_eq!(state.phase, UiPhase::Panels, "the editor does not open");
    assert_eq!(state.pending_open, None, "the late result is discarded and the pending open cleared");
    assert_eq!(state.left.pending_note, None, "the `Opening…` note comes down");

    // The same for the viewer, and for a reply while a dialog owns input.
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("notes.txt", 10)];
    let (state, _) = update(state, Command::RequestViewer);
    let (state, _) = update(state, Command::HelpOpen);
    let (state, _) = update(state, Command::ViewerOpened { request: 3, path: PathBuf::from("/left/notes.txt"), file_len: 10 });
    assert_eq!(state.phase, UiPhase::Panels, "the viewer does not open over the Help window");
    assert!(state.help.is_some());

    // A reply after the user left the panels (a file operation dialog).
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("notes.txt", 10)];
    let (state, _) = update(state, Command::RequestViewer);
    let (state, _) = update(state, Command::RequestDelete);
    assert!(matches!(state.phase, UiPhase::FileOpSetup(_)), "precondition: the delete dialog owns the phase");
    let (state, _) = update(state, Command::ViewerOpened { request: 3, path: PathBuf::from("/left/notes.txt"), file_len: 10 });
    assert!(matches!(state.phase, UiPhase::FileOpSetup(_)), "the dialog is untouched");
    assert_eq!(state.pending_open, None);
}

/// application-shell "Launching a slow handler leaves the panels
/// interactive", applied to the opens: a failure lands on the panel the
/// open was requested from, not on the panel active when it arrives.
#[test]
fn an_open_failure_lands_on_the_recorded_side_after_a_tab() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("notes.txt", 10)];
    let (state, _) = update(state, Command::RequestEditor);
    let (state, _) = update(state, Command::ToggleActivePanel);
    assert_eq!(state.active, PanelSide::Right);
    let (state, _) = update(state, Command::EditorOpenFailed { request: 3, message: "access denied".to_string() });
    assert_eq!(state.left.last_error.as_deref(), Some("access denied"), "the failure lands on the requesting panel");
    assert_eq!(state.right.last_error, None);
    assert_eq!(state.left.pending_note, None);

    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("notes.txt", 10)];
    let (state, _) = update(state, Command::RequestViewer);
    let (state, _) = update(state, Command::ToggleActivePanel);
    let (state, _) = update(state, Command::ViewerOpenFailed { request: 3, message: "access denied".to_string() });
    assert_eq!(state.left.last_error.as_deref(), Some("access denied"));
    assert_eq!(state.right.last_error, None);
}

/// Requesting an open supersedes an earlier failure on the same panel: the
/// `Opening…` note takes the stale error down (design D3).
#[test]
fn requesting_an_open_clears_the_panels_last_error() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("notes.txt", 10)];
    state.left.last_error = Some("an older failure".to_string());
    let (state, _) = update(state, Command::RequestViewer);
    assert_eq!(state.left.last_error, None);
    assert_eq!(state.left.pending_note.as_deref(), Some("Opening notes.txt…"));
}

#[test]
fn editor_open_failed_surfaces_an_inline_error() {
    let state = with_pending_open(test_state(UiPhase::Panels), 3);
    let (state, effects) = update(state, Command::EditorOpenFailed { request: 3, message: "access denied".to_string() });
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
        [Effect::SaveEditor { editor: e, then_quit: false }] => {
            assert_eq!(e.lines, editor.lines);
            assert!(!e.save_pending, "the copy handed to the worker is not itself marked pending");
        }
        other => panic!("expected a single SaveEditor effect, got {other:?}"),
    }
    // The buffer is unchanged until the reply arrives — save is I/O.
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert!(e.is_modified());
    assert!(e.save_pending, "the save is in flight (offload-blocking-io design D6)");

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
    assert!(!e.save_pending, "the reply ends the in-flight save");
}

#[test]
fn editor_save_failed_surfaces_an_inline_message_without_losing_the_buffer() {
    let mut editor = editor_with(&["abc"]);
    editor.type_char('X');
    editor.save_pending = true;
    let state = editor_state(editor);
    let (state, effects) = update(state, Command::EditorSaveFailed { message: "disk full".to_string() });
    assert!(effects.is_empty());
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert_eq!(e.save_error.as_deref(), Some("disk full"));
    assert!(e.is_modified(), "a failed save must not silently discard the unsaved edit");
    assert!(!e.save_pending, "the failure ends the in-flight save");
}

/// application-shell "Saving to a slow share shows progress and accepts no
/// edits": while the save is in flight, edits, caret moves and further
/// save/quit commands leave the editor unchanged and emit nothing; the reply
/// then clears the flag (offload-blocking-io design D6).
#[test]
fn while_a_save_is_pending_the_editor_ignores_everything_but_the_save_reply() {
    let mut editor = editor_with(&["abc"]);
    editor.type_char('X');
    let state = editor_state(editor);
    let (state, effects) = update(state, Command::EditorSave);
    assert!(matches!(effects.as_slice(), [Effect::SaveEditor { then_quit: false, .. }]));
    let UiPhase::Editor(pending) = &state.phase else { panic!("expected editor phase") };
    assert!(pending.save_pending);
    let pending = pending.clone();

    for cmd in [
        Command::EditorChar('Y'),
        Command::EditorNewline,
        Command::EditorBackspace,
        Command::EditorMove(EditorMove::Right),
        Command::EditorMove(EditorMove::Down),
        Command::EditorSave,
        Command::EditorRequestQuit,
        Command::EditorConfirmQuitSave,
        Command::EditorConfirmQuitDiscard,
        Command::EditorUndo,
    ] {
        let (after, effects) = update(state.clone(), cmd.clone());
        assert!(effects.is_empty(), "{cmd:?} emits nothing while a save is pending, got {effects:?}");
        let UiPhase::Editor(e) = &after.phase else { panic!("{cmd:?}: the editor stays open while a save is pending") };
        assert_eq!(*e, pending, "{cmd:?} leaves the editor unchanged while a save is pending");
    }

    // The reply — the worker's copy with its clean snapshot — clears the
    // flag and the modified state, and typing works again.
    let saved = editor_with(&["Xabc"]);
    let (state, effects) = update(state, Command::EditorSaved { editor: Box::new(saved), then_quit: false });
    assert!(effects.is_empty());
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert!(!e.save_pending);
    assert!(!e.is_modified(), "the saved buffer is clean");
    let (state, _) = update(state, Command::EditorChar('Z'));
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert!(e.is_modified(), "typing edits the buffer again once the save completed");
}

/// The save-on-exit confirm's "Y" is gated the same way, and a failure
/// during it clears the flag so the user can act on the error.
#[test]
fn a_save_on_exit_in_flight_is_gated_until_its_reply() {
    let mut editor = editor_with(&["abc"]);
    editor.type_char('X');
    let state = editor_state(editor);
    let (state, _) = update(state, Command::EditorRequestQuit);
    let (state, effects) = update(state, Command::EditorConfirmQuitSave);
    assert!(matches!(effects.as_slice(), [Effect::SaveEditor { then_quit: true, .. }]));
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert!(e.save_pending);
    let (after, effects) = update(state.clone(), Command::EditorCancelQuit);
    assert!(effects.is_empty());
    assert_eq!(after, state, "Esc during the in-flight save changes nothing");

    let (state, _) = update(state, Command::EditorSaveFailed { message: "disk full".to_string() });
    let UiPhase::Editor(e) = &state.phase else { panic!("expected editor phase") };
    assert!(!e.save_pending);
    assert_eq!(e.save_error.as_deref(), Some("disk full"));
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
