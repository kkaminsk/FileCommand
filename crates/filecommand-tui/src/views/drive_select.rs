//! The Alt+F1/F2 drive-select dialog.
//!
//! Drive letters are known synchronously and render on the first frame;
//! each label column stays blank until its worker fetch resolves, then
//! fills in place without moving any other row.

use filecommand_core::dialogs::overlay_rect;
use filecommand_core::drives::DriveSelect;
use filecommand_core::listing::{pad_to_width, truncate_with_ellipsis};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const TITLE: &str = " Drive ";
/// `" C: "` — the letter column.
const LETTER_W: usize = 4;
/// Widest volume label the column shows before truncating.
const LABEL_W: usize = 12;
const MIN_INNER_W: usize = 10;

fn inner_width() -> usize {
    LETTER_W + LABEL_W + 1
}

pub fn render_drive_select(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, dialog: &DriveSelect) {
    let body_style = role_style(theme, Role::MenuBody, depth);
    let highlight_style = role_style(theme, Role::MenuHighlight, depth);

    let preferred_inner_w = inner_width();
    // Two frame rows plus one row per drive; an empty list still draws the
    // (empty) box rather than nothing at all.
    let preferred_h = dialog.drives.len() as u16 + 2;
    let r = overlay_rect((preferred_inner_w as u16 + 2, preferred_h), (MIN_INNER_W as u16, 3), (area.width, area.height));
    let box_h = r.height;
    let inner_w = r.width.saturating_sub(2) as usize;
    let visible_rows = box_h.saturating_sub(2) as usize;
    let x = area.x + r.x;
    let y = area.y + r.y;

    let mut top = format!("\u{250C}{}\u{2510}", "\u{2500}".repeat(inner_w));
    // Set the title into the top border, NC-style.
    if inner_w > TITLE.chars().count() {
        let offset = 1 + (inner_w - TITLE.chars().count()) / 2;
        top = replace_at(&top, offset, TITLE);
    }
    buf.set_string(x, y, &top, body_style);

    for (i, drive) in dialog.drives.iter().take(visible_rows).enumerate() {
        let ry = y + 1 + i as u16;
        let letter = format!(" {}: ", drive.letter.to_ascii_uppercase());
        // `None` is "still fetching" and renders blank — an unlabelled
        // volume resolves to an empty string and also renders blank, which
        // is the same thing to the eye and correct either way.
        let label = drive.label.clone().unwrap_or_default();
        let label_w = inner_w.saturating_sub(LETTER_W);
        let row = format!("{}{}", pad_to_width(&letter, LETTER_W.min(inner_w)), pad_to_width(&truncate_with_ellipsis(&label, label_w), label_w));
        let style = if i == dialog.selected { highlight_style } else { body_style };
        buf.set_string(x, ry, "\u{2502}", body_style);
        buf.set_string(x + 1, ry, pad_to_width(&row, inner_w), style);
        buf.set_string(x + 1 + inner_w as u16, ry, "\u{2502}", body_style);
    }

    buf.set_string(x, y + box_h - 1, format!("\u{2514}{}\u{2518}", "\u{2500}".repeat(inner_w)), body_style);
}

/// Overwrite `count` characters of `s` starting at char offset `at`.
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
    use filecommand_core::PanelSide;

    fn render(dialog: &DriveSelect) -> Vec<String> {
        let area = Rect { x: 0, y: 0, width: 40, height: 12 };
        let mut buf = Buffer::empty(area);
        render_drive_select(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, dialog);
        (0..area.height).map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect()).collect()
    }

    #[test]
    fn every_letter_renders_before_any_label_is_known() {
        let dialog = DriveSelect::new(PanelSide::Left, vec!['A', 'C', 'D'], None);
        let text = render(&dialog).join("\n");
        for letter in ["A:", "C:", "D:"] {
            assert!(text.contains(letter), "`{letter}` missing from:\n{text}");
        }
    }

    #[test]
    fn a_resolved_label_appears_without_shifting_other_rows() {
        let mut dialog = DriveSelect::new(PanelSide::Left, vec!['A', 'C', 'D'], None);
        let before = render(&dialog);
        dialog.apply_label('C', Some("OS".to_string()));
        let after = render(&dialog);

        let row_of = |rows: &[String], letter: &str| rows.iter().position(|r| r.contains(letter)).unwrap();
        assert_eq!(row_of(&before, "A:"), row_of(&after, "A:"), "A: moved");
        assert_eq!(row_of(&before, "D:"), row_of(&after, "D:"), "D: moved");
        assert!(after[row_of(&after, "C:")].contains("OS"));
        assert!(!after[row_of(&after, "A:")].contains("OS"));
    }

    #[test]
    fn a_pending_label_column_is_blank_not_a_placeholder() {
        let dialog = DriveSelect::new(PanelSide::Left, vec!['A'], None);
        let rows = render(&dialog);
        let row = rows.iter().find(|r| r.contains("A:")).unwrap();
        // Only the frame and the letter — no `…`, no spinner.
        assert!(!row.contains('\u{2026}'), "the label column must be blank while pending: `{row}`");
        assert_eq!(row.trim_matches(|c| c == ' ' || c == '\u{2502}'), "A:");
    }

    #[test]
    fn the_selected_row_is_the_one_the_cursor_is_on() {
        let mut dialog = DriveSelect::new(PanelSide::Left, vec!['A', 'C'], None);
        dialog.move_selection(1);
        assert_eq!(dialog.selected_letter(), Some('C'));
        // The highlight is a style, not a glyph; assert the row exists and
        // that styling differs from the body role.
        let text = render(&dialog).join("\n");
        assert!(text.contains("C:"));
        let theme = Theme::classic();
        assert_ne!(
            role_style(&theme, Role::MenuHighlight, ColorDepth::Ansi16),
            role_style(&theme, Role::MenuBody, ColorDepth::Ansi16)
        );
    }

    #[test]
    fn the_box_is_single_line_framed_and_titled() {
        let rows = render(&DriveSelect::new(PanelSide::Left, vec!['C'], None));
        let text = rows.join("\n");
        assert!(text.contains("Drive"), "the title is set into the top border");
        assert!(text.contains('\u{250C}') && text.contains('\u{2518}'));
    }

    #[test]
    fn a_screen_too_small_for_the_preferred_box_clamps_and_still_draws() {
        // Under the unified overlay-geometry rule a too-small screen no
        // longer suppresses the dialog entirely — it clamps to fit and
        // stays fully on-screen (responsive-layout "Unified overlay
        // geometry"), inverting the old "draws nothing" premise.
        let area = Rect { x: 0, y: 0, width: 8, height: 3 };
        let mut buf = Buffer::empty(area);
        let dialog = DriveSelect::new(PanelSide::Left, vec!['C'], None);
        render_drive_select(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, &dialog);
        let text: String = (0..area.height).flat_map(|y| (0..area.width).map(move |x| (x, y))).map(|(x, y)| buf[(x, y)].symbol()).collect();
        assert!(!text.trim().is_empty(), "the clamped dialog must still draw something");
        assert!(text.contains('\u{250C}') && text.contains('\u{2518}'), "the frame stays fully on-screen: `{text}`");
    }
}
