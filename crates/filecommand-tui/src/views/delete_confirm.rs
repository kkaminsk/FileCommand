//! F8 delete confirmation dialog: names a single item or gives a count for
//! a multi-selection, warns the deletion is permanent, asks a second time
//! when a selected directory may be non-empty, and discloses how many of the
//! selected sources the source panel's quick filter hides (selection
//! "Operations disclose selected entries hidden by the quick filter").

use filecommand_core::dialogs::overlay_rect;
use filecommand_core::fs_ops::dialog::{hidden_by_filter_disclosure, FileOpSetup};
use filecommand_core::fs_ops::SourceItem;
use filecommand_core::listing::{display_width, truncate_with_ellipsis};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use filecommand_core::update::ButtonId;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const MIN_INNER_W: usize = 24;

/// The question line, shared by [`render_delete_confirm`] and
/// [`hit_buttons`].
fn question_for(sources: &[SourceItem], needs_second_confirm: bool, confirmed_once: bool) -> String {
    let subject = match sources {
        [one] => format!("{}", one.original_name.to_string_lossy()),
        many => format!("{} files", many.len()),
    };
    if needs_second_confirm && confirmed_once {
        format!("{subject} contains a directory — delete its entire contents?")
    } else {
        format!("Delete {subject}? This cannot be undone.")
    }
}

/// Row geometry shared by [`render_delete_confirm`] and [`hit_buttons`], so
/// the button rects can never drift from what the renderer painted (pending
/// reducer-overlay-structure, B-08). The box is 4 rows tall, or 5 when the
/// hidden-selection disclosure takes its own row between the question and
/// the buttons (filter-selection-safety design D3) — both the preferred and
/// the minimum height move together, staying within responsive-layout
/// "Unified overlay geometry"'s declared-minimum cap.
struct DialogLayout {
    x: u16,
    y: u16,
    box_h: u16,
    inner_w: usize,
    content_w: usize,
    /// The button row's offset from the box's top row.
    buttons_row: u16,
}

fn dialog_layout(area: Rect, question: &str, disclose: bool) -> DialogLayout {
    let preferred_inner_w = display_width(question).max(20) + 2;
    let rows: u16 = if disclose { 5 } else { 4 };
    let r = overlay_rect((preferred_inner_w as u16 + 2, rows), (MIN_INNER_W as u16 + 2, rows), (area.width, area.height));
    let inner_w = r.width.saturating_sub(2) as usize;
    DialogLayout {
        x: area.x + r.x,
        y: area.y + r.y,
        box_h: r.height,
        inner_w,
        content_w: inner_w.saturating_sub(2),
        buttons_row: if disclose { 3 } else { 2 },
    }
}

pub fn render_delete_confirm(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, setup: &FileOpSetup) {
    let FileOpSetup::DeleteConfirm { sources, needs_second_confirm, confirmed_once, hidden_by_filter, .. } = setup else { return };
    let body = role_style(theme, Role::DialogPrimary, depth);

    let question = question_for(sources, *needs_second_confirm, *confirmed_once);
    let disclosure = hidden_by_filter_disclosure(*hidden_by_filter, sources.len());
    let l = dialog_layout(area, &question, disclosure.is_some());

    let top = format!("\u{250C}{}\u{2510}", "\u{2500}".repeat(l.inner_w));
    let bottom = format!("\u{2514}{}\u{2518}", "\u{2500}".repeat(l.inner_w));
    let mid = format!("\u{2502} {} \u{2502}", pad(&truncate_with_ellipsis(&question, l.content_w), l.content_w));
    let buttons = format!("\u{2502} {} \u{2502}", pad(&truncate_with_ellipsis("(Y)es   (N)o", l.content_w), l.content_w));

    buf.set_string(l.x, l.y, &top, body);
    buf.set_string(l.x, l.y + 1, &mid, body);
    if let Some(text) = disclosure {
        let row = format!("\u{2502} {} \u{2502}", pad(&truncate_with_ellipsis(&text, l.content_w), l.content_w));
        buf.set_string(l.x, l.y + 2, &row, body);
    }
    buf.set_string(l.x, l.y + l.buttons_row, &buttons, body);
    buf.set_string(l.x, l.y + l.box_h - 1, &bottom, body);
}

fn pad(s: &str, width: usize) -> String {
    filecommand_core::listing::pad_to_width(s, width)
}

/// The `(Y)es`/`(N)o` button spans `render_delete_confirm` currently draws
/// at `area` (mouse-input "Dialog button"). Derives its geometry from the
/// same [`dialog_layout`] helper as the renderer, so the rects move with the
/// disclosure row exactly like the painted buttons do.
pub fn hit_buttons(area: Rect, setup: &FileOpSetup) -> Vec<(Rect, ButtonId)> {
    let FileOpSetup::DeleteConfirm { sources, needs_second_confirm, confirmed_once, hidden_by_filter, .. } = setup else { return vec![] };
    let question = question_for(sources, *needs_second_confirm, *confirmed_once);
    let l = dialog_layout(area, &question, hidden_by_filter_disclosure(*hidden_by_filter, sources.len()).is_some());
    let buttons_text = truncate_with_ellipsis("(Y)es   (N)o", l.content_w);
    let base_x = l.x + 2;
    let row_y = l.y + l.buttons_row;

    let mut out = Vec::new();
    for (label, id) in [("(Y)es", ButtonId::DeleteConfirmYes), ("(N)o", ButtonId::DeleteConfirmNo)] {
        if let Some(byte_idx) = buttons_text.find(label) {
            let col = buttons_text[..byte_idx].chars().count() as u16;
            out.push((Rect { x: base_x + col, y: row_y, width: display_width(label) as u16, height: 1 }, id));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::*;

    fn source_items(n: usize) -> Vec<SourceItem> {
        (0..n).map(|i| SourceItem { original_name: OsString::from(format!("f{i}.txt")), path: PathBuf::from(format!(r"C:\left\f{i}.txt")), is_dir: false }).collect()
    }

    fn delete_setup(sources: Vec<SourceItem>, hidden_by_filter: usize) -> FileOpSetup {
        FileOpSetup::DeleteConfirm { sources, source_dir: PathBuf::from(r"C:\left"), needs_second_confirm: false, confirmed_once: false, hidden_by_filter }
    }

    fn area() -> Rect {
        Rect { x: 0, y: 0, width: 80, height: 24 }
    }

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

    // -----------------------------------------------------------------
    // filter-selection-safety: the hidden-selection disclosure row
    // (tasks.md 2.4/4.6; design D3)
    // -----------------------------------------------------------------

    #[test]
    fn no_hidden_selection_leaves_the_dialog_unchanged() {
        let theme = Theme::classic();
        let setup = delete_setup(source_items(5), 0);
        let mut buf = Buffer::empty(area());
        render_delete_confirm(&mut buf, area(), &theme, ColorDepth::Ansi16, &setup);
        let text = buffer_to_text(&buf, area());
        assert!(text.contains("Delete 5 files? This cannot be undone."), "the question is untouched:\n{text}");
        assert!(!text.contains("hidden by filter"), "no disclosure without hidden selections:\n{text}");
    }

    #[test]
    fn the_disclosure_takes_its_own_row_between_the_question_and_the_buttons() {
        let theme = Theme::classic();
        let setup = delete_setup(source_items(5), 2);
        let mut buf = Buffer::empty(area());
        render_delete_confirm(&mut buf, area(), &theme, ColorDepth::Ansi16, &setup);
        let text = buffer_to_text(&buf, area());
        assert!(text.contains("Delete 5 files? This cannot be undone."), "the question is untouched:\n{text}");
        assert!(text.contains("2 of 5 selected hidden by filter"), "the disclosure row is shown:\n{text}");
    }

    #[test]
    fn a_single_hidden_source_gets_the_singular_wording() {
        let theme = Theme::classic();
        let setup = delete_setup(source_items(1), 1);
        let mut buf = Buffer::empty(area());
        render_delete_confirm(&mut buf, area(), &theme, ColorDepth::Ansi16, &setup);
        let text = buffer_to_text(&buf, area());
        assert!(text.contains("Delete f0.txt? This cannot be undone."), "the single item is still named:\n{text}");
        assert!(text.contains("Selected item is hidden by filter"), "singular wording:\n{text}");
    }

    #[test]
    fn hit_buttons_sit_on_the_shifted_row_when_the_disclosure_is_shown() {
        // The rects must describe the row the buttons are actually painted
        // on, whether or not the disclosure row is present — and with the
        // disclosure, that row sits directly beneath it (design D3).
        let theme = Theme::classic();
        let painted_row = |needle: &str, hidden: usize| -> u16 {
            let setup = delete_setup(source_items(5), hidden);
            let mut buf = Buffer::empty(area());
            render_delete_confirm(&mut buf, area(), &theme, ColorDepth::Ansi16, &setup);
            let text = buffer_to_text(&buf, area());
            text.lines().position(|line| line.contains(needle)).unwrap_or_else(|| panic!("{needle:?} is painted")) as u16
        };
        let plain_buttons = painted_row("(Y)es", 0);
        let disclosed_buttons = painted_row("(Y)es", 2);
        let disclosure = painted_row("2 of 5 selected hidden by filter", 2);
        assert_eq!(disclosure, disclosed_buttons - 1, "the disclosure row sits between the question and the buttons");
        assert_eq!(painted_row("Delete 5 files?", 2), disclosure - 1);

        for (setup, row) in [(delete_setup(source_items(5), 0), plain_buttons), (delete_setup(source_items(5), 2), disclosed_buttons)] {
            let hits = hit_buttons(area(), &setup);
            assert_eq!(hits.len(), 2);
            assert!(hits.iter().all(|(r, _)| r.y == row), "every button rect sits on the painted button row {row}: {hits:?}");
        }
    }
}
