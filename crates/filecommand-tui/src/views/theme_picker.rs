//! The Options → Themes picker: a primary-style modal list of the six
//! built-in theme names, with the active theme marked, modeled closely on
//! `user_menu.rs` (theme-selection "Options menu opens the theme picker").

use filecommand_core::dialogs::ThemePickerState;
use filecommand_core::listing::{display_width, pad_to_width};
use filecommand_core::theme::{ColorDepth, Role, Theme, BUILTIN_THEME_NAMES};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const TITLE: &str = " Themes ";
/// `"* "` before an active theme's name, `"  "` otherwise — the same
/// asterisk-marks-current convention the built-in editor uses for its
/// modified-buffer indicator, kept CP437/ASCII-safe.
const MARKER_ACTIVE: &str = "* ";
const MARKER_INACTIVE: &str = "  ";
const MIN_INNER_W: usize = 16;

pub fn render_theme_picker(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, dialog: &ThemePickerState, active_theme_name: &str) {
    let body = role_style(theme, Role::DialogPrimary, depth);
    let highlight = role_style(theme, Role::MenuHighlight, depth);

    let widest_name = BUILTIN_THEME_NAMES.iter().map(|n| display_width(n)).max().unwrap_or(0);
    let inner_w = (widest_name + MARKER_ACTIVE.len() + 1).max(MIN_INNER_W).min(area.width.saturating_sub(4) as usize).max(display_width(TITLE));
    let box_w = inner_w as u16 + 2;
    let rows = BUILTIN_THEME_NAMES.len();
    let box_h = rows as u16 + 2;
    if area.width < box_w || area.height < box_h {
        return;
    }
    let x = area.x + (area.width - box_w) / 2;
    let y = area.y + (area.height.saturating_sub(box_h)) / 2;

    buf.set_string(x, y, format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w)), body);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(TITLE))) / 2) as u16;
    buf.set_string(title_x, y, TITLE, body);

    for (i, name) in BUILTIN_THEME_NAMES.iter().enumerate() {
        let ry = y + 1 + i as u16;
        let marker = if *name == active_theme_name { MARKER_ACTIVE } else { MARKER_INACTIVE };
        let style = if i == dialog.highlight { highlight } else { body };
        buf.set_string(x, ry, "\u{2551}", body);
        buf.set_string(x + 1, ry, pad_to_width(&format!("{marker}{name}"), inner_w), style);
        buf.set_string(x + 1 + inner_w as u16, ry, "\u{2551}", body);
    }

    buf.set_string(x, y + box_h - 1, format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w)), body);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(dialog: &ThemePickerState, active_theme_name: &str) -> Vec<String> {
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let mut buf = Buffer::empty(area);
        render_theme_picker(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, dialog, active_theme_name);
        (0..area.height).map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect()).collect()
    }

    #[test]
    fn lists_every_builtin_theme_name() {
        let rows = render(&ThemePickerState::open("nc-classic"), "nc-classic").join("\n");
        for name in BUILTIN_THEME_NAMES {
            assert!(rows.contains(name), "`{name}` missing from the picker");
        }
    }

    #[test]
    fn the_active_theme_row_carries_the_marker() {
        let rows = render(&ThemePickerState::open("terminal-green"), "terminal-green");
        let line = rows.iter().find(|r| r.contains("terminal-green")).expect("terminal-green row present");
        assert!(line.contains('*'), "`{line}`");
        // No other row carries the marker.
        for name in BUILTIN_THEME_NAMES.iter().filter(|n| **n != "terminal-green") {
            let other = rows.iter().find(|r| r.contains(*name)).unwrap();
            assert!(!other.contains('*'), "`{other}` should not carry the active marker");
        }
    }

    #[test]
    fn the_highlighted_row_uses_the_menu_highlight_style() {
        let mut dialog = ThemePickerState::open("nc-classic");
        dialog.highlight = 2; // terminal-green
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let mut buf = Buffer::empty(area);
        render_theme_picker(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, &dialog, "nc-classic");
        let theme = Theme::classic();
        let highlight = role_style(&theme, Role::MenuHighlight, ColorDepth::Ansi16);
        let body = role_style(&theme, Role::DialogPrimary, ColorDepth::Ansi16);
        let text: Vec<String> = (0..area.height).map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect()).collect();
        let row_y = text.iter().position(|r| r.contains("terminal-green")).unwrap() as u16;
        let col = text[row_y as usize].find('t').unwrap() as u16;
        assert_eq!(buf[(col, row_y)].style().fg, highlight.fg);
        assert_ne!(highlight, body);
    }

    #[test]
    fn only_cp437_heritage_glyphs_are_emitted() {
        let rows = render(&ThemePickerState::open("nc-classic"), "nc-classic").join("");
        for ch in rows.chars() {
            assert!(ch.is_ascii() || "\u{2554}\u{2557}\u{255A}\u{255D}\u{2550}\u{2551}".contains(ch), "non-CP437 glyph `{ch}` (U+{:04X})", ch as u32);
        }
    }

    #[test]
    fn a_screen_too_small_draws_nothing() {
        let area = Rect { x: 0, y: 0, width: 5, height: 3 };
        let mut buf = Buffer::empty(area);
        render_theme_picker(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, &ThemePickerState::open("nc-classic"), "nc-classic");
        let text: String = (0..area.height).flat_map(|y| (0..area.width).map(move |x| (x, y))).map(|(x, y)| buf[(x, y)].symbol()).collect();
        assert!(text.trim().is_empty());
    }
}
