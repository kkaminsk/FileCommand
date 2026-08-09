//! The F2 user menu: a primary-style modal list of `usermenu.toml` labels
//! in file order, with an empty-state placeholder when there are none, plus
//! a separator and a compiled-in `Themes` row that is always present
//! (user-menu "Open the F2 user menu").

use filecommand_core::config::UserMenuEntry;
use filecommand_core::dialogs::{overlay_rect, UserMenuState};
use filecommand_core::listing::{display_width, pad_to_width, truncate_with_ellipsis};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const TITLE: &str = " User menu ";
const MIN_INNER_W: usize = 20;
const MAX_INNER_W: usize = 50;
const EMPTY_PLACEHOLDER: &str = "(no entries — see usermenu.toml)";
/// The compiled-in built-in row's label — matches the Options pull-down's
/// `Themes` item (design D2 of `user-menu-themes-entry`).
const THEMES_LABEL: &str = "Themes";

/// Renders the F2 user menu: the user's `usermenu.toml` entries (or the
/// empty-state placeholder), then always a separator row and a compiled-in
/// `Themes` row (user-menu "Open the F2 user menu"). `dialog.cursor ==
/// entries.len()` highlights the built-in row; the separator row is never
/// highlighted (design D2/D3).
pub fn render_user_menu(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, dialog: &UserMenuState, entries: &[UserMenuEntry]) {
    let body = role_style(theme, Role::DialogPrimary, depth);
    let highlight = role_style(theme, Role::MenuHighlight, depth);

    let widest_label = entries.iter().map(|e| display_width(&e.label)).max().unwrap_or(display_width(EMPTY_PLACEHOLDER)).max(display_width(THEMES_LABEL));
    let preferred_inner_w = (widest_label + 2).clamp(MIN_INNER_W, MAX_INNER_W).max(display_width(TITLE));
    // User rows (or the one-row empty placeholder), plus a separator row and
    // the built-in Themes row. The minimum height (5 = 2 borders + 1
    // placeholder/entry row + separator + Themes row) guarantees the
    // built-in entry is always reachable, even in the degraded band
    // (design D2 of `user-menu-themes-entry`; responsive-layout design D6).
    let content_rows = entries.len().max(1) as u16 + 2;
    let r = overlay_rect((preferred_inner_w as u16 + 2, content_rows + 2), (MIN_INNER_W as u16 + 2, 5), (area.width, area.height));
    let box_h = r.height;
    let inner_w = r.width.saturating_sub(2) as usize;
    // Rows available for user entries (or the placeholder), i.e. everything
    // but the two borders and the trailing separator + Themes row.
    let visible_rows = box_h.saturating_sub(4) as usize;
    let x = area.x + r.x;
    let y = area.y + r.y;

    buf.set_string(x, y, format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w)), body);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(TITLE))) / 2) as u16;
    buf.set_string(title_x, y, truncate_with_ellipsis(TITLE, inner_w), body);

    if entries.is_empty() {
        buf.set_string(x, y + 1, "\u{2551}", body);
        buf.set_string(x + 1, y + 1, pad_to_width(&truncate_with_ellipsis(EMPTY_PLACEHOLDER, inner_w), inner_w), body);
        buf.set_string(x + 1 + inner_w as u16, y + 1, "\u{2551}", body);
    } else {
        for (i, entry) in entries.iter().take(visible_rows).enumerate() {
            let ry = y + 1 + i as u16;
            let style = if i == dialog.cursor { highlight } else { body };
            buf.set_string(x, ry, "\u{2551}", body);
            buf.set_string(x + 1, ry, pad_to_width(&truncate_with_ellipsis(&format!(" {}", entry.label), inner_w), inner_w), style);
            buf.set_string(x + 1 + inner_w as u16, ry, "\u{2551}", body);
        }
    }

    // Separator row, then the built-in Themes row — always present, below
    // the user entries actually rendered (or the empty-state placeholder).
    let sep_y = y + 1 + visible_rows as u16;
    buf.set_string(x, sep_y, format!("\u{2560}{}\u{2563}", "\u{2550}".repeat(inner_w)), body);

    let themes_y = sep_y + 1;
    let themes_style = if dialog.cursor == entries.len() { highlight } else { body };
    buf.set_string(x, themes_y, "\u{2551}", body);
    buf.set_string(x + 1, themes_y, pad_to_width(&format!(" {THEMES_LABEL}"), inner_w), themes_style);
    buf.set_string(x + 1 + inner_w as u16, themes_y, "\u{2551}", body);

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
