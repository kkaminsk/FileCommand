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
    display_name_lossy, entry_status_parts, format_date, format_size, format_time, pad_to_width, reading_status, sort_arrow, truncate_with_ellipsis,
    SortColumn,
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

/// The Name column never shrinks below this many display cells in Full
/// mode — the anchor `MIN_NAME_W` for the column ladder (design D3).
const MIN_NAME_W: usize = 12;

/// Which of the optional Full-mode columns currently render. `size`,
/// `date`, and `time` are only ever dropped rightmost-first — `date` true
/// implies `size` true, and `time` true implies both `size` and `date` true
/// — an invariant [`choose_columns`] maintains by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ColumnSet {
    size: bool,
    date: bool,
    time: bool,
}

impl ColumnSet {
    const FULL: ColumnSet = ColumnSet { size: true, date: true, time: true };
    const NAME_ONLY: ColumnSet = ColumnSet { size: false, date: false, time: false };

    /// Display cells consumed by the columns after Name (including the
    /// separator pipe and inter-column spaces), or 0 when only Name renders
    /// — in which case Name spans the full interior with no pipe or slack.
    /// `SIZE_COL_W`/`DATE_COL_W`/`TIME_COL_W` are unchanged from the fixed
    /// 80x24 layout, so the reserved width (and therefore the rendered
    /// output) at the nominal size is byte-identical to before this change
    /// (design D3's anchor).
    fn reserved_w(self) -> usize {
        if !self.size {
            return 0;
        }
        let mut r = SIZE_COL_W;
        if self.date {
            r += DATE_COL_W;
        }
        if self.time {
            r += TIME_COL_W;
        }
        r + 3
    }
}

/// Chooses the widest column set from the ladder `Name+Size+Date+Time →
/// Name+Size+Date → Name+Size → Name` that keeps the Name column at least
/// `MIN_NAME_W` display cells wide, given `interior_w` (the panel's width
/// inside its border) and `marker_w` (1 when the git status-marker column
/// is reserved, else 0). Pure in its inputs, so degradation is reversible
/// by construction: the same `(interior_w, marker_w)` always yields the
/// same set, whether the panel is narrowing or widening (panel-navigation
/// "Full display mode layout"; responsive-layout "Degraded band and
/// per-panel breakpoints").
fn choose_columns(interior_w: usize, marker_w: usize) -> ColumnSet {
    let mut cols = ColumnSet::FULL;
    loop {
        let name_w = interior_w.saturating_sub(marker_w).saturating_sub(cols.reserved_w());
        if name_w >= MIN_NAME_W || cols == ColumnSet::NAME_ONLY {
            return cols;
        }
        cols = if cols.time {
            ColumnSet { time: false, ..cols }
        } else if cols.date {
            ColumnSet { date: false, ..cols }
        } else {
            ColumnSet::NAME_ONLY
        };
    }
}

/// Assembles a Full-mode row (or header row) string from its four fields,
/// appending only the blocks `cols` selects — the same function backs both
/// the header row and every entry row, so "the header row shows exactly the
/// columns currently rendered" holds by construction.
fn format_full_row(name_w: usize, cols: ColumnSet, name: &str, size: &str, date: &str, time: &str) -> String {
    let mut s = pad_to_width(name, name_w);
    if cols.size {
        s.push('\u{2502}');
        s.push(' ');
        s.push_str(&pad_to_width(size, SIZE_COL_W - 2));
    }
    if cols.date {
        s.push(' ');
        s.push_str(&pad_to_width(date, DATE_COL_W - 1));
    }
    if cols.time {
        s.push(' ');
        s.push_str(&pad_to_width(time, TIME_COL_W - 1));
    }
    s
}

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

/// The overflow-only right-border scrollbar's thumb window within a
/// `track_rows`-cell vertical track, or `None` when the visible list fits
/// inside `window_capacity` positions — in which case the border renders as
/// the unbroken `║` double-line, byte-identical to before this change
/// (panel-navigation "Scrollbar indicator on overflow"; design D5).
/// `Some((thumb_start, thumb_len))` otherwise: `thumb_len` is `max(1,
/// round(track_rows * window_capacity / total))` cells, and `thumb_start`
/// places the thumb so it touches the track's top exactly when `offset` is 0
/// and its bottom exactly when the window shows the list's last position.
/// Full and Tree mode pass `window_capacity == track_rows`, matching D5's
/// `rows * rows / total` literally; Brief mode passes the larger `columns *
/// rows_h` capacity so the thumb reflects the cursor window's linear
/// position through the whole list, not just one column's share of it.
fn scrollbar_thumb(track_rows: usize, window_capacity: usize, total: usize, offset: usize) -> Option<(usize, usize)> {
    if track_rows == 0 || total <= window_capacity {
        return None;
    }
    let thumb_len = (((track_rows * window_capacity) as f64 / total as f64).round() as usize).clamp(1, track_rows);
    let travel = track_rows - thumb_len;
    let max_offset = total - window_capacity;
    let start = if travel == 0 { 0 } else { ((offset.min(max_offset) as f64 / max_offset as f64) * travel as f64).round() as usize }.min(travel);
    Some((start, thumb_len))
}

/// Paints one right-border cell at track-relative `row`: the unbroken `║`
/// frame glyph when `thumb` is `None`, else `█` inside the thumb window and
/// `░` elsewhere in the whole track, both in the `panel.scrollbar` role
/// (panel-navigation "Scrollbar indicator on overflow"). Shared by Full,
/// Brief, and Tree bodies so the glyph rules live in exactly one place.
fn draw_scrollbar_cell(buf: &mut Buffer, x: u16, y: u16, row: usize, thumb: Option<(usize, usize)>, frame_style: Style, scrollbar_style: Style) {
    match thumb {
        None => buf.set_string(x, y, "\u{2551}", frame_style),
        Some((start, len)) if row >= start && row < start + len => buf.set_string(x, y, "\u{2588}", scrollbar_style),
        Some(_) => buf.set_string(x, y, "\u{2591}", scrollbar_style),
    }
}

/// The panel's top-border title string — Quick View's fixed label, or the
/// current directory path with the M5 git branch-name suffix on the active
/// panel — exactly as `render_panel` centers and clips it into the top
/// border. Exposed so callers (the clock/title collision check in
/// `views::mod`) can compute the title's on-screen span without
/// duplicating this construction (responsive-layout "Chrome degradation";
/// git-info "Branch-name border suffix on the active panel").
pub fn panel_title(panel: &PanelState, active: bool) -> String {
    match (panel.display_mode, active, &panel.git_info.branch) {
        (DisplayMode::QuickView, _, _) => " Quick view ".to_string(),
        (_, true, Some(branch)) => format!(" {} ({branch}) ", panel.cwd.display()),
        (_, _, _) => format!(" {} ", panel.cwd.display()),
    }
}

/// The clickable regions `render_panel` currently draws for `panel` at
/// `area` (mouse-input "Hit-testing stays in the TUI"): the whole rect, the
/// title row, and — in Full mode only — each drawn entry row keyed by name.
/// Mirrors `render_panel`'s own `has_strip`/`reserved`/`body_y0`/`rows_h`
/// geometry exactly, rather than re-deriving it independently, so this can
/// never describe a row that isn't actually where `render_panel` painted
/// it. Brief/Tree/Info/QuickView modes record `area`/`title` only — no
/// discrete per-file row exists in those layouts the way it does in Full
/// mode.
pub fn hit_test(area: Rect, panel: &PanelState) -> crate::hitmap::PanelHits {
    let mut hits = crate::hitmap::PanelHits { area, ..Default::default() };
    if area.width < 4 || area.height < 3 {
        return hits;
    }
    let x0 = area.x;
    let y0 = area.y;
    let right_x = x0 + area.width - 1;
    hits.title = Rect { x: x0, y: y0, width: area.width, height: 1 };

    let has_strip = area.height >= 4 && tab_strip::is_visible(panel);
    let reserved = if has_strip { 3 } else { 2 };
    if area.height < reserved {
        return hits;
    }
    let body_y0 = y0 + if has_strip { 2 } else { 1 };
    let body_h = area.height - reserved;

    if panel.display_mode != DisplayMode::Full {
        return hits;
    }
    let rows_start = body_y0 + 1;
    let rows_h = body_h.saturating_sub(1);
    let visible = panel.visible_indices();
    for row in 0..rows_h {
        let y = rows_start + row;
        let Some(entry) = visible.get(panel.scroll_offset + row as usize).and_then(|&i| panel.entries.get(i)) else { continue };
        let rect = Rect { x: x0 + 1, y, width: right_x.saturating_sub(x0 + 1), height: 1 };
        hits.rows.push((rect, entry.name.clone()));
    }
    hits
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
    // A clipboard action's feedback (clipboard-export "Clipboard feedback")
    // takes over the mini-status role's color when it reports failure — the
    // same error role dialogs use — otherwise the normal mini-status role.
    let ministatus_style = match &panel.clipboard_feedback {
        Some(feedback) if feedback.is_error => role_style(theme, Role::DialogError, depth),
        _ => role_style(theme, Role::PanelMinistatus, depth),
    };

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
    let title = panel_title(panel, active);
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
    // Clipboard feedback (clipboard-export "Clipboard feedback") wins over
    // the quick-filter/type-ahead pattern and every mode-specific override
    // below — it is the most recent thing the user asked for, is always
    // transient (cleared by `crate::update` on the very next command or
    // after ~3s), and Ctrl+C/Ctrl+Ins/Ctrl+Shift+Ins work over the panels
    // regardless of the active display mode.
    let clipboard_override = panel.clipboard_feedback.as_ref().map(|f| f.message.clone());

    // Info mode replaces the whole body — header row included — with the
    // stacked info boxes, keeping only the panel's own double-line border
    // (and the tab strip, if shown).
    if panel.display_mode == DisplayMode::Info {
        draw_side_borders(buf, x0, body_y0, right_x, body_h, frame_style);
        let inner = Rect { x: x0 + 1, y: body_y0, width: area.width - 2, height: body_h };
        info_panel::render_info(buf, inner, theme, depth, &panel.info, &panel.cwd, identity_lines);
        render_bottom_border(buf, area, panel, frame_style, ministatus_style, clipboard_override.clone().or_else(|| input_override.clone()));
        return;
    }

    // Brief mode: three name-only columns, no header row (additional-panel-
    // modes "Brief display mode").
    if panel.display_mode == DisplayMode::Brief {
        draw_side_borders(buf, x0, body_y0, right_x, body_h, frame_style);
        render_brief_body(buf, x0, body_y0, right_x, w, body_h, panel, theme, depth, active);
        render_bottom_border(buf, area, panel, frame_style, ministatus_style, clipboard_override.clone().or_else(|| input_override.clone()));
        return;
    }

    // Tree mode: a lazily-expanded directory tree of the current drive,
    // driving the opposite panel as the cursor moves (additional-panel-
    // modes "Tree display mode structure and rendering").
    if panel.display_mode == DisplayMode::Tree {
        let mini_status = render_tree_body(buf, x0, body_y0, right_x, w, body_h, panel, theme, depth, active, frame_style);
        render_bottom_border(buf, area, panel, frame_style, ministatus_style, clipboard_override.clone().or(mini_status).or_else(|| input_override.clone()));
        return;
    }

    // Quick View mode: a read-only preview of the opposite panel's cursor
    // file, reusing the F3 viewer's text-body renderer (additional-panel-
    // modes "Quick View preview of the opposite panel cursor file").
    if panel.display_mode == DisplayMode::QuickView {
        draw_side_borders(buf, x0, body_y0, right_x, body_h, frame_style);
        let mini_status = render_quick_view_body(buf, x0, body_y0, w, body_h, opposite, theme, depth);
        render_bottom_border(buf, area, panel, frame_style, ministatus_style, clipboard_override.clone().or(mini_status).or_else(|| input_override.clone()));
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
    let interior_w = w.saturating_sub(2);
    let cols = choose_columns(interior_w, marker_w);
    let name_w = interior_w.saturating_sub(marker_w).saturating_sub(cols.reserved_w());
    let leading = if has_git { " " } else { "" };
    let header = format!(
        "{leading}{}",
        format_full_row(
            name_w,
            cols,
            &header_label("Name", SortColumn::Name, panel),
            &header_label("Size", SortColumn::Size, panel),
            &header_label("Date", SortColumn::Date, panel),
            "Time",
        )
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
    let scrollbar_style = role_style(theme, Role::PanelScrollbar, depth);
    let thumb = scrollbar_thumb(rows_h as usize, rows_h as usize, visible.len(), panel.scroll_offset);
    for row in 0..rows_h {
        let y = rows_start + row;
        buf.set_string(x0, y, "\u{2551}", frame_style);
        draw_scrollbar_cell(buf, right_x, y, row as usize, thumb, frame_style, scrollbar_style);
        if let Some((entry, is_selected)) = visible.get(panel.scroll_offset + row as usize).and_then(|&i| panel.entries.get(i).map(|e| (e, active && i == panel.cursor))) {
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
            let rest = format_full_row(name_w, cols, &name, &size_col, &date_col, &time_col);
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

    render_bottom_border(buf, area, panel, frame_style, ministatus_style, clipboard_override.or(input_override));
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
fn render_brief_body(buf: &mut Buffer, x0: u16, body_y0: u16, right_x: u16, w: usize, body_h: u16, panel: &PanelState, theme: &Theme, depth: ColorDepth, active: bool) {
    let dir_style = role_style(theme, Role::PanelDirectory, depth);
    let file_style = role_style(theme, Role::PanelFile, depth);
    let cursor_style = role_style(theme, Role::PanelCursor, depth);
    let selected_style = role_style(theme, Role::PanelSelected, depth);
    let scrollbar_style = role_style(theme, Role::PanelScrollbar, depth);
    let frame_style = role_style(theme, Role::PanelFrame, depth);

    let inner_w = w.saturating_sub(2);
    // `max(1, floor(interior_width / 12))` columns, equal widths with the
    // division remainder given to the last column — at interior 38 this is
    // still exactly 3 columns of 12/12/14, byte-identical to the old
    // hardcoded `interior / 3` (design D4; additional-panel-modes "Brief
    // display mode").
    let n_cols = (inner_w / 12).max(1);
    let base_w = inner_w / n_cols;
    let mut col_widths = vec![base_w; n_cols];
    if let Some(last) = col_widths.last_mut() {
        *last += inner_w - base_w * n_cols;
    }
    let rows_h = body_h as usize;
    // Narrow to the active quick filter's `visible_indices()` (`..` plus
    // name-matching entries), exactly like Full mode above (quick-filter
    // "Substring narrowing as the pattern is typed").
    let visible = panel.visible_indices();
    // The overflow-only scrollbar reflects the cursor window's linear
    // position through the whole visible list, not one column's share of it
    // — window_capacity is every position the `n_cols`-wide window shows at
    // once (additional-panel-modes "Brief mode column scrolling").
    let thumb = scrollbar_thumb(rows_h, n_cols * rows_h, visible.len(), panel.scroll_offset);

    for row in 0..rows_h {
        let y = body_y0 + row as u16;
        draw_scrollbar_cell(buf, right_x, y, row, thumb, frame_style, scrollbar_style);
        let mut x = x0 + 1;
        for (c, &cw) in col_widths.iter().enumerate() {
            let pos = panel.scroll_offset + c * rows_h + row;
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
    let scrollbar_style = role_style(theme, Role::PanelScrollbar, depth);
    let inner_w = w.saturating_sub(2);

    buf.set_string(x0, body_y0, "\u{2551}", frame_style);
    buf.set_string(x0 + 1, body_y0, pad_to_width("Tree", inner_w), header_style);
    buf.set_string(right_x, body_y0, "\u{2551}", frame_style);

    let rows_start = body_y0 + 1;
    let rows_h = body_h.saturating_sub(1);
    let cursor_row = panel.tree.as_ref().map(|t| t.cursor);
    let tree_offset = panel.tree.as_ref().map(|t| t.scroll_offset).unwrap_or(0);
    let tree_len = panel.tree.as_ref().map(|t| t.nodes.len()).unwrap_or(0);
    let thumb = scrollbar_thumb(rows_h as usize, rows_h as usize, tree_len, tree_offset);

    for row in 0..rows_h {
        let y = rows_start + row;
        buf.set_string(x0, y, "\u{2551}", frame_style);
        draw_scrollbar_cell(buf, right_x, y, row as usize, thumb, frame_style, scrollbar_style);
        let is_cursor = active && cursor_row == Some(tree_offset + row as usize);
        match panel.tree.as_ref().and_then(|t| t.nodes.get(tree_offset + row as usize)) {
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
    let inner_w = w.saturating_sub(2);
    // The bracketed status text is wrapped in a leading/trailing space (see
    // `bracketed` below), so the content itself has 2 fewer cells to work
    // with than the interior.
    let status_budget = inner_w.saturating_sub(2);
    // An inline error (a failed listing, a failed F3/F4 dispatch — §7 "panel
    // shows an inline error state") takes over the mini-status line until
    // the next successful operation clears it (`begin_new_listing`). The
    // multi-select summary takes precedence over the per-entry status while
    // one or more entries are selected (selection "Selection count and size
    // summary"); both drop fields/truncate with `…` in ladder order as the
    // panel narrows (responsive-layout "Chrome degradation").
    let status_text = match status_override {
        Some(text) => text,
        None => match &panel.last_error {
            Some(message) => message.clone(),
            None => match panel.progress {
                ListingProgress::Streaming { count } => reading_status(count),
                ListingProgress::Complete { .. } => match panel.selection_status() {
                    Some(summary) => truncate_with_ellipsis(&summary, status_budget),
                    None => panel.selected().map(|e| fit_mini_status(&entry_status_parts(e), status_budget)).unwrap_or_default(),
                },
            },
        },
    };
    let bottom_y = area.y + area.height - 1;
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

/// Assembles the widest fitting rendering of `parts` (name/size/date/time,
/// from `entry_status_parts`) that fits `max_w` display columns, dropping
/// optional fields in the same rightmost-first order as the Full-mode
/// column ladder — time, then date, then size — and truncating the name
/// with `…` only once every other field is gone (responsive-layout "Chrome
/// degradation"; design D3).
fn fit_mini_status(parts: &(String, Option<String>, Option<String>, Option<String>), max_w: usize) -> String {
    let (name, size, date, time) = parts;
    let build = |include_size: bool, include_date: bool, include_time: bool| -> String {
        let mut s = name.clone();
        if include_size {
            if let Some(sz) = size {
                s.push_str("  ");
                s.push_str(sz);
            }
        }
        if include_date {
            if let Some(d) = date {
                s.push_str("  ");
                s.push_str(d);
            }
        }
        if include_time {
            if let Some(t) = time {
                s.push(' ');
                s.push_str(t);
            }
        }
        s
    };
    for &(is, id, it) in &[(true, true, true), (true, true, false), (true, false, false)] {
        let candidate = build(is, id, it);
        if display_width(&candidate) <= max_w {
            return candidate;
        }
    }
    truncate_with_ellipsis(name, max_w)
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

/// The Brief-mode column widths for `inner_w` — `max(1, floor(inner_w /
/// 12))` equal-width columns with the division remainder on the last
/// column (design D4; additional-panel-modes "Brief display mode"). Shared
/// between `render_brief_body` and its tests below.
#[cfg(test)]
fn brief_column_widths(inner_w: usize) -> Vec<usize> {
    let n_cols = (inner_w / 12).max(1);
    let base_w = inner_w / n_cols;
    let mut col_widths = vec![base_w; n_cols];
    if let Some(last) = col_widths.last_mut() {
        *last += inner_w - base_w * n_cols;
    }
    col_widths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choose_columns_all_four_at_nominal_interior() {
        // interior 38, no git marker — the 80x24 default-split anchor
        // (design D3): all four columns render, Name gets 13 cells.
        let cols = choose_columns(38, 0);
        assert_eq!(cols, ColumnSet::FULL);
        let name_w = 38usize.saturating_sub(0).saturating_sub(cols.reserved_w());
        assert_eq!(name_w, 13);
    }

    #[test]
    fn choose_columns_drops_time_first() {
        // Narrow just enough that Name+Size+Date+Time would leave Name
        // under 12, but Name+Size+Date still fits at >= 12.
        let full_name_w = |w: usize| w.saturating_sub(ColumnSet::FULL.reserved_w());
        let w = (0usize..40).find(|&w| full_name_w(w) < MIN_NAME_W && w.saturating_sub(ColumnSet { time: false, ..ColumnSet::FULL }.reserved_w()) >= MIN_NAME_W).unwrap();
        let cols = choose_columns(w, 0);
        assert_eq!(cols, ColumnSet { size: true, date: true, time: false });
    }

    #[test]
    fn choose_columns_drops_date_next() {
        let two_col = ColumnSet { size: true, date: false, time: false };
        let w = (0usize..40)
            .find(|&w| w.saturating_sub(ColumnSet { time: false, ..ColumnSet::FULL }.reserved_w()) < MIN_NAME_W && w.saturating_sub(two_col.reserved_w()) >= MIN_NAME_W)
            .unwrap();
        let cols = choose_columns(w, 0);
        assert_eq!(cols, two_col);
    }

    #[test]
    fn choose_columns_name_only_at_minimum_panel_width() {
        // A 20-column panel (the panel-split minimum) has interior 18.
        let cols = choose_columns(18, 0);
        assert_eq!(cols, ColumnSet::NAME_ONLY);
        let name_w = 18usize.saturating_sub(cols.reserved_w());
        assert!(name_w >= MIN_NAME_W, "name_w={name_w} spans the interior at the panel minimum");
        assert_eq!(name_w, 18);
    }

    #[test]
    fn choose_columns_accounts_for_git_marker_width() {
        // The same interior width with a reserved marker column drops a
        // column sooner than without one.
        let without_marker = choose_columns(28, 0);
        let with_marker = choose_columns(28, 1);
        assert!(with_marker != ColumnSet::FULL || without_marker == ColumnSet::FULL);
    }

    #[test]
    fn fit_mini_status_shows_all_fields_when_it_fits() {
        let parts = ("a.txt".to_string(), Some("100".to_string()), Some("01-02-26".to_string()), Some("03:04".to_string()));
        assert_eq!(fit_mini_status(&parts, 80), "a.txt  100  01-02-26 03:04");
    }

    #[test]
    fn fit_mini_status_drops_time_then_date_then_size() {
        let parts = ("a.txt".to_string(), Some("100".to_string()), Some("01-02-26".to_string()), Some("03:04".to_string()));
        let full = fit_mini_status(&parts, 80);
        let no_time = fit_mini_status(&parts, display_width(&full) - 1);
        assert_eq!(no_time, "a.txt  100  01-02-26");
        let no_date = fit_mini_status(&parts, display_width(&no_time) - 1);
        assert_eq!(no_date, "a.txt  100");
        let name_only = fit_mini_status(&parts, display_width(&no_date) - 1);
        assert_eq!(name_only, "a.txt");
    }

    #[test]
    fn fit_mini_status_truncates_the_name_last() {
        let parts = ("a-very-long-file-name.txt".to_string(), Some("100".to_string()), None, None);
        assert_eq!(fit_mini_status(&parts, 5), "a-ve\u{2026}");
    }

    #[test]
    fn brief_columns_three_at_nominal_interior() {
        // interior 38 at 80x24 default split — byte-identical to the old
        // hardcoded `interior / 3`.
        assert_eq!(brief_column_widths(38), vec![12, 12, 14]);
    }

    #[test]
    fn brief_columns_five_at_interior_60() {
        assert_eq!(brief_column_widths(60).len(), 5);
    }

    #[test]
    fn brief_columns_single_at_interior_23() {
        let widths = brief_column_widths(23);
        assert_eq!(widths, vec![23]);
    }

    #[test]
    fn brief_columns_never_zero_even_at_tiny_widths() {
        assert_eq!(brief_column_widths(0), vec![0]);
        assert_eq!(brief_column_widths(1), vec![1]);
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn choose_columns_never_yields_name_under_minimum(
                interior_w in 20usize..80,
                marker_w in 0usize..2,
            ) {
                let cols = choose_columns(interior_w, marker_w);
                let name_w = interior_w.saturating_sub(marker_w).saturating_sub(cols.reserved_w());
                // Below the panel minimum's interior, only the floor
                // (name-only, spanning what's left) is reachable — the
                // invariant is guaranteed only once the ladder has room to
                // work with, i.e. at or above the true minimum interior.
                if cols != ColumnSet::NAME_ONLY {
                    prop_assert!(name_w >= MIN_NAME_W);
                }
            }

            #[test]
            fn choose_columns_is_monotonic_in_width(
                marker_w in 0usize..2,
                narrow in 20usize..50,
                widen in 0usize..30,
            ) {
                let wide = narrow + widen;
                let narrow_cols = choose_columns(narrow, marker_w);
                let wide_cols = choose_columns(wide, marker_w);
                // A superset relation: every column the narrower width
                // shows, the wider width shows too.
                prop_assert!(!narrow_cols.size || wide_cols.size);
                prop_assert!(!narrow_cols.date || wide_cols.date);
                prop_assert!(!narrow_cols.time || wide_cols.time);
            }

            #[test]
            fn brief_column_count_matches_direct_formula(inner_w in 0usize..200) {
                let widths = brief_column_widths(inner_w);
                let expected_n = (inner_w / 12).max(1);
                prop_assert_eq!(widths.len(), expected_n);
                prop_assert_eq!(widths.iter().sum::<usize>(), inner_w);
            }
        }
    }
}
