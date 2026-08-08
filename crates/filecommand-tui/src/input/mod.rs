//! Maps crossterm key events to core [`Command`]s.
//!
//! The mapper reads the whole [`State`] rather than just the phase, because
//! from M3 on the same physical key means different things depending on
//! what owns it: Up is history recall with text typed and a cursor move
//! without, Backspace edits the command line or goes to the parent
//! directory, and printable keys go to the command line unless quick-search
//! or a dialog has claimed them. The mapping itself still performs no state
//! mutation and no I/O.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use filecommand_core::config::{KeyBinding, Keys};
use filecommand_core::dialogs::{FileActionMenuState, HelpState};
use filecommand_core::editor::{EditorMove, EditorState};
use filecommand_core::find_file::FindFileState;
use filecommand_core::fs_ops::dialog::{FileOpSetup, RunningDialog};
use filecommand_core::fs_ops::{ConflictChoice, ErrorChoice};
use filecommand_core::listing::SortMode;
use filecommand_core::panel::CursorMove;
use filecommand_core::viewer::ViewerState;
use filecommand_core::{Command, PanelSide, State, UiPhase};

pub fn map_key(key: KeyEvent, state: &State, page_size: usize, keys: &Keys) -> Option<Command> {
    // The quit-confirmation dialog is the topmost modal overlay of all: it
    // can open above panels, the viewer, an open menu, or any other modal
    // dialog/overlay, so it is checked before every other overlay below
    // (application-shell "Quit request keys and confirmation"; design D5).
    // The event loop routes here for it even while `state.phase` is
    // `Viewer`/`Editor` — see `app.rs` — except in the editor, whose own
    // Ctrl+C=Copy binding means this flag can never become true there.
    if state.quit_confirm {
        return map_quit_confirm_key(key);
    }
    // Modal overlays come first: while one is up it owns every key it
    // understands, regardless of the phase underneath. The startup-warning
    // modal is checked first of all since it can only ever be up at the
    // very start of a session, before anything else has had a chance to
    // open (user-menu "Malformed file warns and falls back without
    // overwriting").
    if state.startup_warning.is_some() {
        return map_startup_warning_key(key);
    }
    if state.drive_select.is_some() {
        return map_drive_select_key(key);
    }
    if state.menu.is_some() {
        return map_menu_key(key);
    }
    // The M5 dialogs are likewise modal overlays beside the phase.
    if state.fuzzy_jump.is_some() {
        return map_fuzzy_jump_key(key);
    }
    if let Some(dialog) = &state.find_file {
        return map_find_file_key(key, dialog);
    }
    if state.user_menu.is_some() {
        return map_user_menu_key(key);
    }
    if state.theme_picker.is_some() {
        return map_theme_picker_key(key);
    }
    if let Some(dialog) = &state.help {
        return map_help_key(key, dialog);
    }
    // The Enter-on-file action menu is likewise a modal overlay beside the
    // phase (it opens without changing `state.phase` away from `Panels`),
    // so it must be checked before the phase match below claims panel/
    // command-line keys (file-action-menu "Enter on a file opens the action
    // menu").
    if let Some(dialog) = &state.file_action_menu {
        return map_file_action_menu_key(key, dialog);
    }

    match &state.phase {
        UiPhase::FileOpSetup(setup) => map_file_op_setup_key(key, setup),
        UiPhase::FileOpRunning { dialog, .. } => map_file_op_running_key(key, dialog),
        UiPhase::FileOpSummary(_) => Some(Command::FileOpConfirm),
        // The viewer owns its own key routing (`map_viewer_key`), which
        // needs I/O (a backward/forward line-start scan) that this pure
        // mapper cannot perform — the event loop calls it directly instead
        // of `map_key` while the viewer is open (viewer: Frame-less
        // full-screen chrome — "Viewer owns focus while open").
        UiPhase::Viewer(_) => None,
        // The editor likewise owns its own key routing (`map_editor_key`).
        // It needs no I/O, but its page-size parameter is the editor body's
        // row count (term_size.1 - 2, full width, no panel layout) rather
        // than `map_key`'s panel-layout-derived `page_size`, so the event
        // loop calls it directly the same way it does for the viewer
        // (builtin-editor "Full-screen editor chrome").
        UiPhase::Editor(_) => None,
        _ => {
            if state.quick_search.is_some() {
                return map_quick_search_key(key, page_size);
            }
            map_panel_key(key, state, page_size, keys)
        }
    }
}

/// What a key means while the F3 viewer is open. Simple toggles/prompt edits
/// map straight to a `Command`; navigation needs a byte-source-backed scan
/// (design D1's "the caller computes via `crate::viewer::backward`" — down
/// is the same shape via `crate::viewer::forward`), so it is expressed as a
/// line/column delta the event loop resolves against the open `ByteSource`
/// before issuing `Command::ViewerSetTop`/`ViewerSetHScroll`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewerInput {
    Cmd(Command),
    /// Move the top-of-screen anchor by this many lines (text mode) or rows
    /// (hex mode); negative is upward.
    ScrollLines(i64),
    /// Move the horizontal scroll by this many display columns (unwrap text
    /// mode only); negative is leftward.
    ScrollCols(i64),
    Home,
    End,
}

const VIEWER_H_SCROLL_STEP: i64 = 4;

/// Map a key while the viewer is open. `rows_visible` is the body's row
/// count (from `views::viewer::body_rows`), used as the Page Up/Down step —
/// the same "page size follows the layout" convention `map_panel_key` uses.
pub fn map_viewer_key(key: KeyEvent, viewer: &ViewerState, rows_visible: usize) -> Option<ViewerInput> {
    // Ctrl+C requests quit from the viewer in any state, including while
    // the F7 search prompt is open, ahead of every other viewer key
    // (application-shell "Quit request keys and confirmation").
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(ViewerInput::Cmd(Command::RequestQuit));
    }
    // The F7 search prompt owns the keyboard while it is open, exactly like
    // the command line and quick-search do elsewhere.
    if viewer.search_input.is_some() {
        return match key.code {
            KeyCode::Enter => Some(ViewerInput::Cmd(Command::ViewerSearchConfirm)),
            KeyCode::Esc => Some(ViewerInput::Cmd(Command::ViewerSearchCancel)),
            KeyCode::Backspace => Some(ViewerInput::Cmd(Command::ViewerSearchBackspace)),
            KeyCode::Char(c) if is_plain(&key) => Some(ViewerInput::Cmd(Command::ViewerSearchChar(c))),
            _ => None,
        };
    }
    let rows = rows_visible.max(1) as i64;
    match key.code {
        KeyCode::F(2) => Some(ViewerInput::Cmd(Command::ViewerToggleWrap)),
        KeyCode::F(4) => Some(ViewerInput::Cmd(Command::ViewerToggleMode)),
        KeyCode::F(7) => Some(ViewerInput::Cmd(Command::ViewerSearchStart)),
        KeyCode::F(10) | KeyCode::Esc => Some(ViewerInput::Cmd(Command::ViewerClose)),
        KeyCode::Up => Some(ViewerInput::ScrollLines(-1)),
        KeyCode::Down => Some(ViewerInput::ScrollLines(1)),
        KeyCode::PageUp => Some(ViewerInput::ScrollLines(-rows)),
        KeyCode::PageDown => Some(ViewerInput::ScrollLines(rows)),
        KeyCode::Left if !viewer.wrap => Some(ViewerInput::ScrollCols(-VIEWER_H_SCROLL_STEP)),
        KeyCode::Right if !viewer.wrap => Some(ViewerInput::ScrollCols(VIEWER_H_SCROLL_STEP)),
        KeyCode::Home => Some(ViewerInput::Home),
        KeyCode::End => Some(ViewerInput::End),
        _ => None,
    }
}

fn is_plain(key: &KeyEvent) -> bool {
    !key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT)
}

/// Map a key while the F4 built-in editor is open. Unlike the viewer, the
/// editor's own commands need no file I/O to resolve (design D1's
/// in-memory buffer), so this returns a plain `Command` rather than an
/// intermediate enum the event loop has to resolve further — but the event
/// loop still calls it directly rather than through `map_key`, since its
/// `rows_visible` page-size parameter is the editor body's own row count,
/// not a panel's (see `UiPhase::Editor(_) => None` in `map_key`).
///
/// Precedence, highest first: the save-on-exit confirm, then the
/// search-and-replace prompt, then the plain-search prompt, then normal
/// editing — exactly one of these ever owns a given key, mirroring how
/// `map_viewer_key` lets `search_input` claim the keyboard first.
pub fn map_editor_key(key: KeyEvent, editor: &EditorState, rows_visible: usize) -> Option<Command> {
    if editor.quit_confirm {
        return match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => Some(Command::EditorConfirmQuitSave),
            KeyCode::Char('n') | KeyCode::Char('N') => Some(Command::EditorConfirmQuitDiscard),
            KeyCode::Esc => Some(Command::EditorCancelQuit),
            _ => None,
        };
    }
    if editor.replace_prompt.is_some() {
        return match key.code {
            KeyCode::Enter => Some(Command::EditorReplaceConfirm),
            KeyCode::Esc => Some(Command::EditorReplaceCancel),
            KeyCode::Backspace => Some(Command::EditorReplaceBackspace),
            KeyCode::Char(c) if is_plain(&key) => Some(Command::EditorReplaceChar(c)),
            _ => None,
        };
    }
    if editor.search_prompt.is_some() {
        return match key.code {
            KeyCode::Enter => Some(Command::EditorSearchConfirm),
            KeyCode::Esc => Some(Command::EditorSearchCancel),
            KeyCode::Backspace => Some(Command::EditorSearchBackspace),
            KeyCode::Char(c) if is_plain(&key) => Some(Command::EditorSearchChar(c)),
            _ => None,
        };
    }
    let rows = rows_visible.max(1);
    match key.code {
        KeyCode::F(2) => Some(Command::EditorSave),
        KeyCode::F(3) => Some(Command::EditorMark),
        KeyCode::F(4) => Some(Command::EditorReplaceStart),
        KeyCode::F(7) => Some(Command::EditorSearchStart),
        KeyCode::F(10) => Some(Command::EditorRequestQuit),
        KeyCode::Insert => Some(Command::EditorToggleMode),
        KeyCode::Left => Some(Command::EditorMove(EditorMove::Left)),
        KeyCode::Right => Some(Command::EditorMove(EditorMove::Right)),
        KeyCode::Up => Some(Command::EditorMove(EditorMove::Up)),
        KeyCode::Down => Some(Command::EditorMove(EditorMove::Down)),
        KeyCode::Home => Some(Command::EditorMove(EditorMove::Home)),
        KeyCode::End => Some(Command::EditorMove(EditorMove::End)),
        KeyCode::PageUp => Some(Command::EditorMove(EditorMove::PageUp(rows))),
        KeyCode::PageDown => Some(Command::EditorMove(EditorMove::PageDown(rows))),
        KeyCode::Enter => Some(Command::EditorNewline),
        KeyCode::Backspace => Some(Command::EditorBackspace),
        // Conventional cut/copy/paste/undo bindings: the spec leaves the
        // exact keys up to the implementation (only F3 Mark is named), and
        // these don't collide with anything else the editor's own keymap
        // claims.
        KeyCode::Char('x') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Command::EditorCut),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Command::EditorCopy),
        KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Command::EditorPaste),
        KeyCode::Char('z') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Command::EditorUndo),
        KeyCode::Char(c) if is_plain(&key) => Some(Command::EditorChar(c)),
        _ => None,
    }
}

fn map_panel_key(key: KeyEvent, state: &State, page_size: usize, keys: &Keys) -> Option<Command> {
    let active = state.active;
    let typing = !state.command_line.is_empty();

    // The Ctrl+P quick filter, while active on the active panel, claims
    // plain printables/Backspace before anything else — but leaves every
    // other key (movement, Enter, Esc, ...) to fall through to the normal
    // panel handling below, since navigation still applies with the filter
    // narrowing what it can land on (`PanelState::move_cursor` itself
    // restricts to visible entries) (quick-filter "Navigation is restricted
    // to matching entries"). Esc no longer exits the filter here — it falls
    // through to the unconditional panel-level quit request below, and the
    // activation key (matched further down via `keys.quick_filter`) is what
    // now toggles the filter off (quick-filter "Clearing the quick filter";
    // application-shell "Quit request keys and confirmation").
    if state.active_panel().quick_filter.is_some() {
        match key.code {
            KeyCode::Backspace => return Some(Command::QuickFilterBackspace),
            KeyCode::Char(c) if is_plain(&key) => return Some(Command::QuickFilterChar(c)),
            _ => {}
        }
    }

    // Config-overridable bindings win over the compiled-in map, so a user
    // who rebinds Ctrl+] to something else still gets the default meaning
    // of whatever key they moved it to.
    if matches_binding(&key, &keys.paste_name) {
        return Some(Command::PasteCursorName);
    }
    if matches_binding(&key, &keys.paste_path) {
        return Some(Command::PasteCursorPath);
    }
    if matches_binding(&key, &keys.quick_filter) {
        // The activation key is a toggle: pressed again while a filter is
        // already active on this panel, it exits and clears the filter
        // instead of restarting one (quick-filter "Clearing the quick
        // filter": "the activation key toggles the filter").
        return Some(if state.active_panel().quick_filter.is_some() {
            Command::QuickFilterEnd
        } else {
            Command::QuickFilterStart
        });
    }
    if matches_binding(&key, &keys.fuzzy_jump) {
        return Some(Command::FuzzyJumpOpen);
    }

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    // Ctrl+T/Ctrl+W/Alt+1..9 panel tabs (panel-tabs "New tab", "Close tab",
    // "Switch tab"). Checked ahead of the general match below only because
    // Alt+<digit> would otherwise fall through to nothing (digits aren't
    // claimed by any other alt-combination here).
    if ctrl && !alt {
        match key.code {
            // Ctrl+C requests quit from the panels in any command-line
            // state (typing, quick filter, or type-ahead) — the universal
            // terminal interrupt chord, routed through the same
            // confirmation dialog as every other quit trigger
            // (application-shell "Quit request keys and confirmation").
            // Checked ahead of the quick-filter/type-ahead blocks above
            // implicitly, since neither of those claims Ctrl-modified keys.
            KeyCode::Char('c') | KeyCode::Char('C') => return Some(Command::RequestQuit),
            KeyCode::Char('t') | KeyCode::Char('T') => return Some(Command::OpenTab),
            KeyCode::Char('w') | KeyCode::Char('W') => return Some(Command::CloseTab),
            _ => {}
        }
    }
    if alt && !ctrl {
        if let KeyCode::Char(c) = key.code {
            if let Some(n) = c.to_digit(10) {
                if (1..=9).contains(&n) {
                    return Some(Command::SwitchTab(n as usize));
                }
            }
        }
    }

    match key.code {
        // Sort modes: Ctrl+F3..Ctrl+F6 pick a key, Ctrl+F7 restores
        // enumeration order.
        KeyCode::F(3) if ctrl => Some(Command::SetSortMode { side: active, mode: SortMode::Name }),
        KeyCode::F(4) if ctrl => Some(Command::SetSortMode { side: active, mode: SortMode::Extension }),
        KeyCode::F(5) if ctrl => Some(Command::SetSortMode { side: active, mode: SortMode::Time }),
        KeyCode::F(6) if ctrl => Some(Command::SetSortMode { side: active, mode: SortMode::Size }),
        KeyCode::F(7) if ctrl => Some(Command::SetSortMode { side: active, mode: SortMode::Unsorted }),

        KeyCode::F(1) if alt => Some(Command::OpenDriveSelect(PanelSide::Left)),
        KeyCode::F(2) if alt => Some(Command::OpenDriveSelect(PanelSide::Right)),
        KeyCode::F(7) if alt => Some(Command::FindFileOpen),

        KeyCode::F(1) => Some(Command::HelpOpen),
        KeyCode::F(2) => Some(Command::UserMenuOpen),
        KeyCode::F(3) => Some(Command::RequestViewer),
        // `RequestEditor` resolves the external-editor/built-in/size-cap
        // precedence itself (builtin-editor "External editor takes
        // precedence"); it supersedes the M4 `RequestExternalEditor` as the
        // F4 keybinding target, though that command (and its handler) stays
        // in place, reused internally.
        KeyCode::F(4) => Some(Command::RequestEditor),
        KeyCode::F(5) => Some(Command::RequestCopy),
        KeyCode::F(6) => Some(Command::RequestMove),
        KeyCode::F(7) => Some(Command::RequestMkdir),
        KeyCode::F(8) => Some(Command::RequestDelete),
        KeyCode::F(9) => Some(Command::MenuOpen),
        KeyCode::F(10) => Some(Command::RequestQuit),

        KeyCode::Char('r') | KeyCode::Char('R') if ctrl => Some(Command::RereadPanel(active)),
        KeyCode::Char('l') | KeyCode::Char('L') if ctrl => Some(Command::ToggleInfoMode(active)),
        KeyCode::Char('o') | KeyCode::Char('O') if ctrl => Some(Command::ShowScrollback),

        // Up/Down walk command history while something is typed, and move
        // the panel cursor when the buffer is empty. Esc is the documented
        // way to hand them back to the panel.
        KeyCode::Up if typing => Some(Command::CommandLineHistoryPrev),
        KeyCode::Down if typing => Some(Command::CommandLineHistoryNext),
        KeyCode::Up => Some(Command::MoveCursor(CursorMove::Up(1))),
        KeyCode::Down => Some(Command::MoveCursor(CursorMove::Down(1))),
        // Esc is unconditional at panel level: it asks to quit regardless of
        // command-line content, replacing the old clear-the-buffer meaning
        // (backspacing to empty is now what hands Up/Down back to the panel
        // — see the Backspace arms just below) (application-shell "Quit
        // request keys and confirmation"; command-line "Command history
        // navigation").
        KeyCode::Esc => Some(Command::RequestQuit),
        KeyCode::Backspace if typing => Some(Command::CommandLineBackspace),
        KeyCode::Backspace => Some(Command::ParentDir),

        KeyCode::PageUp if ctrl => Some(Command::ParentDir),
        KeyCode::PageUp => Some(Command::MoveCursor(CursorMove::Up(page_size))),
        KeyCode::PageDown => Some(Command::MoveCursor(CursorMove::Down(page_size))),
        KeyCode::Home => Some(Command::MoveCursor(CursorMove::Home)),
        KeyCode::End => Some(Command::MoveCursor(CursorMove::End)),
        KeyCode::Tab => Some(Command::ToggleActivePanel),
        KeyCode::Enter => Some(Command::Enter),
        KeyCode::Insert => Some(Command::ToggleSelectAtCursor),

        // Alt+letter starts the type-ahead jump, which then owns plain
        // printables until it is dismissed. Mutually exclusive with the
        // Ctrl+P quick filter: if a filter is already active on this panel,
        // Alt+letter is ignored rather than starting a second, competing
        // input mode — Esc must exit the quick filter first (quick-filter
        // "Navigation is restricted to matching entries"; type-ahead-jump
        // "Mini-status display of the active pattern").
        KeyCode::Char(c) if alt && !ctrl && c.is_alphanumeric() && state.active_panel().quick_filter.is_none() => Some(Command::QuickSearchStart(c)),

        // The grey +/-/* selection keys and typed +/-/* are the same key
        // event on Windows (crossterm cannot distinguish the numeric
        // keypad here), so an empty command line means "select" and a
        // non-empty one means "type".
        KeyCode::Char('+') if is_plain(&key) && !typing => Some(Command::GroupSelectAll),
        KeyCode::Char('-') if is_plain(&key) && !typing => Some(Command::GroupDeselectAll),
        KeyCode::Char('*') if is_plain(&key) && !typing => Some(Command::InvertSelection),

        KeyCode::Char(c) if is_plain(&key) => Some(Command::CommandLineChar(c)),
        _ => None,
    }
}

/// The quit-confirmation dialog (`state.quit_confirm`), checked first of
/// every overlay in `map_key`. Esc keeps its universal dialog-cancel
/// meaning; Ctrl+C — pressed again, since it is what opened the dialog from
/// most contexts — confirms instead, the terminal "press Ctrl+C again to
/// exit" convention (application-shell "Quit request keys and
/// confirmation"; design D4).
fn map_quit_confirm_key(key: KeyEvent) -> Option<Command> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Command::ConfirmQuit);
    }
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => Some(Command::ConfirmQuit),
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => Some(Command::CancelQuit),
        _ => None,
    }
}

fn map_menu_key(key: KeyEvent) -> Option<Command> {
    // Ctrl+C requests quit with a pull-down open, ahead of the menu's own
    // key handling (application-shell "Quit request keys and
    // confirmation").
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Command::RequestQuit);
    }
    match key.code {
        KeyCode::Esc => Some(Command::MenuCollapse),
        KeyCode::F(9) | KeyCode::F(10) => Some(Command::MenuClose),
        KeyCode::Up => Some(Command::MenuSelectPrev),
        KeyCode::Down => Some(Command::MenuSelectNext),
        KeyCode::Left => Some(Command::MenuPrevMenu),
        KeyCode::Right => Some(Command::MenuNextMenu),
        KeyCode::Enter => Some(Command::MenuActivate),
        KeyCode::Char(c) if is_plain(&key) => Some(Command::MenuHotkey(c)),
        _ => None,
    }
}

/// Ctrl+J fuzzy-jump dialog (fuzzy-jump "Fuzzy jump dialog invocation").
fn map_fuzzy_jump_key(key: KeyEvent) -> Option<Command> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Command::RequestQuit);
    }
    match key.code {
        KeyCode::Esc => Some(Command::FuzzyJumpCancel),
        KeyCode::Enter => Some(Command::FuzzyJumpConfirm),
        KeyCode::Backspace => Some(Command::FuzzyJumpBackspace),
        KeyCode::Up => Some(Command::FuzzyJumpMove(-1)),
        KeyCode::Down => Some(Command::FuzzyJumpMove(1)),
        KeyCode::Char(c) if is_plain(&key) => Some(Command::FuzzyJumpChar(c)),
        _ => None,
    }
}

/// Alt+F7 find-file dialog. Its own precedence, highest first: the pattern
/// input stage (no search submitted yet) claims printables/Backspace/Enter;
/// once a search is in flight (or done), Up/Down/Enter move over and
/// confirm a result instead (find-file "Find-file invocation", "Navigate to
/// a chosen result").
fn map_find_file_key(key: KeyEvent, dialog: &FindFileState) -> Option<Command> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Command::RequestQuit);
    }
    if dialog.request.is_none() {
        return match key.code {
            KeyCode::Esc => Some(Command::FindFileCancel),
            KeyCode::Enter => Some(Command::FindFileSubmit),
            KeyCode::Backspace => Some(Command::FindFileBackspace),
            KeyCode::Char(c) if is_plain(&key) => Some(Command::FindFileChar(c)),
            _ => None,
        };
    }
    match key.code {
        KeyCode::Esc => Some(Command::FindFileCancel),
        KeyCode::Enter => Some(Command::FindFileConfirm),
        KeyCode::Up => Some(Command::FindFileMove(-1)),
        KeyCode::Down => Some(Command::FindFileMove(1)),
        _ => None,
    }
}

/// F2 user menu (user-menu "Navigate and dismiss the user menu").
fn map_user_menu_key(key: KeyEvent) -> Option<Command> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Command::RequestQuit);
    }
    match key.code {
        KeyCode::Esc => Some(Command::UserMenuCancel),
        KeyCode::Enter => Some(Command::UserMenuConfirm),
        KeyCode::Up => Some(Command::UserMenuMove(-1)),
        KeyCode::Down => Some(Command::UserMenuMove(1)),
        _ => None,
    }
}

/// Options → Themes picker (theme-selection "Picker navigation, apply, and
/// cancel").
fn map_theme_picker_key(key: KeyEvent) -> Option<Command> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Command::RequestQuit);
    }
    match key.code {
        KeyCode::Esc => Some(Command::ThemePickerCancel),
        KeyCode::Enter => Some(Command::ThemePickerConfirm),
        KeyCode::Up => Some(Command::ThemePickerMove(-1)),
        KeyCode::Down => Some(Command::ThemePickerMove(1)),
        _ => None,
    }
}

/// The Enter-on-file action menu: Up/Down moves the highlight, Enter
/// activates it, Esc closes with no action, and any other plain letter is
/// tried as a first-letter hotkey — `core::update` no-ops it if nothing
/// matches (file-action-menu "Menu contents, ordering, and navigation":
/// "Up/Down SHALL move the highlight, Enter SHALL activate the highlighted
/// entry and close the menu, Esc SHALL close the menu with no action taken,
/// and pressing an entry's first letter SHALL activate that entry
/// directly").
fn map_file_action_menu_key(key: KeyEvent, _dialog: &FileActionMenuState) -> Option<Command> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Command::RequestQuit);
    }
    match key.code {
        KeyCode::Esc => Some(Command::FileActionMenuCancel),
        KeyCode::Enter => Some(Command::FileActionMenuConfirm),
        KeyCode::Up => Some(Command::FileActionMenuMove(-1)),
        KeyCode::Down => Some(Command::FileActionMenuMove(1)),
        KeyCode::Char(c) if is_plain(&key) => Some(Command::FileActionMenuHotkey(c)),
        _ => None,
    }
}

/// F1 Help window + About dialog. `H`/`C` activate the `Help`/`Cancel`
/// buttons exactly like Enter/Esc (help-and-about "Help window buttons");
/// the About dialog (layered over the list) and a topic page both treat any
/// of Enter/Esc/`O` as "go back a level", which `Command::HelpCancel`
/// already implements uniformly via `HelpState::back`.
fn map_help_key(key: KeyEvent, dialog: &HelpState) -> Option<Command> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Command::RequestQuit);
    }
    if dialog.about_open {
        return match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('o') | KeyCode::Char('O') => Some(Command::HelpCancel),
            _ => None,
        };
    }
    if dialog.page.is_some() {
        return match key.code {
            KeyCode::Esc => Some(Command::HelpCancel),
            _ => None,
        };
    }
    match key.code {
        KeyCode::Esc | KeyCode::Char('c') | KeyCode::Char('C') => Some(Command::HelpCancel),
        KeyCode::Enter | KeyCode::Char('h') | KeyCode::Char('H') => Some(Command::HelpActivate),
        KeyCode::Up => Some(Command::HelpMove(-1)),
        KeyCode::Down => Some(Command::HelpMove(1)),
        _ => None,
    }
}

/// The startup-warning modal (currently raised only for a malformed
/// `usermenu.toml`): any key dismisses it, matching the "Press any key to
/// continue" convention `app.rs::wait_for_key` already uses for suspend/
/// resume prompts elsewhere in this codebase (user-menu "Malformed file
/// warns and falls back without overwriting").
fn map_startup_warning_key(_key: KeyEvent) -> Option<Command> {
    Some(Command::DismissStartupWarning)
}

fn map_drive_select_key(key: KeyEvent) -> Option<Command> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Command::RequestQuit);
    }
    match key.code {
        KeyCode::Esc => Some(Command::DriveSelectCancel),
        KeyCode::Up => Some(Command::DriveSelectMove(-1)),
        KeyCode::Down => Some(Command::DriveSelectMove(1)),
        KeyCode::Home | KeyCode::PageUp => Some(Command::DriveSelectMove(isize::MIN / 2)),
        KeyCode::End | KeyCode::PageDown => Some(Command::DriveSelectMove(isize::MAX / 2)),
        KeyCode::Enter => Some(Command::DriveSelectConfirm),
        _ => None,
    }
}

/// While the type-ahead jump owns the keyboard, plain printables extend the
/// pattern and Backspace/anything-else it doesn't recognize dismiss it — so
/// the command line never sees a key quick-search consumed, and vice versa.
/// A movement key (arrows, Home/End, Page Up/Down) is special: it exits
/// type-ahead *and* is applied to the panel cursor as a normal movement, in
/// the same keystroke (type-ahead-jump "A movement key exits type-ahead and
/// is applied to the panel"; design D5) — `core::update` clears
/// `quick_search` as a side effect of any `MoveCursor` command while it is
/// active (see `update::UiPhase::Panels` handling), so simply emitting the
/// movement command here does both at once. Esc is no longer an exit key:
/// it requests quit instead, and cancelling that dialog leaves type-ahead
/// active with its pattern intact (type-ahead-jump "Exiting type-ahead and
/// restoring command-line routing": "Esc SHALL NOT exit type-ahead ... it
/// requests application quit"; application-shell "Quit request keys and
/// confirmation").
fn map_quick_search_key(key: KeyEvent, page_size: usize) -> Option<Command> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Command::RequestQuit);
    }
    match key.code {
        KeyCode::Esc => Some(Command::RequestQuit),
        KeyCode::Backspace => Some(Command::QuickSearchBackspace),
        KeyCode::Char(c) if is_plain(&key) => Some(Command::QuickSearchChar(c)),
        KeyCode::Up => Some(Command::MoveCursor(CursorMove::Up(1))),
        KeyCode::Down => Some(Command::MoveCursor(CursorMove::Down(1))),
        KeyCode::PageUp => Some(Command::MoveCursor(CursorMove::Up(page_size))),
        KeyCode::PageDown => Some(Command::MoveCursor(CursorMove::Down(page_size))),
        KeyCode::Home => Some(Command::MoveCursor(CursorMove::Home)),
        KeyCode::End => Some(Command::MoveCursor(CursorMove::End)),
        _ => Some(Command::QuickSearchEnd),
    }
}

/// The name `parse_binding` normalizes a key to, for matching a live event
/// against a configured binding.
fn key_name(code: KeyCode) -> Option<String> {
    Some(match code {
        KeyCode::Char(c) => c.to_lowercase().to_string(),
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Esc => "esc".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Delete => "delete".to_string(),
        KeyCode::Insert => "insert".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::PageUp => "pageup".to_string(),
        KeyCode::PageDown => "pagedown".to_string(),
        KeyCode::Up => "up".to_string(),
        KeyCode::Down => "down".to_string(),
        KeyCode::Left => "left".to_string(),
        KeyCode::Right => "right".to_string(),
        KeyCode::F(n) => format!("f{n}"),
        _ => return None,
    })
}

/// Ctrl and Alt must match exactly. Shift is only checked when the binding
/// asks for it, since for a printable key the shift is already baked into
/// the character the terminal delivered.
pub fn matches_binding(key: &KeyEvent, binding: &KeyBinding) -> bool {
    let Some(name) = key_name(key.code) else { return false };
    name == binding.key
        && key.modifiers.contains(KeyModifiers::CONTROL) == binding.ctrl
        && key.modifiers.contains(KeyModifiers::ALT) == binding.alt
        && (!binding.shift || key.modifiers.contains(KeyModifiers::SHIFT))
}

fn map_file_op_setup_key(key: KeyEvent, setup: &FileOpSetup) -> Option<Command> {
    // Ctrl+C requests quit from every file-op setup dialog, ahead of each
    // dialog's own key handling — checked with an explicit modifier guard
    // since `DestinationInput`/`RenameInput` below otherwise claim every
    // `Char(c)` unconditionally (application-shell "Quit request keys and
    // confirmation").
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Command::RequestQuit);
    }
    match setup {
        FileOpSetup::DeleteConfirm { .. } => match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => Some(Command::FileOpConfirm),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => Some(Command::FileOpCancel),
            _ => None,
        },
        // `RenameInput` reuses the same `FileOpInput*`/`FileOpConfirm`/
        // `FileOpCancel` commands `DestinationInput` uses (fs_ops::dialog
        // "Reuses the same ... commands `DestinationInput` uses").
        FileOpSetup::DestinationInput { .. } | FileOpSetup::RenameInput { .. } => match key.code {
            KeyCode::Enter => Some(Command::FileOpConfirm),
            KeyCode::Esc => Some(Command::FileOpCancel),
            KeyCode::Backspace => Some(Command::FileOpInputBackspace),
            KeyCode::Char(c) => Some(Command::FileOpInputChar(c)),
            _ => None,
        },
    }
}

fn map_file_op_running_key(key: KeyEvent, dialog: &RunningDialog) -> Option<Command> {
    // Ctrl+C requests quit from every running-job dialog — including the
    // Progress dialog, whose own plain `c`/`C` already means "cancel job"
    // (matched below with no modifier guard), so the Ctrl+C case must be
    // intercepted here first. Confirming the quit dialog this opens aborts
    // the job via the same cancel path before quitting (design D3;
    // application-shell "Quit request keys and confirmation").
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Command::RequestQuit);
    }
    match dialog {
        RunningDialog::Progress { .. } => match key.code {
            KeyCode::Esc | KeyCode::Char('c') | KeyCode::Char('C') => Some(Command::FileOpCancelJob),
            _ => None,
        },
        RunningDialog::Conflict { rename_input: Some(_), .. } => match key.code {
            KeyCode::Enter => Some(Command::FileOpConfirm),
            KeyCode::Esc => Some(Command::FileOpCancel),
            KeyCode::Backspace => Some(Command::FileOpInputBackspace),
            KeyCode::Char(c) => Some(Command::FileOpInputChar(c)),
            _ => None,
        },
        RunningDialog::Conflict { rename_input: None, .. } => match key.code {
            KeyCode::Char('o') | KeyCode::Char('O') => Some(Command::FileOpConflictChoice(ConflictChoice::Overwrite)),
            KeyCode::Char('w') | KeyCode::Char('W') => Some(Command::FileOpConflictChoice(ConflictChoice::OverwriteAll)),
            KeyCode::Char('s') | KeyCode::Char('S') => Some(Command::FileOpConflictChoice(ConflictChoice::Skip)),
            KeyCode::Char('a') | KeyCode::Char('A') => Some(Command::FileOpConflictChoice(ConflictChoice::SkipAll)),
            KeyCode::Char('r') | KeyCode::Char('R') => Some(Command::FileOpBeginRename),
            _ => None,
        },
        RunningDialog::Error { .. } => match key.code {
            KeyCode::Char('r') | KeyCode::Char('R') => Some(Command::FileOpErrorChoice(ErrorChoice::Retry)),
            KeyCode::Char('s') | KeyCode::Char('S') => Some(Command::FileOpErrorChoice(ErrorChoice::Skip)),
            KeyCode::Char('a') | KeyCode::Char('A') => Some(Command::FileOpErrorChoice(ErrorChoice::SkipAll)),
            KeyCode::Char('b') | KeyCode::Char('B') | KeyCode::Esc => Some(Command::FileOpErrorChoice(ErrorChoice::Abort)),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests;
