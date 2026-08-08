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
use filecommand_core::editor::{EditorMove, EditorState};
use filecommand_core::fs_ops::dialog::{FileOpSetup, RunningDialog};
use filecommand_core::fs_ops::{ConflictChoice, ErrorChoice};
use filecommand_core::listing::SortMode;
use filecommand_core::panel::CursorMove;
use filecommand_core::viewer::ViewerState;
use filecommand_core::{Command, PanelSide, State, UiPhase};

pub fn map_key(key: KeyEvent, state: &State, page_size: usize, keys: &Keys) -> Option<Command> {
    // Modal overlays come first: while one is up it owns every key it
    // understands, regardless of the phase underneath.
    if state.drive_select.is_some() {
        return map_drive_select_key(key);
    }
    if state.menu.is_some() {
        return map_menu_key(key);
    }

    match &state.phase {
        UiPhase::QuitConfirm => match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => Some(Command::ConfirmQuit),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => Some(Command::CancelQuit),
            _ => None,
        },
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
                return map_quick_search_key(key);
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

    // Config-overridable bindings win over the compiled-in map, so a user
    // who rebinds Ctrl+] to something else still gets the default meaning
    // of whatever key they moved it to.
    if matches_binding(&key, &keys.paste_name) {
        return Some(Command::PasteCursorName);
    }
    if matches_binding(&key, &keys.paste_path) {
        return Some(Command::PasteCursorPath);
    }

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

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
        KeyCode::Esc if typing => Some(Command::CommandLineClear),
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
        // printables until it is dismissed.
        KeyCode::Char(c) if alt && !ctrl && c.is_alphanumeric() => Some(Command::QuickSearchStart(c)),

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

fn map_menu_key(key: KeyEvent) -> Option<Command> {
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

fn map_drive_select_key(key: KeyEvent) -> Option<Command> {
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
/// pattern and anything else dismisses it — so the command line never sees
/// a key quick-search consumed, and vice versa.
fn map_quick_search_key(key: KeyEvent) -> Option<Command> {
    match key.code {
        KeyCode::Backspace => Some(Command::QuickSearchBackspace),
        KeyCode::Char(c) if is_plain(&key) => Some(Command::QuickSearchChar(c)),
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
    match setup {
        FileOpSetup::DeleteConfirm { .. } => match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => Some(Command::FileOpConfirm),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => Some(Command::FileOpCancel),
            _ => None,
        },
        FileOpSetup::DestinationInput { .. } => match key.code {
            KeyCode::Enter => Some(Command::FileOpConfirm),
            KeyCode::Esc => Some(Command::FileOpCancel),
            KeyCode::Backspace => Some(Command::FileOpInputBackspace),
            KeyCode::Char(c) => Some(Command::FileOpInputChar(c)),
            _ => None,
        },
    }
}

fn map_file_op_running_key(key: KeyEvent, dialog: &RunningDialog) -> Option<Command> {
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
