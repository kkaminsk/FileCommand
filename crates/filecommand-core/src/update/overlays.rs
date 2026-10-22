//! The modal overlays that live beside the phase: the F9 menu, drive select,
//! fuzzy jump, find file, the F2 user menu, the Alt+F8 history dialog, the
//! theme picker, the file-action menu, Help, and the startup warning — plus
//! the menu-action, dialog-button and key-bar command tables.
//!
//! Every overlay's state lives in the one [`State::overlay`] slot
//! (reducer-overlay-structure design D1). The reducer takes the open overlay
//! out of the slot, asks [`Overlay::claims`] whether it handles the command,
//! and if so hands the variant's state to its handler here together with
//! `&mut State`; the [`OverlayStep`] the handler returns says whether the
//! overlay stays, closes, is replaced, or closes and re-enters with a
//! follow-up command (design D3). No handler ever runs without its own state.

use super::*;

/// The one modal overlay that can be open above the phase, held in
/// [`State::overlay`] (reducer-overlay-structure design D1). One variant per
/// overlay, so at most one of them is ever open by construction — the
/// property every open path used to maintain only by convention. The
/// quit-confirmation dialog is deliberately not a variant: it is a layer
/// above everything, and cancelling it must leave an open overlay exactly as
/// it was (application-shell "Quit request keys and confirmation"). The
/// file-operation dialogs, the viewer, and the editor stay [`UiPhase`]
/// variants, which are already exclusive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    /// The F9 menu bar and its pull-down.
    Menu(MenuState),
    /// The Alt+F1/Alt+F2 drive-select dialog.
    DriveSelect(DriveSelect),
    /// The Ctrl+J fuzzy-jump dialog.
    FuzzyJump(FuzzyJumpState),
    /// The Alt+F7 find-file dialog.
    FindFile(FindFileState),
    /// The F2 user menu.
    UserMenu(UserMenuState),
    /// The Alt+F8 command-history dialog.
    HistoryDialog(HistoryDialogState),
    /// The Options → Themes picker.
    ThemePicker(ThemePickerState),
    /// The Enter-on-file action menu.
    FileActionMenu(FileActionMenuState),
    /// The F1 Help window, with its nested About dialog.
    Help(HelpState),
    /// The dismissable startup-warning modal (a malformed `usermenu.toml`,
    /// an unknown `--theme`, a configuration-warning summary, …), which must
    /// stay visible over whichever phase is showing, the splash included.
    StartupWarning(String),
}

impl Overlay {
    /// Whether the open overlay handles `cmd` (design D4). Every variant's
    /// command set lives in this one `match`; a command the open overlay does
    /// not claim falls through to the layers below it exactly as it always
    /// did — listing events, `Tick`, worker replies, and so on. `TextEdit`
    /// and `RequestPaste` belong to the two overlays with a text field, and
    /// the find-file walk's streamed replies belong to find file alone, so a
    /// late result of a dismissed search can never touch another overlay
    /// (application-shell "Modal overlays are mutually exclusive by
    /// construction").
    pub fn claims(&self, cmd: &Command) -> bool {
        match self {
            Overlay::Menu(_) => matches!(
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
                    | Command::MenuTitleClick(_)
                    | Command::MenuItemClick(_)
            ),
            Overlay::DriveSelect(_) => matches!(cmd, Command::DriveSelectMove(_) | Command::DriveSelectConfirm | Command::DriveSelectCancel),
            Overlay::FuzzyJump(_) => matches!(
                cmd,
                Command::TextEdit(_) | Command::RequestPaste | Command::FuzzyJumpMove(_) | Command::FuzzyJumpConfirm | Command::FuzzyJumpCancel
            ),
            Overlay::FindFile(_) => matches!(
                cmd,
                Command::TextEdit(_)
                    | Command::RequestPaste
                    | Command::FindFileSubmit
                    | Command::FindFileMatch { .. }
                    | Command::FindFileSearchDone { .. }
                    | Command::FindFileMove(_)
                    | Command::FindFileConfirm
                    | Command::FindFileCancel
            ),
            Overlay::UserMenu(_) => matches!(cmd, Command::UserMenuMove(_) | Command::UserMenuConfirm | Command::UserMenuCancel),
            Overlay::HistoryDialog(_) => matches!(cmd, Command::HistoryDialogMove(_) | Command::HistoryDialogConfirm | Command::HistoryDialogCancel),
            Overlay::ThemePicker(_) => matches!(cmd, Command::ThemePickerMove(_) | Command::ThemePickerConfirm | Command::ThemePickerCancel),
            Overlay::FileActionMenu(_) => matches!(
                cmd,
                Command::FileActionMenuMove(_)
                    | Command::FileActionMenuConfirm
                    | Command::FileActionMenuCancel
                    | Command::FileActionMenuHotkey(_)
                    | Command::FileActionMenuItemClick(_)
            ),
            Overlay::Help(_) => matches!(cmd, Command::HelpMove(_) | Command::HelpActivate | Command::HelpCancel),
            Overlay::StartupWarning(_) => matches!(cmd, Command::DismissStartupWarning),
        }
    }

    /// Run the open overlay's handler for a command it [`claims`](Self::claims)
    /// (design D3). The reducer has taken `self` out of [`State::overlay`],
    /// so the slot is empty while the handler runs — which is what lets a
    /// handler open a dialog or request a listing on `state` without a
    /// second borrow of the overlay it is driving.
    pub(super) fn handle(&mut self, state: &mut State, cmd: Command) -> OverlayStep {
        match self {
            Overlay::Menu(menu) => handle_menu(menu, state, cmd),
            Overlay::DriveSelect(dialog) => handle_drive_select(dialog, state, cmd),
            Overlay::FuzzyJump(dialog) => handle_fuzzy_jump(dialog, state, cmd),
            Overlay::FindFile(dialog) => handle_find_file(dialog, state, cmd),
            Overlay::UserMenu(menu) => handle_user_menu(menu, state, cmd),
            Overlay::HistoryDialog(dialog) => handle_history_dialog(dialog, state, cmd),
            Overlay::ThemePicker(picker) => handle_theme_picker(picker, state, cmd),
            Overlay::FileActionMenu(menu) => handle_file_action_menu(menu, state, cmd),
            Overlay::Help(help) => handle_help(help, state, cmd),
            // The only command the warning claims is its dismissal (any key,
            // per the one-shot dialogs' Esc/Enter-dismiss convention;
            // user-menu "Malformed file warns and falls back without
            // overwriting").
            Overlay::StartupWarning(_) => OverlayStep::Close(vec![]),
        }
    }
}

/// What an overlay handler did with its overlay (design D3). The reducer
/// takes the overlay out of [`State::overlay`], hands the variant's state to
/// its handler together with `&mut State`, and restores the slot from the
/// step it returns — so no handler ever runs without its own state, and none
/// re-checks a gate that lives somewhere else.
#[derive(Debug)]
pub(super) enum OverlayStep {
    /// The overlay stays open; put its (possibly updated) state back.
    Keep(Vec<Effect>),
    /// The overlay closed (Esc, confirm, cancel); the slot stays empty.
    Close(Vec<Effect>),
    /// A different overlay opens in its place: the user menu's Themes slot
    /// opening the theme picker, or a menu-bar title click switching the
    /// open pull-down.
    Replace(Overlay, Vec<Effect>),
    /// The overlay closed and the reducer re-enters with a follow-up
    /// command: an activated menu item, exactly as `handle_menu` used to
    /// return `Option<Command>`.
    CloseThen(Command),
}

// ---------------------------------------------------------------------
// Drive select
// ---------------------------------------------------------------------

fn handle_drive_select(dialog: &mut DriveSelect, state: &mut State, cmd: Command) -> OverlayStep {
    match cmd {
        Command::DriveSelectMove(delta) => {
            dialog.move_selection(delta);
            OverlayStep::Keep(vec![])
        }
        // The target panel keeps its current directory.
        Command::DriveSelectCancel => OverlayStep::Close(vec![]),
        Command::DriveSelectConfirm => {
            let Some(letter) = dialog.selected_letter() else { return OverlayStep::Close(vec![]) };
            // An unavailable drive is not special-cased here: the listing
            // read fails on the worker thread and surfaces through
            // `ListingFailed` as the panel's inline error state.
            let path = drives::drive_root(letter);
            OverlayStep::Close(begin_listing(state, dialog.target, path))
        }
        _ => OverlayStep::Keep(vec![]),
    }
}

// ---------------------------------------------------------------------
// F9 menu
// ---------------------------------------------------------------------

/// Drive the menu state machine. An activated item closes the menu and
/// re-enters the reducer with the command it stands for
/// (`OverlayStep::CloseThen`), so a menu action and its keyboard shortcut
/// share one implementation and the item's dialog opens into the slot the
/// menu just left (application-shell "A menu item opens its dialog in place
/// of the menu").
fn handle_menu(menu: &mut MenuState, state: &mut State, cmd: Command) -> OverlayStep {
    let active_side = state.active;
    match cmd {
        Command::MenuOpen | Command::MenuClose => OverlayStep::Close(vec![]),
        Command::MenuCollapse => {
            // Esc closes the pull-down first, leaving the bar open with its
            // title still highlighted; a second Esc closes the bar.
            if menu.pulldown_open {
                menu.pulldown_open = false;
                OverlayStep::Keep(vec![])
            } else {
                OverlayStep::Close(vec![])
            }
        }
        Command::MenuSelectPrev => {
            menu.move_selection(-1);
            OverlayStep::Keep(vec![])
        }
        Command::MenuSelectNext => {
            menu.move_selection(1);
            OverlayStep::Keep(vec![])
        }
        Command::MenuPrevMenu => {
            let target = menu.active.prev();
            menu.go_to(target);
            OverlayStep::Keep(vec![])
        }
        Command::MenuNextMenu => {
            let target = menu.active.next();
            menu.go_to(target);
            OverlayStep::Keep(vec![])
        }
        Command::MenuHotkey(c) => {
            if let Some(id) = MenuId::from_hotkey(c) {
                menu.go_to(id);
            }
            OverlayStep::Keep(vec![])
        }
        Command::MenuActivate => {
            let Some(item) = menu.selected_item() else { return OverlayStep::Keep(vec![]) };
            let side = menu.active.target_side(active_side);
            // Activating an item closes the whole overlay, restoring the top
            // row and the clock, before the action runs. Disabled items never
            // reach here, so an action with no command simply closes the bar.
            match menu_action_command(item.action, side) {
                Some(next) => OverlayStep::CloseThen(next),
                None => OverlayStep::Close(vec![]),
            }
        }
        // A click on a menu-bar title while the bar is open switches to that
        // pull-down (or re-opens the same one), replacing the menu in place
        // (mouse-input "Key bar, menu bar, pull-down items, and dialog
        // buttons are clickable").
        Command::MenuTitleClick(id) => OverlayStep::Replace(Overlay::Menu(MenuState::for_menu(id)), vec![]),
        // A click on the open pull-down's `index`'th row: highlight it (only
        // if it names an enabled item) and activate it exactly like
        // `MenuActivate`. An index that is out of range, a separator, or a
        // disabled item is a no-op.
        Command::MenuItemClick(index) => match menu.items().get(index) {
            Some(MenuEntry::Item(item)) if item.is_enabled() => {
                menu.selected = index;
                let side = menu.active.target_side(active_side);
                match menu_action_command(item.action, side) {
                    Some(next) => OverlayStep::CloseThen(next),
                    None => OverlayStep::Close(vec![]),
                }
            }
            _ => OverlayStep::Keep(vec![]),
        },
        _ => OverlayStep::Keep(vec![]),
    }
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

/// Drive the Enter-on-file action menu — navigation, dismissal, and
/// activation (file-action-menu "Menu contents, ordering, and navigation").
/// An activated entry closes the menu first and then runs through
/// `activate_file_action` with the slot already empty, so the dialog or open
/// it leads to finds no overlay in its way.
fn handle_file_action_menu(menu: &mut FileActionMenuState, state: &mut State, cmd: Command) -> OverlayStep {
    match cmd {
        Command::FileActionMenuMove(delta) => {
            menu.move_cursor(delta);
            OverlayStep::Keep(vec![])
        }
        Command::FileActionMenuCancel => OverlayStep::Close(vec![]),
        Command::FileActionMenuConfirm => {
            let target_name = menu.target_name.clone();
            OverlayStep::Close(activate_file_action(state, menu.selected(), target_name, menu.selection_scoped))
        }
        Command::FileActionMenuItemClick(index) => {
            if index >= menu.entries.len() {
                // Shouldn't normally happen given hit-testing (the hit map
                // only ever records rects for real entries), but a stale
                // hit map click — same reasoning as
                // `click_entry_on_a_vanished_name_is_a_no_op` — is safer
                // treated as a no-op than an out-of-bounds panic.
                return OverlayStep::Keep(vec![]);
            }
            menu.cursor = index;
            let target_name = menu.target_name.clone();
            OverlayStep::Close(activate_file_action(state, menu.selected(), target_name, menu.selection_scoped))
        }
        Command::FileActionMenuHotkey(c) => match menu.hotkey_action(c) {
            Some(action) => {
                let target_name = menu.target_name.clone();
                OverlayStep::Close(activate_file_action(state, action, target_name, menu.selection_scoped))
            }
            None => OverlayStep::Keep(vec![]),
        },
        _ => OverlayStep::Keep(vec![]),
    }
}

// ---------------------------------------------------------------------
// Ctrl+J fuzzy jump (M5)
// ---------------------------------------------------------------------

/// Drive the fuzzy-jump dialog. The ranked/filtered list itself is never
/// stored on the dialog — it is recomputed from `state.dir_history` each
/// time via [`quicksearch::rank_directories`], which is why every arm
/// re-derives it rather than caching (fuzzy-jump "Fuzzy matching of visited
/// directories", "Frecency ranking").
fn handle_fuzzy_jump(dialog: &mut FuzzyJumpState, state: &mut State, cmd: Command) -> OverlayStep {
    match cmd {
        Command::TextEdit(op) => {
            dialog.apply(&op);
            OverlayStep::Keep(vec![])
        }
        Command::RequestPaste => OverlayStep::Keep(vec![Effect::ReadClipboardText]),
        Command::FuzzyJumpMove(delta) => {
            let len = quicksearch::rank_directories(&state.dir_history, dialog.pattern.text(), state.epoch_ms).len();
            dialog.move_cursor(delta, len);
            OverlayStep::Keep(vec![])
        }
        Command::FuzzyJumpCancel => OverlayStep::Close(vec![]),
        Command::FuzzyJumpConfirm => {
            // Ranked with the epoch clock, the same one the renderer uses,
            // so the row Enter acts on is the row that was drawn (design D1).
            let ranked = quicksearch::rank_directories(&state.dir_history, dialog.pattern.text(), state.epoch_ms);
            match ranked.get(dialog.cursor).map(|e| e.path.clone()) {
                Some(path) => {
                    let side = state.active;
                    OverlayStep::Close(begin_listing(state, side, path))
                }
                None => OverlayStep::Close(vec![]),
            }
        }
        _ => OverlayStep::Keep(vec![]),
    }
}

// ---------------------------------------------------------------------
// Alt+F7 find file (M5)
// ---------------------------------------------------------------------

/// Drive the find-file dialog.
fn handle_find_file(dialog: &mut FindFileState, state: &mut State, cmd: Command) -> OverlayStep {
    match cmd {
        Command::TextEdit(op) => {
            dialog.apply(&op);
            OverlayStep::Keep(vec![])
        }
        Command::RequestPaste => OverlayStep::Keep(vec![Effect::ReadClipboardText]),
        Command::FindFileSubmit => {
            let request = state.next_request_id();
            dialog.submit(request);
            let root = dialog.root.clone();
            let pattern = dialog.pattern.text().to_string();
            OverlayStep::Keep(vec![Effect::FindInSubtree { root, pattern, request }])
        }
        Command::FindFileMatch { request, m } => {
            dialog.push_match(request, m);
            OverlayStep::Keep(vec![])
        }
        Command::FindFileSearchDone { request } => {
            dialog.mark_done(request);
            OverlayStep::Keep(vec![])
        }
        Command::FindFileMove(delta) => {
            dialog.move_cursor(delta);
            OverlayStep::Keep(vec![])
        }
        // Dismissing abandons any in-progress search: the walk is told to
        // stop (find-file "Dismissed or superseded searches stop";
        // listing-request-guard design D4), and whatever it already posted
        // arrives for a slot that no longer holds find file, so
        // `Overlay::claims` lets it fall through and the base layer drops it
        // (find-file "Esc cancels"; application-shell "Late results of a
        // dismissed search touch no other overlay").
        Command::FindFileCancel => OverlayStep::Close(running_find_file_walk(dialog).map(Effect::CancelRequest).into_iter().collect()),
        Command::FindFileConfirm => {
            // Confirming a result while the walk is still running stops it
            // too (find-file "Confirming a result while the walk is running
            // stops it"); the navigation below proceeds either way.
            let mut effects: Vec<Effect> = running_find_file_walk(dialog).map(Effect::CancelRequest).into_iter().collect();
            let Some(m) = dialog.selected().cloned() else { return OverlayStep::Close(effects) };
            let target_dir = match m.relative_path.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => dialog.root.join(parent),
                _ => dialog.root.clone(),
            };
            let side = state.active;
            effects.extend(begin_listing(state, side, target_dir));
            state.panel_mut(side).pending_cursor_target = Some(m.entry.name);
            OverlayStep::Close(effects)
        }
        _ => OverlayStep::Keep(vec![]),
    }
}

/// The request id of a find-file walk that was submitted and has not yet
/// reported `FindFileSearchDone` — the one `FindFileCancel`/`FindFileConfirm`
/// tell to stop (listing-request-guard design D4). `None` before Enter
/// submits a search and once the walk has finished.
fn running_find_file_walk(ff: &FindFileState) -> Option<u64> {
    ff.request.filter(|_| !ff.done)
}

// ---------------------------------------------------------------------
// F2 user menu (M5)
// ---------------------------------------------------------------------

/// Drive the F2 user menu. Entries themselves live in
/// `state.user_menu_entries`, loaded once at startup — the menu overlay only
/// tracks the cursor (user-menu "Open the F2 user menu", "Navigate and
/// dismiss the user menu"). The cursor domain is
/// `0..=state.user_menu_entries.len()`: the last index is the compiled-in
/// "Themes" slot, not a config entry (design D3).
fn handle_user_menu(menu: &mut UserMenuState, state: &mut State, cmd: Command) -> OverlayStep {
    match cmd {
        Command::UserMenuMove(delta) => {
            // Cursor domain is the user entries plus one compiled-in
            // built-in slot at index `entries.len()` for "Themes" (design
            // D3) — not an entry appended to `state.user_menu_entries`.
            let len = state.user_menu_entries.len() + 1;
            menu.move_cursor(delta, len);
            OverlayStep::Keep(vec![])
        }
        Command::UserMenuCancel => OverlayStep::Close(vec![]),
        Command::UserMenuConfirm => {
            if let Some(entry) = state.user_menu_entries.get(menu.cursor).cloned() {
                let side = state.active;
                let cwd = state.panel(side).cwd.clone();
                return OverlayStep::Close(vec![Effect::RunShellCommand(
                    shell::build_command(state.shell.shell.as_deref(), &entry.command, &cwd),
                    side,
                )]);
            }
            // Cursor is at the built-in Themes slot (`entries.len()`): the
            // theme picker opens in the user menu's place, through the exact
            // same constructor `MenuAction::OpenThemes` uses, pre-highlighting
            // the active theme. No shell effect (application-shell "The user
            // menu's Themes entry replaces the user menu").
            OverlayStep::Replace(Overlay::ThemePicker(ThemePickerState::open(&state.theme.name)), vec![])
        }
        _ => OverlayStep::Keep(vec![]),
    }
}

// ---------------------------------------------------------------------
// Alt+F8 command-history dialog (pulldown-menu-completion; design D2)
// ---------------------------------------------------------------------

/// Drive the Alt+F8 command-history dialog. The list renders
/// `state.history` newest first, so `cursor` counts from the end of the
/// vector; Enter places the highlighted entry on the command line (caret at
/// the end, recall cleared) and closes without running anything — the user
/// presses Enter again to run it (design D2's non-goal). On an empty history
/// the dialog shows a single `(no history)` row and Enter does nothing (the
/// dialog stays open until Esc).
fn handle_history_dialog(dialog: &mut HistoryDialogState, state: &mut State, cmd: Command) -> OverlayStep {
    match cmd {
        Command::HistoryDialogMove(delta) => {
            let len = state.history.len();
            dialog.move_cursor(delta, len);
            OverlayStep::Keep(vec![])
        }
        Command::HistoryDialogCancel => OverlayStep::Close(vec![]),
        Command::HistoryDialogConfirm => {
            if state.history.is_empty() {
                return OverlayStep::Keep(vec![]);
            }
            let cursor = dialog.cursor.min(state.history.len() - 1);
            let entry = state.history[state.history.len() - 1 - cursor].clone();
            // Same placement as Up/Down recall: the whole line is replaced
            // with the caret at its end, and any recall in progress ends
            // (command-line "Command history navigation"; design D2).
            state.command_line.set_text_caret_end(entry);
            state.history_cursor = None;
            OverlayStep::Close(vec![])
        }
        _ => OverlayStep::Keep(vec![]),
    }
}

/// `Command::UserMenuReloaded`: the editor over `usermenu.toml` has closed
/// and the TUI re-read it. A well-formed file replaces the F2 entries
/// immediately — and if it carried non-fatal warnings (an unknown key, an
/// entry missing a field), their one-line summary is shown in the same
/// dismissable dialog (configuration-files "Warnings after editing the user
/// menu"); a malformed one raises the existing warning and keeps the
/// previous entries — the on-disk file (the user's own edit) is never
/// overwritten (user-menu "Malformed edit warns without overwriting";
/// design D4). The slot is empty here: the reload follows the return of the
/// editor, which was opened from the F9 menu after the menu closed, and no
/// input is read in between (reducer-overlay-structure design D5).
pub(super) fn apply_user_menu_reload(state: &mut State, result: config::UserMenuLoadResult) {
    if result.malformed {
        state.set_startup_warning(format!("{} is malformed; keeping the previous user menu entries", config::USERMENU_FILE));
    } else {
        state.user_menu_entries = result.entries;
        if let Some(summary) = config::summarize_warnings(&result.warnings) {
            state.set_startup_warning(summary);
        }
    }
}

// ---------------------------------------------------------------------
// Options -> Themes picker (visual-themes)
// ---------------------------------------------------------------------

/// Drive the Options → Themes picker. Enter applies the highlighted theme
/// immediately — swapping `state.theme` in this same reducer step, so the
/// very next frame renders with it — and returns `Effect::PersistTheme` so
/// the TUI event loop writes it to `config.toml`; Esc closes the dialog
/// leaving both the active theme and the config file untouched
/// (theme-selection "Picker navigation, apply, and cancel").
fn handle_theme_picker(picker: &mut ThemePickerState, state: &mut State, cmd: Command) -> OverlayStep {
    match cmd {
        Command::ThemePickerMove(delta) => {
            picker.move_cursor(delta);
            OverlayStep::Keep(vec![])
        }
        Command::ThemePickerCancel => OverlayStep::Close(vec![]),
        Command::ThemePickerConfirm => {
            if let Some(name) = crate::theme::BUILTIN_THEME_NAMES.get(picker.highlight) {
                if let Some(theme) = Theme::by_name(name) {
                    state.theme = theme;
                }
                return OverlayStep::Close(vec![Effect::PersistTheme((*name).to_string())]);
            }
            OverlayStep::Close(vec![])
        }
        _ => OverlayStep::Keep(vec![]),
    }
}

// ---------------------------------------------------------------------
// F1 Help window + About dialog (M5)
// ---------------------------------------------------------------------

/// Drive the F1 Help window.
fn handle_help(help: &mut HelpState, state: &mut State, cmd: Command) -> OverlayStep {
    match cmd {
        Command::HelpMove(delta) => {
            let visible = crate::dialogs::help_topic_visible_rows(crate::dialogs::help_window_height(state.term_size));
            help.move_cursor(delta, visible);
            OverlayStep::Keep(vec![])
        }
        Command::HelpActivate => {
            help.activate();
            OverlayStep::Keep(vec![])
        }
        // `back` returns `true` when it stepped down one level (About ->
        // list, or page -> list) rather than closing the window outright;
        // only then is the window kept open.
        Command::HelpCancel => {
            if help.back() {
                OverlayStep::Keep(vec![])
            } else {
                OverlayStep::Close(vec![])
            }
        }
        _ => OverlayStep::Keep(vec![]),
    }
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
    state.open_overlay(Overlay::FileActionMenu(FileActionMenuState::open(name, is_dir, executable, selection_scoped)));
    vec![]
}
