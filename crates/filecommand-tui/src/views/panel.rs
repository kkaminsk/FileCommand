//! Full-mode panel renderer: double-line border, centered title, sortable
//! header, entry rows with a full-width inverse cursor bar, and a
//! mini-status line embedded in the bottom border.

use filecommand_core::listing::{display_name_lossy, entry_status_line, format_date, format_size, format_time, pad_to_width, reading_status};
use filecommand_core::panel::{ListingProgress, PanelState, SortDirection};
use filecommand_core::listing::EntryKind;
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const SIZE_COL_W: usize = 9;
const DATE_COL_W: usize = 8;
const TIME_COL_W: usize = 5;

pub fn render_panel(buf: &mut Buffer, area: Rect, panel: &PanelState, theme: &Theme, depth: ColorDepth, active: bool) {
    if area.width < 4 || area.height < 3 {
        return;
    }
    let frame_style = role_style(theme, Role::PanelFrame, depth);
    let title_style = role_style(theme, if active { Role::PanelTitleActive } else { Role::PanelTitleInactive }, depth);
    let header_style = role_style(theme, Role::PanelHeader, depth);
    let file_style = role_style(theme, Role::PanelFile, depth);
    let dir_style = role_style(theme, Role::PanelDirectory, depth);
    let cursor_style = role_style(theme, Role::PanelCursor, depth);
    let ministatus_style = role_style(theme, Role::PanelMinistatus, depth);

    let w = area.width as usize;
    let x0 = area.x;
    let y0 = area.y;

    // Top border with centered path title.
    let title = format!(" {} ", panel.cwd.display());
    let top = format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(w.saturating_sub(2)));
    buf.set_string(x0, y0, &top, frame_style);
    let title_x = x0 + 1 + ((w.saturating_sub(2)).saturating_sub(display_width(&title)) / 2) as u16;
    buf.set_string(title_x, y0, clip(&title, w.saturating_sub(2)), title_style);

    // Header row.
    let arrow = match panel.sort_direction {
        SortDirection::Asc => "\u{2193}",
        SortDirection::Desc => "\u{2191}",
    };
    let name_w = w.saturating_sub(2).saturating_sub(SIZE_COL_W + DATE_COL_W + TIME_COL_W + 3);
    let header = format!(
        "{}{} {} {} {}",
        pad_to_width(&format!("Name{arrow}"), name_w),
        "\u{2502}",
        pad_to_width("Size", SIZE_COL_W - 2),
        pad_to_width("Date", DATE_COL_W - 1),
        pad_to_width("Time", TIME_COL_W - 1),
    );
    buf.set_string(x0, y0 + 1, "\u{2551}", frame_style);
    buf.set_string(x0 + 1, y0 + 1, pad_to_width(&header, w.saturating_sub(2)), header_style);
    buf.set_string(x0 + area.width - 1, y0 + 1, "\u{2551}", frame_style);

    // Entry rows.
    let rows_start = y0 + 2;
    let rows_h = area.height.saturating_sub(3); // top, header, bottom
    for row in 0..rows_h {
        let y = rows_start + row;
        buf.set_string(x0, y, "\u{2551}", frame_style);
        buf.set_string(x0 + area.width - 1, y, "\u{2551}", frame_style);
        if let Some(entry) = panel.entries.get(row as usize) {
            let is_selected = active && row as usize == panel.cursor;
            let style = if is_selected {
                cursor_style
            } else if entry.is_dir_like() {
                dir_style
            } else {
                file_style
            };
            let name = display_name_lossy(entry);
            let size_col = match entry.kind {
                EntryKind::ParentDir => "\u{25B6}UP--DIR\u{25C4}".to_string(),
                EntryKind::Directory => "\u{25B6}SUB-DIR\u{25C4}".to_string(),
                EntryKind::File => format_size(entry.size),
            };
            let (date_col, time_col) = match entry.modified {
                Some(dt) => (format_date(dt), format_time(dt)),
                None => (String::new(), String::new()),
            };
            let line = format!(
                "{}{} {} {} {}",
                pad_to_width(&name, name_w),
                "\u{2502}",
                pad_to_width(&size_col, SIZE_COL_W - 2),
                pad_to_width(&date_col, DATE_COL_W - 1),
                pad_to_width(&time_col, TIME_COL_W - 1),
            );
            buf.set_string(x0 + 1, y, pad_to_width(&line, w.saturating_sub(2)), style);
        } else {
            buf.set_string(x0 + 1, y, " ".repeat(w.saturating_sub(2)), file_style);
        }
    }

    // Bottom border with mini-status embedded.
    let status_text = match panel.progress {
        ListingProgress::Streaming { count } => reading_status(count),
        ListingProgress::Complete { .. } => panel
            .selected()
            .map(entry_status_line)
            .unwrap_or_default(),
    };
    let bottom_y = y0 + area.height - 1;
    let inner_w = w.saturating_sub(2);
    let bracketed = if status_text.is_empty() { String::new() } else { format!(" {status_text} ") };
    let fill_total = inner_w.saturating_sub(display_width(&bracketed));
    let left_fill = fill_total / 2;
    let right_fill = fill_total - left_fill;
    let bottom = format!(
        "\u{255A}{}{}{}\u{255D}",
        "\u{2550}".repeat(left_fill),
        bracketed,
        "\u{2550}".repeat(right_fill),
    );
    buf.set_string(x0, bottom_y, &bottom, frame_style);
    if !bracketed.is_empty() {
        buf.set_string(x0 + 1 + left_fill as u16, bottom_y, &bracketed, ministatus_style);
    }
}

fn display_width(s: &str) -> usize {
    filecommand_core::listing::display_width(s)
}

/// Truncate `s` to at most `max_w` display columns without adding padding.
fn clip(s: &str, max_w: usize) -> String {
    let mut out = String::new();
    let mut acc = 0usize;
    for ch in s.chars() {
        let cw = filecommand_core::listing::display_width(&ch.to_string());
        if acc + cw > max_w {
            break;
        }
        out.push(ch);
        acc += cw;
    }
    out
}
