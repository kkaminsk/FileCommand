//! The modal overlays that live beside the phase: the F9 menu, drive select,
//! fuzzy jump, find file, the F2 user menu, the Alt+F8 history dialog, the
//! theme picker, the file-action menu, Help, and the startup warning — plus
//! the menu-action, dialog-button and key-bar command tables.

use super::*;

// ---------------------------------------------------------------------
// Drive select
// ---------------------------------------------------------------------

pub(super) fn is_drive_select_command(cmd: &Command) -> bool {
    matches!(cmd, Command::DriveSelectMove(_) | Command::DriveSelectConfirm | Command::DriveSelectCancel)
}

pub(super) fn handle_drive_select(state: &mut State, cmd: Command) -> Vec<Effect> {
    match cmd {
        Command::DriveSelectMove(delta) => {
            if let Some(dialog) = &mut state.drive_select {
                dialog.move_selection(delta);
            }
            vec![]
        }
        Command::DriveSelectCancel => {
            // The target panel keeps its current directory.
            state.drive_select = None;
            vec![]
        }
        Command::DriveSelectConfirm => {
            let Some(dialog) = state.drive_select.take() else { return vec![] };
            let Some(letter) = dialog.selected_letter() else { return vec![] };
            // An unavailable drive is not special-cased here: the listing
            // read fails on the worker thread and surfaces through
            // `ListingFailed` as the panel's inline error state.
            let path = drives::drive_root(letter);
            begin_listing(state, dialog.target, path)
        }
        _ => vec![],
    }
}

// ---------------------------------------------------------------------
// F9 menu
// ---------------------------------------------------------------------

pub(super) fn is_menu_command(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::MenuOpen
            | Command::MenuClose
            | Command::MenuCollapse
            | Command::MenuSelectPrev
            | Command::MenuSelectNext
            | Command::MenuPrevMenu
            | Command::MenuNextMenu
            | Command::MenuHotkey(_)
            | Command::MenuActivate
    )
}

/// Drive the menu state machine. Returns the command an activated item
/// stands for, which the caller re-enters `update` with.
pub(super) fn handle_menu(state: &mut State, cmd: Command) -> Option<Command> {
    let active_side = state.active;
    let menu = state.menu.as_mut()?;
    match cmd {
        Command::MenuOpen | Command::MenuClose => state.menu = None,
        Command::MenuCollapse => {
            // Esc closes the pull-down first, leaving the bar open with its
            // title still highlighted; a second Esc closes the bar.
            if menu.pulldown_open {
                menu.pulldown_open = false;
            } else {
                state.menu = None;
            }
        }
        Command::MenuSelectPrev => menu.move_selection(-1),
        Command::MenuSelectNext => menu.move_selection(1),
        Command::MenuPrevMenu => {
            let target = menu.active.prev();
            menu.go_to(target);
        }
        Command::MenuNextMenu => {
            let target = menu.active.next();
            menu.go_to(target);
        }
        Command::MenuHotkey(c) => {
            if let Some(id) = MenuId::from_hotkey(c) {
                menu.go_to(id);
            }
        }
        Command::MenuActivate => {
            let item = menu.selected_item()?;
            let side = menu.active.target_side(active_side);
            let action = item.action;
            // Activating an item closes the whole overlay, restoring the top
            // row and the clock, before the action runs.
            state.menu = None;
            return menu_action_command(action, side);
        }
        _ => {}
    }
    None
}

/// The command a menu item stands for. Disabled items never reach here, so
/// `Unimplemented` maps to nothing.
pub fn menu_action_command(action: MenuAction, side: PanelSide) -> Option<Command> {
    match action {
        MenuAction::ToggleInfoMode => Some(Command::ToggleInfoMode(side)),
        MenuAction::SetDisplayMode(mode) => Some(Command::SetDisplayMode { side, mode }),
        MenuAction::EnterTree => Some(Command::EnterTreeMode(side)),
        MenuAction::SortBy(mode) => Some(Command::SetSortMode { side, mode }),
        MenuAction::QuickFilter => Some(Command::QuickFilterStart(side)),
        MenuAction::Reread => Some(Command::RereadPanel(side)),
        MenuAction::DriveSelect => Some(Command::OpenDriveSelect(side)),
        MenuAction::NewTab => Some(Command::OpenTab(side)),
        MenuAction::CloseTab => Some(Command::CloseTab(side)),
        MenuAction::View => Some(Command::RequestViewer),
        MenuAction::Edit => Some(Command::RequestEditor),
        MenuAction::Copy => Some(Command::RequestCopy),
        MenuAction::Move => Some(Command::RequestMove),
        MenuAction::Mkdir => Some(Command::RequestMkdir),
        MenuAction::Delete => Some(Command::RequestDelete),
        MenuAction::ClipboardFiles => Some(Command::CopyToClipboard(ClipboardPayloadKind::Files)),
        MenuAction::ClipboardPaths => Some(Command::CopyToClipboard(ClipboardPayloadKind::Paths)),
        MenuAction::ClipboardNames => Some(Command::CopyToClipboard(ClipboardPayloadKind::Names)),
        MenuAction::SelectGroup => Some(Command::GroupSelectAll),
        MenuAction::DeselectGroup => Some(Command::GroupDeselectAll),
        MenuAction::InvertSelection => Some(Command::InvertSelection),
        MenuAction::PanelsOnOff => Some(Command::ShowScrollback),
        MenuAction::FindFile => Some(Command::FindFileOpen),
        MenuAction::History => Some(Command::HistoryDialogOpen),
        MenuAction::SwapPanels => Some(Command::SwapPanels),
        MenuAction::FuzzyJump => Some(Command::FuzzyJumpOpen),
        MenuAction::EditMenuFile => Some(Command::EditUserMenuFile),
        MenuAction::Quit => Some(Command::RequestQuit),
        MenuAction::OpenThemes => Some(Command::ThemePickerOpen),
        MenuAction::Unimplemented => None,
    }
}

// ---------------------------------------------------------------------
// Enter-on-file action menu (file-action-menu)
// ---------------------------------------------------------------------

pub(super) fn is_file_action_menu_command(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::FileActionMenuMove(_)
            | Command::FileActionMenuConfirm
            | Command::FileActionMenuCancel
            | Command::FileActionMenuHotkey(_)
            | Command::FileActionMenuItemClick(_)
    )
}

/// Drive the Enter-on-file action menu (gated in [`update`] by
/// `state.file_action_menu`) — navigation, dismissal, and activation
/// (file-action-menu "Menu contents, ordering, and navigation").
pub(super) fn handle_file_action_menu(state: &mut State, cmd: Command) -> Vec<Effect> {
    if state.file_action_menu.is_none() {
        return vec![];
    }
    match cmd {
        Command::FileActionMenuMove(delta) => {
            state.file_action_menu.as_mut().unwrap().move_cursor(delta);
            vec![]
        }
        Command::FileActionMenuCancel => {
            state.file_action_menu = None;
            vec![]
        }
        Command::FileActionMenuConfirm => {
            let menu = state.file_action_menu.take().unwrap();
            let target_name = menu.target_name.clone();
            activate_file_action(state, menu.selected(), target_name, menu.selection_scoped)
        }
        Command::FileActionMenuItemClick(index) => {
            let menu = state.file_action_menu.as_ref().unwrap();
            if index >= menu.entries.len() {
                // Shouldn't normally happen given hit-testing (the hit map
                // only ever records rects for real entries), but a stale
                // hit map click — same reasoning as
                // `click_entry_on_a_vanished_name_is_a_no_op` — is safer
                // treated as a no-op than an out-of-bounds panic.
                return vec![];
            }
            state.file_action_menu.as_mut().unwrap().cursor = index;
            let menu = state.file_action_menu.take().unwrap();
            let target_name = menu.target_name.clone();
            activate_file_action(state, menu.selected(), target_name, menu.selection_scoped)
        }
        Command::FileActionMenuHotkey(c) => {
            let menu = state.file_action_menu.as_ref().unwrap();
            let action = menu.hotkey_action(c);
            match action {
                Some(action) => {
                    let target_name = menu.target_name.clone();
                    let selection_scoped = menu.selection_scoped;
                    state.file_action_menu = None;
                    activate_file_action(state, action, target_name, selection_scoped)
                }
                None => vec![],
            }
        }
        _ => vec![],
    }
}

// ---------------------------------------------------------------------
// Ctrl+J fuzzy jump (M5)
// ---------------------------------------------------------------------

pub(super) fn is_fuzzy_jump_command(cmd: &Command) -> bool {
    matches!(cmd, Command::TextEdit(_) | Command::RequestPaste | Command::FuzzyJumpMove(_) | Command::FuzzyJumpConfirm | Command::FuzzyJumpCancel)
}

/// Drive the fuzzy-jump dialog (gated in [`update`] by `state.fuzzy_jump`).
/// The ranked/filtered list itself is never stored on the dialog — it is
/// recomputed from `state.dir_history` each time via
/// [`quicksearch::rank_directories`], which is why every arm re-derives it
/// rather than caching (fuzzy-jump "Fuzzy matching of visited directories",
/// "Frecency ranking").
pub(super) fn handle_fuzzy_jump(state: &mut State, cmd: Command) -> Vec<Effect> {
    if state.fuzzy_jump.is_none() {
        return vec![];
    }
    match cmd {
        Command::TextEdit(op) => state.fuzzy_jump.as_mut().unwrap().apply(&op),
        Command::RequestPaste => return vec![Effect::ReadClipboardText],
        Command::FuzzyJumpMove(delta) => {
            let len = {
                let fj = state.fuzzy_jump.as_ref().unwrap();
                quicksearch::rank_directories(&state.dir_history, fj.pattern.text(), state.epoch_ms).len()
            };
            state.fuzzy_jump.as_mut().unwrap().move_cursor(delta, len);
        }
        Command::FuzzyJumpCancel => state.fuzzy_jump = None,
        Command::FuzzyJumpConfirm => {
            let fj = state.fuzzy_jump.take().unwrap();
            // Ranked with the epoch clock, the same one the renderer uses,
            // so the row Enter acts on is the row that was drawn (design D1).
            let ranked = quicksearch::rank_directories(&state.dir_history, fj.pattern.text(), state.epoch_ms);
            if let Some(path) = ranked.get(fj.cursor).map(|e| e.path.clone()) {
                let side = state.active;
                return begin_listing(state, side, path);
            }
        }
        _ => {}
    }
    vec![]
}

// ---------------------------------------------------------------------
// Alt+F7 find file (M5)
// ---------------------------------------------------------------------

pub(super) fn is_find_file_command(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::TextEdit(_)
            | Command::RequestPaste
            | Command::FindFileSubmit
            | Command::FindFileMatch { .. }
            | Command::FindFileSearchDone { .. }
            | Command::FindFileMove(_)
            | Command::FindFileConfirm
            | Command::FindFileCancel
    )
}

/// Drive the find-file dialog (gated in [`update`] by `state.find_file`).
pub(super) fn handle_find_file(state: &mut State, cmd: Command) -> Vec<Effect> {
    if state.find_file.is_none() {
        return vec![];
    }
    match cmd {
        Command::TextEdit(op) => state.find_file.as_mut().unwrap().apply(&op),
        Command::RequestPaste => return vec![Effect::ReadClipboardText],
        Command::FindFileSubmit => {
            let request = state.next_request_id();
            let ff = state.find_file.as_mut().unwrap();
            ff.submit(request);
            let root = ff.root.clone();
            let pattern = ff.pattern.text().to_string();
            return vec![Effect::FindInSubtree { root, pattern, request }];
        }
        Command::FindFileMatch { request, m } => state.find_file.as_mut().unwrap().push_match(request, m),
        Command::FindFileSearchDone { request } => state.find_file.as_mut().unwrap().mark_done(request),
        Command::FindFileMove(delta) => state.find_file.as_mut().unwrap().move_cursor(delta),
        // Dismissing abandons any in-progress search: the walk is told to
        // stop (find-file "Dismissed or superseded searches stop";
        // listing-request-guard design D4), and whatever it already posted
        // finds `state.find_file` `None` and is dropped by the
        // `state.find_file.is_some()` gate in `update` (find-file "Esc
        // cancels").
        Command::FindFileCancel => {
            let ff = state.find_file.take().unwrap();
            return running_find_file_walk(&ff).map(Effect::CancelRequest).into_iter().collect();
        }
        Command::FindFileConfirm => {
            let ff = state.find_file.take().unwrap();
            // Confirming a result while the walk is still running stops it
            // too (find-file "Confirming a result while the walk is running
            // stops it"); the navigation below proceeds either way.
            let mut effects: Vec<Effect> = running_find_file_walk(&ff).map(Effect::CancelRequest).into_iter().collect();
            let Some(m) = ff.selected().cloned() else { return effects };
            let target_dir = match m.relative_path.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => ff.root.join(parent),
                _ => ff.root.clone(),
            };
            let side = state.active;
            effects.extend(begin_listing(state, side, target_dir));
            state.panel_mut(side).pending_cursor_target = Some(m.entry.name);
            return effects;
        }
        _ => {}
    }
    vec![]
}

/// The request id of a find-file walk that was submitted and has not yet
/// reported `FindFileSearchDone` — the one `FindFileCancel`/`FindFileConfirm`
/// tell to stop (listing-request-guard design D4). `None` before Enter
/// submits a search and once the walk has finished.
pub(super) fn running_find_file_walk(ff: &FindFileState) -> Option<u64> {
    ff.request.filter(|_| !ff.done)
}

// ---------------------------------------------------------------------
// F2 user menu (M5)
// ---------------------------------------------------------------------

pub(super) fn is_user_menu_command(cmd: &Command) -> bool {
    matches!(cmd, Command::UserMenuMove(_) | Command::UserMenuConfirm | Command::UserMenuCancel)
}

/// Drive the F2 user menu (gated in [`update`] by `state.user_menu`).
/// Entries themselves live in `state.user_menu_entries`, loaded once at
/// startup — the menu overlay only tracks the cursor (user-menu "Open the
/// F2 user menu", "Navigate and dismiss the user menu"). The cursor domain
/// is `0..=state.user_menu_entries.len()`: the last index is the
/// compiled-in "Themes" slot, not a config entry (design D3).
pub(super) fn handle_user_menu(state: &mut State, cmd: Command) -> Vec<Effect> {
    if state.user_menu.is_none() {
        return vec![];
    }
    match cmd {
        Command::UserMenuMove(delta) => {
            // Cursor domain is the user entries plus one compiled-in
            // built-in slot at index `entries.len()` for "Themes" (design
            // D3) — not an entry appended to `state.user_menu_entries`.
            let len = state.user_menu_entries.len() + 1;
            state.user_menu.as_mut().unwrap().move_cursor(delta, len);
        }
        Command::UserMenuCancel => state.user_menu = None,
        Command::UserMenuConfirm => {
            let menu = state.user_menu.take().unwrap();
            if let Some(entry) = state.user_menu_entries.get(menu.cursor).cloned() {
                let side = state.active;
                let cwd = state.panel(side).cwd.clone();
                return vec![Effect::RunShellCommand(shell::build_command(state.shell.shell.as_deref(), &entry.command, &cwd), side)];
            }
            // Cursor is at the built-in Themes slot (`entries.len()`): the
            // user menu is already closed above (design D4); open the
            // theme picker via the exact same path as
            // `MenuAction::OpenThemes`, pre-highlighting the active theme.
            // No shell effect.
            state.theme_picker = Some(ThemePickerState::open(&state.theme.name));
        }
        _ => {}
    }
    vec![]
}

// ---------------------------------------------------------------------
// Alt+F8 command-history dialog (pulldown-menu-completion; design D2)
// ---------------------------------------------------------------------

pub(super) fn is_history_dialog_command(cmd: &Command) -> bool {
    matches!(cmd, Command::HistoryDialogMove(_) | Command::HistoryDialogConfirm | Command::HistoryDialogCancel)
}

/// Drive the Alt+F8 command-history dialog (gated in [`update`] by
/// `state.history_dialog`). The list renders `state.history` newest first,
/// so `cursor` counts from the end of the vector; Enter places the
/// highlighted entry on the command line (caret at the end, recall cleared)
/// and closes without running anything — the user presses Enter again to
/// run it (design D2's non-goal). On an empty history the dialog shows a
/// single `(no history)` row and Enter does nothing (the dialog stays open
/// until Esc).
pub(super) fn handle_history_dialog(state: &mut State, cmd: Command) -> Vec<Effect> {
    if state.history_dialog.is_none() {
        return vec![];
    }
    match cmd {
        Command::HistoryDialogMove(delta) => {
            let len = state.history.len();
            state.history_dialog.as_mut().unwrap().move_cursor(delta, len);
        }
        Command::HistoryDialogCancel => state.history_dialog = None,
        Command::HistoryDialogConfirm => {
            if state.history.is_empty() {
                return vec![];
            }
            let dialog = state.history_dialog.take().unwrap();
            let cursor = dialog.cursor.min(state.history.len() - 1);
            let entry = state.history[state.history.len() - 1 - cursor].clone();
            // Same placement as Up/Down recall: the whole line is replaced
            // with the caret at its end, and any recall in progress ends
            // (command-line "Command history navigation"; design D2).
            state.command_line.set_text_caret_end(entry);
            state.history_cursor = None;
        }
        _ => {}
    }
    vec![]
}

/// `Command::UserMenuReloaded`: the editor over `usermenu.toml` has closed
/// and the TUI re-read it. A well-formed file replaces the F2 entries
/// immediately — and if it carried non-fatal warnings (an unknown key, an
/// entry missing a field), their one-line summary is shown in the same
/// dismissable dialog (configuration-files "Warnings after editing the user
/// menu"); a malformed one raises the existing warning and keeps the
/// previous entries — the on-disk file (the user's own edit) is never
/// overwritten (user-menu "Malformed edit warns without overwriting";
/// design D4).
pub(super) fn apply_user_menu_reload(state: &mut State, result: config::UserMenuLoadResult) {
    if result.malformed {
        state.startup_warning = Some(format!("{} is malformed; keeping the previous user menu entries", config::USERMENU_FILE));
    } else {
        state.user_menu_entries = result.entries;
        if let Some(summary) = config::summarize_warnings(&result.warnings) {
            state.startup_warning = Some(summary);
        }
    }
}

// ---------------------------------------------------------------------
// Options -> Themes picker (visual-themes)
// ---------------------------------------------------------------------

pub(super) fn is_theme_picker_command(cmd: &Command) -> bool {
    matches!(cmd, Command::ThemePickerMove(_) | Command::ThemePickerConfirm | Command::ThemePickerCancel)
}

/// Drive the Options → Themes picker (gated in [`update`] by
/// `state.theme_picker`). Enter applies the highlighted theme immediately —
/// swapping `state.theme` in this same reducer step, so the very next frame
/// renders with it — and returns `Effect::PersistTheme` so the TUI event
/// loop writes it to `config.toml`; Esc closes the dialog leaving both the
/// active theme and the config file untouched (theme-selection "Picker
/// navigation, apply, and cancel").
pub(super) fn handle_theme_picker(state: &mut State, cmd: Command) -> Vec<Effect> {
    if state.theme_picker.is_none() {
        return vec![];
    }
    match cmd {
        Command::ThemePickerMove(delta) => {
            state.theme_picker.as_mut().unwrap().move_cursor(delta);
        }
        Command::ThemePickerCancel => state.theme_picker = None,
        Command::ThemePickerConfirm => {
            let picker = state.theme_picker.take().unwrap();
            if let Some(name) = crate::theme::BUILTIN_THEME_NAMES.get(picker.highlight) {
                if let Some(theme) = Theme::by_name(name) {
                    state.theme = theme;
                }
                return vec![Effect::PersistTheme((*name).to_string())];
            }
        }
        _ => {}
    }
    vec![]
}

// ---------------------------------------------------------------------
// F1 Help window + About dialog (M5)
// ---------------------------------------------------------------------

pub(super) fn is_help_command(cmd: &Command) -> bool {
    matches!(cmd, Command::HelpMove(_) | Command::HelpActivate | Command::HelpCancel)
}

/// Drive the F1 Help window (gated in [`update`] by `state.help`).
pub(super) fn handle_help(state: &mut State, cmd: Command) -> Vec<Effect> {
    if state.help.is_none() {
        return vec![];
    }
    match cmd {
        Command::HelpMove(delta) => {
            let visible = crate::dialogs::help_topic_visible_rows(crate::dialogs::help_window_height(state.term_size));
            state.help.as_mut().unwrap().move_cursor(delta, visible);
        }
        Command::HelpActivate => state.help.as_mut().unwrap().activate(),
        Command::HelpCancel => {
            let mut h = state.help.take().unwrap();
            // `back` returns `true` when it stepped down one level (About
            // -> list, or page -> list) rather than closing the window
            // outright; only then is the window kept open.
            if h.back() {
                state.help = Some(h);
            }
        }
        _ => {}
    }
    vec![]
}

// ---------------------------------------------------------------------
// Mouse (mouse-basics, design D2)
// ---------------------------------------------------------------------

/// The command a dialog button click stands for — the exact command the
/// equivalent keypress already issues, so a click is never a new way to do
/// something, only a new way to reach an existing one. `None` for a button
/// that has no reachable meaning outside its own gated context (there is
/// none today — kept `Option` for symmetry with [`menu_action_command`] and
/// so a future button can be added without an infallible-match refactor).
pub fn button_command(id: ButtonId) -> Option<Command> {
    match id {
        ButtonId::DeleteConfirmYes => Some(Command::FileOpConfirm),
        ButtonId::DeleteConfirmNo => Some(Command::FileOpCancel),
        ButtonId::ConflictOverwrite => Some(Command::FileOpConflictChoice(ConflictChoice::Overwrite)),
        ButtonId::ConflictOverwriteAll => Some(Command::FileOpConflictChoice(ConflictChoice::OverwriteAll)),
        ButtonId::ConflictSkip => Some(Command::FileOpConflictChoice(ConflictChoice::Skip)),
        ButtonId::ConflictSkipAll => Some(Command::FileOpConflictChoice(ConflictChoice::SkipAll)),
        ButtonId::ConflictRename => Some(Command::FileOpBeginRename),
        ButtonId::ConflictCancel => Some(Command::FileOpConflictChoice(ConflictChoice::Cancel)),
        ButtonId::ErrorRetry => Some(Command::FileOpErrorChoice(ErrorChoice::Retry)),
        ButtonId::ErrorSkip => Some(Command::FileOpErrorChoice(ErrorChoice::Skip)),
        ButtonId::ErrorSkipAll => Some(Command::FileOpErrorChoice(ErrorChoice::SkipAll)),
        ButtonId::ErrorAbort => Some(Command::FileOpErrorChoice(ErrorChoice::Abort)),
        ButtonId::ProgressCancel => Some(Command::FileOpCancelJob),
        ButtonId::SummaryContinue => Some(Command::FileOpConfirm),
        ButtonId::QuitYes => Some(Command::ConfirmQuit),
        ButtonId::QuitNo => Some(Command::CancelQuit),
        ButtonId::DropDialogCopy => Some(Command::FileOpConfirmAs(JobKind::Copy)),
        ButtonId::DropDialogMove => Some(Command::FileOpConfirmAs(JobKind::Move)),
        ButtonId::DropDialogCancel => Some(Command::FileOpCancel),
    }
}

/// `Command::KeybarPress`: the command each function-key-bar slot stands
/// for, exactly mirroring `input::map_panel_key`'s own F-key arms (mouse-
/// input "Key bar, menu bar, pull-down items, and dialog buttons are
/// clickable"). `None` for a slot number the bar never actually draws.
pub(super) fn keybar_command(slot: u8) -> Option<Command> {
    match slot {
        1 => Some(Command::HelpOpen),
        2 => Some(Command::UserMenuOpen),
        3 => Some(Command::RequestViewer),
        4 => Some(Command::RequestEditor),
        5 => Some(Command::RequestCopy),
        6 => Some(Command::RequestMove),
        7 => Some(Command::RequestMkdir),
        8 => Some(Command::RequestDelete),
        9 => Some(Command::MenuOpen),
        10 => Some(Command::RequestQuit),
        _ => None,
    }
}

/// `Command::MenuItemClick`: highlight `index` (only if it names an enabled
/// item) and return the command activating it stands for, exactly like
/// `handle_menu`'s own `Command::MenuActivate` arm — the caller re-enters
/// `update` with it. `None` (no-op) for an index that is out of range, a
/// separator, or disabled, and whenever no menu is open at all.
pub(super) fn handle_menu_item_click(state: &mut State, index: usize) -> Option<Command> {
    let active_side = state.active;
    let menu = state.menu.as_mut()?;
    match menu.items().get(index) {
        Some(MenuEntry::Item(item)) if item.is_enabled() => {
            menu.selected = index;
            let side = menu.active.target_side(active_side);
            let action = item.action;
            state.menu = None;
            menu_action_command(action, side)
        }
        _ => None,
    }
}

/// `Command::OpenActionMenuAt`: move `side`'s cursor to the named entry and
/// open the file-action menu for it — for a file, the same shape
/// `handle_enter`'s `EntryKind::File` arm builds; for a directory, the
/// `View`/`Edit`/`Run`-less menu `file-action-menu`'s "Directory targets and
/// selection-scoped invocation" requirement adds (mouse-input "Right-click
/// opens the action menu"; design D4). The menu is also opened
/// selection-scoped whenever `name` is already a member of the panel's
/// selection set, so `activate_file_action` later acts on the whole
/// selection rather than `name` alone. A `..` target, or a name the panel no
/// longer lists, is a no-op — `..` is never a valid action-menu target,
/// mirroring Ctrl+click's own "Parent entry ignored" guard.
pub(super) fn handle_open_action_menu_at(state: &mut State, side: PanelSide, name: OsString) -> Vec<Effect> {
    state.active = side;
    let panel = state.panel_mut(side);
    let Some(idx) = panel.entries.iter().position(|e| e.name == name) else { return vec![] };
    panel.cursor = idx;
    panel.cursor_user_moved = true;
    reconcile_panel_viewport(state, side);
    let panel = state.panel(side);
    let kind = panel.entries[idx].kind;
    if kind == EntryKind::ParentDir {
        return vec![];
    }
    let is_dir = kind == EntryKind::Directory;
    let executable = !is_dir && shell::is_executable_name(&name.to_string_lossy(), &state.shell.pathext);
    let selection_scoped = panel.selected.contains(&name);
    state.file_action_menu = Some(FileActionMenuState::open(name, is_dir, executable, selection_scoped));
    vec![]
}
