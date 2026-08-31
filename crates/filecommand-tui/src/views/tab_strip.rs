//! The panel-tabs compact strip: stepwise label shrinking then
//! active-tab-visible scrolling, as a pure function of the tab set, active
//! index, and strip width (panel-tabs "Tab strip visibility", "Tab label
//! rendering and active styling", "Stepwise label shrinking and scrolling
//! overflow"; design D4).

use std::path::Path;

use filecommand_core::listing::display_width;
use filecommand_core::panel::PanelState;
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

/// Whether `panel` needs a tab-strip row at all (panel-tabs "Strip hidden
/// with one tab").
pub fn is_visible(panel: &PanelState) -> bool {
    panel.tab_count() >= 2
}

/// A basename's max display width in the "truncated" shrink stage — long
/// enough that the example in the spec (` n:NAM… ` for `NAME`) is exactly
/// what a name at this cap renders as.
const TRUNCATED_NAME_WIDTH: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LabelForm {
    Full,
    Truncated,
    NumberOnly,
}

/// One rendered tab-strip cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StripCell {
    pub text: String,
    pub active: bool,
    /// This cell's position in `PanelState::tab_dirs()` — the same index
    /// `DropTarget::Tab` carries (mouse-panel-drag). Not used by
    /// `render_tab_strip`'s own drawing, only by [`hit_test`], but recorded
    /// on every cell `layout` produces (rather than re-deriving it
    /// separately in the hit-test) so the two can never disagree about
    /// which tab a given cell stands for.
    pub index: usize,
}

fn truncate_name(name: &str, max_w: usize) -> String {
    if display_width(name) <= max_w || max_w == 0 {
        return name.chars().take(max_w).collect();
    }
    let mut out = String::new();
    let mut w = 0usize;
    for ch in name.chars() {
        let cw = display_width(&ch.to_string());
        if w + cw > max_w.saturating_sub(1) {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push('\u{2026}');
    out
}

fn label_text(index: usize, basename: &str, form: LabelForm) -> String {
    let n = index + 1;
    match form {
        LabelForm::Full => format!(" {n}:{basename} "),
        LabelForm::Truncated => format!(" {n}:{} ", truncate_name(basename, TRUNCATED_NAME_WIDTH)),
        LabelForm::NumberOnly => format!(" {n} "),
    }
}

/// Expand a window `[lo, hi]` (both inclusive, starting at `active`) as far
/// as it will go within `width`, reserving one cell for a `◄`/`►` overflow
/// marker on whichever end still hides a tab. Greedy right-then-left, which
/// terminates in at most `widths.len()` steps since each successful branch
/// strictly grows the window.
fn scroll_window(widths: &[usize], active: usize, width: usize) -> (usize, usize) {
    let n = widths.len();
    let mut lo = active;
    let mut hi = active;
    let mut total = widths[active];
    loop {
        let can_right = hi + 1 < n;
        let can_left = lo > 0;
        if can_right {
            let new_right_marker = hi + 2 < n; // hi+1 becomes the new hi; is there still more beyond it?
            let left_marker = lo > 0;
            let budget = width.saturating_sub(usize::from(left_marker)).saturating_sub(usize::from(new_right_marker));
            if total + widths[hi + 1] <= budget {
                hi += 1;
                total += widths[hi];
                continue;
            }
        }
        if can_left {
            let new_left_marker = lo > 1;
            let right_marker = hi + 1 < n;
            let budget = width.saturating_sub(usize::from(new_left_marker)).saturating_sub(usize::from(right_marker));
            if total + widths[lo - 1] <= budget {
                lo -= 1;
                total += widths[lo];
                continue;
            }
        }
        break;
    }
    // The growth loop above never checked whether the *un-grown* starting
    // window (just the active tab alone) already needed markers it didn't
    // budget for — that only happens when growth fails on the very first
    // attempt. Correct for it by shrinking back in from whichever end sits
    // farther from `active`, until the window (plus whatever markers it
    // still needs) fits, or there is nothing left to shrink.
    while lo < hi {
        let left_marker = lo > 0;
        let right_marker = hi + 1 < n;
        let needed = total + usize::from(left_marker) + usize::from(right_marker);
        if needed <= width {
            break;
        }
        if hi - active >= active - lo {
            total -= widths[hi];
            hi -= 1;
        } else {
            total -= widths[lo];
            lo += 1;
        }
    }
    (lo, hi)
}

/// Deterministic tab-strip layout: which labels are shown, in which shrink
/// stage, and whether the strip has scrolled — a pure function of the tab
/// basenames, the active index, and the strip width. Returns the visible
/// cells in left-to-right order, plus whether a `◄`/`►` marker is needed at
/// each end.
pub fn layout(basenames: &[String], active: usize, width: usize) -> (Vec<StripCell>, bool, bool) {
    if basenames.is_empty() || width == 0 {
        return (Vec::new(), false, false);
    }
    let active = active.min(basenames.len() - 1);

    for form in [LabelForm::Full, LabelForm::Truncated, LabelForm::NumberOnly] {
        let labels: Vec<String> = basenames.iter().enumerate().map(|(i, n)| label_text(i, n, form)).collect();
        let total: usize = labels.iter().map(|l| display_width(l)).sum();
        if total <= width {
            let cells = labels.into_iter().enumerate().map(|(i, text)| StripCell { text, active: i == active, index: i }).collect();
            return (cells, false, false);
        }
    }

    // Even the number-only form overflows: scroll to keep the active tab
    // visible (panel-tabs "Strip scrolls to keep the active tab visible").
    let labels: Vec<String> = basenames.iter().enumerate().map(|(i, n)| label_text(i, n, LabelForm::NumberOnly)).collect();
    let widths: Vec<usize> = labels.iter().map(|l| display_width(l)).collect();
    let (lo, hi) = scroll_window(&widths, active, width);
    let left_marker = lo > 0;
    let right_marker = hi + 1 < labels.len();
    let cells = (lo..=hi).map(|i| StripCell { text: labels[i].clone(), active: i == active, index: i }).collect();
    (cells, left_marker, right_marker)
}

fn tab_basename(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_uppercase()).unwrap_or_else(|| path.display().to_string().to_uppercase())
}

/// Truncate `s` to at most `max_w` display columns without adding padding.
fn clip(s: &str, max_w: usize) -> String {
    let mut out = String::new();
    let mut acc = 0usize;
    for ch in s.chars() {
        let cw = display_width(&ch.to_string());
        if acc + cw > max_w {
            break;
        }
        out.push(ch);
        acc += cw;
    }
    out
}

/// Render `panel`'s tab strip into `area` (expected to be exactly one row
/// tall, spanning the panel's full width). A no-op if the panel has fewer
/// than 2 tabs — callers should check [`is_visible`] before reserving the
/// row in the first place. `target_tab`, when `Some`, is the
/// [`StripCell::index`] of an in-progress drag's current validated tab
/// target (mouse-panel-drag "Drag feedback"): that cell renders in
/// `Role::ButtonFocused` instead of its ordinary active/inactive style,
/// exactly like a target row/node elsewhere in the panel. `None` for every
/// ordinary frame (no drag, or this strip isn't the drag's target), which
/// renders byte-identical to before this parameter existed.
pub fn render_tab_strip(buf: &mut Buffer, area: Rect, panel: &PanelState, theme: &Theme, depth: ColorDepth, target_tab: Option<usize>) {
    if area.height == 0 || area.width == 0 || !is_visible(panel) {
        return;
    }
    let active_style = role_style(theme, Role::TabActive, depth);
    let inactive_style = role_style(theme, Role::TabInactive, depth);
    let target_style = role_style(theme, Role::ButtonFocused, depth);
    buf.set_string(area.x, area.y, " ".repeat(area.width as usize), inactive_style);

    let basenames: Vec<String> = panel.tab_dirs().iter().map(|p| tab_basename(p)).collect();
    let (cells, left_marker, right_marker) = layout(&basenames, panel.active_tab_index, area.width as usize);

    let right_edge = area.x + area.width;
    let mut x = area.x;
    if left_marker {
        buf.set_string(x, area.y, "\u{25C4}", inactive_style);
        x += 1;
    }
    let label_right_edge = if right_marker { right_edge.saturating_sub(1) } else { right_edge };
    for cell in &cells {
        if x >= label_right_edge {
            break;
        }
        let style = if Some(cell.index) == target_tab {
            target_style
        } else if cell.active {
            active_style
        } else {
            inactive_style
        };
        let budget = (label_right_edge - x) as usize;
        let text = clip(&cell.text, budget);
        buf.set_string(x, area.y, &text, style);
        x += display_width(&text) as u16;
    }
    if right_marker && right_edge > area.x {
        buf.set_string(right_edge - 1, area.y, "\u{25BA}", inactive_style);
    }
}

/// The clickable rect for each currently-drawn tab, keyed by
/// [`StripCell::index`] — the same index [`filecommand_core::update::DropTarget::Tab`]
/// carries (mouse-panel-drag; design D7: "a tab in the strip stands for its
/// directory"). Mirrors `render_tab_strip`'s own geometry exactly — the same
/// `layout()` call, the same left-marker offset, the same per-cell `clip`
/// budget — so a hit rect can never describe a cell that isn't actually
/// where the renderer painted it. A no-op (empty) under the same conditions
/// `render_tab_strip` itself no-ops on.
pub fn hit_test(area: Rect, panel: &PanelState) -> Vec<(Rect, usize)> {
    if area.height == 0 || area.width == 0 || !is_visible(panel) {
        return Vec::new();
    }
    let basenames: Vec<String> = panel.tab_dirs().iter().map(|p| tab_basename(p)).collect();
    let (cells, left_marker, right_marker) = layout(&basenames, panel.active_tab_index, area.width as usize);

    let right_edge = area.x + area.width;
    let mut x = area.x;
    if left_marker {
        x += 1;
    }
    let label_right_edge = if right_marker { right_edge.saturating_sub(1) } else { right_edge };
    let mut hits = Vec::new();
    for cell in &cells {
        if x >= label_right_edge {
            break;
        }
        let budget = (label_right_edge - x) as usize;
        let text = clip(&cell.text, budget);
        let w = display_width(&text) as u16;
        if w > 0 {
            hits.push((Rect { x, y: area.y, width: w, height: 1 }, cell.index));
        }
        x += w;
    }
    hits
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use filecommand_core::panel::PanelState;

    use super::*;

    fn names(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("TAB{i}")).collect()
    }

    // -----------------------------------------------------------------
    // mouse-panel-drag: drag-target tab highlighting (tasks.md 2.2).
    // -----------------------------------------------------------------

    #[test]
    fn render_tab_strip_with_no_target_matches_the_pre_drag_rendering() {
        let mut panel = PanelState::new(PathBuf::from(r"C:\left"));
        panel.open_tab();
        let area = Rect { x: 0, y: 0, width: 40, height: 1 };
        let theme = Theme::classic();

        let mut with_none = Buffer::empty(area);
        render_tab_strip(&mut with_none, area, &panel, &theme, ColorDepth::Ansi16, None);
        let mut baseline = Buffer::empty(area);
        render_tab_strip(&mut baseline, area, &panel, &theme, ColorDepth::Ansi16, Some(99)); // out-of-range target: no cell has index 99
        assert_eq!(with_none, baseline, "an out-of-range/absent target must never change any cell's style");
    }

    #[test]
    fn render_tab_strip_paints_the_target_tab_in_the_button_focused_role() {
        let mut panel = PanelState::new(PathBuf::from(r"C:\left"));
        panel.open_tab();
        let area = Rect { x: 0, y: 0, width: 40, height: 1 };
        let theme = Theme::classic();
        let expected = role_style(&theme, Role::ButtonFocused, ColorDepth::Ansi16);

        let mut buf = Buffer::empty(area);
        render_tab_strip(&mut buf, area, &panel, &theme, ColorDepth::Ansi16, Some(1));
        let hits = hit_test(area, &panel);
        let (rect, _) = hits.iter().find(|(_, i)| *i == 1).expect("tab 1 is drawn at this width");
        assert_eq!(buf[(rect.x, rect.y)].style().fg, expected.fg, "the target tab's own cell must use button.focused");
        assert_eq!(buf[(rect.x, rect.y)].style().bg, expected.bg, "the target tab's own cell must use button.focused");
    }

    // -----------------------------------------------------------------
    // mouse-panel-drag: `hit_test` (tasks.md 2.1).
    // -----------------------------------------------------------------

    #[test]
    fn hit_test_returns_one_rect_per_visible_cell_in_left_to_right_order() {
        let mut panel = PanelState::new(PathBuf::from(r"C:\left"));
        panel.open_tab(); // 2 tabs now, both at the same inherited cwd
        let area = Rect { x: 5, y: 0, width: 40, height: 1 };
        let hits = hit_test(area, &panel);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].1, 0);
        assert_eq!(hits[1].1, 1);
        // Left-to-right, non-overlapping, and nested inside `area`.
        assert!(hits[0].0.x >= area.x);
        assert!(hits[1].0.x >= hits[0].0.x + hits[0].0.width);
        assert!(hits[1].0.x + hits[1].0.width <= area.x + area.width);
    }

    #[test]
    fn hit_test_is_empty_with_a_single_tab() {
        let panel = PanelState::new(PathBuf::from(r"C:\left"));
        let area = Rect { x: 0, y: 0, width: 40, height: 1 };
        assert!(hit_test(area, &panel).is_empty());
    }

    #[test]
    fn hit_test_indices_match_scrolled_layout_when_the_strip_overflows() {
        // 20 tabs, active tab far from the start: the strip scrolls, so the
        // visible cells' indices must still name the true tab positions, not
        // their position within the visible window.
        let mut panel = PanelState::new(PathBuf::from(r"C:\left"));
        for _ in 0..19 {
            panel.open_tab();
        }
        panel.switch_tab(11); // one-based; activates tab index 10
        let area = Rect { x: 0, y: 0, width: 12, height: 1 };
        let hits = hit_test(area, &panel);
        assert!(hits.iter().any(|(_, i)| *i == 10), "the active tab's own cell must be among the hits");
        for w in hits.windows(2) {
            assert!(w[1].1 > w[0].1, "indices must stay in ascending tab order");
        }
    }

    #[test]
    fn full_labels_fit_a_wide_strip() {
        let (cells, l, r) = layout(&["ALPHA".to_string(), "BETA".to_string()], 0, 40);
        assert_eq!(cells.iter().map(|c| c.text.clone()).collect::<Vec<_>>(), vec![" 1:ALPHA ", " 2:BETA "]);
        assert!(!l && !r);
    }

    #[test]
    fn active_tab_is_flagged() {
        let (cells, _, _) = layout(&["A".to_string(), "B".to_string()], 1, 40);
        assert!(!cells[0].active);
        assert!(cells[1].active);
    }

    #[test]
    fn long_names_truncate_before_the_strip_scrolls() {
        let (cells, l, r) = layout(&["FILECOMMAND".to_string(), "B".to_string()], 0, 16);
        assert!(!l && !r, "must fit without scrolling at this width");
        assert!(cells[0].text.contains('\u{2026}'), "{:?}", cells[0].text);
    }

    #[test]
    fn very_tight_width_collapses_to_number_only() {
        let (cells, l, r) = layout(&["FILECOMMAND".to_string(), "BETA".to_string()], 0, 8);
        assert!(!l && !r);
        for c in &cells {
            assert!(!c.text.contains(':'), "number-only form must drop the basename: `{}`", c.text);
        }
    }

    #[test]
    fn overflow_at_minimum_form_scrolls_and_marks_both_ends() {
        let (cells, l, r) = layout(&names(20), 10, 12);
        assert!(l, "tabs are hidden to the left of the window");
        assert!(r, "tabs are hidden to the right of the window");
        assert!(cells.iter().any(|c| c.active), "the active tab must remain visible");
    }

    #[test]
    fn scrolling_near_the_start_only_marks_the_right_end() {
        let (_, l, r) = layout(&names(20), 0, 12);
        assert!(!l, "no tabs hidden to the left when the first tab is active");
        assert!(r);
    }

    #[test]
    fn scrolling_near_the_end_only_marks_the_left_end() {
        let (_, l, r) = layout(&names(20), 19, 12);
        assert!(l);
        assert!(!r, "no tabs hidden to the right when the last tab is active");
    }

    #[test]
    fn empty_or_zero_width_never_panics() {
        assert_eq!(layout(&[], 0, 40), (Vec::new(), false, false));
        assert_eq!(layout(&["A".to_string()], 0, 0), (Vec::new(), false, false));
    }

    // -----------------------------------------------------------------
    // Property tests (task 15.9): the layout must never panic and must
    // always keep the active tab visible and within the given width, for
    // arbitrary tab counts, active indices, and strip widths.
    // -----------------------------------------------------------------
    mod proptests {
        use super::*;
        use proptest::prelude::*;

        fn arb_basenames() -> impl Strategy<Value = Vec<String>> {
            prop::collection::vec("[A-Z0-9]{1,12}", 1..25)
        }

        proptest! {
            #[test]
            fn never_panics_and_active_tab_stays_visible(
                basenames in arb_basenames(),
                active_raw in 0usize..40,
                width in 0usize..120,
            ) {
                let active = active_raw % basenames.len();
                let (cells, left_marker, right_marker) = layout(&basenames, active, width);

                // The active tab alone, in its bare number-only form, is the
                // absolute floor of what any width could possibly show: below
                // that, not even the active tab's own label fits, and no
                // layout can be expected to satisfy the invariants below (the
                // renderer's own `clip()` is the real safety net for that
                // irreducible case). At or above the floor, the active tab
                // must stay visible and the layout must fit `width` exactly.
                let active_floor = display_width(&format!(" {} ", active + 1)) + 2; // +2 worst-case markers
                if width >= active_floor {
                    prop_assert!(cells.iter().any(|c| c.active), "active tab {active} missing at width {width}");

                    let labels_w: usize = cells.iter().map(|c| display_width(&c.text)).sum();
                    let markers_w = usize::from(left_marker) + usize::from(right_marker);
                    prop_assert!(labels_w + markers_w <= width, "layout overflowed: {labels_w} + {markers_w} > {width}");
                }

                // A marker is only drawn when a tab is actually hidden on
                // that side — never spuriously.
                if !left_marker && !right_marker && width >= active_floor {
                    prop_assert!(cells.len() <= basenames.len());
                }
            }

            #[test]
            fn single_tab_index_is_always_in_range_after_modulo(
                basenames in arb_basenames(),
                active_raw in 0usize..40,
            ) {
                let active = active_raw % basenames.len();
                let (cells, _, _) = layout(&basenames, active, 200);
                prop_assert!(cells.iter().filter(|c| c.active).count() <= 1);
            }
        }
    }
}
