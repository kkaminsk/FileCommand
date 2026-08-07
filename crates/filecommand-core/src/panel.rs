//! Panel state machine: current directory, entry list, cursor, sort order,
//! and listing progress. No I/O happens here — callers feed in [`Entry`]
//! values produced by the `listing` module.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use crate::listing::{Entry, EntryKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
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
    pub sort_direction: SortDirection,
    pub progress: ListingProgress,
    /// Once the user explicitly moves the cursor, streamed inserts stop
    /// yanking it back to row 0.
    pub cursor_user_moved: bool,
    pub last_error: Option<String>,
}

impl PanelState {
    pub fn new(cwd: PathBuf) -> PanelState {
        PanelState {
            cwd,
            entries: Vec::new(),
            cursor: 0,
            sort_direction: SortDirection::Asc,
            progress: ListingProgress::Streaming { count: 0 },
            cursor_user_moved: false,
            last_error: None,
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
        insert_sorted(&mut self.entries, entry, self.sort_direction);
        if !self.cursor_user_moved {
            self.cursor = 0;
        } else {
            self.clamp_cursor();
        }
    }

    pub fn begin_new_listing(&mut self, cwd: PathBuf) {
        self.cwd = cwd;
        self.entries.clear();
        self.cursor = 0;
        self.cursor_user_moved = false;
        self.progress = ListingProgress::Streaming { count: 0 };
        self.last_error = None;
    }
}

/// Compare two entries for sort order. `..` always sorts first regardless of
/// direction; otherwise a case-insensitive name comparison, directories and
/// files interleaved by name (matching classic Norton Commander behavior).
pub fn cmp_entries(a: &Entry, b: &Entry, dir: SortDirection) -> Ordering {
    match (a.kind == EntryKind::ParentDir, b.kind == EntryKind::ParentDir) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        (false, false) => {}
    }
    let an = a.name.to_string_lossy().to_lowercase();
    let bn = b.name.to_string_lossy().to_lowercase();
    let ord = an.cmp(&bn);
    match dir {
        SortDirection::Asc => ord,
        SortDirection::Desc => ord.reverse(),
    }
}

/// Insert `entry` into `entries` (assumed already sorted by `dir`) at its
/// correct sorted position.
pub fn insert_sorted(entries: &mut Vec<Entry>, entry: Entry, dir: SortDirection) {
    let pos = entries.partition_point(|e| cmp_entries(e, &entry, dir) != Ordering::Greater);
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
        entries.sort_by(|a, b| cmp_entries(a, b, SortDirection::Asc));
        assert_eq!(entries[0].kind, EntryKind::ParentDir);

        let mut entries = vec![file("b.txt"), dir("a"), Entry::parent_dir()];
        entries.sort_by(|a, b| cmp_entries(a, b, SortDirection::Desc));
        assert_eq!(entries[0].kind, EntryKind::ParentDir);
    }

    #[test]
    fn name_sort_is_case_insensitive() {
        let mut entries = vec![file("Banana"), file("apple"), file("Cherry")];
        entries.sort_by(|a, b| cmp_entries(a, b, SortDirection::Asc));
        let names: Vec<String> = entries.iter().map(|e| e.name.to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["apple", "Banana", "Cherry"]);
    }

    #[test]
    fn insert_sorted_keeps_parent_first_and_streams_in_order() {
        let mut entries = vec![Entry::parent_dir()];
        insert_sorted(&mut entries, file("beta"), SortDirection::Asc);
        insert_sorted(&mut entries, file("alpha"), SortDirection::Asc);
        insert_sorted(&mut entries, file("gamma"), SortDirection::Asc);
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
                entries.sort_by(|a, b| cmp_entries(a, b, SortDirection::Asc));
                prop_assert_eq!(entries[0].kind, EntryKind::ParentDir);
            }

            #[test]
            fn insert_sorted_preserves_sorted_order(names in prop::collection::vec(arb_name(), 0..15)) {
                let mut entries: Vec<Entry> = Vec::new();
                for name in names {
                    let e = Entry { name: OsString::from(name), kind: EntryKind::File, size: 0, modified: None };
                    insert_sorted(&mut entries, e, SortDirection::Asc);
                }
                for w in entries.windows(2) {
                    prop_assert_ne!(cmp_entries(&w[0], &w[1], SortDirection::Asc), std::cmp::Ordering::Greater);
                }
            }

            #[test]
            fn cmp_entries_is_antisymmetric_for_non_parent(a in arb_entry(), b in arb_entry()) {
                let ab = cmp_entries(&a, &b, SortDirection::Asc);
                let ba = cmp_entries(&b, &a, SortDirection::Asc);
                prop_assert_eq!(ab.reverse(), ba);
            }

            #[test]
            fn desc_is_reverse_of_asc_for_non_parent(a in arb_entry(), b in arb_entry()) {
                let asc = cmp_entries(&a, &b, SortDirection::Asc);
                let desc = cmp_entries(&a, &b, SortDirection::Desc);
                prop_assert_eq!(asc.reverse(), desc);
            }
        }
    }
}
