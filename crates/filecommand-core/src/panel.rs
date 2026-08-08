//! Panel state machine: current directory, entry list, cursor, sort order,
//! and listing progress. No I/O happens here — callers feed in [`Entry`]
//! values produced by the `listing` module.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::git_info::GitInfo;
use crate::info::InfoValues;
use crate::listing::{cmp_by_mode, format_count, Entry, EntryKind, SortMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

/// How a panel renders its body. M3 added `Info`; M5 adds `Brief` (three
/// name-only columns), `Tree` (lazily-expanded directory tree driving the
/// opposite panel), and `QuickView` (viewer-style preview of the opposite
/// panel's cursor file) — design D7. Their rendering and reducer wiring
/// land with the `additional-panel-modes` capability; this variant surface
/// exists up front so other M5 groups compile against a stable enum
/// (task 1.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayMode {
    #[default]
    Full,
    Info,
    Brief,
    Tree,
    QuickView,
}

/// Whether a listing is still streaming in from the worker thread, or done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListingProgress {
    Streaming { count: usize },
    Complete { count: usize },
}

impl ListingProgress {
    pub fn count(&self) -> usize {
        match self {
            ListingProgress::Streaming { count } | ListingProgress::Complete { count } => *count,
        }
    }

    pub fn is_streaming(&self) -> bool {
        matches!(self, ListingProgress::Streaming { .. })
    }
}

/// Cursor movement request. Page-sized moves and Home/End are always
/// resolvable without knowing the viewport; `Up`/`Down` carry the step size
/// (1 for arrow keys, a page height for PgUp/PgDn) since the panel itself
/// has no notion of the rendered viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorMove {
    Up(usize),
    Down(usize),
    Home,
    End,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelState {
    pub cwd: PathBuf,
    pub entries: Vec<Entry>,
    pub cursor: usize,
    /// Sort key, set with Ctrl+F3..Ctrl+F7. Independent per panel and
    /// preserved across a re-read.
    pub sort_mode: SortMode,
    pub sort_direction: SortDirection,
    /// `Full` listing or the `Info` system/drive/directory summary.
    pub display_mode: DisplayMode,
    /// Async Info-mode values; all `None` (rendered `…`) until their worker
    /// queries resolve.
    pub info: InfoValues,
    /// The id of the most recently issued `QueryInfo` request for this
    /// panel, or `None` if none is outstanding. `Command::InfoResolved`
    /// only applies a result whose id matches this — otherwise it is a
    /// stale answer from a superseded request (e.g. a double Ctrl+R) and is
    /// dropped rather than clobbering a fresher one.
    pub info_request: Option<u64>,
    pub progress: ListingProgress,
    /// Once the user explicitly moves the cursor, streamed inserts stop
    /// yanking it back to row 0.
    pub cursor_user_moved: bool,
    pub last_error: Option<String>,
    /// Selected entries, keyed by original on-disk name — never by row
    /// index, so selection survives cursor movement, re-sort, and scroll.
    /// The parent-directory pseudo-entry can never appear here.
    pub selected: HashSet<OsString>,
    /// The Ctrl+P inline quick-filter pattern, or `None` when no filter is
    /// active. While `Some`, the panel body is narrowed to entries whose
    /// displayed name contains the pattern as a substring (plus `..`), and
    /// cursor movement is restricted to the narrowed set (quick-filter all
    /// requirements).
    pub quick_filter: Option<String>,
    /// Other tabs belonging to this panel, in list order, with the
    /// currently active tab's slot omitted — its state lives inline in the
    /// fields above rather than duplicated here. See [`TabData`] and
    /// [`PanelState::open_tab`]/[`close_tab`]/[`switch_tab`] (panel-tabs "Per-panel
    /// tab list with independent state").
    pub tabs: Vec<TabData>,
    /// This panel's active tab's zero-based position within the full
    /// (`tabs` + the inline active tab) ordered list.
    pub active_tab_index: usize,
    /// Async git info for this panel's directory: no branch and no
    /// per-entry statuses (rendered identically to "outside a repository")
    /// until the worker thread's `git_info::query` result arrives (git-info
    /// "Single-reflow appearance with nothing reserved while pending").
    pub git_info: GitInfo,
    /// The id of the most recently issued git-info query for this panel, or
    /// `None` if none is outstanding. Mirrors [`PanelState::info_request`]:
    /// `Command::GitInfoResolved` only applies a result whose id matches
    /// this, so a reply for a directory/generation the panel has since
    /// moved past — including a timed-out query answered late — is dropped
    /// rather than clobbering a fresher one (git-info "Silent absence on
    /// timeout and stale-result discarding").
    pub git_request: Option<u64>,
}

impl PanelState {
    pub fn new(cwd: PathBuf) -> PanelState {
        PanelState {
            cwd,
            entries: Vec::new(),
            cursor: 0,
            sort_mode: SortMode::default(),
            sort_direction: SortDirection::Asc,
            display_mode: DisplayMode::default(),
            info: InfoValues::default(),
            info_request: None,
            progress: ListingProgress::Streaming { count: 0 },
            cursor_user_moved: false,
            last_error: None,
            selected: HashSet::new(),
            quick_filter: None,
            tabs: Vec::new(),
            active_tab_index: 0,
            git_info: GitInfo::none(),
            git_request: None,
        }
    }

    pub fn selected(&self) -> Option<&Entry> {
        self.entries.get(self.cursor)
    }

    pub fn clamp_cursor(&mut self) {
        if self.entries.is_empty() {
            self.cursor = 0;
        } else if self.cursor >= self.entries.len() {
            self.cursor = self.entries.len() - 1;
        }
    }

    pub fn move_cursor(&mut self, m: CursorMove) {
        if self.entries.is_empty() {
            self.cursor = 0;
            return;
        }
        if self.quick_filter.is_some() {
            self.move_cursor_filtered(m);
            return;
        }
        let last = self.entries.len() - 1;
        self.cursor = match m {
            CursorMove::Up(n) => self.cursor.saturating_sub(n),
            CursorMove::Down(n) => (self.cursor + n).min(last),
            CursorMove::Home => 0,
            CursorMove::End => last,
        };
        self.cursor_user_moved = true;
    }

    /// `move_cursor`, but restricted to entries the active quick filter
    /// leaves visible — a no-op when nothing is visible (quick-filter
    /// "Navigation is restricted to matching entries").
    fn move_cursor_filtered(&mut self, m: CursorMove) {
        let visible = self.visible_indices();
        let Some(last) = visible.len().checked_sub(1) else { return };
        let pos = visible.iter().position(|&i| i == self.cursor).unwrap_or(0);
        let new_pos = match m {
            CursorMove::Up(n) => pos.saturating_sub(n),
            CursorMove::Down(n) => (pos + n).min(last),
            CursorMove::Home => 0,
            CursorMove::End => last,
        };
        self.cursor = visible[new_pos];
        self.cursor_user_moved = true;
    }

    /// Insert a freshly streamed entry in sorted position, then re-pin the
    /// cursor to row 0 if the user hasn't moved it yet.
    pub fn insert_streamed(&mut self, entry: Entry) {
        insert_sorted(&mut self.entries, entry, self.sort_mode, self.sort_direction);
        if !self.cursor_user_moved {
            self.cursor = 0;
        } else {
            self.clamp_cursor();
        }
    }

    /// Re-sort the entries already in hand under the current mode. Stable,
    /// and it issues no directory read and no per-entry metadata query.
    pub fn resort(&mut self) {
        let mode = self.sort_mode;
        let dir = self.sort_direction;
        self.entries.sort_by(|a, b| cmp_entries(a, b, mode, dir));
    }

    /// Set the sort mode and re-sort in place, keeping the cursor on the
    /// same entry it was on rather than on the same row.
    pub fn set_sort_mode(&mut self, mode: SortMode) {
        let anchor = self.entries.get(self.cursor).map(|e| e.name.clone());
        self.sort_mode = mode;
        self.resort();
        if let Some(name) = anchor {
            if let Some(index) = self.entries.iter().position(|e| e.name == name) {
                self.cursor = index;
            }
        }
        self.clamp_cursor();
    }

    pub fn begin_new_listing(&mut self, cwd: PathBuf) {
        self.cwd = cwd;
        self.entries.clear();
        self.cursor = 0;
        self.cursor_user_moved = false;
        self.progress = ListingProgress::Streaming { count: 0 };
        self.last_error = None;
        self.selected.clear();
        // Directory-scoped Info values (and the drive's, if the drive
        // changed) no longer describe what the panel shows. The sort mode
        // and display mode deliberately survive a re-read. Any outstanding
        // Info query is now moot too — `begin_listing` mints a fresh one if
        // the panel is still in Info mode.
        self.info = InfoValues::default();
        self.info_request = None;
        // A quick filter narrowed *this* listing; a fresh directory has an
        // entirely different entry set, so a stale pattern would either
        // hide everything or match nothing meaningful. Esc is the
        // documented way to clear it deliberately, but a directory change
        // clears it implicitly too.
        self.quick_filter = None;
        // Directory-scoped git info (like `info` above) no longer describes
        // what the panel shows; `update::begin_listing` mints a fresh
        // request and issues `Effect::QueryGitInfo` for wherever the panel
        // just landed (git-info "Query re-issued on navigation").
        self.git_info = GitInfo::none();
        self.git_request = None;
    }

    /// Drop any selected names that no longer appear in `entries` — used
    /// after a fresh listing (e.g. following a file-op job) so selection
    /// never references a vanished entry.
    pub fn reconcile_selection(&mut self) {
        let present: HashSet<&OsString> = self.entries.iter().map(|e| &e.name).collect();
        self.selected.retain(|name| present.contains(name));
    }

    /// Toggle selection on the entry under the cursor, then advance the
    /// cursor by one row (never wrapping past the last entry). The parent
    /// `..` pseudo-entry is never selectable and toggling it is a no-op.
    pub fn toggle_selection_and_advance(&mut self) {
        if let Some(entry) = self.entries.get(self.cursor) {
            if entry.kind != EntryKind::ParentDir {
                let name = entry.name.clone();
                if !self.selected.remove(&name) {
                    self.selected.insert(name);
                }
            }
        }
        self.move_cursor(CursorMove::Down(1));
    }

    /// Additively select every selectable entry whose name matches `pattern`
    /// (`*`/`?` DOS-style wildcards, case-insensitive).
    pub fn select_matching(&mut self, pattern: &str) {
        for entry in &self.entries {
            if entry.kind != EntryKind::ParentDir && wildcard_match(pattern, &entry.name.to_string_lossy()) {
                self.selected.insert(entry.name.clone());
            }
        }
    }

    /// Subtractively deselect every entry whose name matches `pattern`.
    pub fn deselect_matching(&mut self, pattern: &str) {
        for entry in &self.entries {
            if entry.kind != EntryKind::ParentDir && wildcard_match(pattern, &entry.name.to_string_lossy()) {
                self.selected.remove(&entry.name);
            }
        }
    }

    /// Invert selection over every selectable entry; `..` is always left
    /// unselected.
    pub fn invert_selection(&mut self) {
        for entry in &self.entries {
            if entry.kind == EntryKind::ParentDir {
                continue;
            }
            if !self.selected.remove(&entry.name) {
                self.selected.insert(entry.name.clone());
            }
        }
    }

    /// Total bytes across selected entries; directories (and `..`) always
    /// contribute 0.
    pub fn selected_bytes(&self) -> u64 {
        self.entries.iter().filter(|e| self.selected.contains(&e.name)).map(|e| if e.is_dir_like() { 0 } else { e.size }).sum()
    }

    /// `"N files selected, X bytes"`, or `None` when nothing is selected (in
    /// which case the panel falls back to the per-entry status line).
    pub fn selection_status(&self) -> Option<String> {
        if self.selected.is_empty() {
            return None;
        }
        Some(format!("{} files selected, {} bytes", format_count(self.selected.len()), format_count(self.selected_bytes() as usize)))
    }

    // -------------------------------------------------------------------
    // Quick filter (Ctrl+P)
    // -------------------------------------------------------------------

    /// Whether `entry` is shown under the current `quick_filter` pattern.
    /// The `..` parent entry always matches so upward navigation is never
    /// blocked (quick-filter "Substring narrowing as the pattern is
    /// typed").
    fn matches_quick_filter(entry: &Entry, pattern: &str) -> bool {
        entry.kind == EntryKind::ParentDir || entry.name.to_string_lossy().to_lowercase().contains(&pattern.to_lowercase())
    }

    /// Indices into `entries` visible under the active `quick_filter`, or
    /// every index when no filter is active.
    pub fn visible_indices(&self) -> Vec<usize> {
        match &self.quick_filter {
            None => (0..self.entries.len()).collect(),
            Some(pattern) => self.entries.iter().enumerate().filter(|(_, e)| Self::matches_quick_filter(e, pattern)).map(|(i, _)| i).collect(),
        }
    }

    /// Ctrl+P: enter quick-filter mode with an empty pattern (quick-filter
    /// "Activating the quick filter").
    pub fn activate_quick_filter(&mut self) {
        self.quick_filter = Some(String::new());
    }

    /// Append `c` to the quick-filter pattern and re-narrow (quick-filter
    /// "Substring narrowing as the pattern is typed").
    pub fn quick_filter_push(&mut self, c: char) {
        if let Some(pattern) = &mut self.quick_filter {
            pattern.push(c);
        }
        self.snap_cursor_to_visible();
    }

    /// Backspace: shorten the quick-filter pattern by one character and
    /// re-narrow. An already-empty pattern is left empty and quick-filter
    /// mode stays active (quick-filter "Editing the pattern re-narrows
    /// live").
    pub fn quick_filter_backspace(&mut self) {
        if let Some(pattern) = &mut self.quick_filter {
            pattern.pop();
        }
        self.snap_cursor_to_visible();
    }

    /// Esc: clear the quick filter and restore the full listing. Selection
    /// and sort mode are untouched, since the filter only narrows what is
    /// shown (quick-filter "Clearing the quick filter").
    pub fn clear_quick_filter(&mut self) {
        self.quick_filter = None;
    }

    /// After a pattern change, move the cursor onto the nearest still-
    /// visible entry if the one it was on got filtered out (quick-filter
    /// "Cursor and mini-status behavior under an active filter").
    fn snap_cursor_to_visible(&mut self) {
        let visible = self.visible_indices();
        if visible.is_empty() || visible.contains(&self.cursor) {
            return;
        }
        if let Some(&nearest) = visible.iter().min_by_key(|&&i| i.abs_diff(self.cursor)) {
            self.cursor = nearest;
        }
    }

    // -------------------------------------------------------------------
    // Panel tabs (Ctrl+T / Ctrl+W / Alt+1..9)
    // -------------------------------------------------------------------

    /// How many tabs this panel currently has (always >= 1).
    pub fn tab_count(&self) -> usize {
        self.tabs.len() + 1
    }

    /// Snapshot everything a tab independently owns from the fields
    /// currently inline (i.e. the active tab's state).
    fn to_tab_data(&self) -> TabData {
        TabData {
            cwd: self.cwd.clone(),
            entries: self.entries.clone(),
            cursor: self.cursor,
            sort_mode: self.sort_mode,
            sort_direction: self.sort_direction,
            display_mode: self.display_mode,
            info: self.info.clone(),
            info_request: self.info_request,
            progress: self.progress,
            cursor_user_moved: self.cursor_user_moved,
            last_error: self.last_error.clone(),
            selected: self.selected.clone(),
            quick_filter: self.quick_filter.clone(),
            git_info: self.git_info.clone(),
            git_request: self.git_request,
        }
    }

    /// Replace the inline (active-tab) fields with `data`'s.
    fn adopt_tab_data(&mut self, data: TabData) {
        self.cwd = data.cwd;
        self.entries = data.entries;
        self.cursor = data.cursor;
        self.sort_mode = data.sort_mode;
        self.sort_direction = data.sort_direction;
        self.display_mode = data.display_mode;
        self.info = data.info;
        self.info_request = data.info_request;
        self.progress = data.progress;
        self.cursor_user_moved = data.cursor_user_moved;
        self.last_error = data.last_error;
        self.selected = data.selected;
        self.quick_filter = data.quick_filter;
        self.git_info = data.git_info;
        self.git_request = data.git_request;
    }

    /// The full ordered tab list, with the active tab's live state
    /// (currently inline) reinserted at its position.
    fn full_tab_list(&self) -> Vec<TabData> {
        let mut list = self.tabs.clone();
        list.insert(self.active_tab_index.min(list.len()), self.to_tab_data());
        list
    }

    /// Replace the whole tab list with `list`, activating `active`
    /// (`list[active]` becomes the new inline state).
    fn apply_tab_list(&mut self, mut list: Vec<TabData>, active: usize) {
        let data = list.remove(active);
        self.tabs = list;
        self.active_tab_index = active;
        self.adopt_tab_data(data);
    }

    /// Ctrl+T: open a new tab inheriting the active tab's directory and
    /// state, inserted right after it and becoming active (panel-tabs "New
    /// tab (Ctrl+T)").
    pub fn open_tab(&mut self) {
        let mut list = self.full_tab_list();
        let inherited = list[self.active_tab_index].clone();
        list.insert(self.active_tab_index + 1, inherited);
        self.apply_tab_list(list, self.active_tab_index + 1);
    }

    /// Ctrl+W: close the active tab and activate an adjacent tab. A no-op
    /// when only one tab remains (panel-tabs "Close tab (Ctrl+W)").
    pub fn close_tab(&mut self) {
        let mut list = self.full_tab_list();
        if list.len() <= 1 {
            return;
        }
        list.remove(self.active_tab_index);
        let new_active = self.active_tab_index.min(list.len() - 1);
        self.apply_tab_list(list, new_active);
    }

    /// Each tab's directory, in tab order — cheap to call every frame for
    /// tab-strip label rendering since it clones only the directory
    /// `PathBuf`s, never the full [`TabData`] (which carries each tab's
    /// entire entry list). See [`Self::full_tab_list`] for the equivalent
    /// that also carries state, used when actually switching tabs (panel-
    /// tabs "Tab label rendering and active styling").
    pub fn tab_dirs(&self) -> Vec<PathBuf> {
        let mut list: Vec<PathBuf> = self.tabs.iter().map(|t| t.cwd.clone()).collect();
        list.insert(self.active_tab_index.min(list.len()), self.cwd.clone());
        list
    }

    /// Alt+`n`: activate the tab at one-based position `n`. Out of range
    /// (including `n == 0`) is a no-op (panel-tabs "Switch tab
    /// (Alt+1..9)").
    pub fn switch_tab(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        let target = n - 1;
        if target == self.active_tab_index || target >= self.tab_count() {
            return;
        }
        let list = self.full_tab_list();
        self.apply_tab_list(list, target);
    }
}

/// One inactive tab's full, independent state — directory, entries,
/// cursor, sort mode, display mode, and filter — snapshotted when it stops
/// being the active tab (design D4; panel-tabs "Per-panel tab list with
/// independent state"). The active tab's equivalent state lives inline on
/// [`PanelState`] rather than duplicated here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabData {
    pub cwd: PathBuf,
    pub entries: Vec<Entry>,
    pub cursor: usize,
    pub sort_mode: SortMode,
    pub sort_direction: SortDirection,
    pub display_mode: DisplayMode,
    pub info: InfoValues,
    pub info_request: Option<u64>,
    pub progress: ListingProgress,
    pub cursor_user_moved: bool,
    pub last_error: Option<String>,
    pub selected: HashSet<OsString>,
    pub quick_filter: Option<String>,
    pub git_info: GitInfo,
    pub git_request: Option<u64>,
}

/// DOS-style wildcard match (`*` = any run of characters, `?` = any single
/// character), case-insensitive.
pub fn wildcard_match(pattern: &str, name: &str) -> bool {
    fn match_bytes(pat: &[u8], s: &[u8]) -> bool {
        match (pat.first(), s.first()) {
            (None, None) => true,
            (Some(b'*'), _) => match_bytes(&pat[1..], s) || (!s.is_empty() && match_bytes(pat, &s[1..])),
            (Some(b'?'), Some(_)) => match_bytes(&pat[1..], &s[1..]),
            (Some(pc), Some(sc)) if pc == sc => match_bytes(&pat[1..], &s[1..]),
            _ => false,
        }
    }
    let pattern = pattern.to_lowercase();
    let name = name.to_lowercase();
    match_bytes(pattern.as_bytes(), name.as_bytes())
}

/// Compare two entries for sort order. `..` always sorts first regardless of
/// mode or direction; otherwise the mode's comparator decides, with
/// directories and files interleaved (matching classic Norton Commander
/// behavior). In `Unsorted` mode every non-parent pair compares equal, so a
/// stable sort leaves enumeration order intact.
pub fn cmp_entries(a: &Entry, b: &Entry, mode: SortMode, dir: SortDirection) -> Ordering {
    match (a.kind == EntryKind::ParentDir, b.kind == EntryKind::ParentDir) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        (false, false) => {}
    }
    let ord = cmp_by_mode(a, b, mode);
    match dir {
        SortDirection::Asc => ord,
        SortDirection::Desc => ord.reverse(),
    }
}

/// Insert `entry` into `entries` (assumed already sorted by `mode`/`dir`) at
/// its correct sorted position. In `Unsorted` mode this appends, which is
/// exactly enumeration order.
pub fn insert_sorted(entries: &mut Vec<Entry>, entry: Entry, mode: SortMode, dir: SortDirection) {
    let pos = entries.partition_point(|e| cmp_entries(e, &entry, mode, dir) != Ordering::Greater);
    entries.insert(pos, entry);
}

/// The parent of `path`, or `None` if `path` has no parent (filesystem
/// root) — callers should treat `None` as a no-op.
pub fn parent_path(path: &Path) -> Option<PathBuf> {
    path.parent().map(|p| p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listing::DateTime;
    use std::ffi::OsString;

    fn file(name: &str) -> Entry {
        Entry { name: OsString::from(name), kind: EntryKind::File, size: 0, modified: None }
    }
    fn dir(name: &str) -> Entry {
        Entry { name: OsString::from(name), kind: EntryKind::Directory, size: 0, modified: None }
    }

    #[test]
    fn parent_dir_sorts_first_regardless_of_direction() {
        let mut entries = vec![file("b.txt"), dir("a"), Entry::parent_dir()];
        entries.sort_by(|a, b| cmp_entries(a, b, SortMode::Name, SortDirection::Asc));
        assert_eq!(entries[0].kind, EntryKind::ParentDir);

        let mut entries = vec![file("b.txt"), dir("a"), Entry::parent_dir()];
        entries.sort_by(|a, b| cmp_entries(a, b, SortMode::Name, SortDirection::Desc));
        assert_eq!(entries[0].kind, EntryKind::ParentDir);
    }

    #[test]
    fn name_sort_is_case_insensitive() {
        let mut entries = vec![file("Banana"), file("apple"), file("Cherry")];
        entries.sort_by(|a, b| cmp_entries(a, b, SortMode::Name, SortDirection::Asc));
        let names: Vec<String> = entries.iter().map(|e| e.name.to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["apple", "Banana", "Cherry"]);
    }

    #[test]
    fn insert_sorted_keeps_parent_first_and_streams_in_order() {
        let mut entries = vec![Entry::parent_dir()];
        insert_sorted(&mut entries, file("beta"), SortMode::Name, SortDirection::Asc);
        insert_sorted(&mut entries, file("alpha"), SortMode::Name, SortDirection::Asc);
        insert_sorted(&mut entries, file("gamma"), SortMode::Name, SortDirection::Asc);
        let names: Vec<String> = entries.iter().map(|e| e.name.to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["..", "alpha", "beta", "gamma"]);
    }

    #[test]
    fn cursor_clamps_within_bounds() {
        let mut p = PanelState::new(PathBuf::from("/"));
        p.entries = vec![file("a"), file("b"), file("c")];
        p.cursor = 10;
        p.clamp_cursor();
        assert_eq!(p.cursor, 2);

        p.entries.clear();
        p.clamp_cursor();
        assert_eq!(p.cursor, 0);
    }

    #[test]
    fn move_cursor_up_down_home_end() {
        let mut p = PanelState::new(PathBuf::from("/"));
        p.entries = vec![file("a"), file("b"), file("c"), file("d")];
        p.cursor = 1;
        p.move_cursor(CursorMove::Down(1));
        assert_eq!(p.cursor, 2);
        p.move_cursor(CursorMove::Down(10));
        assert_eq!(p.cursor, 3); // clamped to last
        p.move_cursor(CursorMove::Up(1));
        assert_eq!(p.cursor, 2);
        p.move_cursor(CursorMove::Up(100));
        assert_eq!(p.cursor, 0); // saturating
        p.move_cursor(CursorMove::End);
        assert_eq!(p.cursor, 3);
        p.move_cursor(CursorMove::Home);
        assert_eq!(p.cursor, 0);
    }

    #[test]
    fn cursor_pinned_to_top_while_streaming_until_user_moves() {
        let mut p = PanelState::new(PathBuf::from("/"));
        p.insert_streamed(file("m"));
        p.insert_streamed(file("a")); // sorts before "m" -> cursor should stay at 0
        assert_eq!(p.cursor, 0);
        assert_eq!(p.entries[p.cursor].name, OsString::from("a"));

        p.move_cursor(CursorMove::Down(1));
        assert_eq!(p.cursor, 1);
        let selected_name = p.entries[p.cursor].name.clone();

        // Further streamed inserts must not yank the cursor back now that
        // the user has moved it; re-clamp only if out of range.
        p.insert_streamed(dir("z"));
        assert_eq!(p.entries[p.cursor].name, selected_name);
    }

    #[test]
    fn begin_new_listing_resets_state() {
        let mut p = PanelState::new(PathBuf::from("/a"));
        p.entries = vec![file("x")];
        p.cursor = 1;
        p.cursor_user_moved = true;
        p.progress = ListingProgress::Complete { count: 1 };
        p.begin_new_listing(PathBuf::from("/b"));
        assert_eq!(p.cwd, PathBuf::from("/b"));
        assert!(p.entries.is_empty());
        assert_eq!(p.cursor, 0);
        assert!(!p.cursor_user_moved);
        assert_eq!(p.progress, ListingProgress::Streaming { count: 0 });
    }

    #[test]
    fn begin_new_listing_clears_selection() {
        let mut p = PanelState::new(PathBuf::from("/a"));
        p.entries = vec![file("x")];
        p.selected.insert(OsString::from("x"));
        p.begin_new_listing(PathBuf::from("/b"));
        assert!(p.selected.is_empty());
    }

    #[test]
    fn ins_toggles_and_advances_without_wrap() {
        let mut p = PanelState::new(PathBuf::from("/"));
        p.entries = vec![file("a"), file("b"), file("c")];
        p.toggle_selection_and_advance();
        assert!(p.selected.contains(&OsString::from("a")));
        assert_eq!(p.cursor, 1);
        p.toggle_selection_and_advance();
        assert!(p.selected.contains(&OsString::from("b")));
        assert_eq!(p.cursor, 2);
        // Toggle "c" while already at the last row: no wrap past the end.
        p.toggle_selection_and_advance();
        assert!(p.selected.contains(&OsString::from("c")));
        assert_eq!(p.cursor, 2);
        // Toggling again deselects.
        p.cursor = 0;
        p.toggle_selection_and_advance();
        assert!(!p.selected.contains(&OsString::from("a")));
    }

    #[test]
    fn parent_dir_is_never_selectable() {
        let mut p = PanelState::new(PathBuf::from("/a/b"));
        p.entries = vec![Entry::parent_dir(), file("x")];
        p.cursor = 0;
        p.toggle_selection_and_advance();
        assert!(p.selected.is_empty());
        assert_eq!(p.cursor, 1);
    }

    #[test]
    fn wildcard_select_and_deselect_are_additive_and_subtractive() {
        let mut p = PanelState::new(PathBuf::from("/"));
        p.entries = vec![Entry::parent_dir(), file("a.txt"), file("b.txt"), dir("c")];
        p.select_matching("*.txt");
        assert_eq!(p.selected.len(), 2);
        assert!(!p.selected.contains(&OsString::from(".."))); // parent excluded even if pattern is "*"
        p.deselect_matching("a.txt");
        assert_eq!(p.selected, HashSet::from([OsString::from("b.txt")]));
    }

    #[test]
    fn invert_selection_flips_all_selectable_leaves_parent_unselected() {
        let mut p = PanelState::new(PathBuf::from("/"));
        p.entries = vec![Entry::parent_dir(), file("a"), file("b"), dir("c")];
        p.selected.insert(OsString::from("a"));
        p.invert_selection();
        assert_eq!(p.selected, HashSet::from([OsString::from("b"), OsString::from("c")]));
        p.invert_selection();
        assert_eq!(p.selected, HashSet::from([OsString::from("a")]));
    }

    #[test]
    fn selection_status_counts_files_and_zeros_directories() {
        let mut p = PanelState::new(PathBuf::from("/"));
        p.entries = vec![
            Entry { name: "a.txt".into(), kind: EntryKind::File, size: 100, modified: None },
            Entry { name: "sub".into(), kind: EntryKind::Directory, size: 0, modified: None },
        ];
        assert_eq!(p.selection_status(), None);
        p.select_matching("*");
        assert_eq!(p.selection_status(), Some("2 files selected, 100 bytes".to_string()));
    }

    #[test]
    fn selection_persists_across_cursor_move_and_resort_clears_only_on_dir_change() {
        let mut p = PanelState::new(PathBuf::from("/"));
        p.entries = vec![file("a"), file("b")];
        p.selected.insert(OsString::from("a"));
        p.move_cursor(CursorMove::Down(1));
        assert!(p.selected.contains(&OsString::from("a")));
        insert_sorted(&mut p.entries, file("aa"), SortMode::Name, SortDirection::Asc);
        assert!(p.selected.contains(&OsString::from("a")), "re-sort/insert must not disturb selection");
    }

    #[test]
    fn reconcile_selection_drops_vanished_entries() {
        let mut p = PanelState::new(PathBuf::from("/"));
        p.entries = vec![file("a"), file("b")];
        p.selected.insert(OsString::from("a"));
        p.selected.insert(OsString::from("b"));
        p.entries = vec![file("b")];
        p.reconcile_selection();
        assert_eq!(p.selected, HashSet::from([OsString::from("b")]));
    }

    #[test]
    fn wildcard_match_supports_star_and_question_case_insensitive() {
        assert!(wildcard_match("*.TXT", "readme.txt"));
        assert!(wildcard_match("a?c", "abc"));
        assert!(!wildcard_match("a?c", "abbc"));
        assert!(wildcard_match("*", "anything"));
        assert!(!wildcard_match("*.txt", "readme.md"));
    }

    mod selection_proptests {
        use super::*;
        use proptest::prelude::*;

        fn arb_name() -> impl Strategy<Value = String> {
            "[a-z]{1,6}\\.[a-z]{1,3}"
        }

        proptest! {
            #[test]
            fn invert_twice_is_identity(names in prop::collection::hash_set(arb_name(), 1..8)) {
                let mut p = PanelState::new(PathBuf::from("/"));
                p.entries = names.iter().map(|n| file(n)).collect();
                let before = p.selected.clone();
                p.invert_selection();
                p.invert_selection();
                prop_assert_eq!(p.selected, before);
            }

            #[test]
            fn selected_bytes_never_counts_directories(names in prop::collection::vec(arb_name(), 1..8)) {
                let mut p = PanelState::new(PathBuf::from("/"));
                p.entries = names.iter().map(|n| dir(n)).collect();
                p.select_matching("*");
                prop_assert_eq!(p.selected_bytes(), 0);
            }
        }
    }

    fn sized(name: &str, size: u64) -> Entry {
        Entry { name: OsString::from(name), kind: EntryKind::File, size, modified: None }
    }

    fn timed(name: &str, day: u8) -> Entry {
        Entry {
            name: OsString::from(name),
            kind: EntryKind::File,
            size: 0,
            modified: Some(DateTime { year: 2026, month: 1, day, hour: 0, minute: 0 }),
        }
    }

    fn names(p: &PanelState) -> Vec<String> {
        p.entries.iter().map(|e| e.name.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn each_sort_mode_reorders_by_its_own_key() {
        let mut p = PanelState::new(PathBuf::from("/"));
        p.entries = vec![sized("c.zzz", 30), sized("a.mmm", 10), sized("b.aaa", 20)];

        p.set_sort_mode(SortMode::Name);
        assert_eq!(names(&p), vec!["a.mmm", "b.aaa", "c.zzz"]);

        p.set_sort_mode(SortMode::Extension);
        assert_eq!(names(&p), vec!["b.aaa", "a.mmm", "c.zzz"]);

        p.set_sort_mode(SortMode::Size);
        assert_eq!(names(&p), vec!["a.mmm", "b.aaa", "c.zzz"]);

        p.entries = vec![timed("new", 3), timed("old", 1), timed("mid", 2)];
        p.set_sort_mode(SortMode::Time);
        assert_eq!(names(&p), vec!["old", "mid", "new"]);
    }

    #[test]
    fn unsorted_mode_preserves_enumeration_order_but_keeps_parent_first() {
        let mut p = PanelState::new(PathBuf::from("/a/b"));
        p.entries = vec![sized("zebra", 1), Entry::parent_dir(), sized("apple", 2), sized("mango", 3)];
        p.set_sort_mode(SortMode::Unsorted);
        assert_eq!(names(&p), vec!["..", "zebra", "apple", "mango"]);
    }

    #[test]
    fn sorting_never_disturbs_the_entry_metadata_it_sorts_on() {
        // The sort must operate on already-gathered metadata: nothing here
        // can re-stat, so a round trip through every mode must leave the
        // multiset of entries identical.
        let mut p = PanelState::new(PathBuf::from("/"));
        let original = vec![sized("a", 3), sized("b", 1), sized("c", 2)];
        p.entries = original.clone();
        for mode in [SortMode::Size, SortMode::Time, SortMode::Extension, SortMode::Name, SortMode::Unsorted] {
            p.set_sort_mode(mode);
            let mut got = p.entries.clone();
            got.sort_by(|a, b| a.name.cmp(&b.name));
            let mut want = original.clone();
            want.sort_by(|a, b| a.name.cmp(&b.name));
            assert_eq!(got, want, "{mode:?} changed the entry set, not just the order");
        }
    }

    #[test]
    fn set_sort_mode_keeps_the_cursor_on_the_same_entry() {
        let mut p = PanelState::new(PathBuf::from("/"));
        p.entries = vec![sized("a", 30), sized("b", 20), sized("c", 10)];
        p.cursor = 0; // on "a"
        p.set_sort_mode(SortMode::Size);
        assert_eq!(names(&p), vec!["c", "b", "a"]);
        assert_eq!(p.entries[p.cursor].name, OsString::from("a"));
    }

    #[test]
    fn sort_mode_is_independent_per_panel() {
        let mut left = PanelState::new(PathBuf::from("/l"));
        let mut right = PanelState::new(PathBuf::from("/r"));
        left.entries = vec![sized("a", 30), sized("b", 10)];
        right.entries = vec![sized("a", 30), sized("b", 10)];
        left.set_sort_mode(SortMode::Size);
        right.set_sort_mode(SortMode::Name);
        assert_eq!(left.sort_mode, SortMode::Size);
        assert_eq!(right.sort_mode, SortMode::Name);
        assert_eq!(names(&left), vec!["b", "a"]);
        assert_eq!(names(&right), vec!["a", "b"]);
    }

    #[test]
    fn re_read_preserves_sort_mode_and_display_mode() {
        let mut p = PanelState::new(PathBuf::from("/a"));
        p.sort_mode = SortMode::Size;
        p.display_mode = DisplayMode::Info;
        p.info.file_count = Some(7);
        p.begin_new_listing(PathBuf::from("/a"));
        assert_eq!(p.sort_mode, SortMode::Size);
        assert_eq!(p.display_mode, DisplayMode::Info);
        assert_eq!(p.info, InfoValues::default(), "stale directory-scoped values are cleared by a re-read");
    }

    #[test]
    fn streamed_inserts_follow_the_active_sort_mode() {
        let mut p = PanelState::new(PathBuf::from("/"));
        p.sort_mode = SortMode::Size;
        p.insert_streamed(sized("big", 100));
        p.insert_streamed(sized("small", 1));
        p.insert_streamed(sized("mid", 50));
        assert_eq!(names(&p), vec!["small", "mid", "big"]);
    }

    #[test]
    fn parent_path_root_is_none() {
        assert_eq!(parent_path(Path::new("/")), None);
        assert_eq!(parent_path(Path::new("/a/b")), Some(PathBuf::from("/a")));
    }

    #[cfg(windows)]
    #[test]
    fn parent_path_windows_drive_root_is_none() {
        assert_eq!(parent_path(Path::new(r"C:\")), None);
        assert_eq!(parent_path(Path::new(r"C:\a\b")), Some(PathBuf::from(r"C:\a")));
    }

    #[test]
    fn dates_are_ordered_for_display_use() {
        let earlier = DateTime { year: 2020, month: 1, day: 1, hour: 0, minute: 0 };
        let later = DateTime { year: 2021, month: 1, day: 1, hour: 0, minute: 0 };
        assert!(earlier < later);
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        fn arb_name() -> impl Strategy<Value = String> {
            "[a-zA-Z0-9_.]{1,12}"
        }

        fn arb_entry() -> impl Strategy<Value = Entry> {
            (arb_name(), any::<bool>()).prop_map(|(name, is_dir)| Entry {
                name: OsString::from(name),
                kind: if is_dir { EntryKind::Directory } else { EntryKind::File },
                size: 0,
                modified: None,
            })
        }

        proptest! {
            #[test]
            fn parent_dir_always_sorts_first(mut entries in prop::collection::vec(arb_entry(), 0..10)) {
                entries.push(Entry::parent_dir());
                entries.sort_by(|a, b| cmp_entries(a, b, SortMode::Name, SortDirection::Asc));
                prop_assert_eq!(entries[0].kind, EntryKind::ParentDir);
            }

            #[test]
            fn insert_sorted_preserves_sorted_order(names in prop::collection::vec(arb_name(), 0..15)) {
                let mut entries: Vec<Entry> = Vec::new();
                for name in names {
                    let e = Entry { name: OsString::from(name), kind: EntryKind::File, size: 0, modified: None };
                    insert_sorted(&mut entries, e, SortMode::Name, SortDirection::Asc);
                }
                for w in entries.windows(2) {
                    prop_assert_ne!(cmp_entries(&w[0], &w[1], SortMode::Name, SortDirection::Asc), std::cmp::Ordering::Greater);
                }
            }

            #[test]
            fn cmp_entries_is_antisymmetric_for_non_parent(a in arb_entry(), b in arb_entry()) {
                let ab = cmp_entries(&a, &b, SortMode::Name, SortDirection::Asc);
                let ba = cmp_entries(&b, &a, SortMode::Name, SortDirection::Asc);
                prop_assert_eq!(ab.reverse(), ba);
            }

            #[test]
            fn desc_is_reverse_of_asc_for_non_parent(a in arb_entry(), b in arb_entry()) {
                let asc = cmp_entries(&a, &b, SortMode::Name, SortDirection::Asc);
                let desc = cmp_entries(&a, &b, SortMode::Name, SortDirection::Desc);
                prop_assert_eq!(asc.reverse(), desc);
            }
        }
    }

    mod sort_mode_proptests {
        use super::*;
        use crate::listing::{cmp_by_mode, DateTime};
        use proptest::prelude::*;

        const MODES: [SortMode; 4] = [SortMode::Name, SortMode::Extension, SortMode::Time, SortMode::Size];

        fn arb_sortable_entry() -> impl Strategy<Value = Entry> {
            ("[a-c]{1,3}(\\.[a-c]{1,3})?", 0u64..4, prop::option::of(1u8..4), any::<bool>()).prop_map(|(name, size, day, is_dir)| Entry {
                name: OsString::from(name),
                kind: if is_dir { EntryKind::Directory } else { EntryKind::File },
                size,
                modified: day.map(|d| DateTime { year: 2026, month: 1, day: d, hour: 0, minute: 0 }),
            })
        }

        fn arb_entries() -> impl Strategy<Value = Vec<Entry>> {
            prop::collection::vec(arb_sortable_entry(), 0..12)
        }

        proptest! {
            /// Each comparator must be a total order: antisymmetric, and
            /// transitive on both `Less` and `Equal`.
            #[test]
            fn comparators_are_antisymmetric(a in arb_sortable_entry(), b in arb_sortable_entry()) {
                for mode in MODES {
                    prop_assert_eq!(cmp_by_mode(&a, &b, mode).reverse(), cmp_by_mode(&b, &a, mode), "{:?}", mode);
                }
            }

            #[test]
            fn comparators_are_transitive(a in arb_sortable_entry(), b in arb_sortable_entry(), c in arb_sortable_entry()) {
                for mode in MODES {
                    let (ab, bc, ac) = (cmp_by_mode(&a, &b, mode), cmp_by_mode(&b, &c, mode), cmp_by_mode(&a, &c, mode));
                    if ab == Ordering::Less && bc == Ordering::Less {
                        prop_assert_eq!(ac, Ordering::Less, "{:?} not transitive on Less", mode);
                    }
                    if ab == Ordering::Equal && bc == Ordering::Equal {
                        prop_assert_eq!(ac, Ordering::Equal, "{:?} not transitive on Equal", mode);
                    }
                }
            }

            #[test]
            fn comparators_are_reflexive(a in arb_sortable_entry()) {
                for mode in MODES {
                    prop_assert_eq!(cmp_by_mode(&a, &a, mode), Ordering::Equal, "{:?}", mode);
                }
            }

            /// Equal-comparing entries keep their pre-sort relative order.
            #[test]
            fn sorting_is_stable_for_equal_keys(entries in arb_entries()) {
                for mode in MODES {
                    let mut p = PanelState::new(PathBuf::from("/"));
                    // Tag each entry with its original index so ties are
                    // detectable after the sort.
                    p.entries = entries.clone();
                    let before: Vec<(usize, Entry)> = p.entries.iter().cloned().enumerate().collect();
                    p.set_sort_mode(mode);

                    let mut cursor = 0usize;
                    let mut order: Vec<usize> = Vec::new();
                    for sorted in &p.entries {
                        // Match each sorted entry back to the earliest
                        // unconsumed identical original.
                        let idx = before.iter().position(|(i, e)| e == sorted && !order.contains(i)).unwrap();
                        order.push(before[idx].0);
                        cursor += 1;
                    }
                    prop_assert_eq!(cursor, p.entries.len());

                    for w in p.entries.windows(2).enumerate() {
                        let (i, pair) = w;
                        if cmp_by_mode(&pair[0], &pair[1], mode) == Ordering::Equal {
                            prop_assert!(order[i] < order[i + 1], "{:?} reordered equal-comparing entries", mode);
                        }
                    }
                }
            }

            /// Sorting is a permutation: no entry is dropped or duplicated.
            #[test]
            fn sorting_preserves_the_entry_multiset(entries in arb_entries()) {
                for mode in [SortMode::Name, SortMode::Extension, SortMode::Time, SortMode::Size, SortMode::Unsorted] {
                    let mut p = PanelState::new(PathBuf::from("/"));
                    p.entries = entries.clone();
                    p.set_sort_mode(mode);
                    let mut got = p.entries.clone();
                    let mut want = entries.clone();
                    let key = |e: &Entry| (e.name.clone(), e.size, e.modified);
                    got.sort_by_key(key);
                    want.sort_by_key(key);
                    prop_assert_eq!(got, want, "{:?} is not a permutation", mode);
                }
            }

            /// One panel's sort mode never leaks into the other's ordering.
            #[test]
            fn sort_modes_stay_independent_between_panels(entries in arb_entries()) {
                let mut left = PanelState::new(PathBuf::from("/l"));
                let mut right = PanelState::new(PathBuf::from("/r"));
                left.entries = entries.clone();
                right.entries = entries.clone();

                right.set_sort_mode(SortMode::Name);
                let right_before = right.entries.clone();
                left.set_sort_mode(SortMode::Size);

                prop_assert_eq!(right.sort_mode, SortMode::Name);
                prop_assert_eq!(right.entries, right_before);
                prop_assert_eq!(left.sort_mode, SortMode::Size);
            }
        }
    }

    // -----------------------------------------------------------------
    // Quick filter (task 15.2)
    // -----------------------------------------------------------------

    mod quick_filter_tests {
        use super::*;

        fn panel_with(names: &[&str]) -> PanelState {
            let mut p = PanelState::new(PathBuf::from("/"));
            p.entries = std::iter::once(Entry::parent_dir()).chain(names.iter().map(|n| file(n))).collect();
            p
        }

        #[test]
        fn typing_narrows_to_substring_matches_keeping_parent_visible() {
            let mut p = panel_with(&["report.txt", "readme.md", "notes.txt"]);
            p.activate_quick_filter();
            p.quick_filter_push('r');
            p.quick_filter_push('e');
            p.quick_filter_push('p');
            let visible: Vec<String> = p.visible_indices().into_iter().map(|i| p.entries[i].name.to_string_lossy().into_owned()).collect();
            assert_eq!(visible, vec!["..", "report.txt"]);
        }

        #[test]
        fn no_matches_yields_an_empty_body_other_than_parent() {
            let mut p = panel_with(&["a.txt", "b.txt"]);
            p.activate_quick_filter();
            for c in "zzz".chars() {
                p.quick_filter_push(c);
            }
            let visible: Vec<String> = p.visible_indices().into_iter().map(|i| p.entries[i].name.to_string_lossy().into_owned()).collect();
            assert_eq!(visible, vec![".."]);
            assert!(p.quick_filter.is_some(), "quick-filter mode must stay active with no matches");
        }

        #[test]
        fn backspace_re_narrows_live() {
            let mut p = panel_with(&["report.txt", "readme.md"]);
            p.activate_quick_filter();
            p.quick_filter_push('r');
            p.quick_filter_push('e');
            p.quick_filter_push('p');
            p.quick_filter_backspace();
            p.quick_filter_backspace();
            assert_eq!(p.quick_filter.as_deref(), Some("r"));
            let visible: Vec<String> = p.visible_indices().into_iter().map(|i| p.entries[i].name.to_string_lossy().into_owned()).collect();
            assert_eq!(visible, vec!["..", "report.txt", "readme.md"]);
        }

        #[test]
        fn cursor_snaps_to_a_visible_entry_when_filtered_out() {
            let mut p = panel_with(&["apple", "banana", "cherry"]);
            p.cursor = 2; // "banana" (index 2 after the parent-dir slot at 0)
            p.activate_quick_filter();
            p.quick_filter_push('c'); // only "cherry" (+ "..") remain
            let cursor_name = p.entries[p.cursor].name.to_string_lossy().into_owned();
            assert_eq!(cursor_name, "cherry", "cursor must land on a still-visible entry, not a hidden one");
        }

        #[test]
        fn navigation_is_restricted_to_matching_entries() {
            let mut p = panel_with(&["apple", "avocado", "banana", "apricot"]);
            p.activate_quick_filter();
            p.quick_filter_push('a');
            p.quick_filter_push('p'); // pattern "ap": apple and apricot match; avocado and banana do not
            p.cursor = 1; // "apple"
            p.move_cursor(CursorMove::Down(1));
            assert_eq!(p.entries[p.cursor].name, OsString::from("apricot"), "avocado and banana must be skipped as they don't match");
            p.move_cursor(CursorMove::Down(1));
            assert_eq!(p.entries[p.cursor].name, OsString::from("apricot"), "movement must not run past the last visible match");
        }

        #[test]
        fn esc_clears_the_filter_and_restores_the_full_listing() {
            let mut p = panel_with(&["report.txt", "readme.md"]);
            p.activate_quick_filter();
            p.quick_filter_push('x');
            p.clear_quick_filter();
            assert_eq!(p.quick_filter, None);
            assert_eq!(p.visible_indices().len(), p.entries.len());
        }

        #[test]
        fn selection_and_sort_mode_survive_activation_and_clearing() {
            let mut p = panel_with(&["a.txt", "b.txt"]);
            p.selected.insert(OsString::from("a.txt"));
            p.sort_mode = SortMode::Size;
            p.activate_quick_filter();
            p.quick_filter_push('b');
            p.clear_quick_filter();
            assert!(p.selected.contains(&OsString::from("a.txt")));
            assert_eq!(p.sort_mode, SortMode::Size);
        }

        #[test]
        fn backspace_on_an_empty_pattern_stays_active_and_empty() {
            let mut p = panel_with(&["a.txt"]);
            p.activate_quick_filter();
            p.quick_filter_backspace();
            assert_eq!(p.quick_filter.as_deref(), Some(""));
        }
    }

    // -----------------------------------------------------------------
    // Panel tabs (task 15.5)
    // -----------------------------------------------------------------

    mod tab_tests {
        use super::*;

        #[test]
        fn starts_with_exactly_one_tab() {
            let p = PanelState::new(PathBuf::from("/a"));
            assert_eq!(p.tab_count(), 1);
        }

        #[test]
        fn each_tab_retains_its_own_directory_and_state() {
            let mut p = PanelState::new(PathBuf::from(r"C:\A"));
            p.sort_mode = SortMode::Size;
            p.selected.insert(OsString::from("x"));
            p.cursor = 2;

            p.open_tab();
            p.begin_new_listing(PathBuf::from(r"C:\B"));
            p.sort_mode = SortMode::Name;
            assert_eq!(p.tab_count(), 2);

            // Switch back to tab 1.
            p.switch_tab(1);
            assert_eq!(p.cwd, PathBuf::from(r"C:\A"));
            assert_eq!(p.sort_mode, SortMode::Size);
            assert_eq!(p.cursor, 2);
            assert!(p.selected.contains(&OsString::from("x")));

            // Switch to tab 2 and confirm it kept its own state.
            p.switch_tab(2);
            assert_eq!(p.cwd, PathBuf::from(r"C:\B"));
            assert_eq!(p.sort_mode, SortMode::Name);
        }

        #[test]
        fn ctrl_t_opens_and_activates_a_new_tab_inheriting_state() {
            let mut p = PanelState::new(PathBuf::from(r"C:\Work"));
            p.cursor = 1;
            p.entries = vec![file("a"), file("b")];
            p.selected.insert(OsString::from("a"));

            p.open_tab();

            assert_eq!(p.tab_count(), 2);
            assert_eq!(p.cwd, PathBuf::from(r"C:\Work"), "the new tab inherits the originating tab's directory");
            assert_eq!(p.cursor, 1);
            assert!(p.selected.contains(&OsString::from("a")));

            // The original tab is untouched by a change made in the new one.
            p.begin_new_listing(PathBuf::from(r"C:\Work\sub"));
            p.switch_tab(1);
            assert_eq!(p.cwd, PathBuf::from(r"C:\Work"));
            assert!(p.selected.contains(&OsString::from("a")), "original tab's selection must be untouched");
        }

        #[test]
        fn ctrl_w_closes_the_active_tab_and_activates_a_neighbor() {
            let mut p = PanelState::new(PathBuf::from(r"C:\1"));
            p.open_tab();
            p.begin_new_listing(PathBuf::from(r"C:\2"));
            p.open_tab();
            p.begin_new_listing(PathBuf::from(r"C:\3"));
            assert_eq!(p.tab_count(), 3);
            p.switch_tab(2);
            assert_eq!(p.cwd, PathBuf::from(r"C:\2"));

            p.close_tab();

            assert_eq!(p.tab_count(), 2);
            assert_ne!(p.cwd, PathBuf::from(r"C:\2"), "the closed tab's directory must no longer be active");
        }

        #[test]
        fn ctrl_w_is_a_no_op_with_a_single_tab() {
            let mut p = PanelState::new(PathBuf::from(r"C:\only"));
            p.close_tab();
            assert_eq!(p.tab_count(), 1);
            assert_eq!(p.cwd, PathBuf::from(r"C:\only"));
        }

        #[test]
        fn alt_n_activates_the_nth_tab() {
            let mut p = PanelState::new(PathBuf::from(r"C:\1"));
            p.open_tab();
            p.begin_new_listing(PathBuf::from(r"C:\2"));
            p.open_tab();
            p.begin_new_listing(PathBuf::from(r"C:\3"));
            p.open_tab();
            p.begin_new_listing(PathBuf::from(r"C:\4"));
            assert_eq!(p.tab_count(), 4);

            p.switch_tab(3);
            assert_eq!(p.cwd, PathBuf::from(r"C:\3"));
        }

        #[test]
        fn tab_dirs_reflects_order_and_the_active_tabs_live_directory() {
            let mut p = PanelState::new(PathBuf::from(r"C:\1"));
            p.open_tab();
            p.begin_new_listing(PathBuf::from(r"C:\2"));
            p.open_tab();
            p.begin_new_listing(PathBuf::from(r"C:\3"));
            assert_eq!(p.tab_dirs(), vec![PathBuf::from(r"C:\1"), PathBuf::from(r"C:\2"), PathBuf::from(r"C:\3")]);

            p.switch_tab(1);
            assert_eq!(p.tab_dirs(), vec![PathBuf::from(r"C:\1"), PathBuf::from(r"C:\2"), PathBuf::from(r"C:\3")], "order is stable across switches");
        }

        #[test]
        fn alt_n_out_of_range_is_a_no_op() {
            let mut p = PanelState::new(PathBuf::from(r"C:\1"));
            p.open_tab();
            p.begin_new_listing(PathBuf::from(r"C:\2"));
            assert_eq!(p.tab_count(), 2);
            let before = p.cwd.clone();
            p.switch_tab(5);
            assert_eq!(p.cwd, before, "out-of-range switch must leave the active tab unchanged");
            p.switch_tab(0);
            assert_eq!(p.cwd, before, "n == 0 must also be a no-op");
        }
    }
}
