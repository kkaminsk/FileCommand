//! The Enter-on-file action menu: a primary-style modal list of the
//! target entry's available actions (Run when executable, then View, Edit,
//! Copy, Rename, Move, Delete), mirroring `user_menu`'s double-line frame
//! and highlighted-row conventions (file-action-menu "Menu contents,
//! ordering, and navigation": "render as a primary-style modal dialog
//! (§4.4)").

use filecommand_core::dialogs::{overlay_rect, FileActionMenuState};
use filecommand_core::listing::{display_width, pad_to_width, truncate_with_ellipsis};
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
    let preferred_inner_w = (widest_label + 2).max(display_width(&title)).clamp(MIN_INNER_W, MAX_INNER_W);
    let content_rows = dialog.entries.len() as u16;
    let r = overlay_rect((preferred_inner_w as u16 + 2, content_rows + 2), (MIN_INNER_W as u16 + 2, 3), (area.width, area.height));
    let box_h = r.height;
    let inner_w = r.width.saturating_sub(2) as usize;
    let visible_rows = box_h.saturating_sub(2) as usize;
    let x = area.x + r.x;
    let y = area.y + r.y;

    buf.set_string(x, y, format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w)), body);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(&title))) / 2) as u16;
    buf.set_string(title_x, y, truncate_with_ellipsis(&title, inner_w), body);

    for (i, entry) in dialog.entries.iter().take(visible_rows).enumerate() {
        let ry = y + 1 + i as u16;
        let style = if i == dialog.cursor { highlight } else { body };
        buf.set_string(x, ry, "\u{2551}", body);
        buf.set_string(x + 1, ry, pad_to_width(&truncate_with_ellipsis(&format!(" {}", entry.label()), inner_w), inner_w), style);
        buf.set_string(x + 1 + inner_w as u16, ry, "\u{2551}", body);
    }

    buf.set_string(x, y + box_h - 1, format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w)), body);
}

/// The open file-action menu's clickable row rects at `area` (the same
/// full-screen `area` `render_file_action_menu` is given), indexed by
/// position in `dialog.entries` so the index matches exactly what
/// `FileActionMenuState::cursor` would land on (mouse-input "File-action
/// menu entries are clickable"). Mirrors `render_file_action_menu`'s own
/// box-geometry math (`overlay_rect`, `box_h`, `inner_w`, `visible_rows`,
/// `x`, `y`) so a row rect can never land somewhere the box isn't actually
/// drawn this frame.
pub fn hit_items(area: Rect, dialog: &FileActionMenuState) -> Vec<(Rect, usize)> {
    let title = format!(" {} ", dialog.target_name.to_string_lossy());
    let widest_label = dialog.entries.iter().map(|e| display_width(e.label())).max().unwrap_or(0);
    let preferred_inner_w = (widest_label + 2).max(display_width(&title)).clamp(MIN_INNER_W, MAX_INNER_W);
    let content_rows = dialog.entries.len() as u16;
    let r = overlay_rect((preferred_inner_w as u16 + 2, content_rows + 2), (MIN_INNER_W as u16 + 2, 3), (area.width, area.height));
    let box_h = r.height;
    let inner_w = r.width.saturating_sub(2);
    let visible_rows = box_h.saturating_sub(2) as usize;
    let x = area.x + r.x;
    let y = area.y + r.y;

    let mut out = Vec::with_capacity(dialog.entries.len().min(visible_rows));
    for i in 0..dialog.entries.len().min(visible_rows) {
        let ry = y + 1 + i as u16;
        out.push((Rect { x: x + 1, y: ry, width: inner_w, height: 1 }, i));
    }
    out
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
