use super::*;

// ---------------------------------------------------------------------
// pulldown-menu-completion: Commands → Menu file edit (task 2.5; user-menu
// "Edit the user menu file from the Commands menu"; design D4)
// ---------------------------------------------------------------------

#[test]
fn edit_user_menu_file_uses_the_builtin_editor_when_none_is_configured() {
    let mut state = test_state(UiPhase::Panels);
    state.user_menu_path = PathBuf::from("/profile/usermenu.toml");
    let (state, effects) = update(state, Command::EditUserMenuFile);
    assert_eq!(effects, vec![Effect::OpenEditor { request: 3, path: PathBuf::from("/profile/usermenu.toml") }]);
    assert_eq!(state.user_menu_path, PathBuf::from("/profile/usermenu.toml"));
    assert_eq!(state.left.pending_note.as_deref(), Some("Opening usermenu.toml…"), "the active panel announces the open");
}

#[test]
fn edit_user_menu_file_uses_the_configured_external_editor() {
    let mut state = test_state(UiPhase::Panels);
    state.editor = Some("notepad".to_string());
    state.user_menu_path = PathBuf::from("/profile/usermenu.toml");
    let (state, effects) = update(state, Command::EditUserMenuFile);
    assert_eq!(effects.len(), 1);
    match &effects[0] {
        Effect::RunExternalEditor(invocation, side) => {
            assert_eq!(*side, PanelSide::Left);
            assert_eq!(invocation.file_arg, OsString::from("/profile/usermenu.toml"));
            assert_eq!(invocation.program, "notepad");
        }
        other => panic!("expected RunExternalEditor, got {other:?}"),
    }
    assert!(state.user_menu().is_none(), "no overlay opened");
}

#[test]
fn user_menu_reloaded_applies_a_wellformed_file() {
    let mut state = test_state(UiPhase::Panels);
    state.user_menu_entries = vec![config::UserMenuEntry { label: "Old".to_string(), command: "old".to_string() }];
    let result = config::UserMenuLoadResult {
        entries: vec![config::UserMenuEntry { label: "New".to_string(), command: "new".to_string() }],
        warnings: Vec::new(),
        created_default: false,
        malformed: false,
        problem: None,
    };
    let (state, effects) = update(state, Command::UserMenuReloaded(result));
    assert!(effects.is_empty());
    assert_eq!(state.user_menu_entries.len(), 1);
    assert_eq!(state.user_menu_entries[0].label, "New", "the reloaded entries replace the old ones");
    assert_eq!(state.startup_warning(), None);
}

/// configuration-files "Warnings after editing the user menu": a
/// well-formed reload with non-fatal warnings applies the entries *and*
/// shows the warning summary naming the file and line.
#[test]
fn user_menu_reloaded_with_warnings_applies_entries_and_shows_the_summary() {
    let mut state = test_state(UiPhase::Panels);
    state.user_menu_entries = vec![config::UserMenuEntry { label: "Old".to_string(), command: "old".to_string() }];
    let parsed = config::parse_usermenu("[[entry]]\nlabel = \"New\"\ncommand = \"new\"\nicon = \"disk\"\n");
    assert!(!parsed.malformed && parsed.warnings.len() == 1, "fixture: one unknown-key warning");
    let result =
        config::UserMenuLoadResult { entries: parsed.entries, warnings: parsed.warnings, created_default: false, malformed: false, problem: None };
    let (state, effects) = update(state, Command::UserMenuReloaded(result));
    assert!(effects.is_empty());
    assert_eq!(state.user_menu_entries.len(), 1);
    assert_eq!(state.user_menu_entries[0].label, "New", "the edited entries replace the F2 menu");
    let warning = state.startup_warning().expect("the warnings raise the dialog");
    assert!(warning.starts_with("usermenu.toml line 4:"), "names the file and line: {warning}");
    assert!(warning.contains("icon"), "names the unknown key: {warning}");
}

#[test]
fn user_menu_reloaded_malformed_warns_and_keeps_the_previous_entries() {
    let mut state = test_state(UiPhase::Panels);
    state.user_menu_entries = vec![config::UserMenuEntry { label: "Previous".to_string(), command: "prev".to_string() }];
    let result = config::UserMenuLoadResult {
        entries: config::default_user_menu_entries(),
        warnings: Vec::new(),
        created_default: false,
        malformed: true,
        problem: Some(config::UserMenuProblem::InvalidToml),
    };
    let (state, effects) = update(state, Command::UserMenuReloaded(result));
    assert!(effects.is_empty());
    assert_eq!(state.user_menu_entries[0].label, "Previous", "the previous entries are kept, not the defaults");
    let warning = state.startup_warning().expect("the malformed reload raises the warning dialog");
    assert!(warning.contains(config::USERMENU_FILE), "the warning names the file: {warning}");
}

#[test]
fn f4_with_no_editor_configured_opens_the_built_in_editor() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("report.txt", 10)];
    let (state, effects) = update(state, Command::RequestEditor);
    assert_eq!(effects, vec![Effect::OpenEditor { request: 3, path: PathBuf::from("/left/report.txt") }]);
    assert_eq!(state.phase, UiPhase::Panels, "phase flips only once EditorOpened comes back — opening is I/O");
    assert_eq!(state.pending_open, Some(PendingOpen { request: 3, side: PanelSide::Left, kind: PendingOpenKind::Editor }));
    assert_eq!(state.left.pending_note.as_deref(), Some("Opening report.txt…"));
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
    let state = with_pending_open(test_state(UiPhase::Panels), 3);
    let editor = editor_with(&["line one", "line two"]);
    let (state, effects) = update(state, Command::EditorOpened { request: 3, editor: Box::new(editor) });
    assert!(effects.is_empty());
    assert_eq!(state.pending_open, None);
    match state.phase {
        UiPhase::Editor(e) => assert_eq!(e.lines, vec!["line one".to_string(), "line two".to_string(), String::new()]),
        other => panic!("expected UiPhase::Editor, got {other:?}"),
    }
}

#[test]
fn editor_too_large_opens_the_viewer_instead_with_an_inline_notice() {
    let state = with_pending_open(test_state(UiPhase::Panels), 3);
    let (state, effects) = update(state, Command::EditorTooLarge { request: 3, path: PathBuf::from("/left/huge.log"), size: 20_000_000 });
    // The redirect is a fresh request for the same side (offload-blocking-io
    // design D4): a late reply to the editor open can no longer match.
    assert_eq!(effects, vec![Effect::OpenViewer { request: 4, path: PathBuf::from("/left/huge.log") }]);
    assert_eq!(state.pending_open, Some(PendingOpen { request: 4, side: PanelSide::Left, kind: PendingOpenKind::Viewer }));
    assert_eq!(state.left.pending_note.as_deref(), Some("Opening huge.log…"));
    assert_eq!(state.phase, UiPhase::Panels);
    let message = state.left.last_error.expect("a notice explaining the redirect");
    assert!(message.contains("huge.log"), "{message}");
    assert!(message.contains("10 MB"), "{message}");
}

/// builtin-editor "UTF-16 file redirects to the viewer", "File containing a
/// NUL byte redirects to the viewer", "Non-UTF-8 file redirects to the
/// viewer": each reason lands its own notice on the active panel and opens
/// the viewer, never the editor (editor-encoding-safety design D2).
#[test]
fn editor_not_editable_opens_the_viewer_instead_with_a_reason_specific_notice() {
    use crate::editor::NotEditableReason;
    let cases = [
        (NotEditableReason::Utf16, "export.reg", "export.reg is UTF-16 text — the editor only edits UTF-8; opened in the viewer"),
        (
            NotEditableReason::Binary,
            "tool.exe",
            "tool.exe looks binary (contains NUL bytes) — the editor only edits UTF-8 text; opened in the viewer",
        ),
        (NotEditableReason::NotUtf8, "setup.bat", "setup.bat is not valid UTF-8 — the editor only edits UTF-8 text; opened in the viewer"),
    ];
    for (reason, name, expected) in cases {
        let state = with_pending_open(test_state(UiPhase::Panels), 3);
        let path = PathBuf::from(format!("/left/{name}"));
        let (state, effects) = update(state, Command::EditorNotEditable { request: 3, path: path.clone(), reason });
        assert_eq!(effects, vec![Effect::OpenViewer { request: 4, path }], "{reason:?} opens the viewer for the same path under a fresh id");
        assert_eq!(state.phase, UiPhase::Panels, "{reason:?} never enters the editor phase");
        assert_eq!(state.left.last_error.as_deref(), Some(expected), "{reason:?} notice");
        assert_eq!(state.right.last_error, None, "the notice lands on the active panel only");
    }
}

#[test]
fn editor_not_editable_notice_lands_on_the_right_panel_when_it_is_active() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Right;
    let state = with_pending_open(state, 3);
    let (state, effects) = update(
        state,
        Command::EditorNotEditable { request: 3, path: PathBuf::from("/right/export.reg"), reason: crate::editor::NotEditableReason::Utf16 },
    );
    assert_eq!(effects, vec![Effect::OpenViewer { request: 4, path: PathBuf::from("/right/export.reg") }]);
    assert!(state.right.last_error.as_deref().unwrap().contains("UTF-16"));
    assert_eq!(state.left.last_error, None);
}

/// builtin-editor "External editor receives non-UTF-8 files unchanged": the
/// reducer never inspects content, so with `editor =` configured F4 on a
/// UTF-16 `.reg` export dispatches the external editor and no
/// `Effect::OpenEditor` (and therefore no classification) ever runs.
#[test]
fn f4_with_an_external_editor_configured_never_classifies_the_file() {
    let mut state = test_state(UiPhase::Panels);
    state.editor = Some("notepad".to_string());
    state.left.entries = vec![file_entry("export.reg", 4096)];
    let (state, effects) = update(state, Command::RequestEditor);
    assert_eq!(effects.len(), 1);
    assert!(matches!(effects[0], Effect::RunExternalEditor(_, PanelSide::Left)), "got {effects:?}");
    assert!(!effects.iter().any(|e| matches!(e, Effect::OpenEditor { .. })), "no OpenEditor effect: {effects:?}");
    assert_eq!(state.left.last_error, None, "no redirect notice");
}
