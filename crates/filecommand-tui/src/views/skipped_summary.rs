//! End-of-job skipped-files summary: shown when a job finished with 1+
//! skipped items and/or 1+ metadata-restore warnings (error-diagnostics;
//! operation-dialogs "End-of-job skipped-files summary", "Metadata-restore
//! failures are reported in the end-of-job summary").

use filecommand_core::dialogs::overlay_rect;
use filecommand_core::fs_ops::{ItemKind, SkippedItem};
use filecommand_core::listing::{display_width, pad_to_width, truncate_with_ellipsis};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use filecommand_core::update::ButtonId;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::DialogLayout;
use crate::style::role_style;

const MAX_ROWS: usize = 8;
const MIN_INNER_W: usize = 32;
/// Chrome rows other than the (variable) item list: top+bottom frame,
/// footer row.
const CHROME_ROWS: u16 = 3;
const FOOTER: &str = "Press any key to continue";

/// The dialog's title and body rows (design D5): skipped items first, each
/// `{path}: {reason}`, then a `Warnings` group — a header row followed by
/// the warning rows in the same format. The title carries both counts:
/// `Skipped N item(s)`, `N warning(s)`, or `Skipped N item(s), M
/// warning(s)`.
fn rows_and_title(items: &[SkippedItem]) -> (String, Vec<String>) {
    let skipped_count = items.iter().filter(|i| i.kind == ItemKind::Skipped).count();
    let warning_count = items.len() - skipped_count;
    let title = match (skipped_count, warning_count) {
        (_, 0) => format!(" Skipped {skipped_count} item(s) "),
        (0, _) => format!(" {warning_count} warning(s) "),
        _ => format!(" Skipped {skipped_count} item(s), {warning_count} warning(s) "),
    };
    let mut rows: Vec<String> = Vec::new();
    rows.extend(items.iter().filter(|i| i.kind == ItemKind::Skipped).map(|s| format!("{}: {}", s.path.display(), s.reason)));
    if warning_count > 0 {
        rows.push("Warnings".to_string());
        rows.extend(items.iter().filter(|i| i.kind == ItemKind::Warning).map(|s| format!("{}: {}", s.path.display(), s.reason)));
    }
    (title, rows)
}

/// The dialog's geometry at `area` (reducer-overlay-structure design D8):
/// its rectangle; its text — the title first, then the item rows that fit
/// the clamped height, then the `Press any key to continue` footer, each
/// clamped to the inner width; and the footer row recorded as one clickable
/// span. This dialog has no literal button glyph, but design D5 gates it as
/// buttons-only, so the whole footer stands in for "any key" (mirroring
/// `Command::FileOpConfirm`, which is what any key already does here).
pub fn layout(area: Rect, skipped: &[SkippedItem]) -> DialogLayout {
    let (title, rows) = rows_and_title(skipped);
    let all_rows: Vec<String> = rows.into_iter().take(MAX_ROWS).collect();
    let preferred_inner_w = all_rows.iter().map(|r| display_width(r)).max().unwrap_or(0).max(display_width(&title)).max(30) + 2;
    let preferred_h = CHROME_ROWS + all_rows.len() as u16;
    let r = overlay_rect((preferred_inner_w as u16 + 2, preferred_h), (MIN_INNER_W as u16, 5), (area.width, area.height));
    let rect = Rect { x: area.x + r.x, y: area.y + r.y, width: r.width, height: r.height };
    let inner_w = rect.width.saturating_sub(2) as usize;
    // The clamped height may show fewer rows than were computed above.
    let visible_rows = rect.height.saturating_sub(CHROME_ROWS) as usize;
    let shown = all_rows.len().min(visible_rows);

    let mut lines = vec![truncate_with_ellipsis(&title, inner_w)];
    lines.extend(all_rows[..shown].iter().map(|row| pad_to_width(&truncate_with_ellipsis(row, inner_w), inner_w)));
    lines.push(pad_to_width(&truncate_with_ellipsis(FOOTER, inner_w), inner_w));
    let footer_y = rect.y + 1 + shown as u16;
    let buttons = vec![(Rect { x: rect.x + 1, y: footer_y, width: inner_w as u16, height: 1 }, ButtonId::SummaryContinue)];
    DialogLayout { rect, buttons, rows: Vec::new(), lines }
}

pub fn render_skipped_summary(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, skipped: &[SkippedItem]) {
    draw_skipped_summary(buf, theme, depth, &layout(area, skipped));
}

/// Paint the dialog from an already computed [`layout`].
pub fn draw_skipped_summary(buf: &mut Buffer, theme: &Theme, depth: ColorDepth, l: &DialogLayout) {
    let body = role_style(theme, Role::DialogPrimary, depth);
    let inner_w = l.rect.width.saturating_sub(2) as usize;
    let (x, y) = (l.rect.x, l.rect.y);
    let Some((title, rows)) = l.lines.split_first() else { return };

    let top = format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w));
    let bottom = format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w));
    buf.set_string(x, y, &top, body);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(title))) / 2) as u16;
    buf.set_string(title_x, y, title, body);

    // The item rows, then the footer, each on its own framed row.
    for (i, row) in rows.iter().enumerate() {
        let dy = 1 + i as u16;
        buf.set_string(x, y + dy, "\u{2551}", body);
        buf.set_string(x + 1, y + dy, row, body);
        buf.set_string(x + 1 + inner_w as u16, y + dy, "\u{2551}", body);
    }
    buf.set_string(x, y + 1 + rows.len() as u16, &bottom, body);
}
