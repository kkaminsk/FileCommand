//! F10 quit confirmation dialog, rendered from `dialog.*` roles.

use filecommand_core::listing::display_width;
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const MESSAGE: &str = " Quit FileCommand? (Y/N) ";

pub fn render_quit_dialog(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth) {
    let style = role_style(theme, Role::DialogPrimary, depth);
    let inner_w = display_width(MESSAGE);
    let box_w = inner_w as u16 + 2;
    let box_h = 3u16;
    if area.width < box_w || area.height < box_h {
        return;
    }
    let x = area.x + (area.width - box_w) / 2;
    let y = area.y + (area.height - box_h) / 2;

    let top = format!("\u{250C}{}\u{2510}", "\u{2500}".repeat(inner_w));
    let bottom = format!("\u{2514}{}\u{2518}", "\u{2500}".repeat(inner_w));
    let mid = format!("\u{2502}{MESSAGE}\u{2502}");

    buf.set_string(x, y, &top, style);
    buf.set_string(x, y + 1, &mid, style);
    buf.set_string(x, y + 2, &bottom, style);
}
