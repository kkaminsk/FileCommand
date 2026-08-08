//! The F2 user menu: a primary-style modal list of `usermenu.toml` labels
//! in file order, with an empty-state placeholder when there are none
//! (user-menu "Open the F2 user menu").

use filecommand_core::config::UserMenuEntry;
use filecommand_core::dialogs::UserMenuState;
use filecommand_core::listing::{display_width, pad_to_width};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const TITLE: &str = " User menu ";
const MIN_INNER_W: usize = 20;
const MAX_INNER_W: usize = 50;
const EMPTY_PLACEHOLDER: &str = "(no entries — see usermenu.toml)";

pub fn render_user_menu(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, dialog: &UserMenuState, entries: &[UserMenuEntry]) {
    let body = role_style(theme, Role::DialogPrimary, depth);
    let highlight = role_style(theme, Role::MenuHighlight, depth);

    let widest_label = entries.iter().map(|e| display_width(&e.label)).max().unwrap_or(display_width(EMPTY_PLACEHOLDER));
    let inner_w = (widest_label + 2).clamp(MIN_INNER_W, MAX_INNER_W).min(area.width.saturating_sub(4) as usize).max(display_width(TITLE));
    let box_w = inner_w as u16 + 2;
    let rows = entries.len().max(1); // the empty-state placeholder is one row
    let box_h = rows as u16 + 2;
    if area.width < box_w || area.height < box_h {
        return;
    }
    let x = area.x + (area.width - box_w) / 2;
    let y = area.y + (area.height.saturating_sub(box_h)) / 2;

    buf.set_string(x, y, format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w)), body);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(TITLE))) / 2) as u16;
    buf.set_string(title_x, y, TITLE, body);

    if entries.is_empty() {
        buf.set_string(x, y + 1, "\u{2551}", body);
        buf.set_string(x + 1, y + 1, pad_to_width(EMPTY_PLACEHOLDER, inner_w), body);
        buf.set_string(x + 1 + inner_w as u16, y + 1, "\u{2551}", body);
    } else {
        for (i, entry) in entries.iter().enumerate() {
            let ry = y + 1 + i as u16;
            let style = if i == dialog.cursor { highlight } else { body };
            buf.set_string(x, ry, "\u{2551}", body);
            buf.set_string(x + 1, ry, pad_to_width(&format!(" {}", entry.label), inner_w), style);
            buf.set_string(x + 1 + inner_w as u16, ry, "\u{2551}", body);
        }
    }

    buf.set_string(x, y + box_h - 1, format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w)), body);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(label: &str, command: &str) -> UserMenuEntry {
        UserMenuEntry { label: label.to_string(), command: command.to_string() }
    }

    fn render(dialog: &UserMenuState, entries: &[UserMenuEntry]) -> Vec<String> {
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let mut buf = Buffer::empty(area);
        render_user_menu(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, dialog, entries);
        (0..area.height).map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect()).collect()
    }

    #[test]
    fn lists_labels_in_file_order_and_never_the_command_string() {
        let entries = vec![entry("Compress", "7z a x.7z"), entry("Backup", r"robocopy . D:\backup /E")];
        let rows = render(&UserMenuState::new(), &entries).join("\n");
        let compress_pos = rows.find("Compress").unwrap();
        let backup_pos = rows.find("Backup").unwrap();
        assert!(compress_pos < backup_pos, "entries must render in file order");
        assert!(!rows.contains("robocopy"), "the underlying command string must never be shown");
    }

    #[test]
    fn empty_menu_shows_a_placeholder_not_nothing() {
        let rows = render(&UserMenuState::new(), &[]).join("\n");
        assert!(rows.contains("no entries"));
    }

    #[test]
    fn the_highlighted_row_uses_the_menu_highlight_style() {
        let entries = vec![entry("A", "a"), entry("B", "b")];
        let mut dialog = UserMenuState::new();
        dialog.cursor = 1;
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let mut buf = Buffer::empty(area);
        render_user_menu(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, &dialog, &entries);
        let theme = Theme::classic();
        let highlight = role_style(&theme, Role::MenuHighlight, ColorDepth::Ansi16);
        for y in 0..area.height {
            let row: String = (0..area.width).map(|x| buf[(x, y)].symbol()).collect();
            if row.trim().ends_with('B') || row.contains(" B ") {
                let x = row.find('B').unwrap() as u16;
                assert_eq!(buf[(x, y)].style().fg, highlight.fg);
            }
        }
    }
}
