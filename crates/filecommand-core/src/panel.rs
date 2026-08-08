//! Panel state machine: current directory, entry list, cursor, sort order,
//! and listing progress. No I/O happens here — callers feed in [`Entry`]
//! values produced by the `listing` module.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::info::InfoValues;
use crate::listing::{cmp_by_mode, format_count, Entry, EntryKind, SortMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

/// How a panel renders its body. M3 adds `Info`; Brief/Tree/Quick View
/// arrive in M5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayMode {
    #[default]
    Full,
    Info,
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
    pub progress: ListingProgress,
    /// Once the user explicitly moves the cursor, streamed inserts stop
    /// yanking it back to row 0.
    pub cursor_user_moved: bool,
    pub last_error: Option<String>,
    /// Selected entries, keyed by original on-disk name — never by row
    /// index, so selection survives cursor movement, re-sort, and scroll.
    /// The parent-directory pseudo-entry can never appear here.
    pub selected: HashSet<OsString>,
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
            progress: ListingProgress::Streaming { count: 0 },
            cursor_user_moved: false,
            last_error: None,
            selected: HashSet::new(),
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
        let last = self.entries.len() - 1;
        self.cursor = match m {
            CursorMove::Up(n) => self.cursor.saturating_sub(n),
            CursorMove::Down(n) => (self.cursor + n).min(last),
            CursorMove::Home => 0,
            CursorMove::End => last,
        };
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
        // and display mode deliberately survive a re-read.
        self.info = InfoValues::default();
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
}
