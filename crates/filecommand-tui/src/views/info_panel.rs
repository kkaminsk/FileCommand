//! The Ctrl+L Info display mode.
//!
//! Renders inside a panel's double-line border as a vertical stack of
//! single-line-framed boxes: the shared identity banner first, then the
//! system / drive / directory figures. Unresolved values show the static
//! `…` glyph — never a spinner.

use filecommand_core::drives::drive_letter_of;
use filecommand_core::info::{info_boxes, InfoBox, InfoValues};
use filecommand_core::listing::{display_width, pad_to_width};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use std::path::Path;

use crate::style::role_style;

/// Render the Info body into `area`, which is the panel's interior (inside
/// its double-line border, below the header row).
pub fn render_info(
    buf: &mut Buffer,
    area: Rect,
    theme: &Theme,
    depth: ColorDepth,
    values: &InfoValues,
    cwd: &Path,
    identity_lines: &[String; 4],
) {
    if area.width < 6 || area.height == 0 {
        return;
    }
    let label_style = role_style(theme, Role::InfoLabel, depth);
    let value_style = role_style(theme, Role::InfoValue, depth);
    let banner_style = role_style(theme, Role::InfoBanner, depth);

    let w = area.width as usize;
    let inner_w = w - 2;
    let mut y = area.y;
    let bottom = area.y + area.height;

    // Banner box: the identity lines verbatim, the same strings the splash
    // and the About dialog use.
    let banner: Vec<String> = identity_lines.iter().map(|l| center(l, inner_w)).collect();
    y = draw_box(buf, area.x, y, bottom, inner_w, None, &banner, label_style, banner_style, banner_style);

    for info_box in info_boxes(values, drive_letter_of(cwd)) {
        if y >= bottom {
            break;
        }
        let rows = box_rows(&info_box, inner_w);
        y = draw_box(buf, area.x, y, bottom, inner_w, Some(&info_box.title), &rows, label_style, label_style, value_style);
    }

    // Paint the remaining interior so it picks up the Info background
    // rather than whatever the previous display mode left behind.
    while y < bottom {
        buf.set_string(area.x, y, " ".repeat(w), label_style);
        y += 1;
    }
}

/// `label` left, value right-aligned, with the split point recorded so the
/// two halves can take different roles.
fn box_rows(info_box: &InfoBox, inner_w: usize) -> Vec<String> {
    info_box
        .fields
        .iter()
        .map(|field| {
            let label = format!(" {}", field.label);
            let used = display_width(&label) + display_width(&field.value) + 1;
            let gap = " ".repeat(inner_w.saturating_sub(used).max(1));
            pad_to_width(&format!("{label}{gap}{}", field.value), inner_w)
        })
        .collect()
}

/// Draw one framed box and return the y directly below it. `value_style`
/// applies from the first run of two spaces onward, which is where a
/// right-aligned value begins.
#[allow(clippy::too_many_arguments)]
fn draw_box(
    buf: &mut Buffer,
    x: u16,
    mut y: u16,
    bottom: u16,
    inner_w: usize,
    title: Option<&str>,
    rows: &[String],
    frame_style: ratatui::style::Style,
    text_style: ratatui::style::Style,
    value_style: ratatui::style::Style,
) -> u16 {
    if y >= bottom {
        return y;
    }
    let mut top = format!("\u{250C}{}\u{2510}", "\u{2500}".repeat(inner_w));
    if let Some(title) = title {
        let titled = format!(" {title} ");
        if inner_w > titled.chars().count() + 2 {
            top = replace_at(&top, 2, &titled);
        }
    }
    buf.set_string(x, y, &top, frame_style);
    y += 1;

    for row in rows {
        if y + 1 >= bottom {
            break;
        }
        buf.set_string(x, y, "\u{2502}", frame_style);
        buf.set_string(x + 1, y, row, text_style);
        // Re-style the value half. Splitting on the padded gap keeps the
        // label cyan and the figure bright-yellow without threading a
        // second string through.
        if let Some(split) = row.rfind("  ") {
            let value = row[split..].trim_start();
            if !value.is_empty() {
                let col = x + 1 + display_width(&row[..row.len() - value.len()]) as u16;
                buf.set_string(col, y, value, value_style);
            }
        }
        buf.set_string(x + 1 + inner_w as u16, y, "\u{2502}", frame_style);
        y += 1;
    }

    if y < bottom {
        buf.set_string(x, y, format!("\u{2514}{}\u{2518}", "\u{2500}".repeat(inner_w)), frame_style);
        y += 1;
    }
    y
}

fn center(s: &str, width: usize) -> String {
    let w = display_width(s);
    if w >= width {
        return pad_to_width(s, width);
    }
    let left = (width - w) / 2;
    pad_to_width(&format!("{}{s}", " ".repeat(left)), width)
}

fn replace_at(s: &str, at: usize, insert: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out: String = chars[..at.min(chars.len())].iter().collect();
    out.push_str(insert);
    let resume = at + insert.chars().count();
    if resume < chars.len() {
        out.extend(&chars[resume..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use filecommand_core::info::PENDING;

    fn identity() -> [String; 4] {
        [
            "FileCommand".to_string(),
            "Version 0.1.0".to_string(),
            "Copyright (C) 2026 The FileCommand Authors".to_string(),
            "Inspired by the Norton Commander, 1986-1998".to_string(),
        ]
    }

    fn render(values: &InfoValues, cwd: &str) -> String {
        let area = Rect { x: 0, y: 0, width: 46, height: 22 };
        let mut buf = Buffer::empty(area);
        render_info(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, values, Path::new(cwd), &identity());
        (0..area.height)
            .map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn banner_uses_the_shared_identity_lines_verbatim() {
        let text = render(&InfoValues::default(), r"C:\Users");
        for line in identity() {
            // Long lines are centered and may be clipped to the panel
            // width; check the leading run survives.
            let head: String = line.chars().take(20).collect();
            assert!(text.contains(&head), "identity line `{line}` missing from:\n{text}");
        }
    }

    #[test]
    fn unresolved_values_render_as_the_static_ellipsis() {
        let text = render(&InfoValues::default(), r"C:\Users");
        assert!(text.contains(PENDING), "pending values show `…`");
        // Every async field is pending, so there is one `…` per field.
        assert_eq!(text.matches(PENDING).count(), 7, "one placeholder per async field in:\n{text}");
    }

    #[test]
    fn a_resolved_value_replaces_its_placeholder_in_place() {
        let values = InfoValues { file_count: Some(42), ..InfoValues::default() };
        let text = render(&values, r"C:\Users");
        assert!(text.contains("42"));
        assert_eq!(text.matches(PENDING).count(), 6, "only the resolved field lost its placeholder");
        // The other rows keep their labels and positions.
        assert!(text.contains("Directories"));
        assert!(text.contains("Volume label"));
    }

    #[test]
    fn every_content_field_is_labelled() {
        let text = render(&InfoValues::default(), r"C:\Users");
        for label in ["Memory free", "Volume label", "Serial number", "Total space", "Free space", "Files", "Directories"] {
            assert!(text.contains(label), "`{label}` missing from:\n{text}");
        }
    }

    #[test]
    fn boxes_are_stacked_and_single_line_framed() {
        let text = render(&InfoValues::default(), r"C:\Users");
        let tops = text.matches('\u{250C}').count();
        let bottoms = text.matches('\u{2514}').count();
        assert_eq!(tops, 4, "banner + system + drive + directory boxes");
        assert_eq!(tops, bottoms, "every box is closed");
    }

    #[test]
    fn the_drive_box_is_titled_with_the_panel_s_drive() {
        assert!(render(&InfoValues::default(), r"D:\work").contains("Drive D:"));
        assert!(render(&InfoValues::default(), r"\\server\share").contains("Drive"), "a UNC path still gets a drive box");
    }

    #[test]
    fn no_animated_or_non_cp437_glyphs_are_emitted() {
        let text = render(&InfoValues::default(), r"C:\Users");
        for ch in text.chars() {
            assert!(
                ch.is_ascii() || "\u{250C}\u{2510}\u{2514}\u{2518}\u{2502}\u{2500}\u{2026}".contains(ch),
                "non-CP437 glyph `{ch}` (U+{:04X}) in the Info panel",
                ch as u32
            );
        }
    }

    #[test]
    fn a_short_panel_truncates_rather_than_overflowing() {
        let area = Rect { x: 0, y: 0, width: 40, height: 4 };
        let mut buf = Buffer::empty(area);
        render_info(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, &InfoValues::default(), Path::new(r"C:\"), &identity());
        // Nothing was written outside the area — `Buffer` would have
        // panicked on an out-of-bounds write, so reaching here is the
        // assertion. Confirm it still drew something.
        let text: String = (0..area.height).flat_map(|y| (0..area.width).map(move |x| (x, y))).map(|(x, y)| buf[(x, y)].symbol()).collect();
        assert!(text.contains('\u{250C}'));
    }

    #[test]
    fn a_panel_too_narrow_for_a_box_draws_nothing() {
        let area = Rect { x: 0, y: 0, width: 4, height: 10 };
        let mut buf = Buffer::empty(area);
        render_info(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, &InfoValues::default(), Path::new(r"C:\"), &identity());
        let text: String = (0..area.height).flat_map(|y| (0..area.width).map(move |x| (x, y))).map(|(x, y)| buf[(x, y)].symbol()).collect();
        assert!(text.trim().is_empty());
    }
}
