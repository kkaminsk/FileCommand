//! F8 delete confirmation dialog: names a single item or gives a count for
//! a multi-selection, warns the deletion is permanent, and asks a second
//! time when a selected directory may be non-empty.

use filecommand_core::fs_ops::dialog::FileOpSetup;
use filecommand_core::listing::display_width;
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

pub fn render_delete_confirm(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, setup: &FileOpSetup) {
    let FileOpSetup::DeleteConfirm { sources, needs_second_confirm, confirmed_once, .. } = setup else { return };
    let body = role_style(theme, Role::DialogPrimary, depth);

    let subject = match sources.as_slice() {
        [one] => format!("{}", one.original_name.to_string_lossy()),
        many => format!("{} files", many.len()),
    };
    let question = if *needs_second_confirm && *confirmed_once {
        format!("{subject} contains a directory — delete its entire contents?")
    } else {
        format!("Delete {subject}? This cannot be undone.")
    };

    let inner_w = display_width(&question).max(20) + 2;
    let box_w = inner_w as u16 + 2;
    let box_h = 4u16;
    if area.width < box_w || area.height < box_h {
        return;
    }
    let x = area.x + (area.width - box_w) / 2;
    let y = area.y + (area.height - box_h) / 2;

    let top = format!("\u{250C}{}\u{2510}", "\u{2500}".repeat(inner_w));
    let bottom = format!("\u{2514}{}\u{2518}", "\u{2500}".repeat(inner_w));
    let mid = format!("\u{2502} {} \u{2502}", pad(&question, inner_w - 2));
    let buttons = format!("\u{2502} {} \u{2502}", pad("(Y)es   (N)o", inner_w - 2));

    buf.set_string(x, y, &top, body);
    buf.set_string(x, y + 1, &mid, body);
    buf.set_string(x, y + 2, &buttons, body);
    buf.set_string(x, y + 3, &bottom, body);
}

fn pad(s: &str, width: usize) -> String {
    filecommand_core::listing::pad_to_width(s, width)
}
