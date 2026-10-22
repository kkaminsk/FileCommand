//! The `Panels` arm of the reducer: the command line and its builtins, panel
//! navigation, tree mode, tabs, the split, quick search, quick filter, swap
//! panels, and mouse clicks on the panels.

use super::*;

// ---------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------

/// Walk `history` by `delta` (-1 = older, +1 = newer) into the buffer.
///
/// Stepping past the newest entry stops recalling but deliberately leaves
/// the buffer as it is: backspacing the buffer to empty, not Down, is the
/// documented way to release Up/Down back to the panel cursor (Esc requests
/// quit instead — command-line "Command history navigation").
pub(super) fn recall_history(state: &mut State, delta: isize) {
    if state.history.is_empty() {
        return;
    }
    let last = state.history.len() - 1;
    let next = match (state.history_cursor, delta) {
        (None, d) if d < 0 => Some(last),
        (None, _) => None,
        (Some(i), d) if d < 0 => Some(i.saturating_sub(1)),
        (Some(i), _) if i < last => Some(i + 1),
        (Some(_), _) => None,
    };
    match next {
        Some(i) => {
            // History recall replaces the whole line and leaves the caret at
            // its end (command-line "Paste filename and path to command
            // line" — caret at end; design D3).
            state.command_line.set_text_caret_end(state.history[i].clone());
            state.history_cursor = Some(i);
        }
        None => state.history_cursor = None,
    }
}

/// Ctrl+Enter / Ctrl+]: append the cursor entry's name or full path,
/// space-separating it from whatever is already typed, and leave the caret
/// at the end (command-line "Paste filename and path to command line":
/// "Paste after caret editing lands at the end").
pub(super) fn paste_cursor_entry(state: &mut State, full_path: bool) {
    let panel = state.active_panel();
    let Some(entry) = panel.selected() else { return };
    let text = if full_path { panel.cwd.join(&entry.name).display().to_string() } else { entry.name.to_string_lossy().into_owned() };
    state.command_line.end();
    if !state.command_line.is_empty() && !state.command_line.text().ends_with(' ') {
        state.command_line.insert(' ');
    }
    state.command_line.insert_str(&text);
    state.history_cursor = None;
}

/// Run the typed line: only the three built-in verbs (`cd`, `del`,
/// `rmdir`) are recognized (command-line "Command-line builtin
/// whitelist") — anything else is rejected without spawning anything. The
/// file-action menu's Run entry and the F2 user menu build their own
/// `Effect::RunShellCommand` independently and are unaffected by this
/// dispatch (design.md "`Effect::RunShellCommand` and `shell::
/// build_command` are untouched").
pub(super) fn run_command_line(state: &mut State) -> Vec<Effect> {
    let text = state.command_line.text().trim().to_string();
    if text.is_empty() {
        return vec![];
    }
    state.command_line.clear();
    state.history_cursor = None;

    // `cd` cannot work through the shell — each command runs in a fresh
    // child, so its working directory dies with it. NC navigates the panel
    // instead, which is also how a UNC path is entered by hand.
    if let Some(target) = parse_cd(&text) {
        return dispatch_cd(state, &text, &target);
    }
    if let Some(target) = parse_del(&text) {
        return dispatch_delete_builtin(state, &target, false);
    }
    if let Some(target) = parse_rmdir(&text) {
        return dispatch_delete_builtin(state, &target, true);
    }

    // Unrecognized: reject outright — no process spawns, and (matching
    // `cd`'s own rejection below) nothing is added to command history
    // (command-line "An unrecognized command is rejected without spawning
    // anything").
    let side = state.active;
    let word = text.split_whitespace().next().unwrap_or(text.as_str());
    state.panel_mut(side).last_error = Some(format!("'{word}' is not a recognized command"));
    vec![]
}

/// `cd <path>`: navigate the active panel to `target` resolved against its
/// cwd, but only after confirming the resolved path exists and is a
/// directory — a nonexistent or non-directory target is rejected outright,
/// leaving `cwd` untouched and starting no listing (command-line "cd
/// navigates the active panel or rejects a nonexistent target"; this is
/// also what fixes the panel switching into a nonexistent directory).
pub(super) fn dispatch_cd(state: &mut State, text: &str, target: &str) -> Vec<Effect> {
    let side = state.active;
    let Some(path) = resolve_cd_target(&state.panel(side).cwd, target) else {
        // ".." at a drive root, the only case `resolve_cd_target` itself
        // rejects — there is nowhere to navigate to.
        state.panel_mut(side).last_error = Some(format!("{target} not found"));
        return vec![];
    };
    match probe_command_line_target(state, side, &path) {
        // Listed in the panel: the listing answers at once (command-line "A
        // listed subdirectory needs no filesystem check").
        Some(ProbeKind::Dir) => complete_cd(state, side, text, path),
        Some(_) => {
            state.panel_mut(side).last_error = Some(format!("{} is not a directory", path.display()));
            vec![]
        }
        None => start_probe(state, side, path, ProbeIntent::Cd { text: text.to_string() }),
    }
}

/// The `cd` outcome once the target is known to be a directory: push the
/// line to command history and navigate. Shared by the listed-entry fast
/// path and the asynchronous probe reply so both produce exactly the same
/// state (offload-blocking-io design D1).
pub(super) fn complete_cd(state: &mut State, side: PanelSide, text: &str, path: PathBuf) -> Vec<Effect> {
    config::push_history(&mut state.history, text);
    // `begin_listing` snapshots both histories into one
    // `Effect::PersistHistory` after the push, so no separate persist is
    // emitted here (history-persistence-hardening D4).
    begin_listing(state, side, path)
}

/// Record a pending command-line target check and ask the TUI to run it on
/// a worker thread (design D1). A newer `cd`/`del`/`rmdir` simply replaces
/// the pending entry, which supersedes the older request: its reply no
/// longer matches and is dropped. The panel announces the check on its
/// mini-status line meanwhile (command-line "cd to an unresponsive UNC path
/// does not freeze the UI").
pub(super) fn start_probe(state: &mut State, side: PanelSide, path: PathBuf, intent: ProbeIntent) -> Vec<Effect> {
    let request = state.next_request_id();
    let cwd = state.panel(side).cwd.clone();
    state.panel_mut(side).set_pending_note(format!("Checking {}…", display_target(&path)));
    state.command_probe = Some(PendingProbe { request, side, cwd, path: path.clone(), intent });
    vec![Effect::ProbePath { request, path }]
}

/// `path` as the pending note shows it: a UNC share root resolves with a
/// trailing separator (`\\nas\share\`), which the note drops so it reads
/// `Checking \\nas\share…`; a drive root (`C:\`) keeps its separator.
pub(super) fn display_target(path: &Path) -> String {
    let shown = path.display().to_string();
    match shown.strip_suffix(['\\', '/']) {
        Some(trimmed) if !trimmed.is_empty() && !trimmed.ends_with(':') => trimmed.to_string(),
        _ => shown,
    }
}

/// `Command::PathProbed`: complete the `cd`/`del`/`rmdir` whose check this
/// answers, through the very same code paths and messages the listed-entry
/// fast path uses, or discard it (design D1). A reply is applied only when
/// it is still the most recent check, the panel the command was entered for
/// still shows the directory it showed then, and — for `del`/`rmdir`, which
/// open a dialog — the panels still own input. The pending entry and the
/// panel's note are cleared in every case but one: a reply for a request
/// that is not the pending one belongs to a superseded check, and the
/// newer, still-outstanding one keeps its note.
pub(super) fn apply_path_probed(state: &mut State, request: u64, kind: ProbeKind) -> Vec<Effect> {
    if state.command_probe.as_ref().map(|p| p.request) != Some(request) {
        return vec![];
    }
    let Some(pending) = state.command_probe.take() else { return vec![] };
    let PendingProbe { side, cwd, path, intent, .. } = pending;
    state.panel_mut(side).pending_note = None;
    if state.panel(side).cwd != cwd {
        return vec![];
    }
    match intent {
        ProbeIntent::Cd { text } => match kind {
            ProbeKind::Dir => complete_cd(state, side, &text, path),
            ProbeKind::File => {
                state.panel_mut(side).last_error = Some(format!("{} is not a directory", path.display()));
                vec![]
            }
            ProbeKind::Missing => {
                state.panel_mut(side).last_error = Some(format!("{} not found", path.display()));
                vec![]
            }
        },
        ProbeIntent::Delete { want_dir } => {
            if !panels_own_input(state) {
                return vec![];
            }
            complete_delete_builtin(state, side, path, want_dir, kind)
        }
    }
}

/// Whether the panels own input: `UiPhase::Panels` with no menu, dialog or
/// overlay open and no file operation in progress — the set `drag_allowed`
/// checks, minus `undersized`: a terminal below the floor hides the panels
/// but is not "leaving" them, so a reply that arrives then still applies
/// (resize-preserves-session-state). Gates the replies that would open a
/// dialog or a full-screen surface over whatever the user has since opened
/// (offload-blocking-io design D1/D4).
pub(super) fn panels_own_input(state: &State) -> bool {
    matches!(state.phase, UiPhase::Panels) && state.overlay.is_none() && !state.quit_confirm
}

/// `del <target>` (`want_dir == false`) / `rmdir <target>`
/// (`want_dir == true`): resolve `target` against the active panel's cwd
/// and, for an existing target of the matching type, open the same F8
/// delete-confirmation dialog `enter_delete_confirm` uses — never deleting
/// directly. `.` and `..` are never valid targets and are rejected before
/// any lookup, so the dialog can never offer the panel's own or its parent
/// directory. A missing target or a type mismatch (`del` on a directory,
/// `rmdir` on a file) is rejected: no dialog opens (command-line "del and
/// rmdir route into the existing delete-confirmation flow").
pub(super) fn dispatch_delete_builtin(state: &mut State, target: &str, want_dir: bool) -> Vec<Effect> {
    let side = state.active;
    // Reject `.`/`..` up front: `components()` normalizes a trailing
    // separator away (`..\` yields just `ParentDir`), so one check covers
    // every spelling, while a multi-component target like `..\sibling` is
    // untouched — its first component is not the whole path. Neither name
    // ever denotes a deletable entry, so no listed-entry lookup or
    // filesystem check is spent on them.
    let mut components = Path::new(target).components();
    if matches!(components.next(), Some(Component::CurDir | Component::ParentDir)) && components.next().is_none() {
        let verb = if want_dir { "rmdir" } else { "del" };
        state.panel_mut(side).last_error = Some(format!("{verb}: invalid target {target}"));
        return vec![];
    }
    let Some(path) = resolve_cd_target(&state.panel(side).cwd, target) else {
        state.panel_mut(side).last_error = Some(format!("{target} not found"));
        return vec![];
    };
    match probe_command_line_target(state, side, &path) {
        Some(kind) => complete_delete_builtin(state, side, path, want_dir, kind),
        None => start_probe(state, side, path, ProbeIntent::Delete { want_dir }),
    }
}

/// The `del`/`rmdir` outcome once the target's kind is known: open the
/// delete-confirmation dialog on a type match, otherwise set the matching
/// rejection. Shared by the listed-entry fast path and the asynchronous
/// probe reply (offload-blocking-io design D1). The dialog's entry name is
/// the target's final path component — for a listed entry that is the
/// on-disk-cased name the listing holds, since the resolved path was built
/// from the typed text joined onto the panel directory and the lookup
/// matched it exactly.
pub(super) fn complete_delete_builtin(state: &mut State, side: PanelSide, path: PathBuf, want_dir: bool, kind: ProbeKind) -> Vec<Effect> {
    match kind {
        ProbeKind::Dir | ProbeKind::File if (kind == ProbeKind::Dir) == want_dir => {
            let is_dir = kind == ProbeKind::Dir;
            let original_name = listed_name(state, side, &path)
                .unwrap_or_else(|| path.file_name().map(OsString::from).unwrap_or_else(|| OsString::from(path.display().to_string())));
            enter_delete_confirm_for_sources(state, vec![SourceItem { original_name, path, is_dir }]);
            vec![]
        }
        ProbeKind::Dir => {
            state.panel_mut(side).last_error = Some(format!("{} is a directory", path.display()));
            vec![]
        }
        ProbeKind::File => {
            state.panel_mut(side).last_error = Some(format!("{} is not a directory", path.display()));
            vec![]
        }
        ProbeKind::Missing => {
            state.panel_mut(side).last_error = Some(format!("{} not found", path.display()));
            vec![]
        }
    }
}

/// The panel's listed entry for `path`, when `path` names a direct child of
/// the panel's directory that is currently listed — the only lookup the
/// reducer performs, with no filesystem access (application-shell "The
/// reducer performs no filesystem I/O").
pub(super) fn listed_entry<'a>(state: &'a State, side: PanelSide, path: &Path) -> Option<&'a Entry> {
    let panel = state.panel(side);
    let (name, parent) = (path.file_name()?, path.parent()?);
    if parent != panel.cwd.as_path() {
        return None;
    }
    panel.entries.iter().find(|e| e.name == name)
}

/// The exact on-disk-cased name of the listed entry `path` names, if any.
pub(super) fn listed_name(state: &State, side: PanelSide, path: &Path) -> Option<OsString> {
    listed_entry(state, side, path).map(|e| e.name.clone())
}

/// What the active panel's own listing says `path` is — `Dir` or `File`
/// when `path` names a currently listed direct child of the panel's
/// directory, and `None` when the listing cannot answer, in which case the
/// caller asks the TUI to probe the filesystem on a worker thread
/// (`start_probe`). This is the listed-entry fast path and the only check
/// the reducer makes itself: it never touches the filesystem
/// (application-shell "The reducer performs no filesystem I/O";
/// offload-blocking-io design D1). Shared by `cd`'s existence check and
/// `del`/`rmdir`'s target resolution.
pub(super) fn probe_command_line_target(state: &State, side: PanelSide, path: &Path) -> Option<ProbeKind> {
    listed_entry(state, side, path).map(|entry| if entry.is_dir_like() { ProbeKind::Dir } else { ProbeKind::File })
}

/// The target of a `cd <path>` line, or `None` if this isn't a `cd`.
pub fn parse_cd(text: &str) -> Option<String> {
    parse_builtin_arg(text, "cd")
}

/// The target of a `del <target>` line, or `None` if this isn't a `del`.
pub fn parse_del(text: &str) -> Option<String> {
    parse_builtin_arg(text, "del")
}

/// The target of a `rmdir <target>` line, or `None` if this isn't a
/// `rmdir`.
pub fn parse_rmdir(text: &str) -> Option<String> {
    parse_builtin_arg(text, "rmdir")
}

/// Strip a case-insensitive `"<verb> "` prefix (`cd`/`CD`/`Cd`/...,
/// matching classic NC/cmd usage — command-line "Builtin verbs are
/// case-insensitive") and return the trimmed, unquoted argument, or `None`
/// if `text` isn't `verb` or carries no argument.
pub(super) fn parse_builtin_arg(text: &str, verb: &str) -> Option<String> {
    let trimmed = text.trim();
    // `get` (rather than `split_at`) never panics on a non-ASCII `trimmed`
    // whose byte offset `verb.len()` doesn't land on a char boundary — it
    // just reports no match, which is the correct outcome either way.
    let head = trimmed.get(..verb.len())?;
    if !head.eq_ignore_ascii_case(verb) {
        return None;
    }
    let rest = &trimmed[verb.len()..];
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let target = rest.trim().trim_matches('"');
    if target.is_empty() {
        None
    } else {
        Some(target.to_string())
    }
}

/// Resolve a `cd` target against `cwd`; this one resolver also backs the
/// F5/F6 destination field and the F7/Rename name (operation-dialogs
/// "Relative destinations resolve against the source panel directory"), so
/// every free-form path the app accepts follows the same rules. The result
/// is absolute and lexically normalized — no `.`/`..` components — whenever
/// `cwd` is (command-line "cd, del and rmdir targets resolve to a
/// normalized absolute path"). `.` and `..`, bare drive letters (`D:`),
/// UNC paths, absolute paths, and paths relative to `cwd` keep their
/// meaning; a drive-relative `D:x` (Windows only) is taken relative to
/// `cwd` when `cwd` is on drive `D:` and relative to `D:\` otherwise, so no
/// result ever depends on the process's launch or per-drive current
/// directory. `..` at a drive or UNC root has nowhere to go: `None`.
pub fn resolve_cd_target(cwd: &Path, target: &str) -> Option<PathBuf> {
    let resolved = resolve_cd_target_unnormalized(cwd, target)?;
    Some(crate::fs_ops::path::lexically_normalize(&resolved))
}

/// The raw resolution behind [`resolve_cd_target`], before the lexical
/// normalization every `Some` result then goes through.
pub(super) fn resolve_cd_target_unnormalized(cwd: &Path, target: &str) -> Option<PathBuf> {
    if target == ".." {
        return crate::panel::parent_path(cwd);
    }
    if target == "." {
        return Some(cwd.to_path_buf());
    }
    let path = Path::new(target);
    if drives::is_unc_path(path) || path.is_absolute() {
        return Some(path.to_path_buf());
    }
    // `D:` on its own means that drive's root, not a relative path.
    let mut chars = target.chars();
    if let (Some(letter), Some(':'), None) = (chars.next(), chars.next(), chars.next()) {
        if letter.is_ascii_alphabetic() {
            return Some(drives::drive_root(letter));
        }
    }
    // `D:x` (a drive letter, a colon, then a non-separator) is a
    // drive-relative path on Windows. FileCommand has no per-drive current
    // directory, so `cwd` stands in for drive `D:`'s when it is on that
    // drive; otherwise the drive's root. On other platforms `D:x` is an
    // ordinary file name, relative to `cwd` like anything else.
    if let Some((letter, rest)) = parse_drive_relative(target) {
        let base = match drives::drive_letter_of(cwd) {
            Some(cwd_letter) if cwd_letter.eq_ignore_ascii_case(&letter) => cwd.to_path_buf(),
            _ => drives::drive_root(letter),
        };
        return Some(base.join(rest));
    }
    Some(cwd.join(target))
}

/// The drive letter and remainder of a drive-relative `X:rest` target
/// (Windows path semantics): an ASCII letter, a colon, then at least one
/// character that is not a path separator. `X:` alone (a bare drive letter)
/// and `X:\…`/`X:/…` (absolute) are both handled before this is consulted.
/// Non-Windows hosts have no drive-relative paths: always `None`, so `D:x`
/// stays an ordinary file name there.
#[cfg(windows)]
pub(super) fn parse_drive_relative(target: &str) -> Option<(char, &str)> {
    let mut chars = target.chars();
    let (Some(letter), Some(':')) = (chars.next(), chars.next()) else {
        return None;
    };
    if !letter.is_ascii_alphabetic() {
        return None;
    }
    let rest = chars.as_str();
    if rest.is_empty() || rest.starts_with(['\\', '/']) {
        return None;
    }
    Some((letter, rest))
}

/// Non-Windows hosts have no drive-relative paths (see above).
#[cfg(not(windows))]
pub(super) fn parse_drive_relative(_target: &str) -> Option<(char, &str)> {
    None
}

/// Move the cursor to the first entry matching the type-ahead `pattern`
/// (`quicksearch::type_ahead_match`). A pattern that matches nothing leaves
/// the cursor where it is (type-ahead-jump "Alt+letter with no matching
/// entry", "Extended pattern no longer matches"). Input routing normally
/// keeps type-ahead and the Ctrl+P quick filter mutually exclusive, but if
/// both are ever active at once the jump must still only land within
/// `visible_indices()` — otherwise the cursor could land on an entry the
/// active filter hides (quick-filter "Navigation is restricted to matching
/// entries").
pub(super) fn jump_to_prefix(state: &mut State, pattern: &str) {
    let side = state.active;
    let panel = state.panel(side);
    let found = if panel.quick_filter.is_some() {
        // The same prefix test as `type_ahead_match`, over the cached visible
        // window and without cloning an entry (streaming-listing-performance
        // design D5).
        if pattern.is_empty() {
            None
        } else {
            let needle = pattern.to_lowercase();
            panel.visible_window(0, panel.visible_len()).find(|&i| panel.entries[i].sort_key().starts_with(&needle))
        }
    } else {
        crate::quicksearch::type_ahead_match(&panel.entries, pattern)
    };
    if let Some(index) = found {
        let panel = state.panel_mut(side);
        panel.cursor = index;
        panel.cursor_user_moved = true;
    }
    reconcile_panel_viewport(state, side);
}

// ---------------------------------------------------------------------
// Adjustable panel split (panel-split)
// ---------------------------------------------------------------------

/// Ctrl+Left/Ctrl+Right: move the divider `delta_cols` columns (negative =
/// left, positive = right) via `panel_split::adjust_percent`, updating and
/// persisting `state.split_percent` only when the adjustment doesn't
/// violate either panel's minimum width — an adjustment at the limit is a
/// no-op, per panel-split "Adjustment at the limit is a no-op".
pub(super) fn adjust_split(state: &mut State, delta_cols: i32) -> Vec<Effect> {
    match panel_split::adjust_percent(state.split_percent, delta_cols, state.term_size.0) {
        Some(new_percent) => {
            state.split_percent = new_percent;
            vec![Effect::PersistPanelSplit(new_percent)]
        }
        None => vec![],
    }
}

// ---------------------------------------------------------------------
// Info mode
// ---------------------------------------------------------------------

pub(super) fn toggle_info_mode(state: &mut State, side: PanelSide) -> Vec<Effect> {
    let entering_info = state.panel(side).display_mode == DisplayMode::Full;
    if entering_info {
        let request = state.next_request_id();
        let panel = state.panel_mut(side);
        panel.display_mode = DisplayMode::Info;
        // Every value starts pending; the worker fills them in place.
        panel.info = InfoValues::default();
        panel.info_request = Some(request);
        // Same reasoning as `SetDisplayMode`/`EnterTreeMode`: Ctrl+L is a
        // second path into Info mode, and a quick filter narrowing the
        // prior Full-mode list must not linger invisibly here either
        // (quick-filter "Substring narrowing as the pattern is typed").
        panel.clear_quick_filter();
        vec![Effect::QueryInfo { panel: side, path: panel.cwd.clone(), request }]
    } else {
        let panel = state.panel_mut(side);
        panel.display_mode = DisplayMode::Full;
        panel.info_request = None;
        vec![]
    }
}

/// Enter: run the typed command line if there is one, otherwise act on the
/// entry under the cursor — descend into a directory, or open the
/// file-action menu for a file (file-action-menu "Enter on a file opens the
/// action menu"). Spawning an executable directly on Enter is gone; the
/// menu's Run entry is now the only way to reach the suspended-shell spawn
/// path (command-line: "Enter on an executable opens the menu instead of
/// spawning").
pub(super) fn handle_enter(state: &mut State) -> Vec<Effect> {
    if !state.command_line.text().trim().is_empty() {
        return run_command_line(state);
    }
    let side = state.active;
    if state.panel(side).display_mode == DisplayMode::Tree {
        return handle_tree_enter(state, side);
    }
    let Some(entry) = state.panel(side).selected() else { return vec![] };
    match entry.kind {
        EntryKind::File => {
            let name = entry.name.clone();
            let executable = shell::is_executable_name(&name.to_string_lossy(), &state.shell.pathext);
            state.open_overlay(Overlay::FileActionMenu(FileActionMenuState::new(name, executable)));
            vec![]
        }
        EntryKind::ParentDir => handle_parent(state, side),
        EntryKind::Directory => {
            let new_dir = state.panel(side).cwd.join(&entry.name);
            begin_listing(state, side, new_dir)
        }
    }
}

pub(super) fn handle_parent(state: &mut State, side: PanelSide) -> Vec<Effect> {
    match crate::panel::parent_path(&state.panel(side).cwd) {
        Some(parent) => begin_listing(state, side, parent),
        None => vec![],
    }
}

// ---------------------------------------------------------------------
// Tree display mode (M5, design D7)
// ---------------------------------------------------------------------

/// Enter Tree mode on `side`: record the panel's current display mode so
/// Enter can restore it, root the tree at the panel's drive, and kick off
/// the drive root's immediate-children read — the only I/O Tree mode
/// performs up front (additional-panel-modes "No up-front full-drive
/// scan").
pub(super) fn enter_tree_mode(state: &mut State, side: PanelSide) -> Vec<Effect> {
    let panel = state.panel(side);
    let prior_mode = panel.display_mode;
    let letter = drives::drive_letter_of(&panel.cwd).unwrap_or('C');
    let root = drives::drive_root(letter);
    let p = state.panel_mut(side);
    p.display_mode = DisplayMode::Tree;
    p.tree = Some(TreeState::new(root.clone(), prior_mode));
    // Same reasoning as `SetDisplayMode`: a quick filter narrowing the
    // prior list mode has no meaning in Tree mode and must not linger
    // invisibly (quick-filter "Substring narrowing as the pattern is
    // typed").
    p.clear_quick_filter();
    // A brand-new tree is a single root node, so this is a no-op today, but
    // it keeps the freshly entered tree's viewport state consistent with
    // every other tree-mutating path rather than relying on it happening to
    // already be zeroed.
    reconcile_panel_viewport(state, side);
    vec![Effect::ExpandTreeNode { panel: side, path: root }]
}

/// Move `side`'s tree cursor, then re-list the opposite panel at whatever
/// directory is now highlighted (additional-panel-modes "Tree mode drives
/// the opposite panel"), expanding the newly highlighted node if it hasn't
/// been already (additional-panel-modes "Children read on expand").
pub(super) fn handle_tree_cursor_move(state: &mut State, side: PanelSide, m: CursorMove) -> Vec<Effect> {
    let (target, needs_expand) = {
        let Some(tree) = state.panel_mut(side).tree.as_mut() else { return vec![] };
        tree.move_cursor(m);
        let Some(node) = tree.selected() else { return vec![] };
        (node.path.clone(), !node.expanded)
    };
    // Keep the tree cursor inside its own scrolled window (additional-
    // panel-modes "Tree mode scrolling") before the opposite panel's
    // re-listing kicks off below.
    reconcile_panel_viewport(state, side);
    let mut effects = begin_listing_inner(state, side.toggle(), target.clone(), false);
    if needs_expand {
        effects.push(Effect::ExpandTreeNode { panel: side, path: target });
    }
    effects
}

/// Enter on a tree node: leave Tree mode, restoring the panel's prior
/// display mode, and navigate this panel (not the opposite one) to the
/// highlighted directory (additional-panel-modes "Enter returns to prior
/// list mode at chosen directory").
pub(super) fn handle_tree_enter(state: &mut State, side: PanelSide) -> Vec<Effect> {
    let Some(tree) = &state.panel(side).tree else { return vec![] };
    let Some(node) = tree.selected() else { return vec![] };
    let target = node.path.clone();
    let prior_mode = tree.prior_mode;
    let p = state.panel_mut(side);
    p.tree = None;
    p.display_mode = prior_mode;
    begin_listing(state, side, target)
}

// ---------------------------------------------------------------------
// Swap panels (Ctrl+U; panel-navigation "Swap panels (Ctrl+U)"; design D3)
// ---------------------------------------------------------------------

/// Exchange the two panels' entire state — directory, tabs, listing, cursor,
/// scroll offset, selection, sort mode, filter, display mode, and git info —
/// keeping the active side and the split percentage unchanged and without
/// re-reading either directory.
///
/// Refused while either panel's listing is still streaming — a rule the
/// pulldown-menu-completion requirement keeps by spec (the mini-status
/// already shows `Reading…`), not a plumbing limitation: listing replies
/// carry a request id and `listing_request` moves with its panel, so a mid-
/// stream swap would no longer misdeliver chunks (listing-request-guard
/// design D7). Info/git replies carry request ids too, so after the swap
/// each side's pending Info-mode and git-info queries are re-issued with
/// fresh ids and any reply from before the swap is discarded as stale.
/// Ctrl+U during a mouse drag is ignored (a menu can never be open
/// mid-drag, so the guard lives here rather than in the menu path).
pub(super) fn handle_swap_panels(state: &mut State) -> Vec<Effect> {
    if state.drag.is_some() || state.left.progress.is_streaming() || state.right.progress.is_streaming() {
        return vec![];
    }
    std::mem::swap(&mut state.left, &mut state.right);
    let mut effects = Vec::new();
    for side in [PanelSide::Left, PanelSide::Right] {
        let path = state.panel(side).cwd.clone();
        // A panel in Info mode needs its figures re-gathered under a fresh
        // request id; the values it carries over still describe its own
        // (moved-with-it) directory, so they are kept while the re-query
        // resolves.
        if state.panel(side).display_mode == DisplayMode::Info {
            let request = state.next_request_id();
            state.panel_mut(side).info_request = Some(request);
            effects.push(Effect::QueryInfo { panel: side, path: path.clone(), request });
        }
        effects.extend(git_info_query_effect(state, side, path));
    }
    effects
}

/// `Command::ClickEntry`: focus `side`, and either move the cursor to the
/// named entry (`Plain`/`Shift`) or toggle its selection in place without
/// advancing the cursor from wherever it lands (`Ctrl`) — mirroring `Ins`'s
/// `toggle_selection_and_advance` except the cursor's destination is the
/// clicked entry, not "one past wherever it already was" (mouse-input
/// "Click focuses and places the cursor", "Ctrl+click toggles selection"). A
/// name the panel no longer lists (e.g. a listing that changed between the
/// hit map being built and the click landing) is a silent no-op rather than
/// a panic. The parent `..` pseudo-entry is never selectable, matching
/// `toggle_selection_and_advance`'s own guard (mouse-input "Parent entry
/// ignored").
pub(super) fn handle_click_entry(state: &mut State, side: PanelSide, name: OsString, mods: ClickMods) -> Vec<Effect> {
    state.active = side;
    let panel = state.panel_mut(side);
    let Some(idx) = panel.entries.iter().position(|e| e.name == name) else { return vec![] };
    panel.cursor = idx;
    panel.cursor_user_moved = true;
    if mods == ClickMods::Ctrl && panel.entries[idx].kind != EntryKind::ParentDir && !panel.selected.remove(&name) {
        panel.selected.insert(name);
    }
    reconcile_panel_viewport(state, side);
    vec![]
}

/// `Command::ScrollPanel`: move `side`'s cursor by `delta` rows and let the
/// existing scroll-offset clamp bring the viewport along, so the
/// cursor-in-window invariant holds exactly as it does for a keyboard
/// Up/Down (mouse-input "Wheel moves the cursor of the panel under the
/// pointer"; design D6). Tree mode has no flat `entries` list to move a
/// bare `CursorMove` over, so it reuses the same tree-cursor path the
/// keyboard's Up/Down already takes there.
pub(super) fn handle_scroll_panel(state: &mut State, side: PanelSide, delta: isize) -> Vec<Effect> {
    let m = if delta < 0 { CursorMove::Up(delta.unsigned_abs()) } else { CursorMove::Down(delta as usize) };
    if state.panel(side).display_mode == DisplayMode::Tree {
        return handle_tree_cursor_move(state, side, m);
    }
    state.panel_mut(side).move_cursor(m);
    reconcile_panel_viewport(state, side);
    vec![]
}

/// The `UiPhase::Panels` arm of `update_impl`: every command the panels,
/// the command line, and their overlays' openers handle while no phase or
/// overlay above them has claimed it. Takes `state` by value because two of
/// its arms (`KeybarPress`, `MenuItemClick`) re-enter `update_impl` with the
/// command they stand for.
pub(super) fn handle_panels(mut state: State, cmd: Command, mut effects: Vec<Effect>) -> (State, Vec<Effect>) {
    match cmd {
        Command::MoveCursor(m) => {
            // A movement key exits type-ahead (if active) *and* is
            // still applied to the panel cursor as a normal movement,
            // in the same keystroke — the `input/` mapper emits exactly
            // this one command for a movement key while type-ahead owns
            // the keyboard, relying on this side effect to also end the
            // mode (type-ahead-jump "A movement key exits type-ahead
            // and is applied to the panel"; design D5).
            state.quick_search = None;
            let side = state.active;
            if state.panel(side).display_mode == DisplayMode::Tree {
                effects.extend(handle_tree_cursor_move(&mut state, side, m));
            } else {
                state.panel_mut(side).move_cursor(m);
                reconcile_panel_viewport(&mut state, side);
            }
        }
        Command::ToggleActivePanel => state.active = state.active.toggle(),
        Command::Enter => effects.extend(handle_enter(&mut state)),
        Command::ParentDir => {
            let side = state.active;
            effects.extend(handle_parent(&mut state, side));
        }
        // `Command::RequestQuit` is handled globally, above, regardless
        // of phase — it never reaches this arm.
        Command::RereadPanel(side) => {
            let path = state.panel(side).cwd.clone();
            effects.extend(begin_listing(&mut state, side, path));
        }
        Command::ToggleSelectAtCursor => {
            let side = state.active;
            state.panel_mut(side).toggle_selection_and_advance();
            reconcile_panel_viewport(&mut state, side);
        }
        Command::GroupSelectAll => state.panel_mut(state.active).select_matching("*"),
        Command::GroupDeselectAll => state.panel_mut(state.active).deselect_matching("*"),
        Command::InvertSelection => state.panel_mut(state.active).invert_selection(),
        Command::RequestCopy => enter_file_op_setup(&mut state, JobKind::Copy),
        Command::RequestMove => enter_file_op_setup(&mut state, JobKind::Move),
        Command::RequestMkdir => enter_file_op_setup(&mut state, JobKind::Mkdir),
        Command::RequestDelete => enter_delete_confirm(&mut state),
        Command::CopyToClipboard(kind) => effects.extend(handle_copy_to_clipboard(&mut state, kind)),
        Command::ClipboardResult { payload, fell_back_to_paths, result } => apply_clipboard_result(&mut state, payload, fell_back_to_paths, result),

        Command::TextEdit(op) => {
            // The command line's own editing arm — `TextEdit` is routed
            // by phase (design D2): the dialog/overlay arms above claim
            // it first whenever one of their fields owns the keyboard,
            // so reaching the Panels arm means the command line does. A
            // text-mutating edit ends history recall, exactly as typing
            // and backspace always have; a pure caret move leaves it.
            let mutating = op.is_text_mutation();
            state.command_line.apply(&op);
            if mutating {
                state.history_cursor = None;
            }
        }
        Command::RequestPaste => effects.push(Effect::ReadClipboardText),
        Command::CommandLineClear => {
            state.command_line.clear();
            state.history_cursor = None;
        }
        Command::CommandLineHistoryPrev => recall_history(&mut state, -1),
        Command::CommandLineHistoryNext => recall_history(&mut state, 1),
        Command::PasteCursorName => paste_cursor_entry(&mut state, false),
        Command::PasteCursorPath => paste_cursor_entry(&mut state, true),
        Command::ShowScrollback => effects.push(Effect::ShowScrollback),

        Command::QuickSearchStart(c) => {
            let mut pattern = String::new();
            pattern.push(c);
            jump_to_prefix(&mut state, &pattern);
            state.quick_search = Some(pattern);
        }
        Command::QuickSearchChar(c) => {
            if let Some(mut pattern) = state.quick_search.take() {
                pattern.push(c);
                jump_to_prefix(&mut state, &pattern);
                state.quick_search = Some(pattern);
            }
        }
        Command::QuickSearchBackspace => {
            if let Some(mut pattern) = state.quick_search.take() {
                pattern.pop();
                // An empty pattern stays active rather than exiting
                // type-ahead, and the cursor holds its position rather
                // than re-jumping (type-ahead-jump "Backspace on a
                // single-character pattern").
                if !pattern.is_empty() {
                    jump_to_prefix(&mut state, &pattern);
                }
                state.quick_search = Some(pattern);
            }
        }
        Command::QuickSearchEnd => state.quick_search = None,

        Command::SetSortMode { side, mode } => {
            state.panel_mut(side).set_sort_mode(mode);
            reconcile_panel_viewport(&mut state, side);
        }

        Command::QuickFilterStart(side) => {
            // Starting the filter also makes `side` the active panel, so
            // typed characters reach the filtered panel (design D1 — the
            // Left/Right menus' Filter on an inactive panel; a visible
            // but expected focus change, as with Left → Info).
            state.active = side;
            state.panel_mut(side).activate_quick_filter();
        }
        Command::QuickFilterChar(c) => {
            let side = state.active;
            state.panel_mut(side).quick_filter_push(c);
            reconcile_panel_viewport(&mut state, side);
        }
        Command::QuickFilterBackspace => {
            let side = state.active;
            state.panel_mut(side).quick_filter_backspace();
            reconcile_panel_viewport(&mut state, side);
        }
        Command::QuickFilterEnd => {
            let side = state.active;
            state.panel_mut(side).clear_quick_filter();
            reconcile_panel_viewport(&mut state, side);
        }

        Command::OpenTab(side) => {
            // The new tab inherits the active tab's state — including
            // `listing_request`, so a listing still streaming keeps
            // filling the new tab, which shows the same directory; no
            // cancel is issued. The original tab, now in the background,
            // is stamped stale by `to_tab_data` when that listing was
            // still being read, so it re-reads on its next activation
            // instead of showing the partial listing it held (panel-tabs
            // "New tab mid-listing keeps the listing"; listing-request-
            // guard design D4/D6). Focus does not move: the Left/Right
            // menus open a tab on their own panel (design D1).
            state.panel_mut(side).open_tab();
            reconcile_panel_viewport(&mut state, side);
        }
        Command::CloseTab(side) => {
            let leaving = leaving_tab_listing(&state, side);
            let tabs_before = state.panel(side).tab_count();
            let stale = state.panel_mut(side).close_tab();
            if state.panel(side).tab_count() < tabs_before {
                // The closed tab's read, if still running, is stopped;
                // its remaining replies carry an id no tab holds any
                // more and are dropped (panel-tabs "Closing a tab mid-
                // listing"; design D4).
                effects.extend(leaving);
            }
            if stale {
                effects.extend(refresh_activated_stale_tab(&mut state, side));
            }
            reconcile_panel_viewport(&mut state, side);
        }
        Command::SwitchTab(n) => {
            let side = state.active;
            let leaving = leaving_tab_listing(&state, side);
            let active_before = state.panel(side).active_tab_index;
            let stale = state.panel_mut(side).switch_tab(n);
            if state.panel(side).active_tab_index != active_before {
                // The leaving tab's read, if still running, is stopped;
                // the tab itself was stamped stale by `to_tab_data` and
                // re-reads when next activated (panel-tabs "Switching
                // tabs mid-listing"; design D4/D6).
                effects.extend(leaving);
            }
            if stale {
                effects.extend(refresh_activated_stale_tab(&mut state, side));
            }
            reconcile_panel_viewport(&mut state, side);
        }

        Command::EnterTreeMode(side) => effects.extend(enter_tree_mode(&mut state, side)),
        Command::SetDisplayMode { side, mode } => {
            let p = state.panel_mut(side);
            p.tree = None;
            p.info_request = None;
            p.display_mode = mode;
            // A quick filter narrowed the panel's prior display mode; a
            // stale pattern must not linger invisibly into Brief/Tree/
            // Info mode, whose renderers don't surface it the same way
            // Full mode does (quick-filter "Substring narrowing as the
            // pattern is typed").
            p.clear_quick_filter();
        }

        // The overlay openers. Each runs with the slot empty — this arm is
        // reached only when no open overlay claimed the command, and an
        // overlay that leads to another closes itself first — which
        // `open_overlay` checks in debug builds (reducer-overlay-structure
        // design D5).
        Command::FuzzyJumpOpen => state.open_overlay(Overlay::FuzzyJump(FuzzyJumpState::new())),
        Command::FindFileOpen => {
            let root = state.active_panel().cwd.clone();
            state.open_overlay(Overlay::FindFile(FindFileState::new(root)));
        }
        Command::UserMenuOpen => state.open_overlay(Overlay::UserMenu(UserMenuState::new())),
        Command::HistoryDialogOpen => state.open_overlay(Overlay::HistoryDialog(HistoryDialogState::new())),
        Command::SwapPanels => effects.extend(handle_swap_panels(&mut state)),
        Command::EditUserMenuFile => effects.extend(handle_edit_user_menu_file(&mut state)),
        Command::UserMenuReloaded(result) => apply_user_menu_reload(&mut state, result),
        Command::ThemePickerOpen => {
            let picker = ThemePickerState::open(&state.theme.name);
            state.open_overlay(Overlay::ThemePicker(picker));
        }
        Command::HelpOpen => state.open_overlay(Overlay::Help(HelpState::new())),

        Command::MenuOpen => state.open_overlay(Overlay::Menu(MenuState::opened())),
        Command::OpenDriveSelect(side) => effects.push(Effect::EnumerateDrives(side)),
        // The one asynchronous opener. The reply is drained before the next
        // input is read, so no other overlay can have opened in between
        // today; should one ever have, the drive list replaces it — the same
        // winner the key router has always given drive select
        // (reducer-overlay-structure design D5).
        Command::DriveListReady { target, drives } => {
            let current = drives::drive_letter_of(&state.panel(target).cwd);
            let generation = state.next_request_id();
            for letter in &drives {
                effects.push(Effect::FetchDriveLabel { target, letter: *letter, generation });
            }
            let mut dialog = DriveSelect::new(target, drives, current);
            dialog.generation = generation;
            state.overlay = Some(Overlay::DriveSelect(dialog));
        }
        Command::ToggleInfoMode(side) => effects.extend(toggle_info_mode(&mut state, side)),

        Command::RequestViewer => effects.extend(handle_request_viewer(&mut state)),
        Command::RequestExternalEditor => effects.extend(handle_request_external_editor(&mut state)),
        Command::ExternalEditorSpawnFailed { message } => state.panel_mut(state.active).last_error = Some(message),

        Command::RequestEditor => effects.extend(handle_request_editor(&mut state)),
        // The `ViewerOpened`/`EditorOpened` family of replies is matched
        // against `State::pending_open` ahead of every phase arm (see
        // `take_current_open`), so none of them reach here.

        // file-action-menu "Open with the associated application": a
        // failed launch is reported inline, on the status line of the
        // panel the menu acted on — even if the user has switched panels
        // since — and no dialog opens (offload-blocking-io design D7).
        Command::OpenWithAssociationFailed { side, message } => state.panel_mut(side).last_error = Some(message),

        Command::SplitGrow => effects.extend(adjust_split(&mut state, panel_split::SPLIT_STEP as i32)),
        Command::SplitShrink => effects.extend(adjust_split(&mut state, -(panel_split::SPLIT_STEP as i32))),
        Command::SplitReset => {
            if state.split_percent != panel_split::DEFAULT_SPLIT_PERCENT {
                state.split_percent = panel_split::DEFAULT_SPLIT_PERCENT;
                effects.push(Effect::PersistPanelSplit(state.split_percent));
            }
        }

        // Mouse (mouse-basics; design D2).
        Command::FocusPanel(side) => state.active = side,
        Command::ClickEntry { side, name, mods } => effects.extend(handle_click_entry(&mut state, side, name, mods)),
        Command::ScrollPanel { side, delta } => effects.extend(handle_scroll_panel(&mut state, side, delta)),
        Command::KeybarPress(slot) => {
            if let Some(next) = keybar_command(slot) {
                return update_impl(state, next);
            }
        }
        // A title click with the bar closed opens that pull-down; with the
        // bar open, the menu overlay itself claims the click (and
        // `MenuItemClick`), so neither reaches here then.
        Command::MenuTitleClick(id) => state.open_overlay(Overlay::Menu(MenuState::for_menu(id))),
        Command::OpenActionMenuAt { side, name } => effects.extend(handle_open_action_menu_at(&mut state, side, name)),

        // Mouse drag-and-drop (mouse-panel-drag; design D2/D4).
        Command::DragBegin { side, name, op } => {
            let items = drag_selection_sources(&state, side, &name);
            if !items.is_empty() {
                let source_dir = state.panel(side).cwd.clone();
                state.drag = Some(DragState { source: side, source_dir, items, op, target: None });
            }
        }
        Command::DragOver { op, target } => {
            // Both borrows below are immutable — computed fully before
            // the mutable one that follows — so there's no conflict with
            // `state.drag.as_mut()` afterwards.
            let valid_target = state.drag.as_ref().and_then(|drag| target.as_ref().filter(|t| valid_drop_target(&state, drag, t)).cloned());
            if let Some(drag) = state.drag.as_mut() {
                drag.op = op;
                drag.target = valid_target;
            }
        }
        Command::DragDrop { op } => {
            if let Some(mut drag) = state.drag.take() {
                drag.op = op;
                // "Robust against listing changes": the source panel
                // must still show the directory the items were captured
                // from, and the target must still resolve to a live
                // directory (mouse-drag "Robust against listing
                // changes").
                let source_ok = state.panel(drag.source).cwd == drag.source_dir;
                let resolved = if source_ok {
                    drag.target.as_ref().filter(|t| valid_drop_target(&state, &drag, t)).and_then(|t| drop_target_path(&state, t))
                } else {
                    None
                };
                if let Some(target_path) = resolved {
                    let prefill = target_path.display().to_string();
                    enter_file_op_setup_for_sources(&mut state, drag.op, drag.items, drag.source, prefill, Some(DropButtons { focused: drag.op }));
                }
                // Otherwise: invalid/cancelled — the drag is already
                // taken (cleared), and nothing else happens (mouse-drag
                // "Release on an invalid spot").
            }
        }
        Command::DragCancel => state.drag = None,

        Command::Tick { .. } => {}
        Command::ConfirmQuit | Command::CancelQuit | Command::Resize(..) | Command::DialogButtonClick(_) => unreachable!("handled above"),
        _ => {}
    }
    (state, effects)
}
