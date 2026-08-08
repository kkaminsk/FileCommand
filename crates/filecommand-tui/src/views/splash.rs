//! Startup splash: solid backdrop plus a centered double-line box with the
//! product identity lines.

use filecommand_core::listing::display_width;
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const BOX_W: u16 = 48;
const BOX_H: u16 = 9;
const INNER_W: usize = (BOX_W - 2) as usize;

pub fn render_splash(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, identity_lines: &[String; 4]) {
    let backdrop = role_style(theme, Role::ScreenBackdrop, depth);
    for y in area.y..area.y + area.height {
        buf.set_string(area.x, y, " ".repeat(area.width as usize), backdrop);
    }

    if area.width < BOX_W || area.height < BOX_H {
        return;
    }
    let frame_style = role_style(theme, Role::SplashFrame, depth);
    let title_style = role_style(theme, Role::SplashTitle, depth);
    let version_style = role_style(theme, Role::SplashVersion, depth);
    let text_style = role_style(theme, Role::SplashText, depth);

    let box_x = area.x + (area.width - BOX_W) / 2;
    let box_y = area.y + (area.height - BOX_H) / 2;

    let top = format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(INNER_W));
    let bottom = format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(INNER_W));
    let blank = format!("\u{2551}{}\u{2551}", " ".repeat(INNER_W));

    buf.set_string(box_x, box_y, &top, frame_style);
    buf.set_string(box_x, box_y + 1, &blank, frame_style);
    write_row(buf, box_x, box_y + 2, &identity_lines[0], title_style, frame_style);
    write_row(buf, box_x, box_y + 3, &identity_lines[1], version_style, frame_style);
    buf.set_string(box_x, box_y + 4, &blank, frame_style);
    write_row(buf, box_x, box_y + 5, &identity_lines[2], text_style, frame_style);
    write_row(buf, box_x, box_y + 6, &identity_lines[3], text_style, frame_style);
    buf.set_string(box_x, box_y + 7, &blank, frame_style);
    buf.set_string(box_x, box_y + 8, &bottom, frame_style);
}

fn write_row(buf: &mut Buffer, box_x: u16, y: u16, text: &str, text_style: ratatui::style::Style, frame_style: ratatui::style::Style) {
    buf.set_string(box_x, y, "\u{2551}", frame_style);
    let pad = INNER_W.saturating_sub(display_width(text));
    let left = pad / 2;
    let right = pad - left;
    let line = format!("{}{}{}", " ".repeat(left), text, " ".repeat(right));
    buf.set_string(box_x + 1, y, &line, text_style);
    buf.set_string(box_x + 1 + INNER_W as u16, y, "\u{2551}", frame_style);
}
