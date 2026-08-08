//! Per-file error-recovery dialog: bright-white-on-red, Retry/Skip/Skip
//! All/Abort.

use filecommand_core::fs_ops::ErrorInfo;
use filecommand_core::listing::{display_width, pad_to_width};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

pub fn render_error(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, info: &ErrorInfo) {
    let body = role_style(theme, Role::DialogError, depth);

    let path_line = info.path.display().to_string();
    let inner_w = display_width(&info.message).max(display_width(&path_line)).max(30) + 2;
    let box_w = inner_w as u16 + 2;
    let box_h = 6u16;
    if area.width < box_w || area.height < box_h {
        return;
    }
    let x = area.x + (area.width - box_w) / 2;
    let y = area.y + (area.height - box_h) / 2;

    let title = " Error ";
    let top = format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w));
    let bottom = format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w));
    buf.set_string(x, y, &top, body);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(title))) / 2) as u16;
    buf.set_string(title_x, y, title, body);

    let row = |buf: &mut Buffer, dy: u16, text: &str| {
        buf.set_string(x, y + dy, "\u{2551}", body);
        buf.set_string(x + 1, y + dy, pad_to_width(text, inner_w), body);
        buf.set_string(x + 1 + inner_w as u16, y + dy, "\u{2551}", body);
    };
    row(buf, 1, &path_line);
    row(buf, 2, &info.message);
    row(buf, 3, "");
    row(buf, 4, "(R)etry  (S)kip  Skip (A)ll  A(b)ort");
    buf.set_string(x, y + 5, &bottom, body);
}
