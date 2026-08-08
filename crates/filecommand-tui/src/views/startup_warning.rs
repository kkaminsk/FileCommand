//! The startup-warning modal: a dismissable, centered warning dialog shown
//! at session start when something recoverable was wrong with on-disk
//! config — currently only a malformed `usermenu.toml`, which falls back to
//! default entries without overwriting the file (user-menu "Malformed file
//! warns and falls back without overwriting"). Modeled on
//! `error_dialog::render_error`'s box layout, but with a single dismiss
//! hint rather than Retry/Skip/Skip All/Abort, since there is no per-item
//! choice to make here.

use filecommand_core::listing::{display_width, pad_to_width};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const DISMISS_HINT: &str = "Press any key to continue";

pub fn render_startup_warning(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, message: &str) {
    let body = role_style(theme, Role::DialogError, depth);

    let inner_w = display_width(message).max(display_width(DISMISS_HINT)).max(30) + 2;
    let box_w = inner_w as u16 + 2;
    let box_h = 5u16;
    if area.width < box_w || area.height < box_h {
        return;
    }
    let x = area.x + (area.width - box_w) / 2;
    let y = area.y + (area.height - box_h) / 2;

    let title = " Warning ";
    let top = format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w));
    let bottom = format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w));
    buf.set_string(x, y, &top, body);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(title))) / 2) as u16;
    buf.set_string(title_x, y, title, body);

    let row = |buf: &mut Buffer, dy: u16, text: &str| {
        buf.set_string(x, y + dy, "\u{2551}", body);
        buf.set_string(x + 1, y + dy, pad_to_width(text, inner_w), body);
        buf.set_string(x + 1 + inner_w as u16, y + dy, "\u{2551}", body);
    };
    row(buf, 1, message);
    row(buf, 2, "");
    row(buf, 3, DISMISS_HINT);
    buf.set_string(x, y + 4, &bottom, body);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(message: &str) -> Vec<String> {
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let mut buf = Buffer::empty(area);
        render_startup_warning(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, message);
        (0..area.height).map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect()).collect()
    }

    #[test]
    fn renders_the_title_message_and_dismiss_hint() {
        let rows = render("usermenu.toml is malformed; using default entries").join("\n");
        assert!(rows.contains("Warning"));
        assert!(rows.contains("usermenu.toml is malformed; using default entries"));
        assert!(rows.contains("Press any key to continue"));
    }
}
