//! The `h:mm a` clock widget drawn over the right end of the right panel's
//! top border (design doc §4.1). It is drawn unconditionally by
//! [`crate::views::render`] right after both panels — the F9 menu bar,
//! rendered afterward, overwrites the whole top row (including this widget)
//! while it is open, and stops doing so the moment it closes. That draw
//! order is the entire "hide while F9 is open, restore on close" mechanism;
//! there is no separate visibility flag to keep in sync.

use filecommand_core::listing::display_width;
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

/// Paint `text` right-aligned into `right_panel_area`'s top row, reaching
/// the top-right corner of the screen. A no-op if `text` doesn't fit.
pub fn render_clock(buf: &mut Buffer, right_panel_area: Rect, theme: &Theme, depth: ColorDepth, text: &str) {
    if right_panel_area.width == 0 || right_panel_area.height == 0 {
        return;
    }
    let w = display_width(text);
    if w == 0 || w > right_panel_area.width as usize {
        return;
    }
    let style = role_style(theme, Role::Clock, depth);
    let x = right_panel_area.x + right_panel_area.width - w as u16;
    buf.set_string(x, right_panel_area.y, text, style);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(area: Rect, text: &str) -> Buffer {
        let mut buf = Buffer::empty(Rect { x: 0, y: 0, width: area.x + area.width, height: area.y + area.height + 1 });
        render_clock(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, text);
        buf
    }

    #[test]
    fn clock_hugs_the_right_edge_of_its_area() {
        let area = Rect { x: 40, y: 0, width: 40, height: 22 };
        let buf = render(area, "3:04 PM");
        let row: String = (0..buf.area.width).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(row.ends_with("3:04 PM"), "`{row}`");
    }

    #[test]
    fn clock_uses_the_clock_role_black_on_cyan() {
        let area = Rect { x: 40, y: 0, width: 40, height: 22 };
        let buf = render(area, "3:04 PM");
        let expected = role_style(&Theme::classic(), Role::Clock, ColorDepth::Ansi16);
        let cell = &buf[(area.x + area.width - 1, 0)];
        assert_eq!(cell.fg, expected.fg.unwrap());
        assert_eq!(cell.bg, expected.bg.unwrap());
    }

    #[test]
    fn too_narrow_an_area_draws_nothing_rather_than_panicking() {
        let area = Rect { x: 0, y: 0, width: 3, height: 22 };
        let buf = render(area, "12:00 AM");
        let row: String = (0..buf.area.width).map(|x| buf[(x, 0)].symbol()).collect();
        assert_eq!(row.trim(), "");
    }
}
