//! "Terminal too small" placeholder, shown below the 60x16 hard floor.

use filecommand_core::listing::display_width;
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const MESSAGE: &str = "Terminal too small \u{2014} resize to at least 60x16";

pub fn render_placeholder(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth) {
    let style = role_style(theme, Role::ScreenPlaceholder, depth);
    for y in area.y..area.y + area.height {
        buf.set_string(area.x, y, " ".repeat(area.width as usize), style);
    }
    if area.height == 0 {
        return;
    }
    let msg_w = display_width(MESSAGE).min(area.width as usize);
    let x = area.x + (area.width as usize - msg_w) as u16 / 2;
    let y = area.y + area.height / 2;
    let clipped: String = MESSAGE.chars().take(msg_w).collect();
    buf.set_string(x, y, &clipped, style);
}
