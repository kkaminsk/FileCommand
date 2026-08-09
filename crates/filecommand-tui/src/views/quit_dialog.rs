//! F10 quit confirmation dialog, rendered from `dialog.*` roles.

use filecommand_core::dialogs::overlay_rect;
use filecommand_core::listing::{display_width, pad_to_width, truncate_with_ellipsis};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const MESSAGE: &str = " Quit FileCommand? (Y/N) ";

pub fn render_quit_dialog(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth) {
    let style = role_style(theme, Role::DialogPrimary, depth);
    // Fixed content, so preferred and minimum coincide — this dialog is
    // never wider than it needs to be, only ever clamped by a terminal
    // smaller than its own message (responsive-layout "Unified overlay
    // geometry").
    let preferred_w = display_width(MESSAGE) as u16 + 2;
    let r = overlay_rect((preferred_w, 3), (preferred_w, 3), (area.width, area.height));
    let box_w = r.width;
    let box_h = r.height;
    let inner_w = box_w.saturating_sub(2) as usize;
    let x = area.x + r.x;
    let y = area.y + r.y;

    let top = format!("\u{250C}{}\u{2510}", "\u{2500}".repeat(inner_w));
    let bottom = format!("\u{2514}{}\u{2518}", "\u{2500}".repeat(inner_w));
    let message = pad_to_width(&truncate_with_ellipsis(MESSAGE, inner_w), inner_w);
    let mid = format!("\u{2502}{message}\u{2502}");

    buf.set_string(x, y, &top, style);
    buf.set_string(x, y + 1, &mid, style);
    buf.set_string(x, y + box_h - 1, &bottom, style);
}
