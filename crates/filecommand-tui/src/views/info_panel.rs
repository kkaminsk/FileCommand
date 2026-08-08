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
    // and the About dialog use. No label/value split applies here, so the
    // "value" half of each row is empty.
    let banner: Vec<(String, String)> = identity_lines.iter().map(|l| (center(l, inner_w), String::new())).collect();
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

/// `label` left, value right-aligned. Returns each row's full text paired
/// with the (possibly truncated) value text alone, so the caller can find
/// the label/value split by measuring the value's own display width — it is
/// always flush against the box's right inner edge — rather than searching
/// the row for a run of spaces, which collapses to a single column (and so
/// becomes unfindable) once the value is long enough to butt right up
/// against the label.
fn box_rows(info_box: &InfoBox, inner_w: usize) -> Vec<(String, String)> {
    info_box
        .fields
        .iter()
        .map(|field| {
            let label = format!(" {}", field.label);
            let label_w = display_width(&label);
            // At least one column must separate label and value; if the
            // label alone doesn't leave room for that, the value is dropped
            // rather than colliding with the label.
            let available_for_value = inner_w.saturating_sub(label_w + 1);
            let value = truncate_with_ellipsis(&field.value, available_for_value);
            let value_w = display_width(&value);
            let gap = " ".repeat(inner_w.saturating_sub(label_w + value_w));
            let row = pad_to_width(&format!("{label}{gap}{value}"), inner_w);
            (row, value)
        })
        .collect()
}

/// Truncate `s` to at most `max_w` display columns, replacing the tail with
/// a single `…` when it doesn't fit — never silently dropping characters
/// without signaling that it happened.
fn truncate_with_ellipsis(s: &str, max_w: usize) -> String {
    if display_width(s) <= max_w {
        return s.to_string();
    }
    if max_w == 0 {
        return String::new();
    }
    if max_w == 1 {
        return "\u{2026}".to_string();
    }
    let budget = max_w - 1; // reserve one column for the ellipsis itself
    let mut out = String::new();
    let mut acc = 0usize;
    for ch in s.chars() {
        let cw = display_width(&ch.to_string());
        if acc + cw > budget {
            break;
        }
        out.push(ch);
        acc += cw;
    }
    out.push('\u{2026}');
    out
}

/// Draw one framed box and return the y directly below it. Each row pairs
/// its full text with its (possibly empty) value substring; `value_style`
/// is applied to exactly that substring, right-flush against the box's
/// inner edge — a structural split, not a re-derived one, so it stays
/// correct even when the gap between label and value has collapsed to a
/// single column.
#[allow(clippy::too_many_arguments)]
fn draw_box(
    buf: &mut Buffer,
    x: u16,
    mut y: u16,
    bottom: u16,
    inner_w: usize,
    title: Option<&str>,
    rows: &[(String, String)],
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

    for (row, value) in rows {
        if y + 1 >= bottom {
            break;
        }
        buf.set_string(x, y, "\u{2502}", frame_style);
        buf.set_string(x + 1, y, row, text_style);
        // Re-style the value half, right-flush against the inner edge.
        if !value.is_empty() {
            let col = x + 1 + (inner_w - display_width(value)) as u16;
            buf.set_string(col, y, value, value_style);
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
    use filecommand_core::info::{InfoField, PENDING};

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

    // -------------------------------------------------------------------
    // Value truncation (long values at a narrow width)
    // -------------------------------------------------------------------

    #[test]
    fn truncate_with_ellipsis_leaves_short_values_untouched() {
        assert_eq!(truncate_with_ellipsis("OK", 10), "OK");
        assert_eq!(truncate_with_ellipsis("exact", 5), "exact");
    }

    #[test]
    fn truncate_with_ellipsis_cuts_the_tail_and_signals_it() {
        let out = truncate_with_ellipsis("A very long volume label indeed", 10);
        assert_eq!(display_width(&out), 10);
        assert!(out.ends_with('\u{2026}'), "`{out}`");
        assert!(out.starts_with("A very lo"), "`{out}`");
    }

    #[test]
    fn truncate_with_ellipsis_handles_degenerate_widths() {
        assert_eq!(truncate_with_ellipsis("anything", 0), "");
        assert_eq!(truncate_with_ellipsis("anything", 1), "\u{2026}");
    }

    #[test]
    fn box_rows_truncates_the_value_not_the_label_when_the_gap_collapses() {
        let info_box = InfoBox {
            title: "Drive C:".to_string(),
            fields: vec![InfoField { label: "Volume label".to_string(), value: "A very long volume label indeed".to_string() }],
        };
        // Just wide enough for the label plus a one-column gap plus a
        // handful of value columns — the exact scenario that used to make
        // `row.rfind("  ")` fail to find any split at all.
        let rows = box_rows(&info_box, 24);
        assert_eq!(rows.len(), 1);
        let (row, value) = &rows[0];
        assert_eq!(display_width(row), 24, "the row is still padded to the full inner width");
        assert!(row.starts_with(" Volume label"), "the label survives intact: `{row}`");
        assert!(value.ends_with('\u{2026}'), "the value carries the ellipsis: `{value}`");
        assert!(row.ends_with(value.as_str()), "the (truncated) value is flush against the right edge: `{row}`");
    }

    #[test]
    fn a_long_volume_label_at_a_narrow_width_renders_with_an_ellipsis_and_keeps_its_label() {
        let area = Rect { x: 0, y: 0, width: 26, height: 22 };
        let mut buf = Buffer::empty(area);
        let values = InfoValues { volume_label: Some("A very long volume label indeed".to_string()), ..InfoValues::default() };
        render_info(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, &values, Path::new(r"C:\"), &identity());
        let text: String = (0..area.height)
            .map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Volume label"), "the label is not eaten by the truncated value:\n{text}");
        assert!(text.contains('\u{2026}'), "the overlong value is truncated with an ellipsis:\n{text}");
        assert!(!text.contains("A very long volume label indeed"), "the full untruncated value must not appear:\n{text}");
    }

    #[test]
    fn the_value_role_still_applies_to_a_truncated_value() {
        // Regression check for the label/value color split breaking once
        // the gap between them collapses to one space.
        let area = Rect { x: 0, y: 0, width: 26, height: 22 };
        let mut buf = Buffer::empty(area);
        let values = InfoValues { volume_label: Some("A very long volume label indeed".to_string()), ..InfoValues::default() };
        render_info(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, &values, Path::new(r"C:\"), &identity());

        let theme = Theme::classic();
        let value_style = role_style(&theme, Role::InfoValue, ColorDepth::Ansi16);
        let label_style = role_style(&theme, Role::InfoLabel, ColorDepth::Ansi16);

        // Row 6 is the "Volume label" row of the Drive box (banner: 4 lines
        // + top border; System box: label + top/bottom; Drive box title +
        // this is its first field row) — rather than hardcode that, scan
        // for the row containing the label and assert its rightmost cell
        // (the end of the truncated, ellipsis-terminated value) carries the
        // value role while the label's own cell carries the label role.
        for y in 0..area.height {
            let row: String = (0..area.width).map(|x| buf[(x, y)].symbol()).collect();
            if row.contains("Volume label") {
                let last_cell = &buf[(area.x + area.width - 2, y)]; // inside the right frame glyph
                assert_eq!(Some(last_cell.fg), value_style.fg, "the truncated value's trailing cell should carry the value role");
                let label_cell = &buf[(area.x + 2, y)]; // inside "Volume label"'s first letter
                assert_eq!(Some(label_cell.fg), label_style.fg, "the label's own cell should carry the label role");
                return;
            }
        }
        panic!("Volume label row not found:\n{}", (0..area.height).map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect::<String>()).collect::<Vec<_>>().join("\n"));
    }
}
