//! F8 delete confirmation dialog: names a single item or gives a count for
//! a multi-selection, warns the deletion is permanent, and asks a second
//! time when a selected directory may be non-empty.

use filecommand_core::dialogs::overlay_rect;
use filecommand_core::fs_ops::dialog::FileOpSetup;
use filecommand_core::listing::{display_width, truncate_with_ellipsis};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use filecommand_core::update::ButtonId;
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

/// The `(Y)es`/`(N)o` button spans `render_delete_confirm` currently draws
/// at `area` (mouse-input "Dialog button"). Mirrors the renderer's own
/// `question`/`overlay_rect` geometry so the rects can never drift from
/// what's actually on screen.
pub fn hit_buttons(area: Rect, setup: &FileOpSetup) -> Vec<(Rect, ButtonId)> {
    let FileOpSetup::DeleteConfirm { sources, needs_second_confirm, confirmed_once, .. } = setup else { return vec![] };
    let subject = match sources.as_slice() {
        [one] => one.original_name.to_string_lossy().into_owned(),
        many => format!("{} files", many.len()),
    };
    let question = if *needs_second_confirm && *confirmed_once {
        format!("{subject} contains a directory — delete its entire contents?")
    } else {
        format!("Delete {subject}? This cannot be undone.")
    };
    let preferred_inner_w = display_width(&question).max(20) + 2;
    let r = overlay_rect((preferred_inner_w as u16 + 2, 4), (MIN_INNER_W as u16 + 2, 4), (area.width, area.height));
    let inner_w = r.width.saturating_sub(2) as usize;
    let x = area.x + r.x;
    let y = area.y + r.y;
    let content_w = inner_w.saturating_sub(2);
    let buttons_text = truncate_with_ellipsis("(Y)es   (N)o", content_w);
    let base_x = x + 2;
    let row_y = y + 2;

    let mut out = Vec::new();
    for (label, id) in [("(Y)es", ButtonId::DeleteConfirmYes), ("(N)o", ButtonId::DeleteConfirmNo)] {
        if let Some(byte_idx) = buttons_text.find(label) {
            let col = buttons_text[..byte_idx].chars().count() as u16;
            out.push((Rect { x: base_x + col, y: row_y, width: display_width(label) as u16, height: 1 }, id));
        }
    }
    out
}
