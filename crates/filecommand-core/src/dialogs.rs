//! F2 user-menu and F1 Help/About window state, plus the Help window's
//! compiled-in static topic text (help-and-about; user-menu).
//!
//! Both dialogs are simple enough (a cursor over a fixed or config-loaded
//! list) that they share this one small module rather than each getting a
//! file of their own.
//!
//! The Enter-on-file action menu (file-action-menu) lives here too: its
//! state is the same shape (a cursor over a fixed list), just with a
//! per-open entry list rather than a config-loaded one.

use std::ffi::OsString;

// ---------------------------------------------------------------------
// Unified overlay geometry (responsive-layout)
// ---------------------------------------------------------------------

/// A clamped, centered overlay rectangle in terminal-relative coordinates
/// (`x`/`y` are offsets from the terminal's top-left corner, not absolute
/// screen coordinates — callers add their own area's origin). Never
/// `ratatui::Rect`: `filecommand-core` has no UI-framework dependency, so
/// every view converts this at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

/// One dimension of the unified overlay-geometry rule (design D6): prefer
/// `preferred`, but never let it push the overlay closer than a 2-cell
/// margin to the terminal edge, and never let it shrink below `minimum`
/// (itself clamped to `terminal`, so a `minimum` larger than the terminal
/// doesn't overflow it) — `terminal` is the hard ceiling either way. A
/// direct generalization of the Help window's pre-existing
/// `help_window_height` (kept for backward-compatible scroll math; see
/// `help_window_height`'s doc comment).
pub fn clamp_overlay_dim(preferred: u16, minimum: u16, terminal: u16) -> u16 {
    let capped = preferred.min(terminal.saturating_sub(2));
    capped.max(minimum.min(terminal)).min(terminal)
}

/// The unified overlay-geometry rule (responsive-layout "Unified overlay
/// geometry"; design D6): given an overlay's `preferred` and `minimum`
/// sizes and the current `terminal` size (each a `(width, height)` pair),
/// compute the clamped size via [`clamp_overlay_dim`] on each dimension
/// independently, then center the result. Shared by every overlay —
/// splash, Help, About, the operation/input/confirmation/error/progress
/// dialogs, drive select, find-file, fuzzy jump, user menu, quit dialog —
/// so they can never disagree about how to fit the screen, and by the F9
/// pull-down boxes for their width/height clamping (though pull-downs
/// reposition themselves rather than centering; see `menubar.rs`).
pub fn overlay_rect(preferred: (u16, u16), minimum: (u16, u16), terminal: (u16, u16)) -> OverlayRect {
    let width = clamp_overlay_dim(preferred.0, minimum.0, terminal.0);
    let height = clamp_overlay_dim(preferred.1, minimum.1, terminal.1);
    OverlayRect {
        x: terminal.0.saturating_sub(width) / 2,
        y: terminal.1.saturating_sub(height) / 2,
        width,
        height,
    }
}

/// The open F2 user menu: just a cursor over `State::user_menu_entries`,
/// which is loaded once at startup from `usermenu.toml` and does not change
/// while the menu is open (user-menu "Open the F2 user menu", "Navigate and
/// dismiss the user menu"). The reducer (`update.rs::handle_user_menu`)
/// extends the cursor's usable domain by one past `entries.len()` for a
/// compiled-in "Themes" slot — `UserMenuEntry`/`state.user_menu_entries`
/// stay untouched (design D3 of `user-menu-themes-entry`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UserMenuState {
    pub cursor: usize,
}

impl UserMenuState {
    pub fn new() -> UserMenuState {
        UserMenuState::default()
    }

    /// Move the highlight by `delta`, clamped within `[0, len)`. A no-op on
    /// an empty menu (user-menu "F2 with no entries opens an empty menu").
    pub fn move_cursor(&mut self, delta: isize, len: usize) {
        if len == 0 {
            self.cursor = 0;
            return;
        }
        let next = self.cursor as isize + delta;
        self.cursor = next.clamp(0, len as isize - 1) as usize;
    }
}

// ---------------------------------------------------------------------
// Options -> Themes picker
// ---------------------------------------------------------------------

/// The open Options → Themes picker: a cursor over the compiled-in theme
/// list (`crate::theme::BUILTIN_THEME_NAMES`), opened with the currently
/// active theme's row pre-highlighted (theme-selection "Options menu opens
/// the theme picker" — "Active theme is marked and pre-highlighted").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ThemePickerState {
    pub highlight: usize,
}

impl ThemePickerState {
    /// Open the picker with `active_theme_name`'s row highlighted. Falls
    /// back to the first row if the active theme name is somehow not among
    /// the built-ins — never happens in practice (every `State::theme` comes
    /// from `Theme::by_name`), but keeps this infallible rather than
    /// panicking.
    pub fn open(active_theme_name: &str) -> ThemePickerState {
        let highlight = crate::theme::BUILTIN_THEME_NAMES.iter().position(|n| *n == active_theme_name).unwrap_or(0);
        ThemePickerState { highlight }
    }

    /// Move the highlight by `delta`, clamped within the theme list
    /// (theme-selection "Picker navigation, apply, and cancel").
    pub fn move_cursor(&mut self, delta: isize) {
        let len = crate::theme::BUILTIN_THEME_NAMES.len();
        let next = self.highlight as isize + delta;
        self.highlight = next.clamp(0, len as isize - 1) as usize;
    }
}

// ---------------------------------------------------------------------
// Enter-on-file action menu (file-action-menu)
// ---------------------------------------------------------------------

/// One entry in the file-action menu, in menu order. `Run` is included only
/// when the target is executable, and always sorts first (file-action-menu
/// "Menu contents, ordering, and navigation"). `SendToClipboard` sits
/// immediately after `Edit` (or first, for directory targets, which omit
/// `View`/`Edit`/`Run`) and never mutates the filesystem (file-action-menu
/// "No mutation without an intervening dialog").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileActionMenuEntry {
    Run,
    View,
    Edit,
    Copy,
    Rename,
    Move,
    Delete,
    SendToClipboard,
}

impl FileActionMenuEntry {
    /// Display label. Its first character is also the entry's first-letter
    /// hotkey (file-action-menu "pressing an entry's first letter SHALL
    /// activate that entry directly").
    pub fn label(self) -> &'static str {
        match self {
            FileActionMenuEntry::Run => "Run",
            FileActionMenuEntry::View => "View",
            FileActionMenuEntry::Edit => "Edit",
            FileActionMenuEntry::Copy => "Copy",
            FileActionMenuEntry::Rename => "Rename",
            FileActionMenuEntry::Move => "Move",
            FileActionMenuEntry::Delete => "Delete",
            FileActionMenuEntry::SendToClipboard => "Send to clipboard",
        }
    }
}

/// The open file-action menu: the name of the cursor entry it targets
/// (captured at open time, independent of the panel's multi-selection —
/// file-action-menu "Enter on a file opens the action menu": "SHALL NOT
/// consume or alter the multi-entry selection"), its ordered entry list,
/// and the highlighted row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileActionMenuState {
    pub target_name: OsString,
    pub entries: Vec<FileActionMenuEntry>,
    pub cursor: usize,
    /// Whether this menu was opened on an entry that was already a member
    /// of the panel's selection set, captured at open time exactly like
    /// `target_name` — when true, `crate::update::activate_file_action`
    /// scopes Copy, Move, Delete, and Send to clipboard to the whole
    /// selection set instead of `target_name` alone, and names the
    /// resulting dialog with the count (mouse-basics design D4;
    /// file-action-menu "Directory targets and selection-scoped
    /// invocation"). Always `false` via [`FileActionMenuState::new`], the
    /// keyboard Enter path, which stays single-target per that same
    /// requirement ("Enter-key invocation SHALL remain single-target and
    /// file-only").
    pub selection_scoped: bool,
}

impl FileActionMenuState {
    /// The keyboard Enter path: file-only, never selection-scoped
    /// (file-action-menu "Enter stays single-target"). Opens with the
    /// first entry highlighted — `Run` when `executable`, else `View`
    /// (file-action-menu "Menu contents, ordering, and navigation").
    pub fn new(target_name: OsString, executable: bool) -> FileActionMenuState {
        Self::open(target_name, false, executable, false)
    }

    /// The general constructor `new` delegates to, and mouse right-click
    /// uses directly (mouse-input "Right-click opens the action menu").
    /// `is_dir` omits View, Edit, and Run from the menu — they have no
    /// meaning for a directory (file-action-menu "Directory targets and
    /// selection-scoped invocation": "Directory menu contents").
    /// `selection_scoped` is recorded for `activate_file_action` to read
    /// later; it does not change which entries are listed.
    pub fn open(target_name: OsString, is_dir: bool, executable: bool, selection_scoped: bool) -> FileActionMenuState {
        let mut entries = Vec::with_capacity(7);
        if !is_dir {
            if executable {
                entries.push(FileActionMenuEntry::Run);
            }
            entries.push(FileActionMenuEntry::View);
            entries.push(FileActionMenuEntry::Edit);
        }
        entries.push(FileActionMenuEntry::SendToClipboard);
        entries.extend([
            FileActionMenuEntry::Copy,
            FileActionMenuEntry::Rename,
            FileActionMenuEntry::Move,
            FileActionMenuEntry::Delete,
        ]);
        FileActionMenuState { target_name, entries, cursor: 0, selection_scoped }
    }

    /// Move the highlight by `delta`, clamped within the entry list — same
    /// clamp-not-wrap convention as [`UserMenuState::move_cursor`].
    pub fn move_cursor(&mut self, delta: isize) {
        let len = self.entries.len();
        if len == 0 {
            self.cursor = 0;
            return;
        }
        let next = self.cursor as isize + delta;
        self.cursor = next.clamp(0, len as isize - 1) as usize;
    }

    pub fn selected(&self) -> FileActionMenuEntry {
        self.entries[self.cursor]
    }

    /// The entry whose label starts with `c` (case-insensitive), preferring
    /// the first match in menu order. This resolves the `R`un/`R`ename
    /// collision in favor of Run, which — when present — is always listed
    /// first (design D1) (file-action-menu "First-letter hotkey activates
    /// directly").
    pub fn hotkey_action(&self, c: char) -> Option<FileActionMenuEntry> {
        let want = c.to_ascii_uppercase();
        self.entries.iter().copied().find(|e| e.label().chars().next().map(|f| f.to_ascii_uppercase()) == Some(want))
    }
}

// ---------------------------------------------------------------------
// F1 Help window + About dialog
// ---------------------------------------------------------------------

/// The fixed v1 Help topic list, in display order. `About FileCommand` is
/// always first and is special-cased (it opens the About dialog rather than
/// a topic page) — see [`HelpState::activate`] (help-and-about "Help topic
/// list").
pub const HELP_TOPICS: [&str; 11] = [
    "About FileCommand",
    "Keyboard reference",
    "Mouse",
    "Panels and display modes",
    "File operations",
    "Menus",
    "Viewer",
    "Editor",
    "Command line",
    "Modern extras",
    "Configuration",
];

/// Index of the always-first `About FileCommand` entry.
pub const ABOUT_TOPIC_INDEX: usize = 0;

/// The open F1 Help window: either the topic list (with a highlight and
/// scroll offset) or a topic page, plus whether the About dialog is layered
/// on top (help-and-about "F1 Help window frame and identity header").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelpState {
    /// Highlighted row in the topic list. Preserved while viewing a page or
    /// the About dialog so Esc/OK returns to the same spot (help-and-about
    /// "Esc returns from a topic page to the list", "OK dismisses the About
    /// dialog").
    pub cursor: usize,
    /// The topic list's scroll offset — the index of its first visible row.
    pub scroll: usize,
    /// `Some(topic index)` while a topic page (not the list) is shown.
    pub page: Option<usize>,
    /// The About dialog is layered over the topic list.
    pub about_open: bool,
}

impl HelpState {
    /// A freshly opened window: the topic list, `About FileCommand`
    /// highlighted first (help-and-about "List opens with About FileCommand
    /// highlighted first").
    pub fn new() -> HelpState {
        HelpState { cursor: 0, scroll: 0, page: None, about_open: false }
    }

    /// Move the topic-list highlight by `delta`, clamped, scrolling to keep
    /// it visible within `visible_rows` (help-and-about "Cursor moves
    /// through the topic list").
    pub fn move_cursor(&mut self, delta: isize, visible_rows: usize) {
        let last = HELP_TOPICS.len() as isize - 1;
        self.cursor = (self.cursor as isize + delta).clamp(0, last) as usize;
        self.scroll_to_cursor(visible_rows);
    }

    fn scroll_to_cursor(&mut self, visible_rows: usize) {
        let visible_rows = visible_rows.max(1);
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + visible_rows {
            self.scroll = self.cursor + 1 - visible_rows;
        }
    }

    /// Enter / the `Help` button on the highlighted topic: opens the About
    /// dialog for the special first entry, or the topic's page otherwise
    /// (help-and-about "Help button opens the highlighted topic", "Enter on
    /// About FileCommand opens the secondary-style dialog").
    pub fn activate(&mut self) {
        if self.cursor == ABOUT_TOPIC_INDEX {
            self.about_open = true;
        } else {
            self.page = Some(self.cursor);
        }
    }

    /// Esc / Cancel: dismiss the About dialog back to the list, else return
    /// a topic page to the list, else signal the whole window should close
    /// (`false`) (help-and-about "Esc returns from a topic page to the
    /// list", "Cancel button closes the window", "OK dismisses the About
    /// dialog").
    pub fn back(&mut self) -> bool {
        if self.about_open {
            self.about_open = false;
            true
        } else {
            self.page.take().is_some()
        }
    }
}

impl Default for HelpState {
    fn default() -> HelpState {
        HelpState::new()
    }
}

/// The Help window's preferred and minimum geometry — 62×19 preferred, down
/// to 40×10 — shared by `crate::update::handle_help`'s scroll math and
/// `filecommand-tui`'s renderer (via [`overlay_rect`]) so the two never
/// disagree about the window's actual on-screen size (help-and-about "F1
/// Help window frame and identity header"). Supersedes the old fixed
/// `help_window_height(term_rows) -> term_rows.clamp(10, 19)`, which had no
/// margin and no width dimension — the unified overlay rule generalizes it
/// (design D6).
pub const HELP_WINDOW_PREFERRED: (u16, u16) = (62, 19);
pub const HELP_WINDOW_MINIMUM: (u16, u16) = (40, 10);

/// The Help window's total height for a given terminal size, via the
/// unified overlay-geometry rule (help-and-about "Help window re-centers on
/// resize"; responsive-layout "Unified overlay geometry").
pub fn help_window_height(term_size: (u16, u16)) -> u16 {
    overlay_rect(HELP_WINDOW_PREFERRED, HELP_WINDOW_MINIMUM, term_size).height
}

/// The number of topic-list rows visible inside a Help window of
/// `window_height` total rows — everything left over once the top/bottom
/// frame, the three-line identity header (plus its separator), and the
/// button row (plus its separator) are accounted for. Shared by
/// `crate::update::handle_help`'s scroll math and `filecommand-tui`'s
/// renderer so the two never disagree about how many rows are actually on
/// screen (help-and-about "Scroll arrows appear only on overflow").
pub fn help_topic_visible_rows(window_height: u16) -> usize {
    const CHROME: u16 = 2 /* top+bottom frame */ + 3 /* identity header */ + 1 /* separator */ + 2 /* buttons + separator */;
    window_height.saturating_sub(CHROME).max(1) as usize
}

/// The v1 topic page bodies, compiled into the binary — no filesystem read
/// is ever performed to show one (help-and-about "Selecting a topic
/// replaces the list with its page"). `Keyboard reference` documents the
/// Ctrl/Alt F-key-bar variants explicitly (help-and-about "Keyboard
/// reference documents the modifier bar variants").
pub fn topic_page_text(topic_index: usize) -> &'static str {
    match topic_index {
        1 => KEYBOARD_REFERENCE,
        2 => MOUSE,
        3 => PANELS_AND_DISPLAY_MODES,
        4 => FILE_OPERATIONS,
        5 => MENUS,
        6 => VIEWER,
        7 => EDITOR,
        8 => COMMAND_LINE,
        9 => MODERN_EXTRAS,
        10 => CONFIGURATION,
        _ => "",
    }
}

const KEYBOARD_REFERENCE: &str = "\
F-key bar modifier variants:
  Ctrl+F3..F7  Sort by name/ext/time/size/unsorted
  Alt+F1/F2    Drive select (left/right panel)
  Alt+F7       Find file

Plain F-keys: F1 Help  F2 User menu  F3 View  F4 Edit
F5 Copy  F6 Move/rename  F7 Mkdir  F8 Delete
F9 Menu bar  F10 Quit

Other bindings:
  Tab          Switch active panel
  Up/Down      Move cursor
  PgUp/PgDn    Page up/down
  Enter        Open / run
  Ctrl+L       Toggle Info mode
  Ctrl+R       Re-read panel
  Ctrl+P       Quick filter
  Ctrl+J       Fuzzy directory jump
  Ctrl+T/W     New/close tab
  Alt+1..9     Switch to tab N
  Alt+letter   Type-ahead jump to entry
  Ctrl+Left/Right  Adjust the panel split
  Ctrl+=       Reset the panel split to 50/50";

const MOUSE: &str = "\
Mouse capture is on by default. Click an entry to focus its panel
and move the cursor there without changing the selection; Ctrl+click
toggles that entry's selection in place. Double-click acts as Enter.
The wheel moves the cursor of the panel under the pointer three rows
per notch; in the viewer it scrolls three lines, in the editor it
moves the caret three lines. Clicking a function-key-bar slot, a
menu-bar title, a pull-down item, or a dialog button does exactly
what the key would; a click outside an open pull-down closes it.

Right-click opens the file-action menu for the clicked entry: on a
directory it omits View, Edit, and Run, and on an already-selected
entry it scopes Copy, Move, Delete, and Send to clipboard to the
whole selection instead of just the one entry. Shift+drag still
selects text natively in the terminal emulator.

config.toml's [mouse] enabled = false, or the --nomouse launch flag,
turns capture off entirely.";

const PANELS_AND_DISPLAY_MODES: &str = "\
Each panel shows a directory listing and can be switched between
several display modes from the Left/Right menu: Full (name, size,
date, time), Brief (names only, three columns), Info (system and
drive summary), Tree (a lazily-expanded directory tree that drives
the opposite panel), and Quick view (a live preview of the file
under the opposite panel's cursor).";

const FILE_OPERATIONS: &str = "\
F5 copies, F6 moves/renames, F7 creates a directory, and F8 deletes
the selected entries (or the entry under the cursor when nothing is
selected). Ins toggles selection on one entry; +, -, and * select,
deselect, and invert a wildcard group.";

const MENUS: &str = "\
F9 opens the menu bar. Left/Right/Files/Commands/Options each drop
down a pull-down; Left and Right act on their own panel regardless
of which one is focused. Esc closes a pull-down, then the bar.";

const VIEWER: &str = "\
F3 opens the read-only viewer. F4 toggles text/hex mode, F2 toggles
line wrap, F7 opens a search prompt. The viewer streams even very
large files without loading them into memory.";

const EDITOR: &str = "\
F4 opens the built-in editor for files under 10 MB (larger files
open in the viewer instead). Insert toggles insert/overwrite. F3
marks a line selection; F7 searches; F4 (while editing) searches
and replaces. F2 saves in place; F10 quits, prompting to save first
if the buffer is modified. Undo is a single level.";

const COMMAND_LINE: &str = "\
Typing over a panel with nothing else active enters text on the
command line. Enter runs it in the active panel's directory; `cd`
navigates the panel instead of spawning a shell. Up/Down recall
history while something is typed; Esc clears the line.";

const MODERN_EXTRAS: &str = "\
Ctrl+P narrows the active panel to substring matches as you type;
Esc clears it. Ctrl+J opens a fuzzy, frecency-ranked jump list of
previously visited directories. Ctrl+T/Ctrl+W/Alt+1..9 manage panel
tabs. Alt+F7 searches the active panel's subtree by name. Ctrl+Left/
Ctrl+Right adjusts the vertical panel split 2 columns at a time;
Ctrl+= resets it to 50/50. The split persists across restarts.

Ctrl+C (or Ctrl+Ins) copies the selected files (or the file under
the cursor) to the clipboard as file objects, ready to paste into
Explorer, Outlook, or any other Windows application. Ctrl+Shift+Ins
copies their absolute paths as text, one per line; the Files menu
also offers copying just their names. All three act on the same
selection scope as F5 Copy.";

const CONFIGURATION: &str = "\
config.toml (next to the executable) sets the theme, shell, F4
external-editor command, and the persisted panel_split percentage,
and can remap the quick-filter, fuzzy-jump, panel-split, and
clipboard (key.clipboard_files, key.clipboard_paths) keys.
usermenu.toml defines the F2 user menu's entries. --nosplash skips
the startup splash. --theme <name> (or --theme=<name>) starts the
session in a built-in theme instead of the configured one, for this
launch only: it never writes config.toml, so applying a theme from
a picker during such a session still persists normally.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_menu_state_move_cursor_clamps_and_holds_on_an_empty_menu() {
        let mut m = UserMenuState::new();
        m.move_cursor(-3, 3);
        assert_eq!(m.cursor, 0);
        m.move_cursor(10, 3);
        assert_eq!(m.cursor, 2);
        m.cursor = 1;
        m.move_cursor(1, 0);
        assert_eq!(m.cursor, 0, "an empty menu holds the cursor at zero");
    }

    #[test]
    fn theme_picker_opens_with_the_active_theme_highlighted() {
        let picker = ThemePickerState::open("terminal-green");
        let expected = crate::theme::BUILTIN_THEME_NAMES.iter().position(|n| *n == "terminal-green").unwrap();
        assert_eq!(picker.highlight, expected);
    }

    #[test]
    fn theme_picker_open_falls_back_to_the_first_row_for_an_unknown_name() {
        let picker = ThemePickerState::open("does-not-exist");
        assert_eq!(picker.highlight, 0);
    }

    #[test]
    fn theme_picker_move_cursor_clamps_within_the_theme_list() {
        let mut picker = ThemePickerState { highlight: 0 };
        picker.move_cursor(-1);
        assert_eq!(picker.highlight, 0);
        let last = crate::theme::BUILTIN_THEME_NAMES.len() - 1;
        picker.move_cursor(100);
        assert_eq!(picker.highlight, last);
        picker.move_cursor(1);
        assert_eq!(picker.highlight, last, "clamped at the end, not wrapped");
    }

    #[test]
    fn file_action_menu_lists_run_first_only_when_executable() {
        let m = FileActionMenuState::new(OsString::from("notes.txt"), false);
        assert_eq!(m.entries[0], FileActionMenuEntry::View, "non-executable: View highlighted first");
        assert!(!m.entries.contains(&FileActionMenuEntry::Run));
        assert_eq!(m.selected(), FileActionMenuEntry::View);

        let m = FileActionMenuState::new(OsString::from("setup.exe"), true);
        assert_eq!(m.entries[0], FileActionMenuEntry::Run, "executable: Run highlighted first");
        assert_eq!(m.selected(), FileActionMenuEntry::Run);
        assert_eq!(
            m.entries,
            vec![
                FileActionMenuEntry::Run,
                FileActionMenuEntry::View,
                FileActionMenuEntry::Edit,
                FileActionMenuEntry::SendToClipboard,
                FileActionMenuEntry::Copy,
                FileActionMenuEntry::Rename,
                FileActionMenuEntry::Move,
                FileActionMenuEntry::Delete,
            ]
        );
    }

    #[test]
    fn file_action_menu_directory_target_lists_send_to_clipboard_first() {
        let m = FileActionMenuState::open(OsString::from("src"), true, false, false);
        assert_eq!(
            m.entries,
            vec![
                FileActionMenuEntry::SendToClipboard,
                FileActionMenuEntry::Copy,
                FileActionMenuEntry::Rename,
                FileActionMenuEntry::Move,
                FileActionMenuEntry::Delete,
            ]
        );
    }

    #[test]
    fn file_action_menu_move_cursor_clamps_at_both_ends() {
        let mut m = FileActionMenuState::new(OsString::from("notes.txt"), false);
        m.move_cursor(-3);
        assert_eq!(m.cursor, 0);
        m.move_cursor(100);
        assert_eq!(m.cursor, m.entries.len() - 1);
    }

    #[test]
    fn file_action_menu_hotkey_matches_first_letter_case_insensitively() {
        let m = FileActionMenuState::new(OsString::from("notes.txt"), false);
        assert_eq!(m.hotkey_action('d'), Some(FileActionMenuEntry::Delete));
        assert_eq!(m.hotkey_action('D'), Some(FileActionMenuEntry::Delete));
        assert_eq!(m.hotkey_action('z'), None);
    }

    #[test]
    fn file_action_menu_hotkey_r_prefers_run_over_rename_when_both_present() {
        let m = FileActionMenuState::new(OsString::from("setup.exe"), true);
        assert_eq!(m.hotkey_action('r'), Some(FileActionMenuEntry::Run));

        let m = FileActionMenuState::new(OsString::from("notes.txt"), false);
        assert_eq!(m.hotkey_action('r'), Some(FileActionMenuEntry::Rename), "no Run entry: R falls through to Rename");
    }

    #[test]
    fn help_state_opens_on_about_filecommand() {
        let h = HelpState::new();
        assert_eq!(h.cursor, ABOUT_TOPIC_INDEX);
        assert_eq!(HELP_TOPICS[h.cursor], "About FileCommand");
        assert!(h.page.is_none());
        assert!(!h.about_open);
    }

    #[test]
    fn help_state_move_cursor_clamps_and_scrolls_to_keep_the_highlight_visible() {
        let mut h = HelpState::new();
        h.move_cursor(-1, 5);
        assert_eq!(h.cursor, 0);
        h.move_cursor(20, 5);
        assert_eq!(h.cursor, HELP_TOPICS.len() - 1);
        assert!(h.scroll + 5 > h.cursor, "the scroll offset must keep the highlight in the visible window");
    }

    #[test]
    fn help_state_activate_opens_about_for_the_first_entry_and_a_page_otherwise() {
        let mut h = HelpState::new();
        h.activate();
        assert!(h.about_open);
        assert!(h.page.is_none());

        let mut h = HelpState::new();
        h.cursor = 2;
        h.activate();
        assert_eq!(h.page, Some(2));
        assert!(!h.about_open);
    }

    #[test]
    fn help_state_back_steps_down_one_level_at_a_time() {
        let mut h = HelpState::new();
        h.about_open = true;
        assert!(h.back(), "About -> list keeps the window open");
        assert!(!h.about_open);

        h.page = Some(3);
        assert!(h.back(), "page -> list keeps the window open");
        assert!(h.page.is_none());

        assert!(!h.back(), "list -> nothing signals the window should close");
    }

    #[test]
    fn help_window_height_uses_the_unified_overlay_rule() {
        // `help_window_height(15) == 15` was the old expectation under the
        // pre-D6 `clamp(10, 19)` rule with no forced margin; the unified
        // overlay rule reserves a 2-cell margin, so 15 now clamps to 13 —
        // an intentional change (D6/D9), not a regression.
        assert_eq!(help_window_height((80, 24)), 19);
        assert_eq!(help_window_height((80, 100)), 19);
        assert_eq!(help_window_height((80, 15)), 13);
        // Below the minimum (10), the terminal itself is the hard ceiling.
        assert_eq!(help_window_height((80, 5)), 5);
    }

    #[test]
    fn help_topic_visible_rows_never_zero_and_grows_with_the_window() {
        assert!(help_topic_visible_rows(0) >= 1);
        assert!(help_topic_visible_rows(19) > help_topic_visible_rows(10));
    }

    #[test]
    fn keyboard_reference_documents_ctrl_and_alt_f_key_bar_variants() {
        let text = topic_page_text(1);
        assert!(text.contains("Ctrl+F3"), "expected a Ctrl F-key-bar variant documented: {text}");
        assert!(text.contains("Alt+F1") || text.contains("Alt+F7"), "expected an Alt F-key-bar variant documented: {text}");
    }

    #[test]
    fn every_non_about_topic_has_non_empty_page_text() {
        for (i, name) in HELP_TOPICS.iter().enumerate().skip(1) {
            assert!(!topic_page_text(i).is_empty(), "topic {i} (`{name}`) has no page text");
        }
    }

    #[test]
    fn about_topic_has_no_compiled_page_text_since_it_opens_a_dialog_instead() {
        assert_eq!(topic_page_text(ABOUT_TOPIC_INDEX), "");
    }

    #[test]
    fn overlay_rect_at_nominal_size_uses_its_preferred_geometry() {
        // responsive-layout "Overlay at nominal size uses its preferred
        // geometry".
        let r = overlay_rect((52, 10), (30, 8), (80, 24));
        assert_eq!(r, OverlayRect { x: 14, y: 7, width: 52, height: 10 });
    }

    #[test]
    fn overlay_rect_clamps_near_the_floor() {
        // responsive-layout "Overlay clamps near the floor": preferred
        // 62x19 at 60x16 clamps to 58x14.
        let r = overlay_rect((62, 19), (40, 10), (60, 16));
        assert_eq!(r.width, 58);
        assert_eq!(r.height, 14);
        assert_eq!(r.x, 1);
        assert_eq!(r.y, 1);
    }

    #[test]
    fn clamp_overlay_dim_never_exceeds_terminal_even_with_a_huge_minimum() {
        assert_eq!(clamp_overlay_dim(20, 200, 60), 60);
    }

    #[test]
    fn clamp_overlay_dim_respects_the_two_cell_margin() {
        // Preferred fits the terminal exactly, but the rule still reserves
        // a 2-cell margin.
        assert_eq!(clamp_overlay_dim(60, 10, 60), 58);
    }

    #[test]
    fn help_window_height_matches_the_new_margin_rule() {
        // `help_window_height(15) == 15` was the old expectation (no forced
        // margin, just `clamp(10,19)`); under the new rule this is
        // `clamp_overlay_dim(19, 10, 15) == 13` (D6/D9's "clamps below the
        // nominal size" scenario expects a `terminal - 2` margin).
        assert_eq!(clamp_overlay_dim(19, 10, 15), 13);
    }

    mod overlay_rect_proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn overlay_rect_is_fully_contained_and_at_least_the_clamped_minimum(
                term_w in 60u16..200,
                term_h in 16u16..60,
                pref_w in 1u16..250,
                pref_h in 1u16..250,
                min_w in 1u16..60,
                min_h in 1u16..20,
            ) {
                // `preferred >= minimum` per the helper's contract.
                let preferred = (pref_w.max(min_w), pref_h.max(min_h));
                let r = overlay_rect(preferred, (min_w, min_h), (term_w, term_h));
                prop_assert!(r.x + r.width <= term_w, "x={} width={} term_w={}", r.x, r.width, term_w);
                prop_assert!(r.y + r.height <= term_h, "y={} height={} term_h={}", r.y, r.height, term_h);
                prop_assert!(r.width >= min_w.min(term_w));
                prop_assert!(r.height >= min_h.min(term_h));
            }
        }
    }
}
