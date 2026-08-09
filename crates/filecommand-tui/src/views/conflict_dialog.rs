//! Overwrite-conflict dialog: source vs. target size/date, and the
//! Overwrite/Skip/Rename/Overwrite All/Skip All choices.

use filecommand_core::dialogs::overlay_rect;
use filecommand_core::fs_ops::ConflictInfo;
use filecommand_core::listing::{display_width, format_date, format_size, format_time, pad_to_width, truncate_with_ellipsis};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const BOX_INNER_W: usize = 46;
const MIN_INNER_W: usize = 30;

pub fn render_conflict(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, info: &ConflictInfo, rename_input: &Option<String>) {
    let body = role_style(theme, Role::DialogPrimary, depth);
    let field_style = role_style(theme, Role::DialogInput, depth);

    let r = overlay_rect((BOX_INNER_W as u16 + 2, 8), (MIN_INNER_W as u16 + 2, 8), (area.width, area.height));
    let box_h = r.height;
    let inner_w = r.width.saturating_sub(2) as usize;
    let x = area.x + r.x;
    let y = area.y + r.y;

    let title = " File Exists ";
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

    row(buf, 1, &format!("{} already exists", info.source_name.to_string_lossy()));
    let src_dt = info.source_modified.map(|d| format!("{} {}", format_date(d), format_time(d))).unwrap_or_default();
    row(buf, 2, &format!("Source:  {}  {}", format_size(info.source_size), src_dt));
    let tgt_dt = info.target_modified.map(|d| format!("{} {}", format_date(d), format_time(d))).unwrap_or_default();
    row(buf, 3, &format!("Target:  {}  {}", format_size(info.target_size), tgt_dt));
    row(buf, 4, "");

    match rename_input {
        Some(name) => {
            let field = format!("[{name}_]");
            buf.set_string(x, y + 5, "\u{2551}", body);
            buf.set_string(x + 1, y + 5, "New name:", body);
            buf.set_string(x + 11, y + 5, pad_to_width(&field, inner_w.saturating_sub(10)), field_style);
            buf.set_string(x + 1 + inner_w as u16, y + 5, "\u{2551}", body);
        }
        None => row(buf, 5, "(O)verwrite  (S)kip  (R)ename  Over(w)rite All  Skip (A)ll"),
    }

    buf.set_string(x, y + 6, "\u{2551}", body);
    buf.set_string(x + 1, y + 6, " ".repeat(inner_w), body);
    buf.set_string(x + 1 + inner_w as u16, y + 6, "\u{2551}", body);
    buf.set_string(x, y + box_h - 1, &bottom, body);
}
