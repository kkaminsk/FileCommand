//! The Alt+F7 find-file dialog: primary style, bracket-and-dots pattern
//! input, period-authentic static progress text, and the streamed result
//! list with subtree-relative locations (find-file "Find-file invocation",
//! "Non-blocking search with static progress").

use filecommand_core::find_file::FindFileState;
use filecommand_core::listing::{display_width, pad_to_width};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const TITLE: &str = " Find file ";
const FIELD_WIDTH: usize = 40;
const MAX_LIST_ROWS: usize = 10;

pub fn render_find_file(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, dialog: &FindFileState) {
    let body = role_style(theme, Role::DialogPrimary, depth);
    let field_style = role_style(theme, Role::DialogInput, depth);
    let highlight_style = role_style(theme, Role::MenuHighlight, depth);

    let inner_w = (FIELD_WIDTH + 4).min(area.width.saturating_sub(4) as usize).max(display_width(TITLE));
    let box_w = inner_w as u16 + 2;
    // frame top + prompt/field row + status row + frame bottom, plus the
    // result list once a search has been submitted.
    let showing_results = dialog.request.is_some();
    let list_rows = if showing_results { dialog.results.len().clamp(1, MAX_LIST_ROWS) } else { 0 };
    let box_h = 2 + 2 + list_rows as u16;
    if area.width < box_w || area.height < box_h {
        return;
    }
    let x = area.x + (area.width - box_w) / 2;
    let y = area.y + (area.height.saturating_sub(box_h)) / 2;

    buf.set_string(x, y, format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w)), body);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(TITLE))) / 2) as u16;
    buf.set_string(title_x, y, TITLE, body);

    // Bracket-and-dots pattern field (§4.4), fixed once a search has begun.
    let field_w = FIELD_WIDTH.min(inner_w.saturating_sub(2));
    let mut field_text = dialog.pattern.clone();
    if display_width(&field_text) > field_w {
        let excess = display_width(&field_text) - field_w;
        field_text = field_text.chars().skip(excess).collect();
    }
    let pad = field_w.saturating_sub(display_width(&field_text));
    let field = format!("[{field_text}{}]", ".".repeat(pad));
    buf.set_string(x, y + 1, "\u{2551}", body);
    buf.set_string(x + 1, y + 1, pad_to_width(&field, inner_w), field_style);
    buf.set_string(x + 1 + inner_w as u16, y + 1, "\u{2551}", body);

    // Static-text progress/status line — never a spinner (§4.10; find-file
    // "Progress is shown as static text").
    let status = if !showing_results {
        String::new()
    } else if !dialog.done {
        format!("Searching\u{2026} {} found", dialog.results.len())
    } else if dialog.results.is_empty() {
        "No matches".to_string()
    } else {
        format!("{} found", dialog.results.len())
    };
    buf.set_string(x, y + 2, "\u{2551}", body);
    buf.set_string(x + 1, y + 2, pad_to_width(&status, inner_w), body);
    buf.set_string(x + 1 + inner_w as u16, y + 2, "\u{2551}", body);

    for row in 0..list_rows {
        let ry = y + 3 + row as u16;
        buf.set_string(x, ry, "\u{2551}", body);
        let text = match dialog.results.get(row) {
            Some(m) => pad_to_width(&m.relative_path.display().to_string(), inner_w),
            None => " ".repeat(inner_w),
        };
        let style = if row == dialog.cursor && !dialog.results.is_empty() { highlight_style } else { body };
        buf.set_string(x + 1, ry, &text, style);
        buf.set_string(x + 1 + inner_w as u16, ry, "\u{2551}", body);
    }

    buf.set_string(x, y + box_h - 1, format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w)), body);
}

#[cfg(test)]
mod tests {
    use super::*;
    use filecommand_core::listing::{Entry, EntryKind, FindMatch};
    use std::ffi::OsString;
    use std::path::PathBuf;

    fn render(dialog: &FindFileState) -> Vec<String> {
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let mut buf = Buffer::empty(area);
        render_find_file(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, dialog);
        (0..area.height).map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect()).collect()
    }

    fn a_match(rel: &str) -> FindMatch {
        FindMatch {
            relative_path: PathBuf::from(rel),
            entry: Entry { name: OsString::from(rel), kind: EntryKind::File, size: 0, modified: None },
        }
    }

    #[test]
    fn renders_the_title_and_bracket_and_dots_field() {
        let rows = render(&FindFileState::new(PathBuf::from("/root"))).join("\n");
        assert!(rows.contains("Find file"));
        assert!(rows.contains('[') && rows.contains(']'));
    }

    #[test]
    fn no_matches_reports_an_empty_result_set() {
        let mut d = FindFileState::new(PathBuf::from("/root"));
        d.submit(1);
        d.mark_done(1);
        let rows = render(&d).join("\n");
        assert!(rows.contains("No matches"));
    }

    #[test]
    fn results_show_subtree_relative_locations() {
        let mut d = FindFileState::new(PathBuf::from("/root"));
        d.submit(1);
        d.push_match(1, a_match(r"sub\report.txt"));
        let rows = render(&d).join("\n");
        assert!(rows.contains("report.txt"));
    }

    #[test]
    fn only_cp437_heritage_glyphs_and_typed_text_are_emitted() {
        let mut d = FindFileState::new(PathBuf::from("/root"));
        d.push('r');
        let rows = render(&d).join("");
        for ch in rows.chars() {
            assert!(ch.is_ascii() || "\u{2554}\u{2557}\u{255A}\u{255D}\u{2551}\u{2550}".contains(ch), "non-CP437 glyph `{ch}`");
        }
    }
}
