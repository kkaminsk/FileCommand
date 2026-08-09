//! F5/F6/F7 destination/name input dialog: double-line frame, black-on-cyan
//! body, bracket-and-dots input field.

use filecommand_core::dialogs::overlay_rect;
use filecommand_core::fs_ops::dialog::FileOpSetup;
use filecommand_core::fs_ops::JobKind;
use filecommand_core::listing::{display_width, truncate_with_ellipsis};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const FIELD_WIDTH: usize = 40;
const BOX_INNER_W: usize = FIELD_WIDTH + 4;

fn title_for(kind: JobKind) -> &'static str {
    match kind {
        JobKind::Copy => "Copy",
        JobKind::Move => "Rename/Move",
        JobKind::Mkdir => "Make Directory",
        JobKind::Delete => "Delete",
        // `FileOpSetup::RenameInput` never carries a `JobKind` (see
        // `render_destination_input` below), so this arm is unreachable in
        // practice; kept so `JobKind` stays exhaustively matched here.
        JobKind::Rename => "Rename",
    }
}

fn prompt_for(kind: JobKind) -> &'static str {
    match kind {
        JobKind::Copy | JobKind::Move => "Copy/move to:",
        JobKind::Mkdir => "Name of new directory:",
        JobKind::Delete | JobKind::Rename => "",
    }
}

pub fn render_destination_input(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, setup: &FileOpSetup) {
    match setup {
        FileOpSetup::DestinationInput { kind, input, .. } => render_input_box(buf, area, theme, depth, title_for(*kind), prompt_for(*kind), input),
        // The file-action menu's in-place Rename: same bracket-and-dots
        // input box, pre-filled with the target's current name (design D2;
        // file-action-menu "In-place Rename": "an input dialog pre-filled
        // with the target entry's current name").
        FileOpSetup::RenameInput { input, .. } => render_input_box(buf, area, theme, depth, "Rename", "New name:", input),
        FileOpSetup::DeleteConfirm { .. } => {}
    }
}

fn render_input_box(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, title: &str, prompt: &str, input: &str) {
    let body = role_style(theme, Role::DialogPrimary, depth);
    let field_style = role_style(theme, Role::DialogInput, depth);

    let preferred = (BOX_INNER_W as u16 + 2, 6);
    let r = overlay_rect(preferred, preferred, (area.width, area.height));
    let box_h = r.height;
    let inner_w = r.width.saturating_sub(2) as usize;
    let field_w = FIELD_WIDTH.min(inner_w.saturating_sub(2));
    let x = area.x + r.x;
    let y = area.y + r.y;

    let title = format!(" {title} ");
    let top = format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w));
    let bottom = format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w));
    let blank = format!("\u{2551}{}\u{2551}", " ".repeat(inner_w));

    buf.set_string(x, y, &top, body);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(&title))) / 2) as u16;
    buf.set_string(title_x, y, &title, body);
    buf.set_string(x, y + 1, &blank, body);

    buf.set_string(x, y + 2, "\u{2551}", body);
    buf.set_string(x + 2, y + 2, truncate_with_ellipsis(prompt, inner_w.saturating_sub(2)), body);
    buf.set_string(x + 1 + inner_w as u16, y + 2, "\u{2551}", body);

    // Bracket-and-dots input field: `[text...........]`.
    let mut field_text = input.to_string();
    if display_width(&field_text) > field_w {
        let excess = display_width(&field_text) - field_w;
        field_text = field_text.chars().skip(excess).collect();
    }
    let pad = field_w - display_width(&field_text);
    let field = format!("[{field_text}{}]", ".".repeat(pad));
    buf.set_string(x, y + 3, "\u{2551}", body);
    buf.set_string(x + 2, y + 3, &field, field_style);
    buf.set_string(x + 1 + inner_w as u16, y + 3, "\u{2551}", body);

    buf.set_string(x, y + 4, &blank, body);
    buf.set_string(x, y + box_h - 1, &bottom, body);
}
