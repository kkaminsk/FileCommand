//! Snapshot coverage for `panel-scrolling`: overflow-only scrollbar
//! rendering and windowed content in Full, Brief, and Tree display modes
//! (panel-navigation "Scrollbar indicator on overflow"; additional-panel-
//! modes "Brief mode column scrolling", "Tree mode scrolling"). Every panel
//! here is built with more entries/nodes than its body window can show, and
//! `scroll_offset` is set through the same pure reconciliation functions
//! `core::update` calls (`PanelState::ensure_cursor_visible`/
//! `ensure_cursor_visible_brief`, `TreeState::ensure_cursor_visible`) rather
//! than hand-picked, so the fixtures stay faithful to real reducer output.
//!
//! The existing goldens in `snapshot_views.rs`/`snapshot_matrix.rs` all use
//! `fixed_entries()`/`sample_panel()` (4 entries) or the 2-child tree in
//! `tree_panel()` — none of which reach a body's row count even at the
//! 60x16 floor (Full/Tree bodies there still have well over 4 rows), so
//! task 4.1's audit is satisfied by inspection plus the zero-diff full
//! `cargo test --workspace` run: no existing fixture overflows, and every
//! one of those goldens stayed byte-identical after this change.

use std::path::PathBuf;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use filecommand_core::listing::{Entry, EntryKind};
use filecommand_core::panel::{DisplayMode, ListingProgress, PanelState, TreeState};
use filecommand_core::theme::{ColorDepth, Theme};
use filecommand_core::update::panel_viewport_rows;
use filecommand_core::PanelSide;

use filecommand_tui::views;

fn buffer_to_text(buf: &Buffer, area: Rect) -> String {
    let mut out = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            out.push_str(buf[(area.x + x, area.y + y)].symbol());
        }
        out.push('\n');
    }
    out
}

/// `..` plus `n` files named `file00`, `file01`, ... — `entries[i+1]` is
/// `filei` for `i` in `0..n`, so position `p` (`p >= 1`) in the unfiltered
/// visible list is always `file{p-1:02}`.
fn many_entries(n: usize) -> Vec<Entry> {
    std::iter::once(Entry::parent_dir())
        .chain((0..n).map(|i| Entry { name: format!("file{i:02}").into(), kind: EntryKind::File, size: 0, modified: None }))
        .collect()
}

fn overflow_panel(cwd: &str, n_files: usize, cursor: usize) -> PanelState {
    let mut panel = PanelState::new(PathBuf::from(cwd));
    panel.entries = many_entries(n_files);
    panel.progress = ListingProgress::Complete { count: panel.entries.len() };
    panel.cursor = cursor;
    panel
}

fn render_panel_at(panel: &PanelState, area: Rect) -> String {
    let opposite = overflow_panel(r"C:\opposite", 3, 0);
    let theme = Theme::classic();
    let mut buf = Buffer::empty(area);
    views::panel::render_panel(&mut buf, area, panel, &theme, ColorDepth::Ansi16, true, &pinned_identity(), &opposite, None, PanelSide::Left, None);
    buffer_to_text(&buf, area)
}

fn pinned_identity() -> [String; 4] {
    [
        "FileCommand".to_string(),
        "Version 0.1.0".to_string(),
        "Copyright (C) 2026 The FileCommand Authors".to_string(),
        "Inspired by the Norton Commander, 1986-1998".to_string(),
    ]
}

// ---------------------------------------------------------------------
// Full mode
// ---------------------------------------------------------------------

const TERM: (u16, u16) = (80, 24);

fn full_mode_area() -> Rect {
    let l = filecommand_tui::layout::compute(TERM, 50);
    Rect { x: 0, y: 0, width: l.left.width, height: l.left.height }
}

#[test]
fn snapshot_full_mode_overflow_thumb_at_top() {
    let rows = panel_viewport_rows(TERM, DisplayMode::Full, 1);
    let mut panel = overflow_panel(r"C:\Users\demo\left", rows + 12, 0);
    panel.ensure_cursor_visible(rows);
    assert_eq!(panel.scroll_offset, 0, "cursor at position 0 keeps the window pinned to the top");

    let text = render_panel_at(&panel, full_mode_area());
    assert!(text.contains("UP--DIR"), "the `..` entry (position 0) is visible at the top: {text}");
    let last_file = format!("file{:02}", rows + 12 - 1);
    assert!(!text.contains(&last_file), "the last file is scrolled far out of view: {text}");
    insta::assert_snapshot!("full_mode_overflow_thumb_top", text);
}

#[test]
fn snapshot_full_mode_overflow_thumb_at_bottom() {
    let rows = panel_viewport_rows(TERM, DisplayMode::Full, 1);
    let n_files = rows + 12;
    let mut panel = overflow_panel(r"C:\Users\demo\left", n_files, n_files); // cursor on the last file (position n_files)
    panel.ensure_cursor_visible(rows);
    let total = n_files + 1;
    assert_eq!(panel.scroll_offset, total - rows, "End-style landing pins the window to the bottom");

    let text = render_panel_at(&panel, full_mode_area());
    assert!(!text.contains("UP--DIR"), "the `..` entry has scrolled out of view: {text}");
    let last_file = format!("file{:02}", n_files - 1);
    assert!(text.contains(&last_file), "the last file is visible at the bottom: {text}");
    insta::assert_snapshot!("full_mode_overflow_thumb_bottom", text);
}

#[test]
fn snapshot_full_mode_overflow_thumb_at_middle_shows_scrolled_window_contents() {
    let rows = panel_viewport_rows(TERM, DisplayMode::Full, 1);
    let n_files = rows + 12;
    let total = n_files + 1;
    let max_offset = total - rows; // == 13, independent of `rows`, since n_files == rows + 12
    // A cursor position just past the initial window's bottom edge lands the
    // reconciled offset partway through `0..max_offset` — strictly between
    // the two ends, unlike a `total / 2` cursor which (at these dimensions)
    // is still inside the unscrolled top window and wouldn't move it at all.
    let mid_cursor = rows + 5;
    let mut panel = overflow_panel(r"C:\Users\demo\left", n_files, mid_cursor);
    panel.ensure_cursor_visible(rows);
    assert!(panel.scroll_offset > 0 && panel.scroll_offset < max_offset, "the offset sits strictly between the two ends: {}", panel.scroll_offset);

    let text = render_panel_at(&panel, full_mode_area());
    assert!(!text.contains("UP--DIR"), "the `..` entry (position 0) is scrolled out of the mid-list window: {text}");
    let first_file = "file00";
    assert!(!text.contains(first_file), "the first file is scrolled out of view: {text}");
    let last_file = format!("file{:02}", n_files - 1);
    assert!(!text.contains(&last_file), "the last file has not scrolled into view yet: {text}");
    // The entry the cursor landed on (position mid_cursor -> file{mid_cursor-1}) must be visible.
    let cursor_file = format!("file{:02}", mid_cursor - 1);
    assert!(text.contains(&cursor_file), "the cursor's own entry is inside the rendered window: {text}");
    insta::assert_snapshot!("full_mode_overflow_thumb_middle_scrolled_contents", text);
}

// ---------------------------------------------------------------------
// Brief mode
// ---------------------------------------------------------------------

#[test]
fn snapshot_brief_mode_overflow_shifted_column_window() {
    // A custom area (rather than the 80x24 default) so the column/row counts
    // are small enough to reason about exactly: interior 42 -> 3 columns of
    // 14 cells; body height 10 -> rows_h = 10 (Brief has no header row).
    let area = Rect { x: 0, y: 0, width: 44, height: 12 };
    let rows_h = 10usize;
    let n_cols = 3usize;
    let n_files = 35; // total = 36 positions; window capacity = 3*10 = 30 -> overflow by 6
    let mut panel = overflow_panel(r"C:\Users\demo\left", n_files, n_files); // cursor on the last file
    panel.display_mode = DisplayMode::Brief;
    panel.ensure_cursor_visible_brief(rows_h, n_cols);

    // The cursor's column (35 / 10 = 3) is past the initial 3-column window
    // (columns 0..2), so the window must shift right by exactly one column
    // (design D4 "cursor past the last visible column shifts the window one
    // column"): the new start column is 1, i.e. scroll_offset == rows_h.
    assert_eq!(panel.scroll_offset, rows_h, "the window shifted by exactly one column");
    assert_eq!(panel.scroll_offset % rows_h, 0, "the offset stays on a column boundary");

    let text = render_panel_at(&panel, area);
    assert!(!text.contains("UP--DIR"), "the `..` entry was in the discarded leftmost column: {text}");
    for i in 0..9 {
        let hidden = format!("file{i:02}");
        assert!(!text.contains(&hidden), "file{i:02} was in the discarded leftmost column: {text}");
    }
    let last_file = format!("file{:02}", n_files - 1);
    assert!(text.contains(&last_file), "the cursor's own entry (last file) is inside the shifted window: {text}");
    insta::assert_snapshot!("brief_mode_overflow_shifted_column_window", text);
}

// ---------------------------------------------------------------------
// Tree mode
// ---------------------------------------------------------------------

fn overflow_tree_panel(n_children: usize, cursor: usize, rows: usize) -> PanelState {
    let mut panel = overflow_panel(r"C:\Users\demo\left", 3, 0);
    let mut tree = TreeState::new(PathBuf::from(r"C:\"), DisplayMode::Full);
    let children: Vec<Entry> = (0..n_children).map(|i| Entry { name: format!("dir{i:02}").into(), kind: EntryKind::Directory, size: 0, modified: None }).collect();
    tree.insert_children(&PathBuf::from(r"C:\"), children);
    tree.cursor = cursor;
    tree.ensure_cursor_visible(rows);
    panel.display_mode = DisplayMode::Tree;
    panel.tree = Some(tree);
    panel
}

#[test]
fn snapshot_tree_mode_overflow_after_expansion() {
    let rows = panel_viewport_rows(TERM, DisplayMode::Tree, 1);
    let n_children = rows + 6; // root + n_children overflows the node window
    let total_nodes = n_children + 1;
    let last_cursor = total_nodes - 1; // highlight the very last node
    let panel = overflow_tree_panel(n_children, last_cursor, rows);

    let offset = panel.tree.as_ref().unwrap().scroll_offset;
    assert_eq!(offset, total_nodes - rows, "End-style landing pins the node window to the bottom");

    let text = render_panel_at(&panel, full_mode_area());
    assert!(text.contains("Tree"), "the `Tree` header row still renders: {text}");
    // The root row (a bare `C:\` on its own row, distinct from the
    // mini-status's `C:\dirNN` highlighted-path text) must have scrolled out
    // of the node window along with the early children.
    let root_row_visible = text.lines().any(|l| l.trim() == r"C:\");
    assert!(!root_row_visible, "the drive-root node row scrolled out of view: {text}");
    let hidden_first_child = "DIR00";
    assert!(!text.contains(hidden_first_child), "the first child scrolled out of the overflowed node window: {text}");
    let last_child = format!("DIR{:02}", n_children - 1);
    assert!(text.contains(&last_child), "the last (highlighted) child is visible at the bottom of the node window: {text}");
    insta::assert_snapshot!("tree_mode_overflow_after_expansion", text);
}
