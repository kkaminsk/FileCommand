//! The F4 built-in editor's full-screen renderer: header row, text body with
//! the caret shown as the real terminal cursor, and its own F-key bar — the
//! same "frame-less full-screen chrome" shape the F3 viewer uses (builtin-
//! editor "Full-screen editor chrome"), reusing the viewer's `viewer.header`/
//! `viewer.text` roles per the M5 doc note on [`filecommand_core::theme::Role::ViewerHeader`].

use filecommand_core::editor::{EditorState, EntryMode};
use filecommand_core::listing::{display_width, format_count, pad_to_width};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::style::role_style;
use crate::views::header::fit_header;
use crate::views::keybar;

const KEY_NUMBERS: [&str; 10] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"];
/// Full and short label forms follow the same widest-form-that-fits rule
/// as the main panel bar (responsive-layout "F-key bar degradation
/// forms").
const KEY_LABELS_FULL: [&str; 10] = ["Help", "Save", "Mark", "Replac", "", "", "Search", "", "", "Quit"];
const KEY_LABELS_SHORT: [&str; 10] = ["Hlp", "Sav", "Mrk", "Rep", "", "", "Sch", "", "", "Qit"];

/// Rows available for the editor body at a given terminal height: the
/// header and the F-key bar each reserve one row — identical to the
/// viewer's `body_rows`, kept as its own tiny function here (rather than a
/// cross-crate call) since `core::update`'s `ensure_caret_visible` call
/// needs the same arithmetic and `filecommand-core` cannot depend on this
/// crate.
pub fn body_rows(term_height: u16) -> usize {
    term_height.saturating_sub(2) as usize
}

/// Render the whole editor: header, text body (with the selection as
/// inverse rows), F-key bar, and — while a save-on-exit confirm is pending —
/// the confirm dialog on top. Returns the terminal cursor's `(x, y)`
/// position for the caller to place the real blinking cursor there via
/// `Frame::set_cursor_position`, or `None` when the caret isn't visible
/// (behind the confirm dialog, or scrolled — which should not happen since
/// `ensure_caret_visible` keeps it in view — out of the body).
pub fn render_editor(buf: &mut Buffer, area: Rect, editor: &EditorState, theme: &Theme, depth: ColorDepth) -> Option<(u16, u16)> {
    if area.width == 0 || area.height < 2 {
        return None;
    }
    let header_style = role_style(theme, Role::ViewerHeader, depth);
    let text_style = role_style(theme, Role::ViewerText, depth);
    let selection_style = role_style(theme, Role::PanelCursor, depth);
    let w = area.width as usize;

    render_header(buf, area.x, area.y, w, editor, header_style);

    let body_h = area.height.saturating_sub(2);
    let body_y = area.y + 1;
    let selection = editor.selection_range();
    for row in 0..body_h {
        let y = body_y + row;
        let line_idx = editor.top_line + row as usize;
        let selected = matches!(selection, Some((s, e)) if line_idx >= s && line_idx <= e);
        let style = if selected { selection_style } else { text_style };
        let text = editor.lines.get(line_idx).map(String::as_str).unwrap_or("");
        let clipped = clip_from(text, editor.left_col, w);
        buf.set_string(area.x, y, pad_to_width(&clipped, w), style);
    }

    let keybar_area = Rect { x: area.x, y: area.y + area.height - 1, width: area.width, height: 1 };
    render_editor_keybar(buf, keybar_area, theme, depth);

    if editor.quit_confirm {
        render_save_on_exit_confirm(buf, area, theme, depth, editor);
        return None;
    }

    let cursor_row = editor.caret.line.checked_sub(editor.top_line)?;
    if cursor_row as u16 >= body_h {
        return None;
    }
    let cursor_col = editor.caret.col.checked_sub(editor.left_col)?;
    if cursor_col >= w {
        return None;
    }
    Some((area.x + cursor_col as u16, body_y + cursor_row as u16))
}

/// The header row's indicators drop right-to-left as width runs out — size
/// and the `Ovr` flag first, then the `Line/Col` indicator — keeping the
/// file path visible last, left-truncated with `…` if even that alone
/// does not fit (responsive-layout "Full-screen surface degradation").
fn render_header(buf: &mut Buffer, x: u16, y: u16, w: usize, editor: &EditorState, style: Style) {
    let modified_marker = if editor.is_modified() { " *" } else { "" };
    let path_field = format!("Edit: {}{modified_marker}", editor.path.display());
    let line_col = format!("Line {}/{}   Col {}", editor.caret.line + 1, editor.lines.len().max(1), editor.caret.col + 1);
    let size = format_count(editor.byte_len() as usize);
    let size_and_ovr = match editor.mode {
        EntryMode::Overwrite => format!("{size} Ovr"),
        EntryMode::Insert => size,
    };
    let line = fit_header(&path_field, &[line_col, size_and_ovr], w);
    buf.set_string(x, y, pad_to_width(&line, w), style);
}

/// The editor F-key bar: `1Help 2Save 3Mark 4Replac 5 6 7Search 8 9 10Quit`,
/// with unused slots 5/6/8/9 rendered as bare numbers with empty label
/// blocks (builtin-editor "F-key bar labels").
pub fn render_editor_keybar(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth) {
    let full: Vec<(&str, &str)> = KEY_NUMBERS.iter().copied().zip(KEY_LABELS_FULL.iter().copied()).collect();
    let short: Vec<(&str, &str)> = KEY_NUMBERS.iter().copied().zip(KEY_LABELS_SHORT.iter().copied()).collect();
    let numbers_only: Vec<(&str, &str)> = KEY_NUMBERS.iter().copied().map(|n| (n, "")).collect();

    let form = keybar::choose_form(area.width, &[&full, &short, &numbers_only]);
    keybar::render_bar(buf, area, theme, depth, form);
}

fn render_save_on_exit_confirm(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, editor: &EditorState) {
    let style = role_style(theme, Role::DialogPrimary, depth);
    let name = editor.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| editor.path.display().to_string());
    let message = format!(" Save changes to {name}? (Y/N) ");
    let inner_w = display_width(&message);
    let box_w = inner_w as u16 + 2;
    let box_h = 3u16;
    if area.width < box_w || area.height < box_h {
        return;
    }
    let x = area.x + (area.width - box_w) / 2;
    let y = area.y + (area.height - box_h) / 2;

    let top = format!("\u{250C}{}\u{2510}", "\u{2500}".repeat(inner_w));
    let bottom = format!("\u{2514}{}\u{2518}", "\u{2500}".repeat(inner_w));
    let mid = format!("\u{2502}{message}\u{2502}");

    buf.set_string(x, y, &top, style);
    buf.set_string(x, y + 1, &mid, style);
    buf.set_string(x, y + 2, &bottom, style);
}

/// A `str` slice starting at `start_col` chars in, taking up to `width`
/// chars — the editor buffer's column model is `char`-indexed (design D1),
/// so this deliberately does not account for display width the way the
/// viewer's `clip_line` does.
fn clip_from(s: &str, start_col: usize, width: usize) -> String {
    s.chars().skip(start_col).take(width).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn editor_from(path: &str, text: &str) -> EditorState {
        EditorState::from_bytes(PathBuf::from(path), text.as_bytes())
    }

    fn render(editor: &EditorState, area: Rect) -> (String, Option<(u16, u16)>) {
        let mut buf = Buffer::empty(area);
        let cursor = render_editor(&mut buf, area, editor, &Theme::classic(), ColorDepth::Ansi16);
        let text = (0..area.height).map(|y| (0..area.width).map(|x| buf[(area.x + x, area.y + y)].symbol()).collect::<String>()).collect::<Vec<_>>().join("\n");
        (text, cursor)
    }

    #[test]
    fn header_shows_path_modified_marker_line_and_col() {
        let mut e = editor_from("C:\\f.txt", &"line\n".repeat(440));
        e.caret.line = 11; // 0-based -> "Line 12"
        e.caret.col = 7; // 0-based -> "Col 8"
        e.type_char('!'); // marks modified without disturbing caret.line
        e.caret.line = 11;
        e.caret.col = 7;
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let (text, _) = render(&e, area);
        let header = text.lines().next().unwrap();
        assert!(header.contains("Edit: C:\\f.txt *"), "`{header}`");
        assert!(header.contains("Line 12/441   Col 8"), "`{header}`");
    }

    #[test]
    fn overwrite_mode_shows_size_and_ovr_in_the_header() {
        let mut e = editor_from("f.txt", "abc\n");
        e.toggle_mode();
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let (text, _) = render(&e, area);
        let header = text.lines().next().unwrap();
        assert!(header.trim_end().ends_with("Ovr"), "`{header}`");
        assert!(header.contains("4 Ovr") || header.contains("4  Ovr"), "byte size precedes Ovr: `{header}`");
    }

    #[test]
    fn a_narrow_header_drops_size_and_ovr_before_line_col_and_keeps_the_path() {
        // responsive-layout "Full-screen surface degradation": size/Ovr
        // drop first, then Line/Col, keeping the path visible last.
        let mut e = editor_from("C:\\a\\deeply\\nested\\notes.txt", "abc\n");
        e.toggle_mode(); // Overwrite, so the header would show "N Ovr"
        let wide_area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let (wide_text, _) = render(&e, wide_area);
        let wide_header = wide_text.lines().next().unwrap();
        assert!(wide_header.contains("Ovr") && wide_header.contains("Line") && wide_header.contains("notes.txt"));

        // Narrow enough that size/Ovr no longer fits alongside everything
        // else, but the path and Line/Col still do.
        let mid_area = Rect { x: 0, y: 0, width: 55, height: 24 };
        let (mid_text, _) = render(&e, mid_area);
        let mid_header = mid_text.lines().next().unwrap();
        assert!(!mid_header.contains("Ovr"), "`{mid_header}`");
        assert!(mid_header.contains("Line 1/2"), "`{mid_header}`");
        assert!(mid_header.contains("notes.txt"), "`{mid_header}`");

        // Narrower still: Line/Col drops too, only the path remains.
        let narrow_area = Rect { x: 0, y: 0, width: 40, height: 24 };
        let (narrow_text, _) = render(&e, narrow_area);
        let narrow_header = narrow_text.lines().next().unwrap();
        assert!(!narrow_header.contains("Line"), "`{narrow_header}`");
        assert!(narrow_header.contains("notes.txt"), "`{narrow_header}`");
    }

    #[test]
    fn a_very_narrow_header_left_truncates_the_path_with_ellipsis() {
        let e = editor_from("C:\\a\\very\\deeply\\nested\\path\\notes.txt", "abc\n");
        let area = Rect { x: 0, y: 0, width: 20, height: 24 };
        let (text, _) = render(&e, area);
        let header = text.lines().next().unwrap();
        assert!(header.contains('\u{2026}'), "`{header}`");
        assert!(header.contains("notes.txt"), "the filename tail survives: `{header}`");
        assert_eq!(display_width(header), 20);
    }

    #[test]
    fn keybar_matches_the_spec_string() {
        let e = editor_from("f.txt", "abc\n");
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let (text, _) = render(&e, area);
        let last = text.lines().last().unwrap();
        assert!(last.trim_end().starts_with("1Help 2Save 3Mark 4Replac 5 6 7Search 8 9 10Quit"), "`{last}`");
    }

    #[test]
    fn marked_lines_render_as_inverse_rows() {
        let mut e = editor_from("f.txt", "a\nb\nc\n");
        e.start_mark();
        e.move_down();
        let area = Rect { x: 0, y: 0, width: 20, height: 6 };
        let mut buf = Buffer::empty(area);
        render_editor(&mut buf, area, &e, &Theme::classic(), ColorDepth::Ansi16);
        let selection_style = role_style(&Theme::classic(), Role::PanelCursor, ColorDepth::Ansi16);
        let text_style = role_style(&Theme::classic(), Role::ViewerText, ColorDepth::Ansi16);
        assert_eq!(Some(buf[(0, 1)].bg), selection_style.bg, "row 0 (line 'a') is selected");
        assert_eq!(Some(buf[(0, 2)].bg), selection_style.bg, "row 1 (line 'b') is selected");
        assert_eq!(Some(buf[(0, 3)].bg), text_style.bg, "row 2 (line 'c') is not selected");
    }

    #[test]
    fn caret_position_is_reported_for_the_real_terminal_cursor() {
        let mut e = editor_from("f.txt", "hello\n");
        e.caret.col = 3;
        let area = Rect { x: 0, y: 0, width: 40, height: 10 };
        let (_, cursor) = render(&e, area);
        assert_eq!(cursor, Some((3, 1)), "column 3, first body row (row 0 is the header)");
    }

    #[test]
    fn quit_confirm_hides_the_cursor_and_shows_the_dialog() {
        let mut e = editor_from("C:\\notes.txt", "abc\n");
        e.type_char('!');
        e.quit_confirm = true;
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let (text, cursor) = render(&e, area);
        assert_eq!(cursor, None);
        assert!(text.contains("Save changes to notes.txt?"), "{text}");
    }

    #[test]
    fn selection_and_body_reuse_the_viewer_roles() {
        // A cheap regression guard for the doc claim that the editor reuses
        // `viewer.header`/`viewer.text` rather than inventing its own roles.
        let e = editor_from("f.txt", "abc\n");
        let area = Rect { x: 0, y: 0, width: 20, height: 5 };
        let mut buf = Buffer::empty(area);
        render_editor(&mut buf, area, &e, &Theme::classic(), ColorDepth::Ansi16);
        let text_style = role_style(&Theme::classic(), Role::ViewerText, ColorDepth::Ansi16);
        assert_eq!(Some(buf[(0, 1)].bg), text_style.bg);
    }
}
