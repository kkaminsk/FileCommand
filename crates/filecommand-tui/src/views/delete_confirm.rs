//! F8 delete confirmation dialog: names a single item or gives a count for
//! a multi-selection, warns the deletion is permanent, and asks a second
//! time when a selected directory may be non-empty.

use filecommand_core::dialogs::overlay_rect;
use filecommand_core::fs_ops::dialog::FileOpSetup;
use filecommand_core::listing::{display_width, truncate_with_ellipsis};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const MIN_INNER_W: usize = 24;

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

    let preferred_inner_w = display_width(&question).max(20) + 2;
    let r = overlay_rect((preferred_inner_w as u16 + 2, 4), (MIN_INNER_W as u16 + 2, 4), (area.width, area.height));
    let box_h = r.height;
    let inner_w = r.width.saturating_sub(2) as usize;
    let x = area.x + r.x;
    let y = area.y + r.y;
    let content_w = inner_w.saturating_sub(2);

    let top = format!("\u{250C}{}\u{2510}", "\u{2500}".repeat(inner_w));
    let bottom = format!("\u{2514}{}\u{2518}", "\u{2500}".repeat(inner_w));
    let mid = format!("\u{2502} {} \u{2502}", pad(&truncate_with_ellipsis(&question, content_w), content_w));
    let buttons = format!("\u{2502} {} \u{2502}", pad(&truncate_with_ellipsis("(Y)es   (N)o", content_w), content_w));

    buf.set_string(x, y, &top, body);
    buf.set_string(x, y + 1, &mid, body);
    buf.set_string(x, y + 2, &buttons, body);
    buf.set_string(x, y + box_h - 1, &bottom, body);
}

fn pad(s: &str, width: usize) -> String {
    filecommand_core::listing::pad_to_width(s, width)
}
