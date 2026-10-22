//! Per-file error-recovery dialog: bright-white-on-red, Retry/Skip/Skip
//! All/Abort.

use filecommand_core::dialogs::overlay_rect;
use filecommand_core::fs_ops::ErrorInfo;
use filecommand_core::listing::{display_width, pad_to_width, truncate_with_ellipsis};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use filecommand_core::update::ButtonId;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::{label_spans, DialogLayout};
use crate::style::role_style;

const MIN_INNER_W: usize = 34;
const CHOICES_ROW: &str = "(R)etry  (S)kip  Skip (A)ll  A(b)ort";

/// The dialog's geometry at `area` (reducer-overlay-structure design D8):
/// its rectangle; its rows — the path, the message, a blank, and the
/// `(R)etry  (S)kip  Skip (A)ll  A(b)ort` row — truncated to the inner
/// width; and that row's hotkey spans recorded as buttons (design D2: no
/// framed buttons, so each hotkey span is a button).
pub fn layout(area: Rect, info: &ErrorInfo) -> DialogLayout {
    let path_line = info.path.display().to_string();
    let preferred_inner_w = display_width(&info.message).max(display_width(&path_line)).max(30) + 2;
    let r = overlay_rect((preferred_inner_w as u16 + 2, 6), (MIN_INNER_W as u16, 6), (area.width, area.height));
    let rect = Rect { x: area.x + r.x, y: area.y + r.y, width: r.width, height: r.height };
    let inner_w = rect.width.saturating_sub(2) as usize;
    let choices = truncate_with_ellipsis(CHOICES_ROW, inner_w);
    let buttons = label_spans(
        &choices,
        rect.x + 1,
        rect.y + 4,
        &[
            ("(R)etry", ButtonId::ErrorRetry),
            ("(S)kip", ButtonId::ErrorSkip),
            ("Skip (A)ll", ButtonId::ErrorSkipAll),
            ("A(b)ort", ButtonId::ErrorAbort),
        ],
    );
    let lines = vec![
        pad_to_width(&truncate_with_ellipsis(&path_line, inner_w), inner_w),
        pad_to_width(&truncate_with_ellipsis(&info.message, inner_w), inner_w),
        pad_to_width("", inner_w),
        pad_to_width(&choices, inner_w),
    ];
    DialogLayout { rect, buttons, rows: Vec::new(), lines }
}

pub fn render_error(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, info: &ErrorInfo) {
    draw_error(buf, theme, depth, &layout(area, info));
}

/// Paint the dialog from an already computed [`layout`].
pub fn draw_error(buf: &mut Buffer, theme: &Theme, depth: ColorDepth, l: &DialogLayout) {
    let body = role_style(theme, Role::DialogError, depth);
    let inner_w = l.rect.width.saturating_sub(2) as usize;
    let (x, y) = (l.rect.x, l.rect.y);

    let title = " Error ";
    let top = format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w));
    let bottom = format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w));
    buf.set_string(x, y, &top, body);
    let title_x = x + 1 + ((inner_w.saturating_sub(display_width(title))) / 2) as u16;
    buf.set_string(title_x, y, title, body);

    for (i, text) in l.lines.iter().enumerate() {
        let dy = 1 + i as u16;
        buf.set_string(x, y + dy, "\u{2551}", body);
        buf.set_string(x + 1, y + dy, text, body);
        buf.set_string(x + 1 + inner_w as u16, y + dy, "\u{2551}", body);
    }
    buf.set_string(x, y + l.rect.height - 1, &bottom, body);
}
