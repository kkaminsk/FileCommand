//! The Alt+F8 command-history dialog: a primary-style modal list of the
//! persisted command history (`State::history`), rendered newest first, in
//! the user menu's geometry (command-line "Command history dialog (Alt+F8)";
//! pulldown-menu-completion design D2).

use filecommand_core::anim;
use filecommand_core::dialogs::{overlay_rect, HistoryDialogState};
use filecommand_core::listing::{display_width, pad_to_width, truncate_with_ellipsis};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const TITLE: &str = " History ";
const MIN_INNER_W: usize = 20;
const MAX_INNER_W: usize = 50;
const EMPTY_PLACEHOLDER: &str = "(no history)";

/// Renders the Alt+F8 history dialog: `history` (stored oldest first) listed
/// newest first, with `dialog.cursor` counting display rows from the top (0 =
/// the newest entry). An empty history shows a single `(no history)` row that
/// never highlights.
///
/// `now` is the reducer clock when animations are on (`State::anim_now`) and
/// `None` for the final frame: the box unfolds top-down on open and a newly
/// highlighted row flashes in the hotkey accent before settling
/// (ui-animations "Pop-up menus unfold and highlights settle").
pub fn render_history_dialog(
    buf: &mut Buffer,
    area: Rect,
    theme: &Theme,
    depth: ColorDepth,
    dialog: &HistoryDialogState,
    history: &[String],
    now: Option<u64>,
) {
    super::unfold(buf, now, dialog.opened_at_ms, |b| draw_history_dialog(b, area, theme, depth, dialog, history, now));
}

fn draw_history_dialog(
    buf: &mut Buffer,
    area: Rect,
    theme: &Theme,
    depth: ColorDepth,
    dialog: &HistoryDialogState,
    history: &[String],
    now: Option<u64>,
) {
    let body = role_style(theme, Role::DialogPrimary, depth);
    let settling = matches!((now, dialog.selected_at_ms), (Some(now), Some(at)) if now.saturating_sub(at) < anim::HIGHLIGHT_SETTLE_MS);
    let highlight = role_style(theme, if settling { Role::MenuHotkey } else { Role::MenuHighlight }, depth);

    let widest_entry = history.iter().map(|entry| display_width(entry)).max().unwrap_or(display_width(EMPTY_PLACEHOLDER));
    let preferred_inner_w = (widest_entry + 2).clamp(MIN_INNER_W, MAX_INNER_W).max(display_width(TITLE));
    let content_rows = history.len().max(1) as u16;
    let r = overlay_rect((preferred_inner_w as u16 + 2, content_rows + 2), (MIN_INNER_W as u16 + 2, 3), (area.width, area.height));
    let inner_w = r.width.saturating_sub(2) as usize;
    let visible_rows = r.height.saturating_sub(2) as usize;
    let x = area.x + r.x;
    let y = area.y + r.y;

    buf.set_string(x, y, format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w)), body);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(TITLE))) / 2) as u16;
    buf.set_string(title_x, y, truncate_with_ellipsis(TITLE, inner_w), body);

    if history.is_empty() {
        buf.set_string(x, y + 1, "\u{2551}", body);
        buf.set_string(x + 1, y + 1, pad_to_width(&truncate_with_ellipsis(EMPTY_PLACEHOLDER, inner_w), inner_w), body);
        buf.set_string(x + 1 + inner_w as u16, y + 1, "\u{2551}", body);
    } else {
        // A 200-entry history can be taller than the box: keep the cursor's
        // display row inside the visible window (same minimal-shift rule as
        // the panel viewport).
        let scroll = if visible_rows == 0 { 0 } else { dialog.cursor.saturating_sub(visible_rows - 1) };
        for i in 0..visible_rows {
            let row = scroll + i;
            // Display row 0 is the newest entry: `history[len - 1 - row]`.
            let Some(entry) = history.len().checked_sub(row + 1).map(|idx| &history[idx]) else { break };
            let ry = y + 1 + i as u16;
            let style = if row == dialog.cursor { highlight } else { body };
            buf.set_string(x, ry, "\u{2551}", body);
            buf.set_string(x + 1, ry, pad_to_width(&truncate_with_ellipsis(&format!(" {entry}"), inner_w), inner_w), style);
            buf.set_string(x + 1 + inner_w as u16, ry, "\u{2551}", body);
        }
    }

    buf.set_string(x, y + r.height - 1, format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w)), body);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(dialog: &HistoryDialogState, history: &[String]) -> Vec<String> {
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let mut buf = Buffer::empty(area);
        render_history_dialog(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, dialog, history, None);
        (0..area.height).map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect()).collect()
    }

    #[test]
    fn lists_history_newest_first() {
        let history = vec!["cd C:\\work".to_string(), "cd D:\\".to_string()];
        let rows = render(&HistoryDialogState::new(), &history).join("\n");
        let newest = rows.find("cd D:\\").unwrap();
        let older = rows.find("cd C:\\work").unwrap();
        assert!(newest < older, "the newest entry renders above the older one");
        assert!(rows.contains("History"), "the dialog is titled");
    }

    #[test]
    fn empty_history_shows_the_no_history_row() {
        let rows = render(&HistoryDialogState::new(), &[]).join("\n");
        assert!(rows.contains("(no history)"));
    }

    #[test]
    fn the_highlighted_row_uses_the_menu_highlight_style() {
        let history = vec!["one".to_string(), "two".to_string()];
        let mut dialog = HistoryDialogState::new();
        dialog.cursor = 1; // "one" (the older entry, on the second row)
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let mut buf = Buffer::empty(area);
        render_history_dialog(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, &dialog, &history, None);
        let theme = Theme::classic();
        let highlight = role_style(&theme, Role::MenuHighlight, ColorDepth::Ansi16);
        for y in 0..area.height {
            let row: String = (0..area.width).map(|x| buf[(x, y)].symbol()).collect();
            if row.contains(" one ") {
                let x = row.find("one").unwrap() as u16;
                assert_eq!(buf[(x, y)].style().fg, highlight.fg, "the cursor row highlights");
            }
            if row.contains(" two ") {
                let x = row.find("two").unwrap() as u16;
                assert_ne!(buf[(x, y)].style().fg, highlight.fg, "other rows do not");
            }
        }
    }

    #[test]
    fn a_history_taller_than_the_box_scrolls_to_keep_the_cursor_visible() {
        let history: Vec<String> = (0..50).map(|i| format!("cmd{i:02}")).collect();
        let mut dialog = HistoryDialogState::new();
        dialog.cursor = 49; // the oldest entry
        let rows = render(&dialog, &history);
        let joined = rows.join("\n");
        assert!(joined.contains("cmd00"), "the oldest entry is shown once the cursor reaches it:\n{joined}");
        assert!(!joined.contains("cmd49"), "the newest entries scrolled out of the window");
    }
}
