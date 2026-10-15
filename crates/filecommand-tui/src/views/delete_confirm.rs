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

use super::{label_spans, DialogLayout};
use crate::style::role_style;

const MIN_INNER_W: usize = 24;
const BUTTONS: &str = "(Y)es   (N)o";

/// The question line.
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

/// The dialog's geometry at `area` (reducer-overlay-structure design D8):
/// its rectangle, its content rows in order — the question, the
/// hidden-selection disclosure when there is one, the `(Y)es   (N)o` row —
/// already truncated to the content width, and the two button spans on the
/// cells those labels occupy. The box is 4 rows tall, or 5 when the
/// disclosure takes its own row between the question and the buttons
/// (filter-selection-safety design D3) — both the preferred and the minimum
/// height move together, staying within responsive-layout "Unified overlay
/// geometry"'s declared-minimum cap. Empty for a setup that is not a delete
/// confirmation.
pub fn layout(area: Rect, setup: &FileOpSetup) -> DialogLayout {
    let FileOpSetup::DeleteConfirm { sources, needs_second_confirm, confirmed_once, hidden_by_filter, .. } = setup else {
        return DialogLayout::default();
    };
    let question = question_for(sources, *needs_second_confirm, *confirmed_once);
    let disclosure = hidden_by_filter_disclosure(*hidden_by_filter, sources.len());
    let preferred_inner_w = display_width(&question).max(20) + 2;
    let rows: u16 = if disclosure.is_some() { 5 } else { 4 };
    let r = overlay_rect((preferred_inner_w as u16 + 2, rows), (MIN_INNER_W as u16 + 2, rows), (area.width, area.height));
    let rect = Rect { x: area.x + r.x, y: area.y + r.y, width: r.width, height: r.height };
    let content_w = rect.width.saturating_sub(4) as usize;

    let mut lines = vec![truncate_with_ellipsis(&question, content_w)];
    if let Some(text) = disclosure {
        lines.push(truncate_with_ellipsis(&text, content_w));
    }
    let buttons_text = truncate_with_ellipsis(BUTTONS, content_w);
    let buttons_row = rect.y + lines.len() as u16 + 1;
    let buttons = label_spans(&buttons_text, rect.x + 2, buttons_row, &[("(Y)es", ButtonId::DeleteConfirmYes), ("(N)o", ButtonId::DeleteConfirmNo)]);
    lines.push(buttons_text);
    DialogLayout { rect, buttons, rows: Vec::new(), lines }
}

pub fn render_delete_confirm(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, setup: &FileOpSetup) {
    draw_delete_confirm(buf, theme, depth, &layout(area, setup));
}

/// Paint the dialog from an already computed [`layout`]: one framed row per
/// content line, in order. A no-op for an empty layout (not a delete
/// confirmation).
pub fn draw_delete_confirm(buf: &mut Buffer, theme: &Theme, depth: ColorDepth, l: &DialogLayout) {
    if l.lines.is_empty() {
        return;
    }
    let body = role_style(theme, Role::DialogPrimary, depth);
    let inner_w = l.rect.width.saturating_sub(2) as usize;
    let content_w = inner_w.saturating_sub(2);
    let (x, y) = (l.rect.x, l.rect.y);

    buf.set_string(x, y, format!("\u{250C}{}\u{2510}", "\u{2500}".repeat(inner_w)), body);
    for (i, line) in l.lines.iter().enumerate() {
        buf.set_string(x, y + 1 + i as u16, format!("\u{2502} {} \u{2502}", pad(line, content_w)), body);
    }
    buf.set_string(x, y + l.rect.height - 1, format!("\u{2514}{}\u{2518}", "\u{2500}".repeat(inner_w)), body);
}

fn pad(s: &str, width: usize) -> String {
    filecommand_core::listing::pad_to_width(s, width)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::*;

    fn source_items(n: usize) -> Vec<SourceItem> {
        (0..n)
            .map(|i| SourceItem {
                original_name: OsString::from(format!("f{i}.txt")),
                path: PathBuf::from(format!(r"C:\left\f{i}.txt")),
                is_dir: false,
            })
            .collect()
    }

    fn delete_setup(sources: Vec<SourceItem>, hidden_by_filter: usize) -> FileOpSetup {
        FileOpSetup::DeleteConfirm {
            sources,
            source_dir: PathBuf::from(r"C:\left"),
            needs_second_confirm: false,
            confirmed_once: false,
            hidden_by_filter,
        }
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
            let hits = layout(area(), &setup).buttons;
            assert_eq!(hits.len(), 2);
            assert!(hits.iter().all(|(r, _)| r.y == row), "every button rect sits on the painted button row {row}: {hits:?}");
        }
    }
}
