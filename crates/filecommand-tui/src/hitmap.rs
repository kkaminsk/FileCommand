//! Records where things were drawn each frame, so `input::map_mouse` can
//! translate a raw mouse event into what the user clicked without any
//! coordinate or `crossterm::event::KeyModifiers` ever reaching
//! `filecommand-core` (design D2; mouse-input "Hit-testing stays in the
//! TUI"). Every field is built by the matching view module's own hit-test
//! helper (`views::panel::hit_test`, `views::keybar::hit_slots`, ...),
//! mirroring the exact geometry that module's renderer just drew — so the
//! hit map can never describe a rect that isn't actually on screen this
//! frame.

use std::ffi::OsString;
use std::path::PathBuf;

use filecommand_core::menu::MenuId;
use filecommand_core::update::ButtonId;
use filecommand_core::PanelSide;
use ratatui::layout::Rect;

/// One panel's clickable regions this frame (mouse-input "Row identity
/// survives scrolling").
#[derive(Debug, Clone, Default)]
pub struct PanelHits {
    /// The panel's whole rendered rect, border included — a click here that
    /// misses every row still focuses the panel (mouse-input "Click focuses
    /// and places the cursor": "a left-click on a panel's title or blank
    /// body area SHALL make that panel active only").
    pub area: Rect,
    /// The top-border title row specifically, same focus-only meaning as a
    /// blank-body click.
    pub title: Rect,
    /// Each currently-drawn entry row, keyed by the entry's *original* name
    /// — never a row index — so identity survives scrolling, sorting, and a
    /// quick filter narrowing what's visible (mouse-input "Row identity
    /// survives scrolling"). Populated for `DisplayMode::Full` only — the
    /// other display modes record `area`/`title` only, except `Tree`, which
    /// records [`Self::tree_nodes`] instead.
    pub rows: Vec<(Rect, OsString)>,
    /// Each currently-drawn Tree-mode node row, keyed by the node's own
    /// path rather than its position in the flattened, lazily-expanded node
    /// list — a row index would go stale the instant a sibling node
    /// expands/collapses or the tree re-flattens, exactly the "keying items
    /// by row index is rejected" reasoning design D4 already applies to
    /// dragged items themselves (mouse-panel-drag; additional-panel-modes
    /// "Tree display mode structure and rendering"). Populated only in
    /// `DisplayMode::Tree`; empty otherwise.
    pub tree_nodes: Vec<(Rect, PathBuf)>,
    /// This panel's currently-drawn tab-strip cells, keyed by position in
    /// `PanelState::tab_dirs()` (design D7: "a tab in the strip stands for
    /// its directory") — the same index `DropTarget::Tab` carries. Empty
    /// when the strip isn't shown (fewer than two tabs, or too little
    /// vertical room for it).
    pub tabs: Vec<(Rect, usize)>,
}

/// Every clickable region drawn this frame. `views::render` returns one of
/// these alongside its existing terminal-cursor-position return value,
/// bundled as [`crate::views::RenderOutput`] (design D2).
#[derive(Debug, Clone, Default)]
pub struct HitMap {
    /// Indexed by `PanelSide` — `Left` = 0, `Right` = 1 — via [`HitMap::panel`]
    /// rather than a `HashMap`, since there are always exactly two.
    pub panels: [PanelHits; 2],
    /// Each function-key-bar slot's rect and its number (1..=10, `10` for
    /// F10).
    pub keybar: Vec<(Rect, u8)>,
    /// Each menu-bar title's rect and which menu it opens.
    pub menu_titles: Vec<(Rect, MenuId)>,
    /// The open pull-down's item rects, indexed by position in
    /// `menu::entries(active)` — separators simply have no entry here, so
    /// the index always names the exact same row `MenuState::selected`
    /// would.
    pub menu_items: Vec<(Rect, usize)>,
    /// The open file-action menu's item rects, indexed by position in
    /// `FileActionMenuState::entries` — the same index `FileActionMenuState::
    /// cursor` would land on (mouse-input "File-action menu entries are
    /// clickable").
    pub file_action_menu_items: Vec<(Rect, usize)>,
    /// Every dialog button currently on screen — including a no-framed-
    /// button dialog's hotkey text spans (design D2: the conflict dialog's
    /// `(O)verwrite  (S)kip …` row).
    pub dialog_buttons: Vec<(Rect, ButtonId)>,
    /// The command-line rect. Not yet hit-tested against anything (no
    /// click-to-position feature exists), but recorded so the shape is
    /// stable for when one does.
    pub cmdline: Rect,
}

impl HitMap {
    pub fn panel(&self, side: PanelSide) -> &PanelHits {
        match side {
            PanelSide::Left => &self.panels[0],
            PanelSide::Right => &self.panels[1],
        }
    }

    pub fn panel_mut(&mut self, side: PanelSide) -> &mut PanelHits {
        match side {
            PanelSide::Left => &mut self.panels[0],
            PanelSide::Right => &mut self.panels[1],
        }
    }
}

/// Point-in-rect test shared by every hit-testing call site — the single
/// definition `input::map_mouse` and every view module's `hit_*` builder
/// both rely on, so "is this cell inside that rect" is never reimplemented
/// slightly differently in two places.
pub fn hit(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x && x < area.x + area.width && y >= area.y && y < area.y + area.height
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_is_inclusive_of_the_top_left_and_exclusive_of_the_far_edges() {
        let r = Rect { x: 2, y: 3, width: 4, height: 2 };
        assert!(hit(r, 2, 3), "top-left corner is inside");
        assert!(hit(r, 5, 4), "bottom-right-most cell is inside");
        assert!(!hit(r, 6, 4), "one past the right edge is outside");
        assert!(!hit(r, 5, 5), "one past the bottom edge is outside");
        assert!(!hit(r, 1, 3), "one before the left edge is outside");
    }

    #[test]
    fn panel_and_panel_mut_index_by_side_consistently() {
        let mut hm = HitMap::default();
        hm.panel_mut(PanelSide::Left).area = Rect { x: 0, y: 0, width: 10, height: 10 };
        hm.panel_mut(PanelSide::Right).area = Rect { x: 10, y: 0, width: 10, height: 10 };
        assert_eq!(hm.panel(PanelSide::Left).area.x, 0);
        assert_eq!(hm.panel(PanelSide::Right).area.x, 10);
    }

    /// mouse-input "Row identity survives scrolling" / design D2: whatever
    /// `views::panel::hit_test` records for a frame, every entry-row rect
    /// must lie entirely inside its own panel's rect (never spill into the
    /// other panel, the command line, or the key bar), and no two row rects
    /// — same panel or the two panels together — may overlap. Real geometry
    /// end to end: `layout::compute` derives the panel rects exactly as
    /// `views::render` does, and `panel::hit_test` walks them exactly as it
    /// does each frame, across varied terminal sizes, panel splits, entry
    /// counts, and scroll offsets (including offsets past the end of the
    /// list, which a stale hit map click can momentarily produce — see
    /// `update::tests::click_entry_on_a_vanished_name_is_a_no_op`'s sibling
    /// reasoning).
    mod proptests {
        use std::ffi::OsString;
        use std::path::PathBuf;

        use filecommand_core::listing::{Entry, EntryKind};
        use filecommand_core::panel::PanelState;
        use proptest::prelude::*;

        use super::*;
        use crate::layout;
        use crate::views::panel as panel_view;

        fn panel_with(cwd: &str, n_entries: usize, scroll_offset: usize) -> PanelState {
            let mut panel = PanelState::new(PathBuf::from(cwd));
            panel.entries = (0..n_entries).map(|i| Entry { name: OsString::from(format!("f{i}.txt")), kind: EntryKind::File, size: 0, modified: None }).collect();
            panel.scroll_offset = scroll_offset;
            panel
        }

        /// Whether `inner` lies entirely within `outer` — not just its
        /// top-left corner, the whole rect.
        fn rect_nests_inside(outer: Rect, inner: Rect) -> bool {
            inner.x >= outer.x && inner.y >= outer.y && inner.x + inner.width <= outer.x + outer.width && inner.y + inner.height <= outer.y + outer.height
        }

        fn rects_overlap(a: Rect, b: Rect) -> bool {
            a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
        }

        proptest! {
            #[test]
            fn hit_map_row_rects_nest_inside_their_panel_and_never_overlap(
                width in 60u16..300,
                height in 10u16..80,
                split_percent in 0u16..=100,
                n_entries in 0usize..80,
                scroll_offset in 0usize..120,
            ) {
                let l = layout::compute((width, height), split_percent);
                let left_panel = panel_with(r"C:\left", n_entries, scroll_offset);
                let right_panel = panel_with(r"C:\right", n_entries, scroll_offset);
                let left_hits = panel_view::hit_test(l.left, &left_panel);
                let right_hits = panel_view::hit_test(l.right, &right_panel);

                for (rect, name) in &left_hits.rows {
                    prop_assert!(rect_nests_inside(left_hits.area, *rect), "left row {name:?} at {rect:?} escapes panel area {:?}", left_hits.area);
                }
                for (rect, name) in &right_hits.rows {
                    prop_assert!(rect_nests_inside(right_hits.area, *rect), "right row {name:?} at {rect:?} escapes panel area {:?}", right_hits.area);
                }

                let mut all_rows: Vec<Rect> = left_hits.rows.iter().map(|(r, _)| *r).collect();
                all_rows.extend(right_hits.rows.iter().map(|(r, _)| *r));
                for i in 0..all_rows.len() {
                    for j in (i + 1)..all_rows.len() {
                        prop_assert!(!rects_overlap(all_rows[i], all_rows[j]), "rows {:?} and {:?} overlap", all_rows[i], all_rows[j]);
                    }
                }
            }
        }
    }
}
