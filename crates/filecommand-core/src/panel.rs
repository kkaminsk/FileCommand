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

/// One row of a Tree-mode flattened directory tree: an already-visible
/// directory node, its display depth, whether it has been expanded yet, and
/// the pre-computed branch-glyph prefix drawn before its name (design D7;
/// additional-panel-modes "Tree display mode structure and rendering").
/// `prefix`/`continuation` are computed once at insertion time
/// ([`TreeState::insert_children`]) rather than derived at render time, so
/// rendering stays a straightforward per-row lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    pub path: PathBuf,
    pub depth: usize,
    pub expanded: bool,
    /// The `│  `/`├─`/`└─` glyphs (in the cyan frame style) drawn
    /// immediately before this row's name; empty for the drive-root row
    /// (additional-panel-modes "Tree branch glyphs and indentation").
    pub prefix: String,
    /// The continuation guide inherited by this node's own children when
    /// they are later spliced in — `prefix` with the trailing connector
    /// swapped for either `│  ` (more siblings follow at this depth) or
    /// `   ` (this was the last sibling).
    continuation: String,
}

/// A panel's Tree-mode navigation state: the flattened, lazily-expanded
/// node list plus the display mode to restore when Enter leaves Tree mode
/// (additional-panel-modes "Tree lazy expansion", "Tree mode drives the
/// opposite panel"; design D7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeState {
    pub nodes: Vec<TreeNode>,
    pub cursor: usize,
    /// The display mode this panel was in before entering Tree mode
    /// (additional-panel-modes "Enter returns to prior list mode at chosen
    /// directory").
    pub prior_mode: DisplayMode,
    /// The topmost visible node row, in the same flat `nodes` index space as
    /// `cursor` — the render-viewport offset for Tree mode's own scrolling
    /// (additional-panel-modes "Tree mode scrolling"; design D2/D3). Wiring
    /// the minimal-shift reconciliation that keeps it in sync with `cursor`
    /// is `additional-panel-modes`' own follow-up group; this field exists
    /// now so it round-trips through tab snapshot/restore (panel-navigation
    /// "Scroll offset is core panel state") the same way `PanelState`'s does.
    pub scroll_offset: usize,
}

impl TreeState {
    /// A freshly entered Tree session rooted at `root` (a drive root, e.g.
    /// `C:\`), with `prior_mode` recorded so Enter can restore it. The root
    /// itself is the only node until its children are expanded (additional-
    /// panel-modes "No up-front full-drive scan").
    pub fn new(root: PathBuf, prior_mode: DisplayMode) -> TreeState {
        TreeState {
            nodes: vec![TreeNode { path: root, depth: 0, expanded: false, prefix: String::new(), continuation: String::new() }],
            cursor: 0,
            prior_mode,
            scroll_offset: 0,
        }
    }

    /// The currently highlighted node, if any.
    pub fn selected(&self) -> Option<&TreeNode> {
        self.nodes.get(self.cursor)
    }

    /// Move the tree cursor exactly like [`PanelState::move_cursor`]'s
    /// unfiltered path — clamped Up/Down/Home/End over the flat node list.
    pub fn move_cursor(&mut self, m: CursorMove) {
        if self.nodes.is_empty() {
            self.cursor = 0;
            return;
        }
        let last = self.nodes.len() - 1;
        self.cursor = match m {
            CursorMove::Up(n) => self.cursor.saturating_sub(n),
            CursorMove::Down(n) => (self.cursor + n).min(last),
            CursorMove::Home => 0,
            CursorMove::End => last,
        };
    }

    /// Scroll the tree's render viewport (`scroll_offset`) by the minimum
    /// amount needed to keep `cursor` inside a `rows`-tall window over the
    /// flattened `nodes` list — a no-op if it's already visible. Mirrors
    /// [`PanelState::ensure_cursor_visible`]'s minimal-shift clamp (design
    /// D2), operating directly on the flat node-index space since Tree mode
    /// has no quick-filter narrowing. Called by `core::update` after every
    /// tree cursor move and after `insert_children` (expansion) changes the
    /// node list out from under the current offset (additional-panel-modes
    /// "Tree mode scrolling").
    pub fn ensure_cursor_visible(&mut self, rows: usize) {
        let rows = rows.max(1);
        if self.nodes.is_empty() {
            self.scroll_offset = 0;
            return;
        }
        let last = self.nodes.len() - 1;
        let pos = self.cursor.min(last);
        if pos < self.scroll_offset {
            self.scroll_offset = pos;
        } else if pos >= self.scroll_offset + rows {
            self.scroll_offset = pos + 1 - rows;
        }
    }

    /// Insert `children` (already-sorted child directories, from
    /// `listing::list_child_dirs`) beneath the first not-yet-expanded node
    /// whose path is `path`, computing each new node's branch-glyph prefix
    /// from its parent's continuation guide. A stale reply — `path` no
    /// longer present, or already expanded — is a no-op, returning `false`
    /// (additional-panel-modes "Children read on expand").
    pub fn insert_children(&mut self, path: &Path, children: Vec<crate::listing::Entry>) -> bool {
        let Some(idx) = self.nodes.iter().position(|n| n.path == path && !n.expanded) else { return false };
        self.nodes[idx].expanded = true;
        let parent_continuation = self.nodes[idx].continuation.clone();
        let parent_path = self.nodes[idx].path.clone();
        let depth = self.nodes[idx].depth + 1;
        let last_i = children.len().saturating_sub(1);
        let new_nodes: Vec<TreeNode> = children
            .into_iter()
            .enumerate()
            .map(|(i, e)| {
                let is_last = i == last_i;
                let connector = if is_last { "\u{2514}\u{2500}" } else { "\u{251C}\u{2500}" };
                let prefix = format!("{parent_continuation}{connector}");
                let continuation = format!("{parent_continuation}{}", if is_last { "   " } else { "\u{2502}  " });
                TreeNode { path: parent_path.join(&e.name), depth, expanded: false, prefix, continuation }
            })
            .collect();
        self.nodes.splice(idx + 1..idx + 1, new_nodes);
        true
    }
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

/// The outcome of a clipboard action (`clipboard-export` "Clipboard
/// feedback"), shown in place of the panel's normal mini-status content
/// until the next key press or `clock_ms` reaches `expires_at_ms` —
/// whichever comes first (`crate::update` clears it on any non-`Tick`
/// command and on a `Tick` once `State::clock_ms >= expires_at_ms`). Unlike
/// `PanelState::last_error`, which lingers until superseded by success, this
/// is always transient — every clipboard action, success or failure,
/// produces exactly one of these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardFeedback {
    pub message: String,
    /// Rendered in the error color role (like `last_error`) rather than the
    /// normal mini-status role.
    pub is_error: bool,
    pub expires_at_ms: u64,
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
    /// The most recent clipboard action's outcome, or `None` when nothing
    /// is being shown (`clipboard-export` "Clipboard feedback"). Set by
    /// `crate::update`'s `Command::CopyToClipboard`/`Command::ClipboardResult`
    /// handling.
    pub clipboard_feedback: Option<ClipboardFeedback>,
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
    /// Tree-mode navigation state, `Some` only while `display_mode ==
    /// DisplayMode::Tree` — `None` the rest of the time, including before
    /// Tree mode has ever been entered (additional-panel-modes "Tree mode
    /// drives the opposite panel"; design D7).
    pub tree: Option<TreeState>,
    /// Set by an M5 find-file navigation (`update::handle_find_file`'s
    /// `FindFileConfirm`) to the matched entry's original name; consumed
    /// once this directory's listing reaches `ListingProgress::Complete`,
    /// settling the cursor there (find-file "Navigate to a chosen result").
    /// `None` for every ordinary navigation.
    pub pending_cursor_target: Option<OsString>,
    /// The topmost visible position of the panel body's render window, in
    /// *visible-position* space — an index into `visible_indices()`, not a
    /// raw `entries` index (panel-navigation "Scroll offset is core panel
    /// state"; design D3). Kept in sync with `cursor` by
    /// [`Self::ensure_cursor_visible`], which `core::update` calls after
    /// every cursor-moving or list-mutating command — the renderer only
    /// ever reads it (design D2, modeled on `EditorState::top_line`/
    /// `ensure_caret_visible`).
    pub scroll_offset: usize,
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
            clipboard_feedback: None,
            selected: HashSet::new(),
            quick_filter: None,
            tabs: Vec::new(),
            active_tab_index: 0,
            git_info: GitInfo::none(),
            git_request: None,
            tree: None,
            pending_cursor_target: None,
            scroll_offset: 0,
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

    /// Scroll the render viewport (`scroll_offset`) by the minimum amount
    /// needed to keep the cursor inside a `rows`-tall window — a no-op if
    /// it's already visible. Operates in *visible-position* space (an index
    /// into `visible_indices()`), the same space `scroll_offset` is defined
    /// in, so it re-clamps correctly whether the cursor itself moved or the
    /// quick-filter-narrowed list changed shape underneath it. Mirrors
    /// `EditorState::ensure_caret_visible`'s minimal-shift clamp (design
    /// D2): single-step moves shift the window by one row, `Home` (cursor
    /// at position 0) pins the window to the top, `End` (cursor at the last
    /// position) pins it to the bottom, and any other jump just lands the
    /// cursor inside the window at the minimum distance. Called by
    /// `core::update` after every cursor-moving or list-mutating command —
    /// never by the renderer, which only reads `scroll_offset` (panel-
    /// navigation "Viewport scrolling keeps the cursor visible", "Scroll
    /// offset is core panel state").
    pub fn ensure_cursor_visible(&mut self, rows: usize) {
        let rows = rows.max(1);
        let visible = self.visible_indices();
        if visible.is_empty() {
            self.scroll_offset = 0;
            return;
        }
        let pos = visible.iter().position(|&i| i == self.cursor).unwrap_or(0);
        if pos < self.scroll_offset {
            self.scroll_offset = pos;
        } else if pos >= self.scroll_offset + rows {
            self.scroll_offset = pos + 1 - rows;
        }
    }

    /// Brief mode's column-window counterpart to [`Self::ensure_cursor_visible`]:
    /// the render window is `cols` whole columns of `rows` consecutive
    /// visible-positions each (column-major, matching the renderer's `pos =
    /// c * rows + row`), and `scroll_offset` is kept on a `rows`-multiple so
    /// it always names a column boundary. Scrolls by the minimum number of
    /// *columns* — never partial columns — that brings the cursor's column
    /// back inside the window; a no-op when it already is (design D4;
    /// additional-panel-modes "Brief mode column scrolling"). Called by
    /// `core::update` instead of `ensure_cursor_visible` whenever the panel
    /// is in Brief mode.
    pub fn ensure_cursor_visible_brief(&mut self, rows: usize, cols: usize) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        let visible = self.visible_indices();
        if visible.is_empty() {
            self.scroll_offset = 0;
            return;
        }
        let pos = visible.iter().position(|&i| i == self.cursor).unwrap_or(0);
        let pos_col = pos / rows;
        let mut start_col = self.scroll_offset / rows;
        if pos_col < start_col {
            start_col = pos_col;
        } else if pos_col >= start_col + cols {
            start_col = pos_col + 1 - cols;
        }
        self.scroll_offset = start_col * rows;
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
        // A fresh directory has an entirely different entry set, so any
        // scroll position from the old one is meaningless — reset alongside
        // the cursor it tracks (panel-navigation "Scroll offset is core
        // panel state").
        self.scroll_offset = 0;
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
        // A fresh directory has an entirely different entry set; any
        // pending find-file cursor target belonged to whatever navigation
        // just landed here (`begin_listing` sets it *after* calling this),
        // so an ordinary navigation must not inherit a stale target from an
        // earlier one.
        self.pending_cursor_target = None;
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
    /// currently inline (i.e. the active tab's state). The active tab is
    /// never itself stale — a completed job re-reads it immediately rather
    /// than deferring — so this always snapshots `stale: false`; only
    /// [`Self::mark_background_tabs_stale`] sets the flag, directly on
    /// entries already sitting in `tabs`.
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
            clipboard_feedback: self.clipboard_feedback.clone(),
            selected: self.selected.clone(),
            quick_filter: self.quick_filter.clone(),
            git_info: self.git_info.clone(),
            git_request: self.git_request,
            tree: self.tree.clone(),
            pending_cursor_target: self.pending_cursor_target.clone(),
            scroll_offset: self.scroll_offset,
            stale: false,
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
        self.clipboard_feedback = data.clipboard_feedback;
        self.selected = data.selected;
        self.quick_filter = data.quick_filter;
        self.git_info = data.git_info;
        self.git_request = data.git_request;
        self.tree = data.tree;
        self.pending_cursor_target = data.pending_cursor_target;
        // Restored as-is; it may no longer fit the current viewport height
        // (the panel may have been resized, or the split/tab-strip state
        // differs from when this tab was stashed) — `core::update` re-clamps
        // via `ensure_cursor_visible` right after calling into tab
        // switch/open/close, which is where the current row count is known
        // (panel-navigation "Tab restore re-clamps against the current
        // viewport").
        self.scroll_offset = data.scroll_offset;
        // `data.stale` is deliberately not stored anywhere inline — there is
        // no "the active tab is stale" state, only "a background tab in
        // `tabs` is stale". Callers that need to know whether the tab being
        // adopted here was stale (`switch_tab`/`close_tab`) read `.stale`
        // off the `TabData` themselves before it reaches this point.
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
    /// when only one tab remains (panel-tabs "Close tab (Ctrl+W)"). Returns
    /// whether the newly-active neighbor had been marked stale by a
    /// completed file-operation job — the flag is consumed by this call;
    /// `core::update`'s `Command::CloseTab` handler uses the return value to
    /// issue a fresh read instead of showing the stale cached listing
    /// (panel-tabs "Stale background tab refresh on activation").
    pub fn close_tab(&mut self) -> bool {
        let mut list = self.full_tab_list();
        if list.len() <= 1 {
            return false;
        }
        list.remove(self.active_tab_index);
        let new_active = self.active_tab_index.min(list.len() - 1);
        let stale = list[new_active].stale;
        self.apply_tab_list(list, new_active);
        stale
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
    /// (Alt+1..9)"). Returns whether the newly-active tab had been marked
    /// stale by a completed file-operation job — the flag is consumed by
    /// this call; see [`Self::close_tab`] for how the return value is used.
    pub fn switch_tab(&mut self, n: usize) -> bool {
        if n == 0 {
            return false;
        }
        let target = n - 1;
        if target == self.active_tab_index || target >= self.tab_count() {
            return false;
        }
        let list = self.full_tab_list();
        let stale = list[target].stale;
        self.apply_tab_list(list, target);
        stale
    }

    /// Mark every background tab (an entry in `tabs`; the active tab's own
    /// state lives inline and is refreshed immediately elsewhere, never
    /// deferred) whose directory is exactly `dir` as stale, so it is
    /// refreshed with a fresh read the moment it next becomes active
    /// instead of keeping its now possibly-incorrect cached listing
    /// (file-operations "Automatic panel re-read on completion"; panel-tabs
    /// "Stale background tab refresh on activation").
    pub fn mark_background_tabs_stale(&mut self, dir: &Path) {
        for tab in &mut self.tabs {
            if tab.cwd == dir {
                tab.stale = true;
            }
        }
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
    pub clipboard_feedback: Option<ClipboardFeedback>,
    pub selected: HashSet<OsString>,
    pub quick_filter: Option<String>,
    pub git_info: GitInfo,
    pub git_request: Option<u64>,
    pub tree: Option<TreeState>,
    pub pending_cursor_target: Option<OsString>,
    pub scroll_offset: usize,
    /// Set when a completed/cancelled-with-partial-changes file-operation
    /// job touched `cwd` while this tab was in the background, so its
    /// cached `entries` may no longer reflect the on-disk state. Cleared
    /// (consumed) the moment this tab becomes active again — `switch_tab`/
    /// `close_tab` read it before reinstating this data and issue a fresh
    /// read instead of restoring the stale cache (file-operations
    /// "Automatic panel re-read on completion"; panel-tabs "Stale
    /// background tab refresh on activation").
    pub stale: bool,
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

    // -----------------------------------------------------------------
    // Tree display mode (task 15.8 / additional-panel-modes)
    // -----------------------------------------------------------------

    mod tree_tests {
        use super::*;
        use crate::listing::Entry as ListEntry;

        fn dir_child(name: &str) -> ListEntry {
            ListEntry { name: OsString::from(name), kind: EntryKind::Directory, size: 0, modified: None }
        }

        #[test]
        fn new_tree_starts_with_only_the_root_node() {
            let tree = TreeState::new(PathBuf::from(r"C:\"), DisplayMode::Full);
            assert_eq!(tree.nodes.len(), 1);
            assert_eq!(tree.nodes[0].path, PathBuf::from(r"C:\"));
            assert_eq!(tree.nodes[0].depth, 0);
            assert!(!tree.nodes[0].expanded);
            assert_eq!(tree.cursor, 0);
            assert_eq!(tree.prior_mode, DisplayMode::Full);
        }

        #[test]
        fn insert_children_expands_the_named_node_and_splices_children_beneath_it() {
            let mut tree = TreeState::new(PathBuf::from(r"C:\"), DisplayMode::Full);
            let ok = tree.insert_children(&PathBuf::from(r"C:\"), vec![dir_child("alpha"), dir_child("beta")]);
            assert!(ok);
            assert!(tree.nodes[0].expanded);
            assert_eq!(tree.nodes.len(), 3);
            assert_eq!(tree.nodes[1].path, PathBuf::from(r"C:\alpha"));
            assert_eq!(tree.nodes[1].depth, 1);
            assert_eq!(tree.nodes[2].path, PathBuf::from(r"C:\beta"));
            // Not-last sibling gets the tee glyph, last sibling the corner.
            assert_eq!(tree.nodes[1].prefix, "\u{251C}\u{2500}");
            assert_eq!(tree.nodes[2].prefix, "\u{2514}\u{2500}");
        }

        #[test]
        fn insert_children_on_a_grandchild_nests_the_prefix_under_its_parents_continuation() {
            let mut tree = TreeState::new(PathBuf::from(r"C:\"), DisplayMode::Full);
            tree.insert_children(&PathBuf::from(r"C:\"), vec![dir_child("alpha"), dir_child("beta")]);
            // "alpha" (index 1) is not the last sibling, so its continuation
            // guide carries a vertical bar down to its own children.
            let ok = tree.insert_children(&PathBuf::from(r"C:\alpha"), vec![dir_child("inner")]);
            assert!(ok);
            let inner = tree.nodes.iter().find(|n| n.path == Path::new(r"C:\alpha\inner")).unwrap();
            assert_eq!(inner.depth, 2);
            assert_eq!(inner.prefix, "\u{2502}  \u{2514}\u{2500}");
        }

        #[test]
        fn insert_children_no_up_front_scan_only_the_expanded_node_gains_children() {
            let mut tree = TreeState::new(PathBuf::from(r"C:\"), DisplayMode::Full);
            tree.insert_children(&PathBuf::from(r"C:\"), vec![dir_child("alpha"), dir_child("beta")]);
            // "beta" (index 2) has not been expanded — it must show no
            // children of its own (additional-panel-modes "Unexpanded
            // directory shows no children").
            assert!(!tree.nodes[2].expanded);
            assert_eq!(tree.nodes.len(), 3, "no other node's children were fetched up front");
        }

        #[test]
        fn insert_children_is_a_no_op_for_an_already_expanded_or_absent_node() {
            let mut tree = TreeState::new(PathBuf::from(r"C:\"), DisplayMode::Full);
            assert!(tree.insert_children(&PathBuf::from(r"C:\"), vec![dir_child("alpha")]));
            // Already expanded: a stale/duplicate reply must not double-insert.
            let ok_again = tree.insert_children(&PathBuf::from(r"C:\"), vec![dir_child("alpha")]);
            assert!(!ok_again);
            assert_eq!(tree.nodes.len(), 2);
            // Absent path: also a no-op.
            assert!(!tree.insert_children(&PathBuf::from(r"C:\nowhere"), vec![dir_child("x")]));
        }

        #[test]
        fn move_cursor_clamps_over_the_flat_node_list() {
            let mut tree = TreeState::new(PathBuf::from(r"C:\"), DisplayMode::Full);
            tree.insert_children(&PathBuf::from(r"C:\"), vec![dir_child("alpha"), dir_child("beta")]);
            assert_eq!(tree.nodes.len(), 3);
            tree.move_cursor(CursorMove::Down(1));
            assert_eq!(tree.cursor, 1);
            tree.move_cursor(CursorMove::Down(10));
            assert_eq!(tree.cursor, 2, "clamped to the last node");
            tree.move_cursor(CursorMove::Home);
            assert_eq!(tree.cursor, 0);
            tree.move_cursor(CursorMove::End);
            assert_eq!(tree.cursor, 2);
            tree.move_cursor(CursorMove::Up(100));
            assert_eq!(tree.cursor, 0, "saturating, never underflows");
        }

        #[test]
        fn selected_returns_the_node_under_the_cursor() {
            let mut tree = TreeState::new(PathBuf::from(r"C:\"), DisplayMode::Full);
            tree.insert_children(&PathBuf::from(r"C:\"), vec![dir_child("alpha")]);
            tree.move_cursor(CursorMove::Down(1));
            assert_eq!(tree.selected().unwrap().path, PathBuf::from(r"C:\alpha"));
        }
    }

    // -----------------------------------------------------------------
    // Scroll offset / viewport reconciliation (panel-scrolling task 1.5)
    // -----------------------------------------------------------------

    mod scroll_offset_tests {
        use super::*;

        /// A panel of `n` plain files named `e0`, `e1`, … in insertion
        /// order, sorted `Unsorted` so the entry order is exactly the
        /// creation order — keeps every test's expected positions obvious.
        fn panel_of(n: usize) -> PanelState {
            let mut p = PanelState::new(PathBuf::from("/"));
            p.sort_mode = SortMode::Unsorted;
            p.entries = (0..n).map(|i| file(&format!("e{i}"))).collect();
            p
        }

        #[test]
        fn fresh_panel_starts_with_a_zero_offset() {
            let p = panel_of(20);
            assert_eq!(p.scroll_offset, 0);
        }

        #[test]
        fn no_op_while_the_cursor_stays_inside_the_window() {
            let mut p = panel_of(20);
            p.cursor = 3;
            p.ensure_cursor_visible(10);
            assert_eq!(p.scroll_offset, 0);
            p.cursor = 9; // last row of a 10-row window starting at 0
            p.ensure_cursor_visible(10);
            assert_eq!(p.scroll_offset, 0, "the window must not move while the cursor is still inside it");
        }

        #[test]
        fn cursor_below_the_bottom_edge_shifts_the_window_by_exactly_one_line() {
            let mut p = panel_of(20);
            p.scroll_offset = 0;
            p.cursor = 9; // last visible row of a 10-row window
            p.ensure_cursor_visible(10);
            assert_eq!(p.scroll_offset, 0);
            p.cursor = 10; // one step past the bottom edge
            p.ensure_cursor_visible(10);
            assert_eq!(p.scroll_offset, 1, "must shift by exactly one line, not re-center");
        }

        #[test]
        fn cursor_above_the_top_edge_shifts_the_window_by_exactly_one_line() {
            let mut p = panel_of(20);
            p.scroll_offset = 5;
            p.cursor = 5; // first visible row of a window starting at 5
            p.ensure_cursor_visible(10);
            assert_eq!(p.scroll_offset, 5);
            p.cursor = 4; // one step above the top edge
            p.ensure_cursor_visible(10);
            assert_eq!(p.scroll_offset, 4, "must shift by exactly one line, not re-center");
        }

        #[test]
        fn home_pins_the_window_to_the_top() {
            let mut p = panel_of(20);
            p.scroll_offset = 8;
            p.cursor = 15;
            p.move_cursor(CursorMove::Home);
            p.ensure_cursor_visible(10);
            assert_eq!(p.cursor, 0);
            assert_eq!(p.scroll_offset, 0, "Home must pin the window's first row to the list's first position");
        }

        #[test]
        fn end_pins_the_window_to_the_bottom() {
            let mut p = panel_of(20);
            p.scroll_offset = 0;
            p.cursor = 0;
            p.move_cursor(CursorMove::End);
            p.ensure_cursor_visible(10);
            assert_eq!(p.cursor, 19);
            assert_eq!(p.scroll_offset, 10, "End must pin the window's last row to the list's last position (20 - 10 = 10)");
        }

        #[test]
        fn a_jump_that_lands_far_outside_the_window_still_lands_the_cursor_in_view() {
            // Simulates a type-ahead/find-file settle: the cursor is set
            // directly, far from the current window, in one step.
            let mut p = panel_of(50);
            p.scroll_offset = 0;
            p.cursor = 42;
            p.ensure_cursor_visible(10);
            assert!(
                p.scroll_offset <= p.cursor && p.cursor < p.scroll_offset + 10,
                "cursor {} must be inside the window [{}, {})",
                p.cursor,
                p.scroll_offset,
                p.scroll_offset + 10
            );
            assert_eq!(p.scroll_offset, 33, "minimal-shift lands the cursor exactly on the window's last row: 42 + 1 - 10 = 33");
        }

        #[test]
        fn quick_filter_narrowing_re_clamps_the_offset_in_visible_position_space() {
            let mut p = panel_of(30);
            p.cursor = 25;
            p.scroll_offset = 20; // cursor at visible-position 25, well inside [20, 30)
            p.ensure_cursor_visible(10);
            assert_eq!(p.scroll_offset, 20);

            // Narrow to only even-numbered entries: "e25" is filtered out
            // and the cursor snaps to the nearest still-visible entry.
            p.quick_filter = Some(String::new());
            p.activate_quick_filter();
            for c in "e2".chars() {
                p.quick_filter_push(c);
            }
            // "e2","e20".."e29" all contain "e2"; narrow further to land on
            // a single, known entry.
            p.quick_filter = Some("e24".to_string());
            let visible = p.visible_indices();
            assert_eq!(visible.len(), 1, "exactly \"e24\" should match");
            p.cursor = visible[0];
            // Re-clamp: with only one visible position (position 0), the
            // stale offset of 20 must be pulled back to 0.
            p.ensure_cursor_visible(10);
            assert_eq!(p.scroll_offset, 0, "offset is in visible-position space and must re-clamp when the visible list shrinks");
        }

        #[test]
        fn re_sort_keeps_the_cursors_entry_in_view_after_it_moves_far_in_the_list() {
            let mut p = PanelState::new(PathBuf::from("/"));
            p.entries = (0..30).map(|i| sized(&format!("e{i}"), i as u64)).collect();
            p.sort_mode = SortMode::Unsorted;
            p.cursor = 0; // "e0"
            p.scroll_offset = 0;
            p.ensure_cursor_visible(10);
            // Reverse the order by size: "e0" (size 0) moves to the very
            // end of the list.
            p.set_sort_mode(SortMode::Size);
            p.resort(); // no-op re-affirmation the mode is size-sorted already
            p.entries.reverse(); // now largest-to-smallest ("e0" last)
            p.cursor = p.entries.iter().position(|e| e.name == OsString::from("e0")).unwrap();
            assert_eq!(p.cursor, 29);
            p.ensure_cursor_visible(10);
            assert!(p.scroll_offset <= p.cursor && p.cursor < p.scroll_offset + 10);
            assert_eq!(p.scroll_offset, 20, "\"e0\" is now at position 29: 29 + 1 - 10 = 20");
        }

        #[test]
        fn streamed_inserts_keep_the_offset_pinned_to_zero_while_the_cursor_is_unmoved() {
            let mut p = PanelState::new(PathBuf::from("/"));
            p.sort_mode = SortMode::Name;
            for i in 0..15 {
                p.insert_streamed(file(&format!("e{i:02}")));
                // Mirrors what `update::apply_listing_event` does after
                // every `ListingChunk`: re-clamp with a fixed 10-row window.
                p.ensure_cursor_visible(10);
            }
            assert_eq!(p.cursor, 0, "cursor stays pinned to the top while the user hasn't moved it");
            assert_eq!(p.scroll_offset, 0, "the window stays pinned to the top right along with the cursor");
        }

        #[test]
        fn tab_restore_round_trips_the_scroll_offset_and_a_later_reconcile_can_re_clamp_it() {
            let mut p = panel_of(20);
            p.cursor = 15;
            p.scroll_offset = 10;
            p.open_tab(); // new tab inherits the same state, including scroll_offset
            p.begin_new_listing(PathBuf::from("/other"));
            p.entries = (0..3).map(|i| file(&format!("x{i}"))).collect();
            p.cursor = 0;
            assert_eq!(p.scroll_offset, 0, "a fresh listing resets the offset alongside the cursor");

            p.switch_tab(1);
            assert_eq!(p.cursor, 15);
            assert_eq!(p.scroll_offset, 10, "the stashed tab's offset round-trips through open_tab/switch_tab");

            // Restoring against a shorter viewport than when it was stashed
            // must re-clamp so the cursor stays visible.
            p.ensure_cursor_visible(3);
            assert!(p.scroll_offset <= p.cursor && p.cursor < p.scroll_offset + 3);
            assert_eq!(p.scroll_offset, 13, "15 + 1 - 3 = 13");
        }
    }

    // -----------------------------------------------------------------
    // Brief column-window and Tree scrolling (panel-scrolling task 2.3)
    // -----------------------------------------------------------------

    mod brief_scroll_tests {
        use super::*;

        /// A panel of `n` plain files, `Unsorted` so creation order is
        /// preserved and each position is exactly `ei`'s index `i`.
        fn panel_of(n: usize) -> PanelState {
            let mut p = PanelState::new(PathBuf::from("/"));
            p.sort_mode = SortMode::Unsorted;
            p.entries = (0..n).map(|i| file(&format!("e{i}"))).collect();
            p
        }

        #[test]
        fn fresh_panel_starts_column_offset_at_zero() {
            let p = panel_of(30);
            assert_eq!(p.scroll_offset, 0);
        }

        #[test]
        fn no_op_while_the_cursor_stays_inside_the_column_window() {
            // rows = 5, cols = 2 -> a 10-position window [0, 10).
            let mut p = panel_of(30);
            p.cursor = 8; // column 1, row 3 -- still inside the window
            p.ensure_cursor_visible_brief(5, 2);
            assert_eq!(p.scroll_offset, 0, "the window must not move while the cursor is already inside it");
        }

        #[test]
        fn cursor_past_the_last_visible_column_shifts_the_window_by_exactly_one_column() {
            let mut p = panel_of(30);
            p.scroll_offset = 0;
            p.cursor = 9; // column 1, row 4: the window's last column
            p.ensure_cursor_visible_brief(5, 2);
            assert_eq!(p.scroll_offset, 0);
            p.cursor = 10; // column 2, row 0: one step past the last column
            p.ensure_cursor_visible_brief(5, 2);
            assert_eq!(p.scroll_offset, 5, "shifts by exactly one column (5 positions), not further");
            // The leftmost column (positions 0..5, i.e. column 0) has left
            // the window; the cursor's column (2) is now the window's last.
            assert!(p.scroll_offset / 5 <= 2 && 2 < p.scroll_offset / 5 + 2);
        }

        #[test]
        fn cursor_before_the_first_visible_column_shifts_the_window_by_exactly_one_column() {
            let mut p = panel_of(30);
            p.scroll_offset = 10; // window starts at column 2
            p.cursor = 10; // column 2, row 0: the window's first column
            p.ensure_cursor_visible_brief(5, 2);
            assert_eq!(p.scroll_offset, 10);
            p.cursor = 9; // column 1, row 4: one step before the first column
            p.ensure_cursor_visible_brief(5, 2);
            assert_eq!(p.scroll_offset, 5, "shifts left by exactly one column");
        }

        #[test]
        fn window_start_always_lands_on_a_column_boundary() {
            let mut p = panel_of(47);
            let rows = 5;
            let cols = 2;
            // Walk the cursor across the whole list; after every step the
            // offset must be an exact multiple of `rows` (additional-panel-
            // modes "Window start stays on a column boundary").
            for pos in 0..47 {
                p.cursor = pos;
                p.ensure_cursor_visible_brief(rows, cols);
                assert_eq!(p.scroll_offset % rows, 0, "offset {} is not a multiple of rows_h {} at cursor {}", p.scroll_offset, rows, pos);
            }
        }

        #[test]
        fn end_lands_the_cursor_in_the_windows_last_column() {
            // 23 entries, rows = 5 -> columns 0..4 (col 4 has only 3 rows).
            let mut p = panel_of(23);
            p.scroll_offset = 0;
            p.cursor = 0;
            p.move_cursor(CursorMove::End);
            assert_eq!(p.cursor, 22);
            p.ensure_cursor_visible_brief(5, 2);
            // cursor's column = 22 / 5 = 4; minimal shift lands columns [3,4)
            // in the window: offset = (4 + 1 - 2) * 5 = 15.
            assert_eq!(p.scroll_offset, 15);
            assert_eq!(p.scroll_offset % 5, 0, "offset stays on a column boundary");
        }

        #[test]
        fn home_pins_the_window_to_the_first_column() {
            let mut p = panel_of(30);
            p.scroll_offset = 15;
            p.cursor = 20;
            p.move_cursor(CursorMove::Home);
            assert_eq!(p.cursor, 0);
            p.ensure_cursor_visible_brief(5, 2);
            assert_eq!(p.scroll_offset, 0, "Home must pin the window to the first column");
        }

        #[test]
        fn fitting_list_keeps_the_window_at_the_first_column() {
            // Every position fits within a single window (cols * rows = 30
            // exactly covers a 30-entry list): the offset must stay put at 0
            // wherever the cursor lands.
            let mut p = panel_of(30);
            for pos in [0usize, 14, 29] {
                p.cursor = pos;
                p.ensure_cursor_visible_brief(10, 3);
                assert_eq!(p.scroll_offset, 0, "a fully-fitting list never needs to scroll (cursor {pos})");
            }
        }

        #[test]
        fn empty_visible_list_leaves_the_offset_at_zero() {
            let mut p = PanelState::new(PathBuf::from("/"));
            p.ensure_cursor_visible_brief(5, 2);
            assert_eq!(p.scroll_offset, 0);
        }
    }

    mod tree_scroll_tests {
        use super::*;
        use crate::listing::Entry as ListEntry;

        fn dir_child(name: &str) -> ListEntry {
            ListEntry { name: OsString::from(name), kind: EntryKind::Directory, size: 0, modified: None }
        }

        /// A flattened tree of `n` top-level directories under `C:\`.
        fn tree_of(n: usize) -> TreeState {
            let mut t = TreeState::new(PathBuf::from(r"C:\"), DisplayMode::Full);
            let children: Vec<ListEntry> = (0..n).map(|i| dir_child(&format!("d{i}"))).collect();
            t.insert_children(&PathBuf::from(r"C:\"), children);
            t
        }

        #[test]
        fn fresh_tree_starts_with_a_zero_offset() {
            let t = TreeState::new(PathBuf::from(r"C:\"), DisplayMode::Full);
            assert_eq!(t.scroll_offset, 0);
        }

        #[test]
        fn no_op_while_the_tree_cursor_stays_inside_the_window() {
            let mut t = tree_of(20); // node 0 is the root, 1..=20 are d0..d19
            t.cursor = 5;
            t.ensure_cursor_visible(10);
            assert_eq!(t.scroll_offset, 0, "cursor is within the first 10-row window");
        }

        #[test]
        fn tree_cursor_below_the_bottom_scrolls_the_node_window_by_one_row() {
            let mut t = tree_of(20);
            t.scroll_offset = 0;
            t.cursor = 9; // last visible row of a 10-row window
            t.ensure_cursor_visible(10);
            assert_eq!(t.scroll_offset, 0);
            t.move_cursor(CursorMove::Down(1));
            assert_eq!(t.cursor, 10);
            t.ensure_cursor_visible(10);
            assert_eq!(t.scroll_offset, 1, "shifts by exactly one row (additional-panel-modes \"Tree cursor below the bottom scrolls the nodes\")");
        }

        #[test]
        fn tree_cursor_above_the_top_scrolls_the_node_window_by_one_row() {
            let mut t = tree_of(20);
            t.scroll_offset = 5;
            t.cursor = 5;
            t.ensure_cursor_visible(10);
            assert_eq!(t.scroll_offset, 5);
            t.cursor = 4;
            t.ensure_cursor_visible(10);
            assert_eq!(t.scroll_offset, 4);
        }

        #[test]
        fn tree_home_and_end_pin_the_window() {
            let mut t = tree_of(20);
            t.scroll_offset = 8;
            t.cursor = 15;
            t.move_cursor(CursorMove::Home);
            t.ensure_cursor_visible(10);
            assert_eq!(t.scroll_offset, 0);

            t.move_cursor(CursorMove::End);
            t.ensure_cursor_visible(10);
            assert_eq!(t.cursor, 20); // root + 20 children = 21 nodes, last index 20
            assert_eq!(t.scroll_offset, 11, "21 nodes: 20 + 1 - 10 = 11");
        }

        #[test]
        fn expanding_a_directory_that_overflows_the_window_re_clamps_the_offset() {
            // A small tree that fits entirely in a 10-row window at first.
            let mut t = tree_of(3); // root + 3 children = 4 nodes
            t.cursor = 3; // "d2", the last node
            t.ensure_cursor_visible(10);
            assert_eq!(t.scroll_offset, 0, "4 nodes fit comfortably inside 10 rows");

            // Expanding "d2" (the node the cursor sits on) splices many more
            // nodes in beneath it, without moving the cursor itself, growing
            // the flattened list past the window (additional-panel-modes
            // "Expanding a directory can overflow and shows the scrollbar").
            let grandchildren: Vec<ListEntry> = (0..20).map(|i| dir_child(&format!("g{i}"))).collect();
            let ok = t.insert_children(&PathBuf::from(r"C:\d2"), grandchildren);
            assert!(ok);
            assert_eq!(t.nodes.len(), 24, "4 original nodes + 20 newly spliced grandchildren");
            assert_eq!(t.cursor, 3, "expansion does not move the cursor");

            // The cursor's node is still at flat index 3, well within any
            // 10-row window starting at 0 -- expansion inserted *after* it,
            // so no re-clamp is actually needed here, but the call must
            // remain a correct no-op rather than clamping to something
            // arbitrary.
            t.ensure_cursor_visible(10);
            assert_eq!(t.scroll_offset, 0);

            // Move the cursor down into the newly-overflowing tail and
            // confirm the window follows it exactly as it would for any
            // other growth of the node list.
            t.move_cursor(CursorMove::Down(15)); // index 18
            t.ensure_cursor_visible(10);
            assert_eq!(t.cursor, 18);
            assert_eq!(t.scroll_offset, 9, "18 + 1 - 10 = 9");
        }

        #[test]
        fn empty_tree_leaves_the_offset_at_zero() {
            let mut t = TreeState { nodes: Vec::new(), cursor: 0, prior_mode: DisplayMode::Full, scroll_offset: 7 };
            t.ensure_cursor_visible(10);
            assert_eq!(t.scroll_offset, 0);
        }
    }
}
