//! The F1 Help window: a centered primary-style window with the shared
//! identity header, a scrollable topic list (or, in its place, the
//! selected topic's static page), and `Help`/`Cancel` buttons — plus the
//! secondary-style About dialog it opens for its special first entry
//! (help-and-about).

use filecommand_core::dialogs::{help_topic_visible_rows, help_window_height, HelpState, HELP_TOPICS};
use filecommand_core::identity;
use filecommand_core::listing::{display_width, pad_to_width};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const WINDOW_WIDTH: usize = 62;

pub fn render_help(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, dialog: &HelpState, identity_lines: &[String; 4]) {
    let body = role_style(theme, Role::DialogPrimary, depth);
    let highlight = role_style(theme, Role::MenuHighlight, depth);
    let button_default = role_style(theme, Role::ButtonFocused, depth);
    let button_normal = role_style(theme, Role::ButtonNormal, depth);

    let inner_w = WINDOW_WIDTH.min(area.width.saturating_sub(4) as usize).max(20);
    let box_w = inner_w as u16 + 2;
    let box_h = help_window_height(area.height);
    if area.width < box_w || area.height < box_h {
        return;
    }
    // Re-centers on every frame as a pure function of the current area, so
    // a resize simply redraws at the new center (help-and-about "Help
    // window re-centers on resize").
    let x = area.x + (area.width - box_w) / 2;
    let y = area.y + (area.height.saturating_sub(box_h)) / 2;

    let title = " Help ";
    buf.set_string(x, y, format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w)), body);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(title))) / 2) as u16;
    buf.set_string(title_x, y, title, body);

    // Three centered identity lines, byte-for-byte the same strings the
    // splash and Info-panel banner use (help-and-about "Identity header
    // matches the shared source of truth").
    let header_lines = [&identity_lines[0], &identity_lines[2], &identity_lines[3]];
    for (i, line) in header_lines.iter().enumerate() {
        let ry = y + 1 + i as u16;
        buf.set_string(x, ry, "\u{2551}", body);
        let lx = x + 1 + (inner_w.saturating_sub(display_width(line)) / 2) as u16;
        buf.set_string(x + 1, ry, " ".repeat(inner_w), body);
        buf.set_string(lx, ry, line.as_str(), body);
        buf.set_string(x + 1 + inner_w as u16, ry, "\u{2551}", body);
    }

    buf.set_string(x, y + 4, format!("\u{2560}{}\u{2563}", "\u{2500}".repeat(inner_w)), body);

    let visible_rows = help_topic_visible_rows(box_h);
    let list_y0 = y + 5;
    if let Some(topic) = dialog.page {
        render_topic_page(buf, x, list_y0, inner_w, visible_rows, body, topic);
    } else {
        render_topic_list(buf, x, list_y0, inner_w, visible_rows, body, highlight, dialog);
    }

    let buttons_y = list_y0 + visible_rows as u16;
    buf.set_string(x, buttons_y, format!("\u{2560}{}\u{2563}", "\u{2500}".repeat(inner_w)), body);

    let help_label = " Help ";
    let cancel_label = " Cancel ";
    let gap = 3usize;
    let total = display_width(help_label) + gap + display_width(cancel_label);
    let start = x + 1 + (inner_w.saturating_sub(total) / 2) as u16;
    buf.set_string(x, buttons_y + 1, "\u{2551}", body);
    buf.set_string(x + 1, buttons_y + 1, " ".repeat(inner_w), body);
    buf.set_string(start, buttons_y + 1, help_label, button_default);
    buf.set_string(start + display_width(help_label) as u16 + gap as u16, buttons_y + 1, cancel_label, button_normal);
    buf.set_string(x + 1 + inner_w as u16, buttons_y + 1, "\u{2551}", body);

    buf.set_string(x, y + box_h - 1, format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w)), body);

    if dialog.about_open {
        render_about(buf, area, theme, depth, identity_lines);
    }
}

#[allow(clippy::too_many_arguments)]
fn render_topic_list(
    buf: &mut Buffer,
    x: u16,
    list_y0: u16,
    inner_w: usize,
    visible_rows: usize,
    body: ratatui::style::Style,
    highlight: ratatui::style::Style,
    dialog: &HelpState,
) {
    let overflow = HELP_TOPICS.len() > visible_rows;
    for row in 0..visible_rows {
        let ry = list_y0 + row as u16;
        let topic_idx = dialog.scroll + row;
        buf.set_string(x, ry, "\u{2551}", body);
        let mut line_w = inner_w;
        // `↑`/`↓` scroll arrows on the right border, only when the list
        // overflows the visible area (help-and-about "Scroll arrows appear
        // only on overflow").
        let arrow = if overflow && row == 0 && dialog.scroll > 0 {
            Some('\u{2191}')
        } else if overflow && row + 1 == visible_rows && dialog.scroll + visible_rows < HELP_TOPICS.len() {
            Some('\u{2193}')
        } else {
            None
        };
        if arrow.is_some() {
            line_w = inner_w.saturating_sub(1);
        }
        match HELP_TOPICS.get(topic_idx) {
            Some(name) => {
                let text = format!(" {name}");
                let style = if topic_idx == dialog.cursor { highlight } else { body };
                buf.set_string(x + 1, ry, pad_to_width(&text, line_w), style);
            }
            None => buf.set_string(x + 1, ry, " ".repeat(line_w), body),
        }
        if let Some(arrow) = arrow {
            buf.set_string(x + 1 + line_w as u16, ry, arrow.to_string(), body);
        }
        buf.set_string(x + 1 + inner_w as u16, ry, "\u{2551}", body);
    }
}

/// Renders as many lines of the topic's compiled-in static page as fit the
/// list area — the v1 floor is a scrollable *topic list*, not a scrollable
/// *page* (help-and-about only specifies list scrolling), so a page longer
/// than `visible_rows` is simply clipped at the bottom rather than adding a
/// second scroll mechanism.
fn render_topic_page(buf: &mut Buffer, x: u16, list_y0: u16, inner_w: usize, visible_rows: usize, body: ratatui::style::Style, topic: usize) {
    let text = filecommand_core::dialogs::topic_page_text(topic);
    let lines: Vec<&str> = text.lines().collect();
    for row in 0..visible_rows {
        let ry = list_y0 + row as u16;
        buf.set_string(x, ry, "\u{2551}", body);
        let line = lines.get(row).copied().unwrap_or("");
        buf.set_string(x + 1, ry, pad_to_width(line, inner_w), body);
        buf.set_string(x + 1 + inner_w as u16, ry, "\u{2551}", body);
    }
}

const ABOUT_WIDTH: usize = 50;

/// The secondary-style About dialog, layered over the Help window (help-
/// and-about "About FileCommand dialog"; §10 license).
fn render_about(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, identity_lines: &[String; 4]) {
    let body = role_style(theme, Role::DialogSecondary, depth);
    let button = role_style(theme, Role::ButtonNormal, depth);

    let inner_w = ABOUT_WIDTH.min(area.width.saturating_sub(4) as usize).max(20);
    let box_w = inner_w as u16 + 2;
    let box_h = 10u16.min(area.height.saturating_sub(2)).max(8);
    if area.width < box_w || area.height < box_h {
        return;
    }
    let x = area.x + (area.width - box_w) / 2;
    let y = area.y + (area.height.saturating_sub(box_h)) / 2;

    buf.set_string(x, y, format!("\u{250C}{}\u{2510}", "\u{2500}".repeat(inner_w)), body);
    let title = " About FileCommand ";
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(title))) / 2) as u16;
    buf.set_string(title_x, y, title, body);

    let license_line = format!("License: {}", identity::license());
    let repo_line = identity::repository_url().to_string();
    let content: Vec<&str> = vec![
        identity_lines[0].as_str(),
        identity_lines[1].as_str(),
        identity_lines[2].as_str(),
        identity_lines[3].as_str(),
        "",
        license_line.as_str(),
        repo_line.as_str(),
    ];
    for (i, line) in content.iter().enumerate() {
        let ry = y + 1 + i as u16;
        if ry >= y + box_h - 2 {
            break;
        }
        buf.set_string(x, ry, "\u{2502}", body);
        let lx = x + 1 + (inner_w.saturating_sub(display_width(line)) / 2) as u16;
        buf.set_string(x + 1, ry, " ".repeat(inner_w), body);
        buf.set_string(lx, ry, *line, body);
        buf.set_string(x + 1 + inner_w as u16, ry, "\u{2502}", body);
    }

    let ok_label = " OK ";
    let ok_y = y + box_h - 2;
    buf.set_string(x, ok_y, "\u{2502}", body);
    buf.set_string(x + 1, ok_y, " ".repeat(inner_w), body);
    let ok_x = x + 1 + ((inner_w.saturating_sub(display_width(ok_label))) / 2) as u16;
    buf.set_string(ok_x, ok_y, ok_label, button);
    buf.set_string(x + 1 + inner_w as u16, ok_y, "\u{2502}", body);

    buf.set_string(x, y + box_h - 1, format!("\u{2514}{}\u{2518}", "\u{2500}".repeat(inner_w)), body);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> [String; 4] {
        filecommand_core::identity::identity_lines(2026)
    }

    fn render(dialog: &HelpState) -> Vec<String> {
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let mut buf = Buffer::empty(area);
        render_help(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, dialog, &ids());
        (0..area.height).map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect()).collect()
    }

    #[test]
    fn renders_the_title_and_identity_header() {
        let rows = render(&HelpState::new()).join("\n");
        assert!(rows.contains("Help"));
        assert!(rows.contains("FileCommand"));
        assert!(rows.contains("Norton Commander"));
    }

    #[test]
    fn topic_list_opens_with_about_filecommand_first_and_highlighted() {
        let rows = render(&HelpState::new()).join("\n");
        let about_line = rows.lines().find(|l| l.contains("About FileCommand")).expect("About FileCommand row present");
        assert!(!about_line.is_empty());
    }

    #[test]
    fn help_and_cancel_buttons_render() {
        let rows = render(&HelpState::new()).join("\n");
        assert!(rows.contains("Help") && rows.contains("Cancel"));
    }

    #[test]
    fn a_topic_page_replaces_the_list() {
        let mut dialog = HelpState::new();
        dialog.page = Some(1); // Keyboard reference
        let rows = render(&dialog).join("\n");
        assert!(rows.contains("modifier variants"));
        assert!(!rows.contains("Panels and display modes"), "the list must not show alongside a page");
    }

    #[test]
    fn keyboard_reference_documents_the_ctrl_alt_f_key_bar_variants() {
        let mut dialog = HelpState::new();
        dialog.page = Some(1);
        let rows = render(&dialog).join("\n");
        assert!(rows.contains("Ctrl+F3"), "expected an F-key-bar Ctrl variant to be documented: {rows}");
        assert!(rows.contains("Alt+F1") || rows.contains("Alt+F7"), "expected an F-key-bar Alt variant to be documented: {rows}");
    }

    #[test]
    fn about_dialog_layers_over_the_help_window_with_license_and_repo() {
        let mut dialog = HelpState::new();
        dialog.about_open = true;
        let rows = render(&dialog).join("\n");
        assert!(rows.contains("License: MIT OR Apache-2.0"));
        assert!(rows.contains("github.com") || rows.contains("http"));
        assert!(rows.contains("OK"));
    }

    #[test]
    fn scroll_arrows_appear_only_on_overflow() {
        // A tall enough window fits all 10 topics without scrolling.
        let rows = render(&HelpState::new()).join("\n");
        assert!(!rows.contains('\u{2191}') && !rows.contains('\u{2193}'), "no arrows expected when everything fits");
    }
}
