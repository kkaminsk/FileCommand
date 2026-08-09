//! The live command line: `C:\NORTON>dir_`.
//!
//! The prompt is derived from the active panel's path (never stored), so it
//! follows Tab and directory changes for free.

use filecommand_core::listing::{display_width, pad_to_width};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

/// The block standing in for the text cursor. The terminal cursor itself is
/// hidden while the TUI owns the screen.
const CURSOR: char = '_';

pub fn render_command_line(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, prompt: &str, buffer_text: &str) {
    if area.height == 0 || area.width == 0 {
        return;
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
    let full = format!("{prompt}{buffer_text}{CURSOR}");
    // A long path plus a long command would otherwise push what is being
    // typed off the right edge, so scroll the *tail* into view instead of
    // truncating it away.
    let text = if display_width(&full) > width { clip_left(&full, width) } else { pad_to_width(&full, width) };
    buf.set_string(area.x, area.y, text, style);
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

/// Keep the last `width` display columns of `s`.
fn clip_left(s: &str, width: usize) -> String {
    let mut out: Vec<char> = Vec::new();
    let mut acc = 0usize;
    for ch in s.chars().rev() {
        let cw = display_width(&ch.to_string());
        if acc + cw > width {
            break;
        }
        out.push(ch);
        acc += cw;
    }
    out.reverse();
    let mut text: String = out.into_iter().collect();
    while display_width(&text) < width {
        text.push(' ');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(width: u16, prompt: &str, buffer_text: &str) -> String {
        let area = Rect { x: 0, y: 0, width, height: 1 };
        let mut buf = Buffer::empty(area);
        render_command_line(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, prompt, buffer_text);
        (0..width).map(|x| buf[(x, 0)].symbol()).collect()
    }

    #[test]
    fn renders_prompt_then_buffer_then_cursor() {
        assert_eq!(render(20, r"C:\NORTON>", "dir").trim_end(), r"C:\NORTON>dir_");
    }

    #[test]
    fn empty_buffer_renders_just_the_prompt_and_cursor() {
        assert_eq!(render(20, r"C:\>", "").trim_end(), r"C:\>_");
    }

    #[test]
    fn overlong_lines_keep_the_typed_tail_visible() {
        let line = render(10, r"C:\a\very\long\path>", "echo hello");
        assert_eq!(line, "cho hello_");
    }

    #[test]
    fn output_always_fills_the_row_exactly() {
        for width in [1u16, 5, 40, 80] {
            assert_eq!(render(width, r"C:\>", "dir").chars().count(), width as usize);
        }
    }

    #[test]
    fn a_prompt_over_half_the_width_left_truncates_with_ellipsis() {
        // Prompt is 20 columns wide; at width 30, half is 15 — over the
        // limit, so it left-truncates to 15 columns with a leading `…`.
        let line = render(30, r"C:\a\very\long\path>", "");
        assert!(line.starts_with('\u{2026}'), "`{line}`");
        assert!(line.trim_end().ends_with("path>_"), "the tail (with the `>`) survives: `{line}`");
        let prompt_part = line.trim_end().strip_suffix('_').unwrap();
        assert_eq!(display_width(prompt_part), 15, "prompt truncates to half the width");
    }

    #[test]
    fn a_short_prompt_is_never_truncated() {
        let line = render(30, r"C:\>", "");
        assert!(!line.contains('\u{2026}'), "`{line}`");
        assert!(line.starts_with(r"C:\>_"), "`{line}`");
    }

    #[test]
    fn prompt_truncation_and_caret_scrolling_combine() {
        // A long prompt (left-truncated with `…`) plus a long typed command
        // (right-scrolled to keep the caret visible) at once.
        let line = render(20, r"C:\a\very\long\nested\path>", "echo hello world");
        assert_eq!(display_width(&line), 20);
        assert!(line.ends_with('_'), "the caret stays visible: `{line}`");
        assert!(line.ends_with("world_"), "`{line}`");
    }

    #[test]
    fn zero_width_area_is_a_noop() {
        let area = Rect { x: 0, y: 0, width: 0, height: 1 };
        let mut buf = Buffer::empty(area);
        render_command_line(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, ">", "");
    }
}
