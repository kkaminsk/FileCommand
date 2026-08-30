//! F5/F6/F7 destination/name input dialog: double-line frame, black-on-cyan
//! body, bracket-and-dots input field — plus, for a drop-initiated dialog
//! only, a `[ Copy ] [ Move ] [ Cancel ]` button row (operation-dialogs
//! "Drop-initiated destination dialog"; mouse-panel-drag design D3).

use filecommand_core::dialogs::overlay_rect;
use filecommand_core::fs_ops::dialog::{DropButtons, FileOpSetup};
use filecommand_core::fs_ops::JobKind;
use filecommand_core::listing::{display_width, truncate_with_ellipsis};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use filecommand_core::update::ButtonId;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const FIELD_WIDTH: usize = 40;
const BOX_INNER_W: usize = FIELD_WIDTH + 4;

/// The button row's labels in on-screen order, paired with the `JobKind`
/// each represents — `None` for `[ Cancel ]`, which is never the focused
/// button (only `[ Copy ]`/`[ Move ]` alternate focus, per the verb the drag
/// proposed — mouse-drag design D1/D2).
const DROP_BUTTONS: [(&str, Option<JobKind>, ButtonId); 3] =
    [("[ Copy ]", Some(JobKind::Copy), ButtonId::DropDialogCopy), ("[ Move ]", Some(JobKind::Move), ButtonId::DropDialogMove), ("[ Cancel ]", None, ButtonId::DropDialogCancel)];

/// The gap, in display columns, between adjacent button labels.
const BUTTON_GAP: usize = 2;

/// The box height with vs. without the drop dialog's extra button row —
/// `render_input_box`'s only height-affecting branch, so the plain keyboard
/// dialog (`buttons: None`) always gets the original 6-row box, unchanged
/// (operation-dialogs "F5 dialog unchanged").
fn box_height(buttons: Option<DropButtons>) -> u16 {
    if buttons.is_some() {
        7
    } else {
        6
    }
}

/// `Copy`/`Move`, matching the mini-status's own verb naming
/// (mouse-panel-drag "Drag feedback") rather than `title_for`'s keyboard-
/// dialog "Rename/Move".
fn drop_verb_label(kind: JobKind) -> &'static str {
    match kind {
        JobKind::Move => "Move",
        _ => "Copy",
    }
}

/// The drop-initiated dialog's title: the focused verb and the item count
/// (operation-dialogs "Drop-initiated destination dialog": "the title SHALL
/// name the focused verb and the item count" — e.g. `Copy 3 files`).
fn drop_dialog_title(focused: JobKind, count: usize) -> String {
    let plural = if count == 1 { "" } else { "s" };
    format!("{} {count} file{plural}", drop_verb_label(focused))
}

/// The button row's total display width, including the gaps between labels
/// — shared by the renderer and [`hit_buttons`] so their centering can never
/// disagree.
fn buttons_total_width() -> usize {
    DROP_BUTTONS.iter().map(|(label, ..)| display_width(label)).sum::<usize>() + BUTTON_GAP * (DROP_BUTTONS.len() - 1)
}

fn title_for(kind: JobKind) -> &'static str {
    match kind {
        JobKind::Copy => "Copy",
        JobKind::Move => "Rename/Move",
        JobKind::Mkdir => "Make Directory",
        JobKind::Delete => "Delete",
        // `FileOpSetup::RenameInput` never carries a `JobKind` (see
        // `render_destination_input` below), so this arm is unreachable in
        // practice; kept so `JobKind` stays exhaustively matched here.
        JobKind::Rename => "Rename",
    }
}

fn prompt_for(kind: JobKind) -> &'static str {
    match kind {
        JobKind::Copy | JobKind::Move => "Copy/move to:",
        JobKind::Mkdir => "Name of new directory:",
        JobKind::Delete | JobKind::Rename => "",
    }
}

pub fn render_destination_input(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, setup: &FileOpSetup) {
    match setup {
        FileOpSetup::DestinationInput { kind, sources, input, buttons, .. } => {
            let title = match buttons {
                Some(db) => drop_dialog_title(db.focused, sources.len()),
                None => title_for(*kind).to_string(),
            };
            render_input_box(buf, area, theme, depth, &title, prompt_for(*kind), input, *buttons);
        }
        // The file-action menu's in-place Rename: same bracket-and-dots
        // input box, pre-filled with the target's current name (design D2;
        // file-action-menu "In-place Rename": "an input dialog pre-filled
        // with the target entry's current name"). Never a drop-initiated
        // dialog, so it always gets the plain 6-row box.
        FileOpSetup::RenameInput { input, .. } => render_input_box(buf, area, theme, depth, "Rename", "New name:", input, None),
        FileOpSetup::DeleteConfirm { .. } => {}
    }
}

/// `buttons: None` renders exactly the pre-`mouse-panel-drag` 6-row box —
/// every row below is drawn at the same offset it always was, and the
/// button row is only ever drawn when `buttons: Some` — so the F5/F6/F7/
/// Rename dialogs stay byte-for-byte unchanged (operation-dialogs "F5
/// dialog unchanged").
#[allow(clippy::too_many_arguments)]
fn render_input_box(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, title: &str, prompt: &str, input: &str, buttons: Option<DropButtons>) {
    let body = role_style(theme, Role::DialogPrimary, depth);
    let field_style = role_style(theme, Role::DialogInput, depth);

    let preferred = (BOX_INNER_W as u16 + 2, box_height(buttons));
    let r = overlay_rect(preferred, preferred, (area.width, area.height));
    let box_h = r.height;
    let inner_w = r.width.saturating_sub(2) as usize;
    let field_w = FIELD_WIDTH.min(inner_w.saturating_sub(2));
    let x = area.x + r.x;
    let y = area.y + r.y;

    let title = format!(" {title} ");
    let top = format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w));
    let bottom = format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w));
    let blank = format!("\u{2551}{}\u{2551}", " ".repeat(inner_w));

    buf.set_string(x, y, &top, body);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(&title))) / 2) as u16;
    buf.set_string(title_x, y, &title, body);
    buf.set_string(x, y + 1, &blank, body);

    buf.set_string(x, y + 2, "\u{2551}", body);
    buf.set_string(x + 2, y + 2, truncate_with_ellipsis(prompt, inner_w.saturating_sub(2)), body);
    buf.set_string(x + 1 + inner_w as u16, y + 2, "\u{2551}", body);

    // Bracket-and-dots input field: `[text...........]`.
    let mut field_text = input.to_string();
    if display_width(&field_text) > field_w {
        let excess = display_width(&field_text) - field_w;
        field_text = field_text.chars().skip(excess).collect();
    }
    let pad = field_w - display_width(&field_text);
    let field = format!("[{field_text}{}]", ".".repeat(pad));
    buf.set_string(x, y + 3, "\u{2551}", body);
    buf.set_string(x + 2, y + 3, &field, field_style);
    buf.set_string(x + 1 + inner_w as u16, y + 3, "\u{2551}", body);

    buf.set_string(x, y + 4, &blank, body);
    if let Some(db) = buttons {
        render_button_row(buf, x, y + 5, inner_w, theme, depth, db.focused, body);
    }
    buf.set_string(x, y + box_h - 1, &bottom, body);
}

/// The drop dialog's `[ Copy ] [ Move ] [ Cancel ]` row, centered in
/// `inner_w` — the button representing `focused` (Copy or Move, whichever
/// the drag proposed) renders in `Role::ButtonFocused`; the other verb and
/// `Cancel` always render in `Role::ButtonNormal` (operation-dialogs
/// "Drop-initiated destination dialog"; mouse-drag design D1/D2).
#[allow(clippy::too_many_arguments)]
fn render_button_row(buf: &mut Buffer, x: u16, y: u16, inner_w: usize, theme: &Theme, depth: ColorDepth, focused: JobKind, body: ratatui::style::Style) {
    let normal = role_style(theme, Role::ButtonNormal, depth);
    let focused_style = role_style(theme, Role::ButtonFocused, depth);
    buf.set_string(x, y, "\u{2551}", body);
    buf.set_string(x + 1, y, " ".repeat(inner_w), body);
    let mut bx = x + 1 + (inner_w.saturating_sub(buttons_total_width()) / 2) as u16;
    for (label, kind, _) in DROP_BUTTONS {
        let style = if kind == Some(focused) { focused_style } else { normal };
        buf.set_string(bx, y, label, style);
        bx += display_width(label) as u16 + BUTTON_GAP as u16;
    }
    buf.set_string(x + 1 + inner_w as u16, y, "\u{2551}", body);
}

/// The `[ Copy ] [ Move ] [ Cancel ]` button rects `render_input_box`
/// currently draws — empty unless `setup` is a drop-initiated
/// `DestinationInput` (`buttons: Some`), since the plain keyboard dialog has
/// no button row at all (operation-dialogs "F5 dialog unchanged"). Mirrors
/// `render_button_row`'s own `box_height`/`overlay_rect`/centering geometry
/// exactly, so a hit rect can never describe a button that isn't actually
/// where the renderer painted it.
pub fn hit_buttons(area: Rect, setup: &FileOpSetup) -> Vec<(Rect, ButtonId)> {
    let FileOpSetup::DestinationInput { buttons: Some(db), .. } = setup else { return vec![] };
    let preferred = (BOX_INNER_W as u16 + 2, box_height(Some(*db)));
    let r = overlay_rect(preferred, preferred, (area.width, area.height));
    let inner_w = r.width.saturating_sub(2) as usize;
    let x = area.x + r.x;
    let y = area.y + r.y + 5;

    let mut bx = x + 1 + (inner_w.saturating_sub(buttons_total_width()) / 2) as u16;
    let mut out = Vec::new();
    for (label, _, id) in DROP_BUTTONS {
        out.push((Rect { x: bx, y, width: display_width(label) as u16, height: 1 }, id));
        bx += display_width(label) as u16 + BUTTON_GAP as u16;
    }
    out
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use filecommand_core::fs_ops::SourceItem;

    use super::*;

    fn buffer_to_text(buf: &Buffer, area: Rect) -> String {
        let mut out = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                out.push_str(buf[(area.x + x, area.y + y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn source_items(n: usize) -> Vec<SourceItem> {
        (0..n).map(|i| SourceItem { original_name: OsString::from(format!("f{i}.txt")), path: PathBuf::from(format!(r"C:\left\f{i}.txt")), is_dir: false }).collect()
    }

    fn area() -> Rect {
        Rect { x: 0, y: 0, width: 80, height: 24 }
    }

    // -----------------------------------------------------------------
    // mouse-panel-drag / operation-dialogs: the drop-initiated dialog
    // (tasks.md 2.3).
    // -----------------------------------------------------------------

    #[test]
    fn f5_dialog_with_no_buttons_renders_byte_identical_to_the_pre_drag_box() {
        // operation-dialogs "F5 dialog unchanged": the plain keyboard-
        // initiated dialog gets the original 6-row box with no button row,
        // whatever `buttons` field the setup carries elsewhere.
        let theme = Theme::classic();
        let setup = FileOpSetup::DestinationInput {
            kind: JobKind::Copy,
            sources: source_items(1),
            source_dir: PathBuf::from(r"C:\left"),
            input: r"D:\BACKUP".to_string(),
            buttons: None,
        };
        let mut buf = Buffer::empty(area());
        render_destination_input(&mut buf, area(), &theme, ColorDepth::Ansi16, &setup);
        let text = buffer_to_text(&buf, area());
        assert!(text.contains(" Copy "), "title must stay the plain title_for(kind) form:\n{text}");
        assert!(!text.contains("[ Copy ]"), "no button row must be drawn when buttons is None:\n{text}");
        assert!(hit_buttons(area(), &setup).is_empty(), "no clickable buttons exist for the keyboard dialog");
    }

    #[test]
    fn drop_dialog_titles_the_focused_verb_and_item_count_and_draws_the_button_row() {
        // operation-dialogs "Drop dialog contents": titled `Copy 3 files`,
        // `[ Copy ]` focused.
        let theme = Theme::classic();
        let setup = FileOpSetup::DestinationInput {
            kind: JobKind::Copy,
            sources: source_items(3),
            source_dir: PathBuf::from(r"C:\left"),
            input: r"D:\BACKUP\OLD".to_string(),
            buttons: Some(DropButtons { focused: JobKind::Copy }),
        };
        let mut buf = Buffer::empty(area());
        render_destination_input(&mut buf, area(), &theme, ColorDepth::Ansi16, &setup);
        let text = buffer_to_text(&buf, area());
        assert!(text.contains("Copy 3 files"), "title must name the focused verb and item count:\n{text}");
        assert!(text.contains("[ Copy ]") && text.contains("[ Move ]") && text.contains("[ Cancel ]"), "the button row must be drawn:\n{text}");

        let hits = hit_buttons(area(), &setup);
        assert_eq!(hits.len(), 3, "all three buttons are clickable");
        assert!(hits.iter().any(|(_, id)| *id == ButtonId::DropDialogCopy));
        assert!(hits.iter().any(|(_, id)| *id == ButtonId::DropDialogMove));
        assert!(hits.iter().any(|(_, id)| *id == ButtonId::DropDialogCancel));

        let copy_rect = hits.iter().find(|(_, id)| *id == ButtonId::DropDialogCopy).unwrap().0;
        let move_rect = hits.iter().find(|(_, id)| *id == ButtonId::DropDialogMove).unwrap().0;
        let focused = role_style(&theme, Role::ButtonFocused, ColorDepth::Ansi16);
        let normal = role_style(&theme, Role::ButtonNormal, ColorDepth::Ansi16);
        assert_eq!(buf[(copy_rect.x, copy_rect.y)].style().fg, focused.fg, "[ Copy ] must be focused");
        assert_eq!(buf[(copy_rect.x, copy_rect.y)].style().bg, focused.bg, "[ Copy ] must be focused");
        assert_eq!(buf[(move_rect.x, move_rect.y)].style().fg, normal.fg, "[ Move ] must not be focused");
        assert_eq!(buf[(move_rect.x, move_rect.y)].style().bg, normal.bg, "[ Move ] must not be focused");
    }

    #[test]
    fn move_focused_titles_move_and_highlights_the_move_button_instead() {
        let theme = Theme::classic();
        let setup = FileOpSetup::DestinationInput {
            kind: JobKind::Move,
            sources: source_items(1),
            source_dir: PathBuf::from(r"C:\left"),
            input: r"D:\BACKUP".to_string(),
            buttons: Some(DropButtons { focused: JobKind::Move }),
        };
        let mut buf = Buffer::empty(area());
        render_destination_input(&mut buf, area(), &theme, ColorDepth::Ansi16, &setup);
        let text = buffer_to_text(&buf, area());
        assert!(text.contains("Move 1 file"), "singular count, no trailing s:\n{text}");

        let hits = hit_buttons(area(), &setup);
        let move_rect = hits.iter().find(|(_, id)| *id == ButtonId::DropDialogMove).unwrap().0;
        let copy_rect = hits.iter().find(|(_, id)| *id == ButtonId::DropDialogCopy).unwrap().0;
        let focused = role_style(&theme, Role::ButtonFocused, ColorDepth::Ansi16);
        let normal = role_style(&theme, Role::ButtonNormal, ColorDepth::Ansi16);
        assert_eq!(buf[(move_rect.x, move_rect.y)].style().fg, focused.fg);
        assert_eq!(buf[(copy_rect.x, copy_rect.y)].style().fg, normal.fg);
    }

    #[test]
    fn rename_input_never_gets_a_button_row() {
        let theme = Theme::classic();
        let setup = FileOpSetup::RenameInput { source_dir: PathBuf::from(r"C:\left"), original_name: OsString::from("a.txt"), is_dir: false, input: "a.txt".to_string() };
        let mut buf = Buffer::empty(area());
        render_destination_input(&mut buf, area(), &theme, ColorDepth::Ansi16, &setup);
        let text = buffer_to_text(&buf, area());
        assert!(!text.contains("[ Copy ]"));
        assert!(hit_buttons(area(), &setup).is_empty());
    }
}
