//! In-flight job progress dialog: file counts, current file, a block byte
//! gauge (`\u{2588}` filled / `\u{2591}` empty), and a Cancel control.

use filecommand_core::fs_ops::{JobKind, ProgressInfo};
use filecommand_core::listing::{display_width, format_count};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const GAUGE_WIDTH: usize = 40;
const BOX_INNER_W: usize = GAUGE_WIDTH + 2;

fn title_for(kind: JobKind) -> &'static str {
    match kind {
        JobKind::Copy => "Copying",
        JobKind::Move => "Moving",
        JobKind::Mkdir => "Creating Directory",
        JobKind::Delete => "Deleting",
    }
}

pub fn render_progress(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, kind: JobKind, progress: &ProgressInfo) {
    let body = role_style(theme, Role::DialogPrimary, depth);
    let filled_style = role_style(theme, Role::DialogGaugeFilled, depth);
    let empty_style = role_style(theme, Role::DialogGaugeEmpty, depth);

    let box_w = (BOX_INNER_W + 2) as u16;
    let box_h = 7u16;
    if area.width < box_w || area.height < box_h {
        return;
    }
    let x = area.x + (area.width - box_w) / 2;
    let y = area.y + (area.height - box_h) / 2;

    let title = format!(" {} ", title_for(kind));
    let top = format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(BOX_INNER_W));
    let bottom = format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(BOX_INNER_W));
    buf.set_string(x, y, &top, body);
    let title_x = x + 1 + ((BOX_INNER_W.saturating_sub(display_width(&title))) / 2) as u16;
    buf.set_string(title_x, y, &title, body);

    let counts = format!("Files: {} of {}", format_count(progress.files_done), format_count(progress.files_total));
    buf.set_string(x, y + 1, "\u{2551}", body);
    buf.set_string(x + 1, y + 1, filecommand_core::listing::pad_to_width(&counts, BOX_INNER_W), body);
    buf.set_string(x + 1 + BOX_INNER_W as u16, y + 1, "\u{2551}", body);

    let current = progress.current_file.to_string_lossy();
    buf.set_string(x, y + 2, "\u{2551}", body);
    buf.set_string(x + 1, y + 2, filecommand_core::listing::pad_to_width(&current, BOX_INNER_W), body);
    buf.set_string(x + 1 + BOX_INNER_W as u16, y + 2, "\u{2551}", body);

    // Byte gauge.
    let ratio = if progress.bytes_total == 0 { 0.0 } else { progress.bytes_done as f64 / progress.bytes_total as f64 };
    let filled = ((ratio.clamp(0.0, 1.0)) * GAUGE_WIDTH as f64).round() as usize;
    buf.set_string(x, y + 3, "\u{2551}", body);
    buf.set_string(x + 1, y + 3, " ", body);
    buf.set_string(x + 2, y + 3, "\u{2588}".repeat(filled), filled_style);
    buf.set_string(x + 2 + filled as u16, y + 3, "\u{2591}".repeat(GAUGE_WIDTH - filled), empty_style);
    buf.set_string(x + 1 + BOX_INNER_W as u16, y + 3, "\u{2551}", body);

    let bytes_line = format!("{} / {} bytes", format_count(progress.bytes_done as usize), format_count(progress.bytes_total as usize));
    buf.set_string(x, y + 4, "\u{2551}", body);
    buf.set_string(x + 1, y + 4, filecommand_core::listing::pad_to_width(&bytes_line, BOX_INNER_W), body);
    buf.set_string(x + 1 + BOX_INNER_W as u16, y + 4, "\u{2551}", body);

    buf.set_string(x, y + 5, "\u{2551}", body);
    buf.set_string(x + 1, y + 5, filecommand_core::listing::pad_to_width("[ Cancel ]", BOX_INNER_W), body);
    buf.set_string(x + 1 + BOX_INNER_W as u16, y + 5, "\u{2551}", body);

    buf.set_string(x, y + 6, &bottom, body);
}
