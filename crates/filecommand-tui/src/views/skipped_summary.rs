//! End-of-job skipped-files summary: shown only when 1+ items were skipped.

use filecommand_core::dialogs::overlay_rect;
use filecommand_core::fs_ops::SkippedItem;
use filecommand_core::listing::{display_width, pad_to_width, truncate_with_ellipsis};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const MAX_ROWS: usize = 8;
const MIN_INNER_W: usize = 32;
/// Chrome rows other than the (variable) item list: top+bottom frame,
/// footer row.
const CHROME_ROWS: u16 = 3;

pub fn render_skipped_summary(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, skipped: &[SkippedItem]) {
    let body = role_style(theme, Role::DialogPrimary, depth);

    let title = format!(" Skipped {} item(s) ", skipped.len());
    let all_rows: Vec<String> = skipped.iter().take(MAX_ROWS).map(|s| format!("{}: {}", s.path.display(), s.reason)).collect();
    let preferred_inner_w = all_rows.iter().map(|r| display_width(r)).max().unwrap_or(0).max(display_width(&title)).max(30) + 2;
    let preferred_h = CHROME_ROWS + all_rows.len() as u16;
    let r = overlay_rect((preferred_inner_w as u16 + 2, preferred_h), (MIN_INNER_W as u16, 5), (area.width, area.height));
    let box_h = r.height;
    let inner_w = r.width.saturating_sub(2) as usize;
    // The clamped height may show fewer rows than were computed above.
    let visible_rows = box_h.saturating_sub(CHROME_ROWS) as usize;
    let rows = &all_rows[..all_rows.len().min(visible_rows)];
    let x = area.x + r.x;
    let y = area.y + r.y;

    let top = format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w));
    let bottom = format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w));
    buf.set_string(x, y, &top, body);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(&title))) / 2) as u16;
    buf.set_string(title_x, y, truncate_with_ellipsis(&title, inner_w), body);

    for (i, row) in rows.iter().enumerate() {
        let dy = 1 + i as u16;
        buf.set_string(x, y + dy, "\u{2551}", body);
        buf.set_string(x + 1, y + dy, pad_to_width(&truncate_with_ellipsis(row, inner_w), inner_w), body);
        buf.set_string(x + 1 + inner_w as u16, y + dy, "\u{2551}", body);
    }
    let footer_y = y + 1 + rows.len() as u16;
    buf.set_string(x, footer_y, "\u{2551}", body);
    buf.set_string(x + 1, footer_y, pad_to_width(&truncate_with_ellipsis("Press any key to continue", inner_w), inner_w), body);
    buf.set_string(x + 1 + inner_w as u16, footer_y, "\u{2551}", body);
    buf.set_string(x, footer_y + 1, &bottom, body);
}
