//! Screen layout: splits the terminal into left panel / right panel /
//! command line / F-key bar rects, and derives the page size used for
//! PgUp/PgDn cursor moves.

use filecommand_core::panel_split;
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy)]
pub struct Layout {
    pub left: Rect,
    pub right: Rect,
    pub cmdline: Rect,
    pub keybar: Rect,
    /// Number of entry rows visible inside a panel — used both for
    /// rendering and for PgUp/PgDn step size.
    pub entries_visible: usize,
}

/// Rows consumed by a panel's own chrome: top border+title, header, bottom
/// border (which doubles as the mini-status line).
const PANEL_CHROME_ROWS: u16 = 3;

/// `split_percent` is the stored left-panel percentage (panel-split "Split
/// ratio semantics and panel minimum"); the effective column split is
/// derived via `panel_split::effective_left_width`, shared with
/// `core::update`'s reducer so the two never disagree about where the
/// divider sits. At the default 50%, this reproduces the old fixed 50/50
/// split exactly.
pub fn compute(term_size: (u16, u16), split_percent: u16) -> Layout {
    let (w, h) = term_size;
    let cmdline_h: u16 = 1;
    let keybar_h: u16 = 1;
    let panels_h = h.saturating_sub(cmdline_h + keybar_h);
    let left_w = panel_split::effective_left_width(split_percent, w);
    let right_w = w - left_w;

    let left = Rect { x: 0, y: 0, width: left_w, height: panels_h };
    let right = Rect { x: left_w, y: 0, width: right_w, height: panels_h };
    let cmdline = Rect { x: 0, y: panels_h, width: w, height: cmdline_h };
    let keybar = Rect { x: 0, y: panels_h + cmdline_h, width: w, height: keybar_h };

    let entries_visible = panels_h.saturating_sub(PANEL_CHROME_ROWS).max(1) as usize;

    Layout { left, right, cmdline, keybar, entries_visible }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panels_split_width_evenly_and_stack_cmdline_keybar() {
        // At the default 50% split this reproduces the old fixed 50/50
        // split exactly (panel-split regression: "50/50 at default percent
        // reproduces the old even split").
        let l = compute((80, 24), panel_split::DEFAULT_SPLIT_PERCENT);
        assert_eq!(l.left.width + l.right.width, 80);
        assert_eq!(l.left.x, 0);
        assert_eq!(l.right.x, l.left.width);
        assert_eq!(l.left.width, 40);
        assert_eq!(l.cmdline.height, 1);
        assert_eq!(l.keybar.height, 1);
        assert_eq!(l.keybar.y, l.cmdline.y + 1);
        assert_eq!(l.left.height, 22);
    }

    #[test]
    fn entries_visible_is_never_zero() {
        let l = compute((80, 3), panel_split::DEFAULT_SPLIT_PERCENT);
        assert!(l.entries_visible >= 1);
    }

    #[test]
    fn a_non_default_split_percent_shifts_the_divider() {
        let l = compute((100, 24), 60);
        assert_eq!(l.left.width, 60);
        assert_eq!(l.right.width, 40);
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// Walking widths 60..=200 at several fixed split percentages,
            /// `compute(...).left.width` is monotone non-decreasing in
            /// width with no jitter — this mostly validates that
            /// `layout::compute` and `panel_split::effective_left_width`
            /// stay in sync, since the underlying math is exercised
            /// directly by `panel_split`'s own property tests (design D7's
            /// "1-column divider jitter" risk).
            #[test]
            fn left_width_is_monotonic_in_terminal_width(percent in 1u16..100, width in 60u16..200) {
                let a = compute((width, 24), percent).left.width;
                let b = compute((width + 1, 24), percent).left.width;
                prop_assert!(b >= a, "percent={percent} width={width}: {a} -> {b}");
            }
        }
    }
}
