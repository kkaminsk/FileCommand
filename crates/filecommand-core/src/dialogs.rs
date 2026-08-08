//! F2 user-menu and F1 Help/About window state, plus the Help window's
//! compiled-in static topic text (help-and-about; user-menu).
//!
//! Both dialogs are simple enough (a cursor over a fixed or config-loaded
//! list) that they share this one small module rather than each getting a
//! file of their own.

/// The open F2 user menu: just a cursor over `State::user_menu_entries`,
/// which is loaded once at startup from `usermenu.toml` and does not change
/// while the menu is open (user-menu "Open the F2 user menu", "Navigate and
/// dismiss the user menu").
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
// F1 Help window + About dialog
// ---------------------------------------------------------------------

/// The fixed v1 Help topic list, in display order. `About FileCommand` is
/// always first and is special-cased (it opens the About dialog rather than
/// a topic page) — see [`HelpState::activate`] (help-and-about "Help topic
/// list").
pub const HELP_TOPICS: [&str; 10] = [
    "About FileCommand",
    "Keyboard reference",
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

/// The Help window's total height for a given terminal row count: capped at
/// the nominal 19 rows (help-and-about "approximately 62x19, capped near
/// that proportion") but shrinking, down to a small floor, to stay fully
/// on-screen on a shorter terminal (help-and-about "Help window re-centers
/// on resize").
pub fn help_window_height(term_rows: u16) -> u16 {
    term_rows.clamp(10, 19)
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
        2 => PANELS_AND_DISPLAY_MODES,
        3 => FILE_OPERATIONS,
        4 => MENUS,
        5 => VIEWER,
        6 => EDITOR,
        7 => COMMAND_LINE,
        8 => MODERN_EXTRAS,
        9 => CONFIGURATION,
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
  Alt+letter   Type-ahead jump to entry";

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
tabs. Alt+F7 searches the active panel's subtree by name.";

const CONFIGURATION: &str = "\
config.toml (next to the executable) sets the theme, shell, and F4
external-editor command, and can remap the quick-filter and fuzzy-
jump keys. usermenu.toml defines the F2 user menu's entries.";

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
    fn help_window_height_is_capped_and_floored() {
        assert_eq!(help_window_height(24), 19);
        assert_eq!(help_window_height(100), 19);
        assert_eq!(help_window_height(5), 10);
        assert_eq!(help_window_height(15), 15);
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
}
