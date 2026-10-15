//! File operations: the F5/F6/F7/F8 setup dialogs, the running-job dialogs
//! and their worker replies, clipboard export, the file-action menu's action
//! routing, and the source/selection and drag-and-drop helpers.

use super::*;

// ---------------------------------------------------------------------
// Clipboard export (clipboard-export)
// ---------------------------------------------------------------------

/// Ctrl+C/Ctrl+Ins, Ctrl+Shift+Ins, and the Files pull-down's clipboard
/// group: resolve the F5 scope and either request the write (leaving
/// feedback to `apply_clipboard_result` once `Effect::SetClipboard`
/// replies) or, when the scope is empty, report that directly — the parent
/// pseudo-entry is never included, since `active_selection_sources` already
/// excludes it (clipboard-export "Clipboard payloads and scope": "Parent
/// entry is never copied").
pub(super) fn handle_copy_to_clipboard(state: &mut State, kind: ClipboardPayloadKind) -> Vec<Effect> {
    let sources = active_selection_sources(state);
    if sources.is_empty() {
        set_clipboard_feedback(state, "Nothing to copy".to_string(), false);
        return vec![];
    }
    let items = sources.into_iter().map(|s| s.path).collect();
    vec![Effect::SetClipboard(ClipboardPayload { kind, items })]
}

/// The file-action menu's `Send to clipboard`: always `Files`, scoped to
/// whatever `selection_or_single_source` resolved — the menu's single
/// target entry, or the whole selection set for a selection-scoped
/// invocation (design D3 of `file-action-menu`, D4 of `mouse-basics`,
/// mirrored here from `activate_file_action`'s existing Copy/Move/Delete
/// handling). A no-op if the target has already vanished from the panel.
pub(super) fn handle_send_target_to_clipboard(sources: Vec<SourceItem>) -> Vec<Effect> {
    if sources.is_empty() {
        return vec![];
    }
    let items = sources.into_iter().map(|s| s.path).collect();
    vec![Effect::SetClipboard(ClipboardPayload { kind: ClipboardPayloadKind::Files, items })]
}

/// Reply to `Effect::SetClipboard`: render the outcome into the active
/// panel's mini-status feedback (clipboard-export "Clipboard feedback",
/// "Non-Windows fallback").
pub(super) fn apply_clipboard_result(state: &mut State, payload: ClipboardPayload, fell_back_to_paths: bool, result: Result<(), String>) {
    let (message, is_error) = match result {
        Err(message) => (message, true),
        Ok(()) => (clipboard_success_message(&payload, fell_back_to_paths), false),
    };
    set_clipboard_feedback(state, message, is_error);
}

/// The success feedback template for a completed `ClipboardPayload`
/// (clipboard-export "Clipboard feedback"). `Paths` gets a singular
/// variant naming the one path copied; `Files`/`Names` always use the
/// plural template, matching the literal message set the requirement
/// enumerates. A `Files` request downgraded to a `Paths` text write on a
/// non-Windows host reports that explicitly instead (clipboard-export
/// "Non-Windows fallback").
pub(super) fn clipboard_success_message(payload: &ClipboardPayload, fell_back_to_paths: bool) -> String {
    if fell_back_to_paths {
        return "Paths copied (file objects unsupported here)".to_string();
    }
    let n = payload.items.len();
    match payload.kind {
        ClipboardPayloadKind::Files => format!("{n} files copied to clipboard"),
        ClipboardPayloadKind::Paths if n == 1 => format!("Path copied: {}", payload.items[0].display()),
        ClipboardPayloadKind::Paths => format!("{n} paths copied"),
        ClipboardPayloadKind::Names => format!("{n} names copied"),
    }
}

/// Show `message` in the active panel's mini-status until the next key
/// press or `CLIPBOARD_FEEDBACK_MS` elapses, whichever comes first
/// (clipboard-export "Clipboard feedback").
pub(super) fn set_clipboard_feedback(state: &mut State, message: String, is_error: bool) {
    let side = state.active;
    let expires_at_ms = state.clock_ms.saturating_add(CLIPBOARD_FEEDBACK_MS);
    state.panel_mut(side).clipboard_feedback = Some(ClipboardFeedback { message, is_error, expires_at_ms });
}

// ---------------------------------------------------------------------
// File operations (M2)
// ---------------------------------------------------------------------

/// Which selectable entry(ies) an F5/F6/F8 request applies to: the explicit
/// selection if non-empty, else the single entry under the cursor.
pub(super) fn active_selection_sources(state: &State) -> Vec<SourceItem> {
    let panel = state.active_panel();
    if !panel.selected.is_empty() {
        return panel
            .entries
            .iter()
            .filter(|e| panel.selected.contains(&e.name))
            .map(|e| SourceItem { original_name: e.name.clone(), path: panel.cwd.join(&e.name), is_dir: e.is_dir_like() })
            .collect();
    }
    match panel.selected() {
        Some(e) if e.kind != EntryKind::ParentDir => {
            vec![SourceItem { original_name: e.name.clone(), path: panel.cwd.join(&e.name), is_dir: e.is_dir_like() }]
        }
        _ => vec![],
    }
}

/// F5/F6/F7: enter the destination/name-input setup dialog. A no-op for
/// Copy/Move when there is nothing selected; Mkdir is always available.
pub(super) fn enter_file_op_setup(state: &mut State, kind: JobKind) {
    if kind == JobKind::Mkdir {
        let side = state.active;
        let source_dir = state.panel(side).cwd.clone();
        state.phase = UiPhase::FileOpSetup(FileOpSetup::DestinationInput {
            kind,
            sources: vec![],
            source_dir,
            input: LineEdit::default(),
            buttons: None,
            hidden_by_filter: 0,
        });
        return;
    }
    let sources = active_selection_sources(state);
    if sources.is_empty() {
        return;
    }
    let side = state.active;
    let prefill = state.panel(side.toggle()).cwd.display().to_string();
    enter_file_op_setup_for_sources(state, kind, sources, side, prefill, None);
}

/// Shared by `enter_file_op_setup` (F5/F6, selection- or cursor-scoped), the
/// file-action menu's Copy/Move (cursor-entry-only, D3), and `Command::
/// DragDrop` (mouse-panel-drag) — the setup dialog itself doesn't care which
/// chose its `sources`. `source_side`/`prefill` are explicit rather than
/// derived from `state.active`/its opposite panel internally, so a caller
/// acting on a side other than the active panel (a drag begun on the
/// inactive panel, which a press never focuses before the drag completes)
/// gets the right source directory and the right prefill without the dialog
/// silently assuming the active panel (operation-dialogs design D3).
/// `buttons` is `None` for every keyboard-reached caller (byte-identical to
/// the pre-`mouse-panel-drag` dialog) and `Some` only from `DragDrop`, which
/// also passes the exact drop path as `prefill` rather than the opposite
/// panel's cwd (operation-dialogs "Drop-initiated destination dialog").
/// `hidden_by_filter` is computed from the *source* panel — for a drag begun
/// on the inactive panel, that panel's own filter and selection decide the
/// count (selection "Operations disclose selected entries hidden by the
/// quick filter"; filter-selection-safety design D2).
pub(super) fn enter_file_op_setup_for_sources(
    state: &mut State,
    kind: JobKind,
    sources: Vec<SourceItem>,
    source_side: PanelSide,
    prefill: String,
    buttons: Option<DropButtons>,
) {
    let source_dir = state.panel(source_side).cwd.clone();
    let hidden_by_filter = state.panel(source_side).hidden_selected_count(&sources);
    // A pre-filled destination opens with the caret at the end of the text,
    // so Enter-to-accept is unchanged and editing starts from the tail
    // (text-field-editing "Pre-filled field accepts unchanged with Enter").
    state.phase =
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, source_dir, input: LineEdit::from(prefill), buttons, hidden_by_filter });
}

/// F8: enter the delete-confirmation dialog. A no-op when there is nothing
/// selected.
pub(super) fn enter_delete_confirm(state: &mut State) {
    let sources = active_selection_sources(state);
    if sources.is_empty() {
        return;
    }
    enter_delete_confirm_for_sources(state, sources);
}

/// Shared by `enter_delete_confirm` (F8) and the file-action menu's Delete
/// (cursor-entry-only, D3). `hidden_by_filter` counts the sources that are
/// selected entries of the source (active) panel hidden by its quick filter
/// (selection "Operations disclose selected entries hidden by the quick
/// filter"; filter-selection-safety design D2).
pub(super) fn enter_delete_confirm_for_sources(state: &mut State, sources: Vec<SourceItem>) {
    let side = state.active;
    let panel = state.panel(side);
    let source_dir = panel.cwd.clone();
    let hidden_by_filter = panel.hidden_selected_count(&sources);
    // A directory *link* listed by this panel looks like a non-empty
    // directory but Delete removes only the link (file-operations "Delete
    // removes the link, not the target"), so it does not trigger the
    // "contains a directory — delete its entire contents?" second prompt
    // (panel-navigation "Deleting a directory link asks once"; link-aware-
    // listing design D5). Usually an empty set.
    let listed_links: HashSet<&OsString> = panel.entries.iter().filter(|e| e.link).map(|e| &e.name).collect();
    let is_listed_link = |s: &SourceItem| s.path.parent() == Some(source_dir.as_path()) && listed_links.contains(&s.original_name);
    let needs_second_confirm = sources.iter().any(|s| s.is_dir && !is_listed_link(s));
    state.phase =
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { sources, source_dir, needs_second_confirm, confirmed_once: false, hidden_by_filter });
}

/// The file-action menu's Rename: enter the in-place rename input dialog,
/// pre-filled with the target's current name and the caret at the end of
/// the field (file-action-menu "In-place Rename": "an input dialog
/// pre-filled with the target entry's current name, with the caret at the
/// end of the field").
pub(super) fn enter_rename_input(state: &mut State, original_name: OsString, is_dir: bool) {
    let side = state.active;
    let source_dir = state.panel(side).cwd.clone();
    let input = LineEdit::from(original_name.to_string_lossy().into_owned());
    state.phase = UiPhase::FileOpSetup(FileOpSetup::RenameInput { source_dir, original_name, is_dir, input });
}

/// Route an activated menu entry into its existing capability flow, applied
/// to `target_name` — the entry the menu was opened on, captured in
/// `FileActionMenuState` at open time and threaded through here rather than
/// re-read from the panel's live cursor, which can drift while the menu (a
/// modal overlay) is open: a background `ListingChunk`/`ListingComplete` is
/// applied unconditionally regardless of any open modal and resets the
/// cursor to row 0 whenever the panel hasn't seen a user-driven move since
/// the listing began (file-action-menu "Menu actions route to existing
/// flows"). `selection_scoped` — also captured at open time — widens
/// Copy/Move/Delete/Send to clipboard to the whole selection set when the
/// menu was opened on an already-selected entry (mouse-basics design D4;
/// file-action-menu "Directory targets and selection-scoped invocation").
pub(super) fn activate_file_action(state: &mut State, action: FileActionMenuEntry, target_name: OsString, selection_scoped: bool) -> Vec<Effect> {
    let side = state.active;
    match action {
        // Open: hand the target to the platform's file association,
        // fire-and-forget — the TUI stays live and nothing is waited on or
        // re-read (open-with-associated-app design D1/D3; file-action-menu
        // "Open with the associated application"). Always single-target,
        // even when the target is selected: Open never widens to the
        // selection (file-action-menu "Open ignores the selection").
        FileActionMenuEntry::Open => {
            let Some(source) = named_entry_source(state, side, &target_name) else { return vec![] };
            let cwd = state.panel(side).cwd.clone();
            vec![Effect::OpenWithAssociation { side, path: source.path, cwd }]
        }
        // Run: the same suspended-shell spawn path a typed command line
        // uses, and the only way to reach it now that Enter no longer
        // spawns an executable directly (command-line: "Enter on an
        // executable opens the menu instead of spawning"). Always
        // single-target — Run has no selection-scoped meaning.
        FileActionMenuEntry::Run => {
            let Some(_) = named_entry_source(state, side, &target_name) else { return vec![] };
            let name = target_name.to_string_lossy().into_owned();
            let cwd = state.panel(side).cwd.clone();
            // Quoted so a name with spaces reaches the shell as one token.
            let text = format!("\"{name}\"");
            vec![Effect::RunShellCommand(shell::build_command(state.shell.shell.as_deref(), &text, &cwd), side)]
        }
        // View/Edit: the exact F3/F4 core paths, unchanged.
        FileActionMenuEntry::View => handle_request_viewer(state),
        FileActionMenuEntry::Edit => handle_request_editor(state),
        // Copy/Move/Delete: the existing F5/F6/F8 setup dialogs, scoped to
        // the whole selection when `selection_scoped`, else to the menu's
        // target entry only (design D3/D4) — `enter_file_op_setup_for_sources`/
        // `enter_delete_confirm_for_sources` already title their dialog with
        // `sources.len()`, so a selection-scoped invocation gets the count
        // for free.
        FileActionMenuEntry::Copy => {
            let sources = selection_or_single_source(state, side, &target_name, selection_scoped);
            if !sources.is_empty() {
                let prefill = state.panel(side.toggle()).cwd.display().to_string();
                enter_file_op_setup_for_sources(state, JobKind::Copy, sources, side, prefill, None);
            }
            vec![]
        }
        FileActionMenuEntry::Move => {
            let sources = selection_or_single_source(state, side, &target_name, selection_scoped);
            if !sources.is_empty() {
                let prefill = state.panel(side.toggle()).cwd.display().to_string();
                enter_file_op_setup_for_sources(state, JobKind::Move, sources, side, prefill, None);
            }
            vec![]
        }
        FileActionMenuEntry::Delete => {
            let sources = selection_or_single_source(state, side, &target_name, selection_scoped);
            if !sources.is_empty() {
                enter_delete_confirm_for_sources(state, sources);
            }
            vec![]
        }
        // Rename: new in-place variant (file-action-menu "In-place Rename").
        // Always single-target — an in-place rename of several entries at
        // once has no meaning, and the requirement only lists
        // Copy/Move/Delete/Send to clipboard as selection-scoped.
        FileActionMenuEntry::Rename => {
            if let Some(source) = named_entry_source(state, side, &target_name) {
                enter_rename_input(state, source.original_name, source.is_dir);
            }
            vec![]
        }
        // Send to clipboard: the `clipboard-export` Files action, scoped the
        // same way Copy/Move/Delete are above — never mutates the filesystem
        // (file-action-menu "Menu actions route to existing flows", "No
        // mutation without an intervening dialog").
        FileActionMenuEntry::SendToClipboard => {
            let sources = selection_or_single_source(state, side, &target_name, selection_scoped);
            handle_send_target_to_clipboard(sources)
        }
    }
}

/// Copy/Move/Delete/Send to clipboard's shared scope resolution: the whole
/// selection set via the same `active_selection_sources` F5/F6/Ctrl+C
/// already use when `selection_scoped`, else `target_name` alone — falling
/// back to the single target if the selection somehow emptied out while the
/// menu was open (mouse-basics design D4; file-action-menu "Directory
/// targets and selection-scoped invocation").
pub(super) fn selection_or_single_source(state: &State, side: PanelSide, target_name: &OsStr, selection_scoped: bool) -> Vec<SourceItem> {
    if selection_scoped {
        let sources = active_selection_sources(state);
        if !sources.is_empty() {
            return sources;
        }
    }
    named_entry_source(state, side, target_name).into_iter().collect()
}

/// The panel entry on `side` named `name` as a single-item `SourceItem`,
/// ignoring the panel's cursor position and selection entirely — used only
/// by the file-action menu, which targets the entry it was opened on
/// (captured by name in `FileActionMenuState`) regardless of any cursor
/// drift from a background listing refresh or any multi-entry selection
/// (design D3; file-action-menu "Enter on a file opens the action menu":
/// "SHALL NOT consume or alter the multi-entry selection"). Returns `None`
/// if the named entry is no longer present (e.g. deleted or renamed away by
/// something else while the menu was open) or is a directory-like pseudo
/// entry, matching the existing not-found-is-a-no-op behavior of every
/// other dialog-open path.
pub(super) fn named_entry_source(state: &State, side: PanelSide, name: &OsStr) -> Option<SourceItem> {
    let panel = state.panel(side);
    let entry = panel.entries.iter().find(|e| e.name == name)?;
    if entry.kind == EntryKind::ParentDir {
        return None;
    }
    Some(SourceItem { original_name: entry.name.clone(), path: panel.cwd.join(&entry.name), is_dir: entry.is_dir_like() })
}

// ---------------------------------------------------------------------
// Mouse drag-and-drop (mouse-panel-drag)
// ---------------------------------------------------------------------

/// The items a drag beginning on `side`'s entry named `name` freezes at
/// `DragBegin` (mouse-drag "Drag lifecycle"; design D4): the panel's
/// selection set if `name` is a member of it, else `name` alone. Empty (and
/// so a no-op for the caller) if `name` no longer names a selectable entry —
/// `named_entry_source` already excludes the `..` pseudo-entry, matching
/// "the parent-directory pseudo-entry SHALL never be dragged". Unlike
/// `selection_or_single_source` (used by the file-action menu, always
/// scoped to `state.active`), this is keyed by the drag's own source `side`:
/// a press-drag never changes `state.active` before the drag completes, so
/// the source panel need not be the active one at all.
pub(super) fn drag_selection_sources(state: &State, side: PanelSide, name: &OsStr) -> Vec<SourceItem> {
    let panel = state.panel(side);
    if panel.selected.contains(name) {
        panel
            .entries
            .iter()
            .filter(|e| panel.selected.contains(&e.name))
            .map(|e| SourceItem { original_name: e.name.clone(), path: panel.cwd.join(&e.name), is_dir: e.is_dir_like() })
            .collect()
    } else {
        named_entry_source(state, side, name).into_iter().collect()
    }
}

/// The directory `target` currently names, re-resolved live against
/// whichever panel it points into — `None` if it no longer resolves to a
/// directory (mouse-drag "Robust against listing changes": "the target row
/// no longer resolves to a directory"). Never touches the filesystem: a
/// renamed-away or deleted row is indistinguishable here from one that never
/// existed, and both are simply not found.
pub(super) fn drop_target_path(state: &State, target: &DropTarget) -> Option<PathBuf> {
    match target {
        DropTarget::PanelDir(side) => Some(state.panel(*side).cwd.clone()),
        DropTarget::SubDir { side, name } => {
            let panel = state.panel(*side);
            let entry = panel.entries.iter().find(|e| &e.name == name)?;
            match entry.kind {
                // The `..` row: the panel's own parent, exactly like
                // `Command::ParentDir` resolves it.
                EntryKind::ParentDir => crate::panel::parent_path(&panel.cwd),
                EntryKind::Directory => Some(panel.cwd.join(name)),
                EntryKind::File => None,
            }
        }
        DropTarget::TreeNode { path, .. } => Some(path.clone()),
        DropTarget::Tab { side, index } => state.panel(*side).tab_dirs().get(*index).cloned(),
    }
}

/// Whether `candidate` is a valid drop target for `drag`'s frozen items
/// right now (mouse-drag "Valid drop targets"): the region it names must
/// still make sense for the panel's current display mode (Info/Quick View
/// panels, and a Tree node/subdirectory row that no longer matches the
/// panel's actual mode, are never targets), the target must still resolve
/// to a live directory ([`drop_target_path`]), and it must be neither the
/// items' own source directory nor equal to/inside a directory being
/// dragged. Shared by `DragOver` (validating the TUI's raw geometric hit
/// before it is allowed onto `state.drag.target`) and `DragDrop`
/// (re-validating at release, since a listing can change between the last
/// `DragOver` and the button-up — mouse-drag "Robust against listing
/// changes").
pub(super) fn valid_drop_target(state: &State, drag: &DragState, candidate: &DropTarget) -> bool {
    let region_ok = match candidate {
        DropTarget::PanelDir(side) => !matches!(state.panel(*side).display_mode, DisplayMode::Info | DisplayMode::QuickView),
        DropTarget::SubDir { side, .. } => matches!(state.panel(*side).display_mode, DisplayMode::Full | DisplayMode::Brief),
        DropTarget::TreeNode { side, .. } => state.panel(*side).display_mode == DisplayMode::Tree,
        // A tab always stands for its own directory, independent of
        // whichever mode its panel's *active* tab currently renders in
        // (design D7: "a tab in the strip stands for its directory").
        DropTarget::Tab { .. } => true,
    };
    if !region_ok {
        return false;
    }
    let Some(target_path) = drop_target_path(state, candidate) else { return false };
    // "A target equal to the items' own directory ... SHALL be invalid"
    // (mouse-drag "Valid drop targets").
    if target_path == drag.source_dir {
        return false;
    }
    // "... or equal to or inside a dragged directory, SHALL be invalid."
    // Only dragged directories can have descendants to protect; a dragged
    // file has none.
    drag.items.iter().filter(|item| item.is_dir).all(|item| target_path != item.path && !target_path.starts_with(&item.path))
}

pub(super) fn panels_matching(state: &State, dirs: &[&PathBuf]) -> Vec<PanelSide> {
    let mut out = Vec::new();
    for side in [PanelSide::Left, PanelSide::Right] {
        if dirs.iter().any(|d| &state.panel(side).cwd == *d) {
            out.push(side);
        }
    }
    out
}

pub(super) fn handle_file_op(state: &mut State, cmd: Command) -> Vec<Effect> {
    let mut effects = Vec::new();
    match std::mem::replace(&mut state.phase, UiPhase::Panels) {
        UiPhase::FileOpSetup(setup) => {
            state.phase = handle_file_op_setup(setup, cmd, &mut effects);
        }
        UiPhase::FileOpRunning { source_dir, dest_dir, dialog } => {
            state.phase = handle_file_op_running(state, source_dir, dest_dir, dialog, cmd, &mut effects);
        }
        UiPhase::FileOpSummary(skipped) => {
            state.phase = match cmd {
                Command::FileOpConfirm | Command::FileOpCancel => UiPhase::Panels,
                _ => UiPhase::FileOpSummary(skipped),
            };
        }
        // A Job* event arrived outside FileOpRunning. Purely defensive: a
        // resize can no longer bring this about (the running phase survives
        // the placeholder — design D1), and the single-active-job design
        // rules out a stray event otherwise; drop it rather than lose the
        // phase we were actually in.
        other => state.phase = other,
    }
    effects
}

/// Shared by `Command::FileOpConfirm` (Enter — confirms with whatever `kind`
/// the dialog opened with) and `Command::FileOpConfirmAs` (the drop
/// dialog's `[ Copy ]`/`[ Move ]` buttons — confirms with an explicit `kind`
/// override instead): an empty/whitespace destination re-shows the same
/// dialog unchanged, exactly as before this was split out; otherwise builds
/// and dispatches the job. `buttons` and `hidden_by_filter` only ever ride
/// along unchanged — neither has any bearing on which job runs.
///
/// The entered text resolves against the source panel's directory through
/// the same rules `cd` uses (`resolve_cd_target`), never against the
/// process's launch or per-drive current directory (operation-dialogs
/// "Relative destinations resolve against the source panel directory";
/// design D1/D3). A destination that resolves to nothing (`..` at a drive
/// or UNC root) re-shows the dialog with its text intact, exactly like an
/// empty field.
pub(super) fn confirm_destination_input(
    kind: JobKind,
    sources: Vec<SourceItem>,
    source_dir: PathBuf,
    input: LineEdit,
    buttons: Option<DropButtons>,
    hidden_by_filter: usize,
    effects: &mut Vec<Effect>,
) -> UiPhase {
    let trimmed = input.text().trim();
    if trimmed.is_empty() {
        return UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, source_dir, input, buttons, hidden_by_filter });
    }
    let Some(target) = resolve_cd_target(&source_dir, trimmed) else {
        return UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, source_dir, input, buttons, hidden_by_filter });
    };
    let job = match kind {
        // F7 takes a *name*, but the field is free-form, so it resolves the
        // same way: the job then carries the resolved parent directory and
        // the bare new name — a plain name resolves to `source_dir` + that
        // name, exactly the job this arm has always built — and a panel
        // showing that parent re-reads on completion. A result with no
        // parent or file name (a drive or UNC root) names nothing to
        // create, so the dialog stays open, like an empty field.
        JobKind::Mkdir => {
            let (Some(parent), Some(name)) = (target.parent(), target.file_name()) else {
                return UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, source_dir, input, buttons, hidden_by_filter });
            };
            Job { kind, sources: vec![], source_dir: source_dir.clone(), dest_dir: parent.to_path_buf(), new_dir_name: Some(name.to_os_string()) }
        }
        _ => Job { kind, sources, source_dir: source_dir.clone(), dest_dir: target, new_dir_name: None },
    };
    let running = UiPhase::FileOpRunning {
        source_dir: job.source_dir.clone(),
        dest_dir: job.dest_dir.clone(),
        dialog: RunningDialog::Progress { kind: job.kind, progress: ProgressInfo::starting(0, 0) },
    };
    effects.push(Effect::RunJob(job));
    running
}

pub(super) fn handle_file_op_setup(setup: FileOpSetup, cmd: Command, effects: &mut Vec<Effect>) -> UiPhase {
    match setup {
        FileOpSetup::DestinationInput { kind, sources, source_dir, mut input, buttons, hidden_by_filter } => match cmd {
            Command::TextEdit(op) => {
                input.apply(&op);
                UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, source_dir, input, buttons, hidden_by_filter })
            }
            Command::RequestPaste => {
                effects.push(Effect::ReadClipboardText);
                UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, source_dir, input, buttons, hidden_by_filter })
            }
            Command::FileOpCancel => UiPhase::Panels,
            // Enter: confirm with whatever `kind` the dialog opened with —
            // the drop dialog's initially proposed verb, or the plain
            // keyboard dialog's fixed Copy/Move/Mkdir.
            Command::FileOpConfirm => confirm_destination_input(kind, sources, source_dir, input, buttons, hidden_by_filter, effects),
            // The drop dialog's `[ Copy ]`/`[ Move ]` buttons: confirm with
            // an explicit verb instead, overriding `kind` (operation-dialogs
            // "Switching the verb in the dialog").
            Command::FileOpConfirmAs(explicit_kind) => {
                confirm_destination_input(explicit_kind, sources, source_dir, input, buttons, hidden_by_filter, effects)
            }
            _ => UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, source_dir, input, buttons, hidden_by_filter }),
        },
        FileOpSetup::DeleteConfirm { sources, source_dir, needs_second_confirm, confirmed_once, hidden_by_filter } => match cmd {
            Command::FileOpCancel => UiPhase::Panels,
            Command::FileOpConfirm => {
                if needs_second_confirm && !confirmed_once {
                    UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm {
                        sources,
                        source_dir,
                        needs_second_confirm,
                        confirmed_once: true,
                        hidden_by_filter,
                    })
                } else {
                    let job =
                        Job { kind: JobKind::Delete, sources, source_dir: source_dir.clone(), dest_dir: source_dir.clone(), new_dir_name: None };
                    let running = UiPhase::FileOpRunning {
                        source_dir: job.source_dir.clone(),
                        dest_dir: job.dest_dir.clone(),
                        dialog: RunningDialog::Progress { kind: JobKind::Delete, progress: ProgressInfo::starting(0, 0) },
                    };
                    effects.push(Effect::RunJob(job));
                    running
                }
            }
            _ => UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { sources, source_dir, needs_second_confirm, confirmed_once, hidden_by_filter }),
        },
        FileOpSetup::RenameInput { source_dir, original_name, is_dir, mut input } => match cmd {
            Command::TextEdit(op) => {
                input.apply(&op);
                UiPhase::FileOpSetup(FileOpSetup::RenameInput { source_dir, original_name, is_dir, input })
            }
            Command::RequestPaste => {
                effects.push(Effect::ReadClipboardText);
                UiPhase::FileOpSetup(FileOpSetup::RenameInput { source_dir, original_name, is_dir, input })
            }
            Command::FileOpCancel => UiPhase::Panels,
            Command::FileOpConfirm => {
                let trimmed = input.text().trim();
                if trimmed.is_empty() {
                    return UiPhase::FileOpSetup(FileOpSetup::RenameInput { source_dir, original_name, is_dir, input });
                }
                // The name goes through the same resolver as every other
                // destination (design D3): a plain name resolves to
                // `source_dir` + that name, yielding exactly the job this
                // arm has always built, while a drive-relative or
                // `..`-carrying name no longer escapes onto the process's
                // per-drive/launch directory. A name that resolves to
                // nothing, or to something with no final component (a drive
                // or UNC root), keeps the dialog open, like an empty field.
                let Some(target) = resolve_cd_target(&source_dir, trimmed) else {
                    return UiPhase::FileOpSetup(FileOpSetup::RenameInput { source_dir, original_name, is_dir, input });
                };
                let (Some(parent), Some(name)) = (target.parent(), target.file_name()) else {
                    return UiPhase::FileOpSetup(FileOpSetup::RenameInput { source_dir, original_name, is_dir, input });
                };
                let source = SourceItem { original_name: original_name.clone(), path: source_dir.join(&original_name), is_dir };
                let job = Job {
                    kind: JobKind::Rename,
                    sources: vec![source],
                    source_dir: source_dir.clone(),
                    dest_dir: parent.to_path_buf(),
                    new_dir_name: Some(name.to_os_string()),
                };
                let running = UiPhase::FileOpRunning {
                    source_dir: job.source_dir.clone(),
                    dest_dir: job.dest_dir.clone(),
                    dialog: RunningDialog::Progress { kind: JobKind::Rename, progress: ProgressInfo::starting(0, 0) },
                };
                effects.push(Effect::RunJob(job));
                running
            }
            _ => UiPhase::FileOpSetup(FileOpSetup::RenameInput { source_dir, original_name, is_dir, input }),
        },
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_file_op_running(
    state: &mut State,
    source_dir: PathBuf,
    dest_dir: PathBuf,
    dialog: RunningDialog,
    cmd: Command,
    effects: &mut Vec<Effect>,
) -> UiPhase {
    match cmd {
        Command::FileOpCancelJob => {
            effects.push(Effect::CancelJob);
            UiPhase::FileOpRunning { source_dir, dest_dir, dialog }
        }
        Command::FileOpConflictChoice(choice) => match dialog {
            RunningDialog::Conflict { kind, progress, .. } => {
                effects.push(Effect::SendConflictReply(choice));
                UiPhase::FileOpRunning { source_dir, dest_dir, dialog: RunningDialog::Progress { kind, progress } }
            }
            other => UiPhase::FileOpRunning { source_dir, dest_dir, dialog: other },
        },
        Command::FileOpBeginRename => match dialog {
            RunningDialog::Conflict { kind, info, progress, .. } => UiPhase::FileOpRunning {
                source_dir,
                dest_dir,
                dialog: RunningDialog::Conflict { kind, info, progress, rename_input: Some(LineEdit::default()) },
            },
            other => UiPhase::FileOpRunning { source_dir, dest_dir, dialog: other },
        },
        Command::TextEdit(op) => match dialog {
            RunningDialog::Conflict { kind, info, progress, rename_input: Some(mut edit) } => {
                edit.apply(&op);
                UiPhase::FileOpRunning { source_dir, dest_dir, dialog: RunningDialog::Conflict { kind, info, progress, rename_input: Some(edit) } }
            }
            other => UiPhase::FileOpRunning { source_dir, dest_dir, dialog: other },
        },
        Command::RequestPaste => match dialog {
            RunningDialog::Conflict { kind, info, progress, rename_input: Some(edit) } => {
                effects.push(Effect::ReadClipboardText);
                UiPhase::FileOpRunning { source_dir, dest_dir, dialog: RunningDialog::Conflict { kind, info, progress, rename_input: Some(edit) } }
            }
            other => UiPhase::FileOpRunning { source_dir, dest_dir, dialog: other },
        },
        Command::FileOpConfirm => match dialog {
            RunningDialog::Conflict { kind, progress, rename_input: Some(name), .. } if !name.text().trim().is_empty() => {
                effects.push(Effect::SendConflictReply(ConflictChoice::Rename(OsString::from(name.text()))));
                UiPhase::FileOpRunning { source_dir, dest_dir, dialog: RunningDialog::Progress { kind, progress } }
            }
            other => UiPhase::FileOpRunning { source_dir, dest_dir, dialog: other },
        },
        Command::FileOpCancel => match dialog {
            RunningDialog::Conflict { kind, info, progress, rename_input: Some(_) } => {
                UiPhase::FileOpRunning { source_dir, dest_dir, dialog: RunningDialog::Conflict { kind, info, progress, rename_input: None } }
            }
            other => UiPhase::FileOpRunning { source_dir, dest_dir, dialog: other },
        },
        Command::FileOpErrorChoice(choice) => match dialog {
            RunningDialog::Error { kind, progress, .. } => {
                effects.push(Effect::SendErrorReply(choice));
                UiPhase::FileOpRunning { source_dir, dest_dir, dialog: RunningDialog::Progress { kind, progress } }
            }
            other => UiPhase::FileOpRunning { source_dir, dest_dir, dialog: other },
        },
        Command::JobProgress(info) => match dialog {
            RunningDialog::Progress { kind, .. } => {
                UiPhase::FileOpRunning { source_dir, dest_dir, dialog: RunningDialog::Progress { kind, progress: info } }
            }
            other => UiPhase::FileOpRunning { source_dir, dest_dir, dialog: other },
        },
        Command::JobConflict(info) => {
            let kind = dialog.kind();
            let progress = dialog.progress().clone();
            UiPhase::FileOpRunning { source_dir, dest_dir, dialog: RunningDialog::Conflict { kind, info, progress, rename_input: None } }
        }
        Command::JobError(info) => {
            let kind = dialog.kind();
            let progress = dialog.progress().clone();
            UiPhase::FileOpRunning { source_dir, dest_dir, dialog: RunningDialog::Error { kind, info, progress } }
        }
        Command::JobDone { outcome, .. } => {
            let skipped = match outcome {
                JobOutcome::Completed { skipped } => skipped,
                JobOutcome::Cancelled { skipped } => skipped,
            };
            for side in panels_matching(state, &[&source_dir, &dest_dir]) {
                let path = state.panel(side).cwd.clone();
                effects.extend(begin_listing(state, side, path));
            }
            // Background tabs (not currently displayed on either side)
            // don't get the immediate re-read above — their cached listing
            // would otherwise go silently stale. Mark any whose directory
            // was touched by this job so activating them later (Alt+`n`,
            // or the neighbor a Ctrl+W close falls back to) triggers a
            // fresh read instead (file-operations "Automatic panel re-read
            // on completion"; panel-tabs "Stale background tab refresh on
            // activation"). `dest_dir == source_dir` for Delete/Rename, so
            // this is a harmless duplicate match in that case, not a bug.
            for side in [PanelSide::Left, PanelSide::Right] {
                let panel = state.panel_mut(side);
                panel.mark_background_tabs_stale(&source_dir);
                panel.mark_background_tabs_stale(&dest_dir);
            }
            if skipped.is_empty() {
                UiPhase::Panels
            } else {
                UiPhase::FileOpSummary(skipped)
            }
        }
        _ => UiPhase::FileOpRunning { source_dir, dest_dir, dialog },
    }
}
