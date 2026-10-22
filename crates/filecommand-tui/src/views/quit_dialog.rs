//! F10 quit confirmation dialog, rendered from `dialog.*` roles.

use filecommand_core::dialogs::overlay_rect;
use filecommand_core::listing::{display_width, pad_to_width, truncate_with_ellipsis};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use filecommand_core::update::ButtonId;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::{label_spans, DialogLayout};
use crate::style::role_style;

const MESSAGE: &str = " Quit FileCommand? (Y/N) ";

/// The dialog's geometry at `area`: its rectangle, the one message line, and
/// the `Y`/`N` character spans recorded as button hits (design D2: a
/// buttonless dialog's hotkey spans count as buttons). The renderer and the
/// hit map both draw from this (mouse-input "Click targets match the
/// rendered geometry").
pub fn layout(area: Rect) -> DialogLayout {
    // Fixed content, so preferred and minimum coincide — this dialog is
    // never wider than it needs to be, only ever clamped by a terminal
    // smaller than its own message (responsive-layout "Unified overlay
    // geometry").
    let preferred_w = display_width(MESSAGE) as u16 + 2;
    let r = overlay_rect((preferred_w, 3), (preferred_w, 3), (area.width, area.height));
    let rect = Rect { x: area.x + r.x, y: area.y + r.y, width: r.width, height: r.height };
    let inner_w = rect.width.saturating_sub(2) as usize;
    let message = pad_to_width(&truncate_with_ellipsis(MESSAGE, inner_w), inner_w);
    let buttons = label_spans(&message, rect.x + 1, rect.y + 1, &[("Y", ButtonId::QuitYes), ("N", ButtonId::QuitNo)]);
    DialogLayout { rect, buttons, rows: Vec::new(), lines: vec![message] }
}

pub fn render_quit_dialog(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth) {
    draw_quit_dialog(buf, theme, depth, &layout(area));
}

/// Paint the dialog from an already computed [`layout`].
pub fn draw_quit_dialog(buf: &mut Buffer, theme: &Theme, depth: ColorDepth, l: &DialogLayout) {
    let style = role_style(theme, Role::DialogPrimary, depth);
    let inner_w = l.rect.width.saturating_sub(2) as usize;
    let (x, y) = (l.rect.x, l.rect.y);

    let top = format!("\u{250C}{}\u{2510}", "\u{2500}".repeat(inner_w));
    let bottom = format!("\u{2514}{}\u{2518}", "\u{2500}".repeat(inner_w));
    let mid = format!("\u{2502}{}\u{2502}", l.lines[0]);

    buf.set_string(x, y, &top, style);
    buf.set_string(x, y + 1, &mid, style);
    buf.set_string(x, y + l.rect.height - 1, &bottom, style);
}
