//! In-flight job progress dialog: file counts, current file, a block byte
//! gauge (`\u{2588}` filled / `\u{2591}` empty), and a Cancel control.

use filecommand_core::dialogs::overlay_rect;
use filecommand_core::fs_ops::{JobKind, ProgressInfo};
use filecommand_core::listing::{display_width, format_count, pad_to_width, truncate_with_ellipsis};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use filecommand_core::update::ButtonId;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::DialogLayout;
use crate::style::role_style;

const GAUGE_WIDTH: usize = 40;
const BOX_INNER_W: usize = GAUGE_WIDTH + 2;
const CANCEL: &str = "[ Cancel ]";

fn title_for(kind: JobKind) -> &'static str {
    match kind {
        JobKind::Copy => "Copying",
        JobKind::Move => "Moving",
        JobKind::Mkdir => "Creating Directory",
        JobKind::Delete => "Deleting",
        JobKind::Rename => "Renaming",
    }
}

/// The dialog's geometry at `area` (reducer-overlay-structure design D8):
/// its rectangle; its text rows — the title, the file counts, the current
/// file, the byte line, and the `[ Cancel ]` row — each already clamped to
/// the inner width; and the `[ Cancel ]` button span, the only mouse target
/// a running job's progress dialog offers (mouse-input "Running job accepts
/// Cancel only").
pub fn layout(area: Rect, kind: JobKind, progress: &ProgressInfo) -> DialogLayout {
    let preferred = (BOX_INNER_W as u16 + 2, 7);
    let r = overlay_rect(preferred, preferred, (area.width, area.height));
    let rect = Rect { x: area.x + r.x, y: area.y + r.y, width: r.width, height: r.height };
    let inner_w = rect.width.saturating_sub(2) as usize;

    let title = format!(" {} ", title_for(kind));
    let counts = format!("Files: {} of {}", format_count(progress.files_done), format_count(progress.files_total));
    let current = progress.current_file.to_string_lossy();
    let bytes_line = format!("{} / {} bytes", format_count(progress.bytes_done as usize), format_count(progress.bytes_total as usize));
    let lines = vec![
        title,
        pad_to_width(&truncate_with_ellipsis(&counts, inner_w), inner_w),
        pad_to_width(&truncate_with_ellipsis(&current, inner_w), inner_w),
        pad_to_width(&truncate_with_ellipsis(&bytes_line, inner_w), inner_w),
        pad_to_width(CANCEL, inner_w),
    ];
    let buttons = vec![(Rect { x: rect.x + 1, y: rect.y + 5, width: display_width(CANCEL) as u16, height: 1 }, ButtonId::ProgressCancel)];
    DialogLayout { rect, buttons, rows: Vec::new(), lines }
}

pub fn render_progress(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, kind: JobKind, progress: &ProgressInfo) {
    draw_progress(buf, theme, depth, progress, &layout(area, kind, progress));
}

/// Paint the dialog from an already computed [`layout`]; `progress` supplies
/// the byte ratio the gauge is drawn from.
pub fn draw_progress(buf: &mut Buffer, theme: &Theme, depth: ColorDepth, progress: &ProgressInfo, l: &DialogLayout) {
    let body = role_style(theme, Role::DialogPrimary, depth);
    let filled_style = role_style(theme, Role::DialogGaugeFilled, depth);
    let empty_style = role_style(theme, Role::DialogGaugeEmpty, depth);

    let inner_w = l.rect.width.saturating_sub(2) as usize;
    let gauge_width = inner_w.saturating_sub(2).min(GAUGE_WIDTH);
    let (x, y) = (l.rect.x, l.rect.y);
    let [title, counts, current, bytes_line, cancel] = &l.lines[..] else { return };

    let top = format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w));
    let bottom = format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w));
    buf.set_string(x, y, &top, body);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(title))) / 2) as u16;
    buf.set_string(title_x, y, title, body);

    let row = |buf: &mut Buffer, dy: u16, text: &str| {
        buf.set_string(x, y + dy, "\u{2551}", body);
        buf.set_string(x + 1, y + dy, text, body);
        buf.set_string(x + 1 + inner_w as u16, y + dy, "\u{2551}", body);
    };
    row(buf, 1, counts);
    row(buf, 2, current);

    // Byte gauge.
    let ratio = if progress.bytes_total == 0 { 0.0 } else { progress.bytes_done as f64 / progress.bytes_total as f64 };
    let filled = ((ratio.clamp(0.0, 1.0)) * gauge_width as f64).round() as usize;
    buf.set_string(x, y + 3, "\u{2551}", body);
    buf.set_string(x + 1, y + 3, " ", body);
    buf.set_string(x + 2, y + 3, "\u{2588}".repeat(filled), filled_style);
    buf.set_string(x + 2 + filled as u16, y + 3, "\u{2591}".repeat(gauge_width - filled), empty_style);
    buf.set_string(x + 1 + inner_w as u16, y + 3, "\u{2551}", body);

    row(buf, 4, bytes_line);
    row(buf, 5, cancel);

    buf.set_string(x, y + l.rect.height - 1, &bottom, body);
}
