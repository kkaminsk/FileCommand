//! Per-file error-recovery dialog: bright-white-on-red, Retry/Skip/Skip
//! All/Abort.

use filecommand_core::dialogs::overlay_rect;
use filecommand_core::fs_ops::ErrorInfo;
use filecommand_core::listing::{display_width, pad_to_width, truncate_with_ellipsis};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const MIN_INNER_W: usize = 34;

pub fn render_error(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, info: &ErrorInfo) {
    let body = role_style(theme, Role::DialogError, depth);

    let path_line = info.path.display().to_string();
    let preferred_inner_w = display_width(&info.message).max(display_width(&path_line)).max(30) + 2;
    let r = overlay_rect((preferred_inner_w as u16 + 2, 6), (MIN_INNER_W as u16, 6), (area.width, area.height));
    let box_h = r.height;
    let inner_w = r.width.saturating_sub(2) as usize;
    let x = area.x + r.x;
    let y = area.y + r.y;

    let title = " Error ";
    let top = format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w));
    let bottom = format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w));
    buf.set_string(x, y, &top, body);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(title))) / 2) as u16;
    buf.set_string(title_x, y, title, body);

    let row = |buf: &mut Buffer, dy: u16, text: &str| {
        buf.set_string(x, y + dy, "\u{2551}", body);
        buf.set_string(x + 1, y + dy, pad_to_width(&truncate_with_ellipsis(text, inner_w), inner_w), body);
        buf.set_string(x + 1 + inner_w as u16, y + dy, "\u{2551}", body);
    };
    row(buf, 1, &path_line);
    row(buf, 2, &info.message);
    row(buf, 3, "");
    row(buf, 4, "(R)etry  (S)kip  Skip (A)ll  A(b)ort");
    buf.set_string(x, y + box_h - 1, &bottom, body);
}
