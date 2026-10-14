//! Overwrite-conflict dialog: source vs. target size/date, the
//! Overwrite/Skip/Rename/Overwrite All/Skip All choices, and — on its own
//! row below them — Cancel (operation-dialogs "Overwrite-conflict dialog";
//! process-lifecycle-hardening design D7).

use filecommand_core::dialogs::overlay_rect;
use filecommand_core::fs_ops::ConflictInfo;
use filecommand_core::line_edit::LineEdit;
use filecommand_core::listing::{display_width, format_date, format_size, format_time, pad_to_width, truncate_with_ellipsis};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use filecommand_core::update::ButtonId;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const BOX_INNER_W: usize = 46;
const MIN_INNER_W: usize = 30;
/// Frame rows: title, "already exists", Source, Target, blank, the choice
/// row (or the rename field), the Cancel row, blank, bottom border. One
/// taller than before the Cancel row existed (design D7).
const BOX_H: u16 = 9;
const CHOICES_ROW: &str = "(O)verwrite  (S)kip  (R)ename  Over(w)rite All  Skip (A)ll";
const CANCEL_ROW: &str = "(C)ancel";

/// Renders the dialog. Returns the terminal cursor's `(x, y)` position
/// while the in-place rename field is showing (it is a shared text field
/// per text-field-editing, so the cursor sits at its caret), `None`
/// otherwise.
pub fn render_conflict(
    buf: &mut Buffer,
    area: Rect,
    theme: &Theme,
    depth: ColorDepth,
    info: &ConflictInfo,
    rename_input: &Option<LineEdit>,
) -> Option<(u16, u16)> {
    let body = role_style(theme, Role::DialogPrimary, depth);
    let field_style = role_style(theme, Role::DialogInput, depth);

    let r = overlay_rect((BOX_INNER_W as u16 + 2, BOX_H), (MIN_INNER_W as u16 + 2, BOX_H), (area.width, area.height));
    let box_h = r.height;
    let inner_w = r.width.saturating_sub(2) as usize;
    let x = area.x + r.x;
    let y = area.y + r.y;

    let title = " File Exists ";
    let top = format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w));
    let bottom = format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w));
    buf.set_string(x, y, &top, body);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(title))) / 2) as u16;
    buf.set_string(title_x, y, title, body);

    let row = |buf: &mut Buffer, dy: u16, text: &str| {
        buf.set_string(x, y + dy, "\u{2551}", body);
        buf.set_string(x + 1, y + dy, pad_to_width(&truncate_with_ellipsis(text, inner_w), inner_w), body);
        buf.set_string(x + 1 + inner_w as u16, y + dy, "\u{2551}", body);
    };

    row(buf, 1, &format!("{} already exists", info.source_name.to_string_lossy()));
    let src_dt = info.source_modified.map(|d| format!("{} {}", format_date(d), format_time(d))).unwrap_or_default();
    row(buf, 2, &format!("Source:  {}  {}", format_size(info.source_size), src_dt));
    let tgt_dt = info.target_modified.map(|d| format!("{} {}", format_date(d), format_time(d))).unwrap_or_default();
    row(buf, 3, &format!("Target:  {}  {}", format_size(info.target_size), tgt_dt));
    row(buf, 4, "");

    let caret = match rename_input {
        Some(edit) => {
            // The rename field keeps its pre-editing `[name]` space-padded
            // look (the drawn `_` caret is gone — the terminal cursor at the
            // caret replaces it, design D5), scrolled via `window` like
            // every other field.
            let field_w = inner_w.saturating_sub(12).max(1);
            let (start, caret_col) = edit.window(field_w);
            let slice: String = edit.text().chars().skip(start).take(field_w).collect();
            let pad = inner_w.saturating_sub(10).saturating_sub(2 + display_width(&slice));
            buf.set_string(x, y + 5, "\u{2551}", body);
            buf.set_string(x + 1, y + 5, "New name:", body);
            buf.set_string(x + 11, y + 5, "[", field_style);
            buf.set_string(x + 12, y + 5, &slice, field_style);
            buf.set_string(x + 12 + display_width(&slice) as u16, y + 5, &format!("]{}", " ".repeat(pad)), field_style);
            buf.set_string(x + 1 + inner_w as u16, y + 5, "\u{2551}", body);
            // While the field is open Esc closes it rather than cancelling
            // the job, and `c` is text, so the Cancel row is not offered.
            row(buf, 6, "");
            let caret_cells = display_width(&slice.chars().take(caret_col).collect::<String>());
            Some((x + 12 + caret_cells.min(field_w.saturating_sub(1)) as u16, y + 5))
        }
        None => {
            row(buf, 5, CHOICES_ROW);
            row(buf, 6, CANCEL_ROW);
            None
        }
    };

    row(buf, 7, "");
    buf.set_string(x, y + box_h - 1, &bottom, body);
    caret
}

/// The hotkey-text button spans `render_conflict` currently draws at `area`
/// for the `(O)verwrite  (S)kip  (R)ename  Over(w)rite All  Skip (A)ll` row
/// and the `(C)ancel` row below it — this dialog has no framed buttons, so
/// each hotkey span itself is recorded as a button (design D2). Empty while
/// the in-place rename input is showing instead of those rows, mirroring
/// the renderer exactly. Mirrors `render_conflict`'s own `overlay_rect`
/// geometry so the rects can never drift from what's actually on screen.
pub fn hit_buttons(area: Rect, rename_input: &Option<LineEdit>) -> Vec<(Rect, ButtonId)> {
    if rename_input.is_some() {
        return vec![];
    }
    let r = overlay_rect((BOX_INNER_W as u16 + 2, BOX_H), (MIN_INNER_W as u16 + 2, BOX_H), (area.width, area.height));
    let inner_w = r.width.saturating_sub(2) as usize;
    let x = area.x + r.x;
    let y = area.y + r.y;
    let base_x = x + 1;

    let mut out = Vec::new();
    let mut spans = |text: &str, row_y: u16, labels: &[(&str, ButtonId)]| {
        for (label, id) in labels {
            if let Some(byte_idx) = text.find(label) {
                let col = text[..byte_idx].chars().count() as u16;
                out.push((Rect { x: base_x + col, y: row_y, width: display_width(label) as u16, height: 1 }, *id));
            }
        }
    };
    spans(
        &truncate_with_ellipsis(CHOICES_ROW, inner_w),
        y + 5,
        &[
            ("(O)verwrite", ButtonId::ConflictOverwrite),
            ("(S)kip", ButtonId::ConflictSkip),
            ("(R)ename", ButtonId::ConflictRename),
            ("Over(w)rite All", ButtonId::ConflictOverwriteAll),
            ("Skip (A)ll", ButtonId::ConflictSkipAll),
        ],
    );
    spans(&truncate_with_ellipsis(CANCEL_ROW, inner_w), y + 6, &[("(C)ancel", ButtonId::ConflictCancel)]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// process-lifecycle-hardening task 4.6: the `(C)ancel` control sits on
    /// its own row directly below the choice row, at the dialog's left text
    /// column, and the choice row's own spans are exactly what they were
    /// before the Cancel row existed (design D7). Note the pre-existing
    /// state of that row: at the dialog's 46-column inner width it is
    /// truncated with an ellipsis, so only the first three labels are drawn
    /// whole and recorded as spans — unchanged by this change.
    #[test]
    fn hit_buttons_record_the_cancel_span_below_the_choice_row() {
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let buttons = hit_buttons(area, &None);
        let ids: Vec<&ButtonId> = buttons.iter().map(|(_, id)| id).collect();
        assert_eq!(ids, vec![&ButtonId::ConflictOverwrite, &ButtonId::ConflictSkip, &ButtonId::ConflictRename, &ButtonId::ConflictCancel]);
        let choice_row = buttons[0].0.y;
        let (cancel_rect, _) = buttons.last().unwrap();
        assert_eq!(cancel_rect.y, choice_row + 1, "one row below the choices");
        assert_eq!(cancel_rect.x, buttons[0].0.x, "at the same left column as (O)verwrite");
        assert_eq!(cancel_rect.width, display_width("(C)ancel") as u16);
        assert!(buttons[..3].iter().all(|(r, _)| r.y == choice_row), "the choice spans keep their row");
    }

    #[test]
    fn hit_buttons_are_empty_while_the_rename_field_is_open() {
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        assert!(hit_buttons(area, &Some(LineEdit::default())).is_empty());
    }
}
