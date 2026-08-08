//! Panel renderer: double-line border, centered title (plus the M5 git
//! branch-name suffix on the active panel), tab strip, and the body for
//! each of the five [`DisplayMode`]s — `Full` (sortable header, entry rows
//! with a full-width inverse cursor bar, and the M5 per-file git
//! status-marker column), `Info` (delegates to `views::info_panel`),
//! `Brief` (three name-only columns), `Tree` (a lazily-expanded directory
//! tree that drives the opposite panel), and `QuickView` (a preview of the
//! opposite panel's cursor file, reusing the F3 viewer's text-body
//! renderer) — plus a mini-status line embedded in the bottom border.

use filecommand_core::git_info::FileStatus;
use filecommand_core::listing::EntryKind;
use filecommand_core::listing::{
    display_name_lossy, entry_status_line, format_date, format_size, format_time, pad_to_width, reading_status, sort_arrow, SortColumn,
};
use filecommand_core::panel::{DisplayMode, ListingProgress, PanelState, SortDirection};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use filecommand_core::viewer::{ByteSource, ViewerState};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::style::role_style;
use crate::views::info_panel;
use crate::views::tab_strip;
use crate::views::viewer::render_text_body;

const SIZE_COL_W: usize = 9;
const DATE_COL_W: usize = 8;
const TIME_COL_W: usize = 5;

/// A column header label, with the sort arrow appended when this column is
/// the active sort key. `Unsorted` marks no column, so no header shows one.
fn header_label(base: &str, column: SortColumn, panel: &PanelState) -> String {
    if panel.sort_mode.column() == Some(column) {
        format!("{base}{}", sort_arrow(panel.sort_direction == SortDirection::Desc))
    } else {
        base.to_string()
    }
}

/// The single-cell git status-marker glyph + role for `entry` in `panel`,
/// or `None` for a clean entry or the `..` parent (which git never reports
/// status for) (git-info "Per-file status marker column", "Clean entry has
/// blank marker").
fn git_marker(panel: &PanelState, entry: &filecommand_core::listing::Entry) -> Option<(char, Role)> {
    if entry.kind == EntryKind::ParentDir {
        return None;
    }
    let status = panel.git_info.statuses.get(&entry.name)?;
    let role = match status {
        FileStatus::Modified => Role::PanelGitModified,
        FileStatus::Untracked => Role::PanelGitUntracked,
        FileStatus::Staged => Role::PanelGitStaged,
    };
    Some((status.marker(), role))
}

/// The Ctrl+P quick-filter input or the Alt+letter type-ahead pattern,
/// rendered in the mini-status (`panel.ministatus` role) in place of the
/// normal status text while either is active — quick filter takes
/// precedence if somehow both are active at once. `type_ahead` is `None`
/// whenever `panel` isn't the active one, so the opposite panel's
/// mini-status is never affected (quick-filter "Quick filter only affects
/// the active panel"; type-ahead-jump "Mini-status display of the active
/// pattern").
fn quick_filter_or_type_ahead_status(panel: &PanelState, type_ahead: Option<&str>) -> Option<String> {
    panel.quick_filter.clone().or_else(|| type_ahead.map(|p| p.to_string()))
}

#[allow(clippy::too_many_arguments)]
pub fn render_panel(
    buf: &mut Buffer,
    area: Rect,
    panel: &PanelState,
    theme: &Theme,
    depth: ColorDepth,
    active: bool,
    identity_lines: &[String; 4],
    opposite: &PanelState,
    type_ahead: Option<&str>,
) {
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
    let right_x = x0 + area.width - 1;

    // Top border with centered title. Quick View replaces the path with a
    // fixed `Quick view` title (additional-panel-modes "Quick View preview
    // of the opposite panel cursor file"); otherwise the active panel shows
    // the M5 git branch-name suffix immediately after the path once its
    // git query has resolved (git-info "Branch-name border suffix on the
    // active panel").
    let title = match (panel.display_mode, active, &panel.git_info.branch) {
        (DisplayMode::QuickView, _, _) => " Quick view ".to_string(),
        (_, true, Some(branch)) => format!(" {} ({branch}) ", panel.cwd.display()),
        (_, _, _) => format!(" {} ", panel.cwd.display()),
    };
    let top = format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(w.saturating_sub(2)));
    buf.set_string(x0, y0, &top, frame_style);
    let title_x = x0 + 1 + ((w.saturating_sub(2)).saturating_sub(display_width(&title)) / 2) as u16;
    buf.set_string(title_x, y0, clip(&title, w.saturating_sub(2)), title_style);

    // The compact tab strip (panel-tabs "Tab strip visibility"), shown only
    // with 2+ tabs, is a full-width row nested directly below the top
    // border — inside this panel's own Rect rather than a row carved out of
    // `layout::compute`'s shared rect — so the clock overlay and F9 menu
    // bar, both keyed to that unmodified outer rect, need no changes
    // (design D4). It has no left/right frame verticals of its own; its
    // `tab.active`/`tab.inactive` backgrounds (cyan/blue) already match the
    // panel frame's palette, so it reads as an extension of the border.
    let has_strip = area.height >= 4 && tab_strip::is_visible(panel);
    if has_strip {
        let strip_area = Rect { x: x0, y: y0 + 1, width: area.width, height: 1 };
        tab_strip::render_tab_strip(buf, strip_area, panel, theme, depth);
    }
    let reserved = if has_strip { 3 } else { 2 }; // top(+strip) + bottom border
    if area.height < reserved {
        return;
    }
    let body_y0 = y0 + if has_strip { 2 } else { 1 };
    let body_h = area.height - reserved;

    // The Ctrl+P quick filter / Alt+letter type-ahead pattern, when either
    // is active, replaces the normal mini-status content (quick-filter
    // "Activating the quick filter"; type-ahead-jump "Mini-status display
    // of the active pattern"). Tree/Quick View compute their own override
    // (the highlighted path / previewed file) below, which takes
    // precedence when present.
    let input_override = quick_filter_or_type_ahead_status(panel, type_ahead);

    // Info mode replaces the whole body — header row included — with the
    // stacked info boxes, keeping only the panel's own double-line border
    // (and the tab strip, if shown).
    if panel.display_mode == DisplayMode::Info {
        draw_side_borders(buf, x0, body_y0, right_x, body_h, frame_style);
        let inner = Rect { x: x0 + 1, y: body_y0, width: area.width - 2, height: body_h };
        info_panel::render_info(buf, inner, theme, depth, &panel.info, &panel.cwd, identity_lines);
        render_bottom_border(buf, area, panel, frame_style, ministatus_style, input_override);
        return;
    }

    // Brief mode: three name-only columns, no header row (additional-panel-
    // modes "Brief display mode").
    if panel.display_mode == DisplayMode::Brief {
        draw_side_borders(buf, x0, body_y0, right_x, body_h, frame_style);
        render_brief_body(buf, x0, body_y0, w, body_h, panel, theme, depth, active);
        render_bottom_border(buf, area, panel, frame_style, ministatus_style, input_override);
        return;
    }

    // Tree mode: a lazily-expanded directory tree of the current drive,
    // driving the opposite panel as the cursor moves (additional-panel-
    // modes "Tree display mode structure and rendering").
    if panel.display_mode == DisplayMode::Tree {
        let mini_status = render_tree_body(buf, x0, body_y0, right_x, w, body_h, panel, theme, depth, active, frame_style);
        render_bottom_border(buf, area, panel, frame_style, ministatus_style, mini_status.or(input_override));
        return;
    }

    // Quick View mode: a read-only preview of the opposite panel's cursor
    // file, reusing the F3 viewer's text-body renderer (additional-panel-
    // modes "Quick View preview of the opposite panel cursor file").
    if panel.display_mode == DisplayMode::QuickView {
        draw_side_borders(buf, x0, body_y0, right_x, body_h, frame_style);
        let mini_status = render_quick_view_body(buf, x0, body_y0, w, body_h, opposite, theme, depth);
        render_bottom_border(buf, area, panel, frame_style, ministatus_style, mini_status.or(input_override));
        return;
    }

    // Full mode: sortable header, entry rows, and — once git info has
    // resolved for a repository directory — the M5 git status-marker
    // column immediately before each name. The column is only reserved
    // once resolved: outside a repo, or while the query is still pending,
    // the layout is laid out identically to before git-info existed at all
    // (git-info "Single-reflow appearance with nothing reserved while
    // pending").
    let has_git = panel.git_info.is_repo();
    let marker_w = if has_git { 1 } else { 0 };
    let name_w = w.saturating_sub(2).saturating_sub(marker_w + SIZE_COL_W + DATE_COL_W + TIME_COL_W + 3);
    let leading = if has_git { " " } else { "" };
    let header = format!(
        "{leading}{}{} {} {} {}",
        pad_to_width(&header_label("Name", SortColumn::Name, panel), name_w),
        "\u{2502}",
        pad_to_width(&header_label("Size", SortColumn::Size, panel), SIZE_COL_W - 2),
        pad_to_width(&header_label("Date", SortColumn::Date, panel), DATE_COL_W - 1),
        pad_to_width("Time", TIME_COL_W - 1),
    );
    buf.set_string(x0, body_y0, "\u{2551}", frame_style);
    buf.set_string(x0 + 1, body_y0, pad_to_width(&header, w.saturating_sub(2)), header_style);
    buf.set_string(right_x, body_y0, "\u{2551}", frame_style);

    // Entry rows. When a quick filter is active the body is narrowed to
    // `visible_indices()` (`..` plus name-matching entries) rather than the
    // full `entries` list, matching `move_cursor`'s cursor-restriction and
    // the spec's "the panel body is narrowed to entries whose displayed
    // name contains the pattern" requirement (quick-filter "Substring
    // narrowing as the pattern is typed").
    let visible = panel.visible_indices();
    let rows_start = body_y0 + 1;
    let rows_h = body_h.saturating_sub(1); // header row
    for row in 0..rows_h {
        let y = rows_start + row;
        buf.set_string(x0, y, "\u{2551}", frame_style);
        buf.set_string(right_x, y, "\u{2551}", frame_style);
        if let Some((entry, is_selected)) = visible.get(row as usize).and_then(|&i| panel.entries.get(i).map(|e| (e, active && i == panel.cursor))) {
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
            let rest = format!(
                "{}{} {} {} {}",
                pad_to_width(&name, name_w),
                "\u{2502}",
                pad_to_width(&size_col, SIZE_COL_W - 2),
                pad_to_width(&date_col, DATE_COL_W - 1),
                pad_to_width(&time_col, TIME_COL_W - 1),
            );
            if has_git {
                let marker = git_marker(panel, entry);
                let marker_style = if is_selected {
                    cursor_style
                } else if let Some((_, role)) = marker {
                    role_style(theme, role, depth)
                } else {
                    style
                };
                buf.set_string(x0 + 1, y, marker.map(|(c, _)| c).unwrap_or(' ').to_string(), marker_style);
                buf.set_string(x0 + 2, y, pad_to_width(&rest, w.saturating_sub(3)), style);
            } else {
                buf.set_string(x0 + 1, y, pad_to_width(&rest, w.saturating_sub(2)), style);
            }
        } else {
            buf.set_string(x0 + 1, y, " ".repeat(w.saturating_sub(2)), file_style);
        }
    }

    render_bottom_border(buf, area, panel, frame_style, ministatus_style, input_override);
}

/// The `\u{2551}` left/right frame verticals for every row of a body region
/// that renders its own interior (Info/Brief/QuickView) — shared so each
/// mode doesn't repeat the loop.
fn draw_side_borders(buf: &mut Buffer, x0: u16, body_y0: u16, right_x: u16, body_h: u16, frame_style: Style) {
    for row in 0..body_h {
        let y = body_y0 + row;
        buf.set_string(x0, y, "\u{2551}", frame_style);
        buf.set_string(right_x, y, "\u{2551}", frame_style);
    }
}

/// Brief mode's body: entry names only, laid out across three columns in
/// column-major order, using `unicode-width` for alignment and preserving
/// the standard directory/file/selected/cursor styling and the `..`
/// up-dir marker (additional-panel-modes "Brief display mode").
#[allow(clippy::too_many_arguments)]
fn render_brief_body(buf: &mut Buffer, x0: u16, body_y0: u16, w: usize, body_h: u16, panel: &PanelState, theme: &Theme, depth: ColorDepth, active: bool) {
    let dir_style = role_style(theme, Role::PanelDirectory, depth);
    let file_style = role_style(theme, Role::PanelFile, depth);
    let cursor_style = role_style(theme, Role::PanelCursor, depth);
    let selected_style = role_style(theme, Role::PanelSelected, depth);

    let inner_w = w.saturating_sub(2);
    let col_w = inner_w / 3;
    let col_widths = [col_w, col_w, inner_w.saturating_sub(col_w * 2)];
    let rows_h = body_h as usize;
    // Narrow to the active quick filter's `visible_indices()` (`..` plus
    // name-matching entries), exactly like Full mode above (quick-filter
    // "Substring narrowing as the pattern is typed").
    let visible = panel.visible_indices();

    for row in 0..rows_h {
        let y = body_y0 + row as u16;
        let mut x = x0 + 1;
        for (c, &cw) in col_widths.iter().enumerate() {
            let pos = c * rows_h + row;
            if let Some((entry, idx)) = visible.get(pos).and_then(|&i| panel.entries.get(i).map(|e| (e, i))) {
                let is_cursor = active && idx == panel.cursor;
                let style = if is_cursor {
                    cursor_style
                } else if entry.kind != EntryKind::ParentDir && panel.selected.contains(&entry.name) {
                    selected_style
                } else if entry.is_dir_like() {
                    dir_style
                } else {
                    file_style
                };
                let text = match entry.kind {
                    EntryKind::ParentDir => "\u{25B6}UP--DIR\u{25C4}".to_string(),
                    _ => display_name_lossy(entry),
                };
                buf.set_string(x, y, pad_to_width(&text, cw), style);
            } else {
                buf.set_string(x, y, " ".repeat(cw), file_style);
            }
            x += cw as u16;
        }
    }
}

/// Tree mode's body: the `Tree` header row, the drive-root row, and every
/// visible node's branch-glyph prefix + name, with the cursor as the
/// standard inverse bar (additional-panel-modes "Tree display mode
/// structure and rendering"; design D7). Returns the mini-status override
/// — the highlighted node's full path — or `None` when Tree mode has no
/// tree state yet (additional-panel-modes "Tree mini-status shows
/// highlighted path").
#[allow(clippy::too_many_arguments)]
fn render_tree_body(
    buf: &mut Buffer,
    x0: u16,
    body_y0: u16,
    right_x: u16,
    w: usize,
    body_h: u16,
    panel: &PanelState,
    theme: &Theme,
    depth: ColorDepth,
    active: bool,
    frame_style: Style,
) -> Option<String> {
    let header_style = role_style(theme, Role::PanelHeader, depth);
    let dir_style = role_style(theme, Role::PanelDirectory, depth);
    let cursor_style = role_style(theme, Role::PanelCursor, depth);
    let inner_w = w.saturating_sub(2);

    buf.set_string(x0, body_y0, "\u{2551}", frame_style);
    buf.set_string(x0 + 1, body_y0, pad_to_width("Tree", inner_w), header_style);
    buf.set_string(right_x, body_y0, "\u{2551}", frame_style);

    let rows_start = body_y0 + 1;
    let rows_h = body_h.saturating_sub(1);
    let cursor_row = panel.tree.as_ref().map(|t| t.cursor);

    for row in 0..rows_h {
        let y = rows_start + row;
        buf.set_string(x0, y, "\u{2551}", frame_style);
        buf.set_string(right_x, y, "\u{2551}", frame_style);
        let is_cursor = active && cursor_row == Some(row as usize);
        match panel.tree.as_ref().and_then(|t| t.nodes.get(row as usize)) {
            Some(node) if node.depth == 0 => {
                let text = node.path.display().to_string();
                buf.set_string(x0 + 1, y, pad_to_width(&text, inner_w), if is_cursor { cursor_style } else { dir_style });
            }
            Some(node) => {
                let name = node.path.file_name().map(|n| n.to_string_lossy().to_uppercase()).unwrap_or_default();
                if is_cursor {
                    let text = format!("{}{}", node.prefix, name);
                    buf.set_string(x0 + 1, y, pad_to_width(&text, inner_w), cursor_style);
                } else {
                    buf.set_string(x0 + 1, y, &node.prefix, frame_style);
                    let name_x = x0 + 1 + display_width(&node.prefix) as u16;
                    let remaining_w = inner_w.saturating_sub(display_width(&node.prefix));
                    buf.set_string(name_x, y, pad_to_width(&name, remaining_w), dir_style);
                }
            }
            None => buf.set_string(x0 + 1, y, " ".repeat(inner_w), dir_style),
        }
    }

    panel.tree.as_ref().and_then(|t| t.selected()).map(|n| n.path.display().to_string())
}

/// Quick View mode's body: the opposite panel cursor file's head, rendered
/// exactly like the F3 viewer's text mode, or a centered `SUB-DIR` marker
/// when the opposite cursor is on a directory (additional-panel-modes
/// "Quick View preview of the opposite panel cursor file", "Quick View
/// directory indicator"; design D7). Returns the mini-status override — the
/// previewed file's name and size.
#[allow(clippy::too_many_arguments)]
fn render_quick_view_body(buf: &mut Buffer, x0: u16, body_y0: u16, w: usize, body_h: u16, opposite: &PanelState, theme: &Theme, depth: ColorDepth) -> Option<String> {
    let text_style = role_style(theme, Role::ViewerText, depth);
    let dir_style = role_style(theme, Role::PanelDirectory, depth);
    let inner_w = w.saturating_sub(2);

    for row in 0..body_h {
        buf.set_string(x0 + 1, body_y0 + row, " ".repeat(inner_w), text_style);
    }

    let Some(entry) = opposite.selected() else { return Some(String::new()) };

    if entry.is_dir_like() {
        let label = "\u{25B6}SUB-DIR\u{25C4}";
        let x = x0 + 1 + (inner_w.saturating_sub(display_width(label)) / 2) as u16;
        let y = body_y0 + body_h / 2;
        buf.set_string(x, y, label, dir_style);
        return Some(format!("{}  SUB-DIR", display_name_lossy(entry)));
    }

    let path = opposite.cwd.join(&entry.name);
    let Ok(source) = ByteSource::open(&path) else { return Some(display_name_lossy(entry)) };
    let mut viewer = ViewerState::new(path, source.len());
    viewer.wrap = true;
    render_text_body(buf, x0 + 1, body_y0, inner_w, body_h as usize, &viewer, &source, text_style, text_style);
    Some(format!("{}  {}", display_name_lossy(entry), format_size(entry.size)))
}

/// Bottom border with the mini-status embedded, shared by every display
/// mode. `status_override`, when `Some`, replaces the default
/// error/streaming/selected-entry status text (Tree's highlighted path,
/// Quick View's previewed file) — `None` keeps the M1-M4 behavior.
fn render_bottom_border(
    buf: &mut Buffer,
    area: Rect,
    panel: &PanelState,
    frame_style: Style,
    ministatus_style: Style,
    status_override: Option<String>,
) {
    let w = area.width as usize;
    // An inline error (a failed listing, a failed F3/F4 dispatch — §7 "panel
    // shows an inline error state") takes over the mini-status line until
    // the next successful operation clears it (`begin_new_listing`).
    let status_text = match status_override {
        Some(text) => text,
        None => match &panel.last_error {
            Some(message) => message.clone(),
            None => match panel.progress {
                ListingProgress::Streaming { count } => reading_status(count),
                ListingProgress::Complete { .. } => panel.selected().map(entry_status_line).unwrap_or_default(),
            },
        },
    };
    let bottom_y = area.y + area.height - 1;
    let inner_w = w.saturating_sub(2);
    let bracketed = if status_text.is_empty() { String::new() } else { format!(" {status_text} ") };
    let bracketed = if display_width(&bracketed) > inner_w { clip(&bracketed, inner_w) } else { bracketed };
    let fill_total = inner_w.saturating_sub(display_width(&bracketed));
    let left_fill = fill_total / 2;
    let right_fill = fill_total - left_fill;
    let bottom = format!(
        "\u{255A}{}{}{}\u{255D}",
        "\u{2550}".repeat(left_fill),
        bracketed,
        "\u{2550}".repeat(right_fill),
    );
    buf.set_string(area.x, bottom_y, &bottom, frame_style);
    if !bracketed.is_empty() {
        buf.set_string(area.x + 1 + left_fill as u16, bottom_y, &bracketed, ministatus_style);
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
