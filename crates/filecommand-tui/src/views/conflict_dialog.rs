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

use super::{label_spans, DialogLayout};
use crate::style::role_style;

const BOX_INNER_W: usize = 46;
const MIN_INNER_W: usize = 30;
/// Frame rows: title, "already exists", Source, Target, blank, the choice
/// row (or the rename field), the Cancel row, blank, bottom border. One
/// taller than before the Cancel row existed (design D7).
const BOX_H: u16 = 9;
const CHOICES_ROW: &str = "(O)verwrite  (S)kip  (R)ename  Over(w)rite All  Skip (A)ll";
const CANCEL_ROW: &str = "(C)ancel";

/// The dialog's geometry at `area` (reducer-overlay-structure design D8):
/// its rectangle; its text rows — `<name> already exists`, Source, Target,
/// a blank, then either the choice row and the `(C)ancel` row or two blanks
/// while the in-place rename field is showing, then a blank — each clamped
/// to the inner width; and the hotkey spans of the choice and Cancel rows
/// recorded as buttons (design D2: this dialog has no framed buttons, so
/// each hotkey span itself is a button). The buttons are empty while the
/// rename input is showing instead of those rows: there Esc closes the field
/// rather than cancelling the job, and `c` is text.
pub fn layout(area: Rect, info: &ConflictInfo, rename_input: &Option<LineEdit>) -> DialogLayout {
    let r = overlay_rect((BOX_INNER_W as u16 + 2, BOX_H), (MIN_INNER_W as u16 + 2, BOX_H), (area.width, area.height));
    let rect = Rect { x: area.x + r.x, y: area.y + r.y, width: r.width, height: r.height };
    let inner_w = rect.width.saturating_sub(2) as usize;
    let fit = |text: &str| pad_to_width(&truncate_with_ellipsis(text, inner_w), inner_w);

    let src_dt = info.source_modified.map(|d| format!("{} {}", format_date(d), format_time(d))).unwrap_or_default();
    let tgt_dt = info.target_modified.map(|d| format!("{} {}", format_date(d), format_time(d))).unwrap_or_default();
    let mut lines = vec![
        fit(&format!("{} already exists", info.source_name.to_string_lossy())),
        fit(&format!("Source:  {}  {}", format_size(info.source_size), src_dt)),
        fit(&format!("Target:  {}  {}", format_size(info.target_size), tgt_dt)),
        fit(""),
    ];
    let mut buttons = Vec::new();
    if rename_input.is_some() {
        lines.push(fit(""));
        lines.push(fit(""));
    } else {
        let choices = truncate_with_ellipsis(CHOICES_ROW, inner_w);
        let cancel = truncate_with_ellipsis(CANCEL_ROW, inner_w);
        buttons.extend(label_spans(
            &choices,
            rect.x + 1,
            rect.y + 5,
            &[
                ("(O)verwrite", ButtonId::ConflictOverwrite),
                ("(S)kip", ButtonId::ConflictSkip),
                ("(R)ename", ButtonId::ConflictRename),
                ("Over(w)rite All", ButtonId::ConflictOverwriteAll),
                ("Skip (A)ll", ButtonId::ConflictSkipAll),
            ],
        ));
        buttons.extend(label_spans(&cancel, rect.x + 1, rect.y + 6, &[("(C)ancel", ButtonId::ConflictCancel)]));
        lines.push(pad_to_width(&choices, inner_w));
        lines.push(pad_to_width(&cancel, inner_w));
    }
    lines.push(fit(""));
    DialogLayout { rect, buttons, rows: Vec::new(), lines }
}

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
    draw_conflict(buf, theme, depth, rename_input, &layout(area, info, rename_input))
}

/// Paint the dialog from an already computed [`layout`]; `rename_input` is
/// the field drawn over row 5 while it is open. Returns the field's caret
/// position, as [`render_conflict`] does.
pub fn draw_conflict(buf: &mut Buffer, theme: &Theme, depth: ColorDepth, rename_input: &Option<LineEdit>, l: &DialogLayout) -> Option<(u16, u16)> {
    let body = role_style(theme, Role::DialogPrimary, depth);
    let field_style = role_style(theme, Role::DialogInput, depth);
    let inner_w = l.rect.width.saturating_sub(2) as usize;
    let (x, y) = (l.rect.x, l.rect.y);

    let title = " File Exists ";
    let top = format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w));
    let bottom = format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w));
    buf.set_string(x, y, &top, body);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(title))) / 2) as u16;
    buf.set_string(title_x, y, title, body);

    let row = |buf: &mut Buffer, dy: u16, text: &str| {
        buf.set_string(x, y + dy, "\u{2551}", body);
        buf.set_string(x + 1, y + dy, text, body);
        buf.set_string(x + 1 + inner_w as u16, y + dy, "\u{2551}", body);
    };
    for (i, text) in l.lines.iter().enumerate() {
        row(buf, 1 + i as u16, text);
    }

    let caret = rename_input.as_ref().map(|edit| {
        // The rename field keeps its pre-editing `[name]` space-padded look
        // (the drawn `_` caret is gone — the terminal cursor at the caret
        // replaces it, design D5), scrolled via `window` like every other
        // field. It overdraws the blank row 5 the layout left for it.
        let field_w = inner_w.saturating_sub(12).max(1);
        let (start, caret_col) = edit.window(field_w);
        let slice: String = edit.text().chars().skip(start).take(field_w).collect();
        let pad = inner_w.saturating_sub(10).saturating_sub(2 + display_width(&slice));
        buf.set_string(x + 1, y + 5, "New name:", body);
        buf.set_string(x + 11, y + 5, "[", field_style);
        buf.set_string(x + 12, y + 5, &slice, field_style);
        buf.set_string(x + 12 + display_width(&slice) as u16, y + 5, format!("]{}", " ".repeat(pad)), field_style);
        let caret_cells = display_width(&slice.chars().take(caret_col).collect::<String>());
        (x + 12 + caret_cells.min(field_w.saturating_sub(1)) as u16, y + 5)
    });

    buf.set_string(x, y + l.rect.height - 1, &bottom, body);
    caret
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> ConflictInfo {
        ConflictInfo {
            source_name: "a.txt".into(),
            source_size: 1,
            source_modified: None,
            target_path: std::path::PathBuf::from("/right/a.txt"),
            target_size: 2,
            target_modified: None,
        }
    }

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
        let buttons = layout(area, &info(), &None).buttons;
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
        assert!(layout(area, &info(), &Some(LineEdit::default())).buttons.is_empty());
    }
}
