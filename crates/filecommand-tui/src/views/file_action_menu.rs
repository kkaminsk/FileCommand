//! The Enter-on-file action menu: a primary-style modal list of the
//! target entry's available actions (Run when executable, then View, Edit,
//! Copy, Rename, Move, Delete), mirroring `user_menu`'s double-line frame
//! and highlighted-row conventions (file-action-menu "Menu contents,
//! ordering, and navigation": "render as a primary-style modal dialog
//! (§4.4)").

use filecommand_core::dialogs::FileActionMenuState;
use filecommand_core::listing::{display_width, pad_to_width};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const MIN_INNER_W: usize = 16;
const MAX_INNER_W: usize = 50;

pub fn render_file_action_menu(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, dialog: &FileActionMenuState) {
    let body = role_style(theme, Role::DialogPrimary, depth);
    let highlight = role_style(theme, Role::MenuHighlight, depth);

    let title = format!(" {} ", dialog.target_name.to_string_lossy());
    let widest_label = dialog.entries.iter().map(|e| display_width(e.label())).max().unwrap_or(0);
    let inner_w = (widest_label + 2)
        .max(display_width(&title))
        .clamp(MIN_INNER_W, MAX_INNER_W)
        .min(area.width.saturating_sub(4) as usize);
    let box_w = inner_w as u16 + 2;
    let box_h = dialog.entries.len() as u16 + 2;
    if area.width < box_w || area.height < box_h {
        return;
    }
    let x = area.x + (area.width - box_w) / 2;
    let y = area.y + (area.height.saturating_sub(box_h)) / 2;

    buf.set_string(x, y, format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w)), body);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(&title))) / 2) as u16;
    buf.set_string(title_x, y, &title, body);

    for (i, entry) in dialog.entries.iter().enumerate() {
        let ry = y + 1 + i as u16;
        let style = if i == dialog.cursor { highlight } else { body };
        buf.set_string(x, ry, "\u{2551}", body);
        buf.set_string(x + 1, ry, pad_to_width(&format!(" {}", entry.label()), inner_w), style);
        buf.set_string(x + 1 + inner_w as u16, ry, "\u{2551}", body);
    }

    buf.set_string(x, y + box_h - 1, format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w)), body);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn render(dialog: &FileActionMenuState) -> Vec<String> {
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let mut buf = Buffer::empty(area);
        render_file_action_menu(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, dialog);
        (0..area.height).map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect()).collect()
    }

    #[test]
    fn non_executable_lists_view_first_without_run() {
        let dialog = FileActionMenuState::new(OsString::from("notes.txt"), false);
        let rows = render(&dialog).join("\n");
        assert!(rows.contains("View"));
        assert!(rows.contains("Edit"));
        assert!(rows.contains("Copy"));
        assert!(rows.contains("Rename"));
        assert!(rows.contains("Move"));
        assert!(rows.contains("Delete"));
        assert!(!rows.contains("Run"), "non-executable target must not list Run");
        let view_pos = rows.find("View").unwrap();
        let edit_pos = rows.find("Edit").unwrap();
        assert!(view_pos < edit_pos, "entries must render in menu order");
    }

    #[test]
    fn executable_lists_run_first() {
        let dialog = FileActionMenuState::new(OsString::from("setup.exe"), true);
        let rows = render(&dialog).join("\n");
        let run_pos = rows.find("Run").unwrap();
        let view_pos = rows.find("View").unwrap();
        assert!(run_pos < view_pos, "Run must render before View when the target is executable");
    }

    #[test]
    fn the_highlighted_row_uses_the_menu_highlight_style() {
        let mut dialog = FileActionMenuState::new(OsString::from("notes.txt"), false);
        dialog.cursor = 1;
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let mut buf = Buffer::empty(area);
        render_file_action_menu(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, &dialog);
        let theme = Theme::classic();
        let highlight = role_style(&theme, Role::MenuHighlight, ColorDepth::Ansi16);
        let expected_label = dialog.entries[1].label();
        for y in 0..area.height {
            let row: String = (0..area.width).map(|x| buf[(x, y)].symbol()).collect();
            if row.contains(expected_label) {
                let x = row.find(expected_label).unwrap() as u16;
                assert_eq!(buf[(x, y)].style().fg, highlight.fg);
            }
        }
    }
}
