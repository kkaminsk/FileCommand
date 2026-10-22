//! The live command line: `C:\NORTON>dir`.
//!
//! The prompt is derived from the active panel's path (never stored), so it
//! follows Tab and directory changes for free. The text is the shared
//! `LineEdit` model behind every single-line field (text-field-editing);
//! the drawn `_` caret of the old append-only line is gone — the real
//! terminal cursor, positioned at the caret by the caller via
//! `Frame::set_cursor_position`, replaces it (design D5).

use filecommand_core::line_edit::LineEdit;
use filecommand_core::listing::{display_width, pad_to_width};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

/// Renders the command line and returns the terminal cursor's `(x, y)`
/// position — the caret's cell — or `None` when the row has no room to
/// draw at all. The text window scrolls around the caret so what is being
/// edited never falls off the right edge (responsive-layout "Command line
/// keeps the caret visible").
pub fn render_command_line(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, prompt: &str, buffer: &LineEdit) -> Option<(u16, u16)> {
    if area.height == 0 || area.width == 0 {
        return None;
    }
    let style = role_style(theme, Role::CommandLine, depth);
    let width = area.width as usize;
    // A deeply nested cwd can make the prompt itself wider than half the
    // line; past that point it left-truncates with a leading `…` so the
    // typed command still has room to show (responsive-layout "Chrome
    // degradation").
    let truncated_prompt;
    let prompt = if display_width(prompt) > width / 2 {
        truncated_prompt = left_truncate_with_ellipsis(prompt, width / 2);
        truncated_prompt.as_str()
    } else {
        prompt
    };
    let prompt_w = display_width(prompt);
    let text_w = width.saturating_sub(prompt_w);
    let (start, caret_col) = buffer.window(text_w.max(1));
    let slice: String = buffer.text().chars().skip(start).take(text_w).collect();
    buf.set_string(area.x, area.y, pad_to_width(&format!("{prompt}{slice}"), width), style);
    // `caret_col` counts window chars; the cursor column is the display
    // width of the chars before it. A caret at the end of a full window
    // (`caret_col == text_w`) clamps onto the last cell, which is also the
    // row's rightmost column — the cursor never lands outside the area.
    let caret_cells = display_width(&slice.chars().take(caret_col).collect::<String>());
    let col = (prompt_w + caret_cells).min(width.saturating_sub(1));
    Some((area.x + col as u16, area.y))
}

/// Truncate `s` from the left, keeping its last `max_w - 1` display columns
/// and prefixing a leading `…` — the mirror image of
/// `listing::truncate_with_ellipsis`'s trailing-ellipsis truncation, used
/// here because it's the *end* of the prompt (the `>`) that matters most.
fn left_truncate_with_ellipsis(s: &str, max_w: usize) -> String {
    if display_width(s) <= max_w {
        return s.to_string();
    }
    if max_w == 0 {
        return String::new();
    }
    if max_w == 1 {
        return "\u{2026}".to_string();
    }
    let budget = max_w - 1;
    let mut kept: Vec<char> = Vec::new();
    let mut acc = 0usize;
    for ch in s.chars().rev() {
        let cw = display_width(&ch.to_string());
        if acc + cw > budget {
            break;
        }
        kept.push(ch);
        acc += cw;
    }
    kept.reverse();
    let mut out = String::from('\u{2026}');
    out.extend(kept);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(width: u16, prompt: &str, buffer: &LineEdit) -> (String, Option<(u16, u16)>) {
        let area = Rect { x: 0, y: 0, width, height: 1 };
        let mut buf = Buffer::empty(area);
        let caret = render_command_line(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, prompt, buffer);
        ((0..width).map(|x| buf[(x, 0)].symbol()).collect(), caret)
    }

    #[test]
    fn renders_prompt_then_buffer_with_the_caret_past_the_text() {
        let (line, caret) = render(20, r"C:\NORTON>", &LineEdit::from("dir"));
        assert_eq!(line.trim_end(), r"C:\NORTON>dir");
        assert_eq!(caret, Some((13, 0)), "the terminal cursor replaces the old drawn `_` (prompt is 10 cells, text 3)");
    }

    #[test]
    fn empty_buffer_reports_the_caret_right_after_the_prompt() {
        let (line, caret) = render(20, r"C:\>", &LineEdit::default());
        assert_eq!(line.trim_end(), r"C:\>");
        assert_eq!(caret, Some((4, 0)));
    }

    #[test]
    fn caret_mid_text_stays_put_and_reports_its_cell() {
        let mut buffer = LineEdit::from("echo hello");
        buffer.left();
        buffer.left();
        let (line, caret) = render(30, r"C:\>", &buffer);
        assert_eq!(line.trim_end(), r"C:\>echo hello");
        assert_eq!(caret, Some((4 + 8, 0)), "caret before `lo`: prompt width 4 plus 8 chars");
    }

    #[test]
    fn overlong_lines_scroll_to_keep_the_caret_visible() {
        let (line, caret) = render(10, r"C:\a\very\long\path>", &LineEdit::from("echo hello"));
        assert_eq!(line, "…ath>hello", "the prompt left-truncates and the window shows the text's tail");
        assert_eq!(caret, Some((9, 0)), "the caret at the end clamps onto the row's last cell");
    }

    #[test]
    fn output_always_fills_the_row_exactly() {
        for width in [1u16, 5, 40, 80] {
            let (line, _) = render(width, r"C:\>", &LineEdit::from("dir"));
            assert_eq!(line.chars().count(), width as usize);
        }
    }

    #[test]
    fn a_prompt_over_half_the_width_left_truncates_with_ellipsis() {
        // Prompt is 20 columns wide; at width 30, half is 15 — over the
        // limit, so it left-truncates to 15 columns with a leading `…`.
        let (line, _) = render(30, r"C:\a\very\long\path>", &LineEdit::default());
        assert!(line.starts_with('\u{2026}'), "`{line}`");
        assert!(line.trim_end().ends_with("path>"), "the tail (with the `>`) survives: `{line}`");
        assert_eq!(display_width(line.trim_end()), 15, "prompt truncates to half the width");
    }

    #[test]
    fn a_short_prompt_is_never_truncated() {
        let (line, _) = render(30, r"C:\>", &LineEdit::default());
        assert!(!line.contains('\u{2026}'), "`{line}`");
        assert!(line.starts_with(r"C:\>"), "`{line}`");
    }

    #[test]
    fn prompt_truncation_and_caret_scrolling_combine() {
        // A long prompt (left-truncated with `…`) plus a long typed command
        // (right-scrolled to keep the caret visible) at once.
        let (line, caret) = render(20, r"C:\a\very\long\nested\path>", &LineEdit::from("echo hello world"));
        assert_eq!(display_width(&line), 20);
        assert!(line.ends_with(" world"), "the caret's end of the text stays visible: `{line}`");
        assert_eq!(caret, Some((19, 0)), "the caret clamps onto the last cell");
    }

    #[test]
    fn zero_width_area_is_a_noop() {
        let area = Rect { x: 0, y: 0, width: 0, height: 1 };
        let mut buf = Buffer::empty(area);
        assert_eq!(render_command_line(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, ">", &LineEdit::default()), None);
    }
}
