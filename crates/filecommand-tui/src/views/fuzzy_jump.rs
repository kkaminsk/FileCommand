//! The Ctrl+J fuzzy-jump dialog: a primary-style modal box with a
//! single-line pattern input and the live frecency-ranked, subsequence-
//! filtered visited-directory list beneath it (fuzzy-jump "Fuzzy jump
//! dialog invocation").

use filecommand_core::listing::{display_width, pad_to_width};
use filecommand_core::quicksearch::{rank_directories, FrecencyEntry, FuzzyJumpState};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const TITLE: &str = " Fuzzy jump ";
const MAX_INNER_W: usize = 60;
const MAX_LIST_ROWS: usize = 10;

pub fn render_fuzzy_jump(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, dialog: &FuzzyJumpState, history: &[FrecencyEntry], now_ms: u64) {
    let body_style = role_style(theme, Role::DialogPrimary, depth);
    let input_style = role_style(theme, Role::DialogInput, depth);
    let highlight_style = role_style(theme, Role::MenuHighlight, depth);

    let ranked = rank_directories(history, &dialog.pattern, now_ms);
    let list_rows = ranked.len().clamp(1, MAX_LIST_ROWS);
    let inner_w = MAX_INNER_W.min(area.width.saturating_sub(4) as usize).max(display_width(TITLE));
    let box_w = inner_w as u16 + 2;
    // Frame top + input row + a separator + list rows + frame bottom.
    let box_h = 2 + 1 + 1 + list_rows as u16;
    if area.width < box_w || area.height < box_h {
        return;
    }
    let x = area.x + (area.width - box_w) / 2;
    let y = area.y + (area.height.saturating_sub(box_h)) / 2;

    let top = format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w));
    buf.set_string(x, y, &top, body_style);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(TITLE))) / 2) as u16;
    buf.set_string(title_x, y, TITLE, body_style);

    // Pattern input row, an empty-caret dot when nothing has been typed yet
    // — matching the bracket-and-dots idiom used elsewhere for an empty
    // single-line field.
    let input_text = if dialog.pattern.is_empty() { ".".repeat(inner_w) } else { pad_to_width(&dialog.pattern, inner_w) };
    buf.set_string(x, y + 1, "\u{2551}", body_style);
    buf.set_string(x + 1, y + 1, &input_text, input_style);
    buf.set_string(x + 1 + inner_w as u16, y + 1, "\u{2551}", body_style);

    buf.set_string(x, y + 2, format!("\u{2560}{}\u{2563}", "\u{2500}".repeat(inner_w)), body_style);

    for row in 0..list_rows {
        let ry = y + 3 + row as u16;
        buf.set_string(x, ry, "\u{2551}", body_style);
        let text = match ranked.get(row) {
            Some(entry) => pad_to_width(&entry.path.display().to_string(), inner_w),
            None => " ".repeat(inner_w),
        };
        let style = if row == dialog.cursor && !ranked.is_empty() { highlight_style } else { body_style };
        buf.set_string(x + 1, ry, &text, style);
        buf.set_string(x + 1 + inner_w as u16, ry, "\u{2551}", body_style);
    }

    buf.set_string(x, y + box_h - 1, format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w)), body_style);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn render(dialog: &FuzzyJumpState, history: &[FrecencyEntry]) -> Vec<String> {
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let mut buf = Buffer::empty(area);
        render_fuzzy_jump(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, dialog, history, 0);
        (0..area.height).map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect()).collect()
    }

    #[test]
    fn renders_the_title_and_double_line_frame() {
        let rows = render(&FuzzyJumpState::new(), &[]).join("\n");
        assert!(rows.contains("Fuzzy jump"));
        assert!(rows.contains('\u{2554}') && rows.contains('\u{255D}'));
    }

    #[test]
    fn empty_pattern_shows_the_full_ranked_list() {
        let history = vec![
            FrecencyEntry { path: PathBuf::from(r"C:\Low"), visit_count: 1, last_visited_ms: 0 },
            FrecencyEntry { path: PathBuf::from(r"C:\High"), visit_count: 10, last_visited_ms: 0 },
        ];
        let rows = render(&FuzzyJumpState::new(), &history).join("\n");
        assert!(rows.contains(r"C:\Low") && rows.contains(r"C:\High"));
    }

    #[test]
    fn the_highlighted_row_uses_the_menu_highlight_style() {
        let history = vec![FrecencyEntry { path: PathBuf::from(r"C:\A"), visit_count: 1, last_visited_ms: 0 }];
        let mut dialog = FuzzyJumpState::new();
        dialog.cursor = 0;
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let mut buf = Buffer::empty(area);
        render_fuzzy_jump(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, &dialog, &history, 0);
        let theme = Theme::classic();
        let highlight = role_style(&theme, Role::MenuHighlight, ColorDepth::Ansi16);
        // Find the row containing "C:\A" and check a cell within it.
        let mut found = false;
        for y in 0..area.height {
            let row: String = (0..area.width).map(|x| buf[(x, y)].symbol()).collect();
            if row.contains(r"C:\A") {
                let x = row.find('C').unwrap() as u16;
                assert_eq!(buf[(x, y)].style().fg, highlight.fg);
                found = true;
            }
        }
        assert!(found, "expected to find the rendered directory row");
    }

    #[test]
    fn typed_pattern_appears_in_the_input_row() {
        let mut dialog = FuzzyJumpState::new();
        dialog.pattern = "docu".to_string();
        let rows = render(&dialog, &[]).join("\n");
        assert!(rows.contains("docu"));
    }
}
