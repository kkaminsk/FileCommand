//! Screen layout: splits the terminal into left panel / right panel /
//! command line / F-key bar rects, and derives the page size used for
//! PgUp/PgDn cursor moves.

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

pub fn compute(term_size: (u16, u16)) -> Layout {
    let (w, h) = term_size;
    let cmdline_h: u16 = 1;
    let keybar_h: u16 = 1;
    let panels_h = h.saturating_sub(cmdline_h + keybar_h);
    let left_w = w / 2;
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
        let l = compute((80, 24));
        assert_eq!(l.left.width + l.right.width, 80);
        assert_eq!(l.left.x, 0);
        assert_eq!(l.right.x, l.left.width);
        assert_eq!(l.cmdline.height, 1);
        assert_eq!(l.keybar.height, 1);
        assert_eq!(l.keybar.y, l.cmdline.y + 1);
        assert_eq!(l.left.height, 22);
    }

    #[test]
    fn entries_visible_is_never_zero() {
        let l = compute((80, 3));
        assert!(l.entries_visible >= 1);
    }
}
