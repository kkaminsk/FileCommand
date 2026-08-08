//! End-of-job skipped-files summary: shown only when 1+ items were skipped.

use filecommand_core::fs_ops::SkippedItem;
use filecommand_core::listing::{display_width, pad_to_width};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const MAX_ROWS: usize = 8;

pub fn render_skipped_summary(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, skipped: &[SkippedItem]) {
    let body = role_style(theme, Role::DialogPrimary, depth);

    let title = format!(" Skipped {} item(s) ", skipped.len());
    let rows: Vec<String> = skipped.iter().take(MAX_ROWS).map(|s| format!("{}: {}", s.path.display(), s.reason)).collect();
    let inner_w = rows.iter().map(|r| display_width(r)).max().unwrap_or(0).max(display_width(&title)).max(30) + 2;
    let box_w = inner_w as u16 + 2;
    let box_h = rows.len() as u16 + 4;
    if area.width < box_w || area.height < box_h {
        return;
    }
    let x = area.x + (area.width - box_w) / 2;
    let y = area.y + (area.height - box_h) / 2;

    let top = format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w));
    let bottom = format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w));
    buf.set_string(x, y, &top, body);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(&title))) / 2) as u16;
    buf.set_string(title_x, y, &title, body);

    for (i, row) in rows.iter().enumerate() {
        let dy = 1 + i as u16;
        buf.set_string(x, y + dy, "\u{2551}", body);
        buf.set_string(x + 1, y + dy, pad_to_width(row, inner_w), body);
        buf.set_string(x + 1 + inner_w as u16, y + dy, "\u{2551}", body);
    }
    let footer_y = y + 1 + rows.len() as u16;
    buf.set_string(x, footer_y, "\u{2551}", body);
    buf.set_string(x + 1, footer_y, pad_to_width("Press any key to continue", inner_w), body);
    buf.set_string(x + 1 + inner_w as u16, footer_y, "\u{2551}", body);
    buf.set_string(x, footer_y + 1, &bottom, body);
}
