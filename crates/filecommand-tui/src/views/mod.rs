pub mod clock;
pub mod command_line;
pub mod conflict_dialog;
pub mod delete_confirm;
pub mod destination_input;
pub mod drive_select;
pub mod editor;
pub mod error_dialog;
pub mod file_action_menu;
pub mod find_file;
pub mod fuzzy_jump;
pub mod header;
pub mod help;
pub mod history_dialog;
pub mod info_panel;
pub mod keybar;
pub mod menubar;
pub mod panel;
pub mod placeholder;
pub mod progress_dialog;
pub mod quit_dialog;
pub mod skipped_summary;
pub mod splash;
pub mod startup_warning;
pub mod tab_strip;
pub mod theme_picker;
pub mod user_menu;
pub mod viewer;

use filecommand_core::anim;
use filecommand_core::dialogs::OverlayRect;
use filecommand_core::fs_ops::dialog::RunningDialog;
use filecommand_core::line_edit::LineEdit;
use filecommand_core::listing::display_width;
use filecommand_core::panel::PanelState;
use filecommand_core::theme::{ColorDepth, Role};
use filecommand_core::update::ButtonId;
use filecommand_core::viewer::ByteSource;
use filecommand_core::{Overlay, PanelSide, State, UiPhase};
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;

use crate::hitmap::HitMap;
use crate::layout;
use crate::style::role_style;

// ---------------------------------------------------------------------
// Dialog geometry shared by the renderer and the hit map
// (reducer-overlay-structure design D8)
// ---------------------------------------------------------------------

/// One dialog's or menu's geometry for a frame: computed once by the
/// module's `layout` function, then both painted from (`draw_*`) and
/// recorded into the [`HitMap`] by `render`'s dispatch — so the cells a
/// click maps to a button or row are, by construction, exactly the cells
/// where that button's label or that row is drawn (mouse-input "Click
/// targets match the rendered geometry"). The rectangle itself still comes
/// from the unified overlay geometry rule (`filecommand_core::dialogs::
/// overlay_rect`; responsive-layout "Unified overlay geometry").
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DialogLayout {
    /// The dialog's rectangle on screen, frame included.
    pub rect: Rect,
    /// Each clickable button's cells — exactly the cells its label occupies
    /// — and what it activates.
    pub buttons: Vec<(Rect, ButtonId)>,
    /// Each clickable row (menus), keyed by its position in the model's
    /// entry list.
    pub rows: Vec<(Rect, usize)>,
    /// The content strings the renderer paints, built once here so a
    /// button's hit rectangle and its painted label come from the same text.
    /// What each entry is depends on the dialog; see its `layout`.
    pub lines: Vec<String>,
}

/// The cells each `label` occupies inside `text`, drawn starting at `(x,
/// y)`: the button span the hit map records for it. A label the row's
/// truncation dropped gets no span, so a clipped dialog never offers a
/// button it does not show.
pub fn label_spans<T: Copy>(text: &str, x: u16, y: u16, labels: &[(&str, T)]) -> Vec<(Rect, T)> {
    labels
        .iter()
        .filter_map(|(label, id)| {
            let byte_idx = text.find(label)?;
            let col = display_width(&text[..byte_idx]) as u16;
            Some((Rect { x: x + col, y, width: display_width(label) as u16, height: 1 }, *id))
        })
        .collect()
}

// ---------------------------------------------------------------------
// Shared single-line text field (text-field-editing; design D5)
// ---------------------------------------------------------------------

/// Draw one [`LineEdit`]'s visible window at `(x, y)`: `window(width)`
/// chars of text followed by dot padding out to `width` cells — the
/// bracket-and-dots field body used by the destination/name input, the
/// find-file pattern, and the fuzzy-jump pattern (callers draw any `[` `]`
/// brackets themselves). Returns the screen column the terminal cursor
/// should sit on: the caret's column within the window, clamped onto the
/// last cell when the caret is at the end of a full window (`window`'s
/// `caret_col == width` case — the cursor can never land past the field).
pub fn draw_text_field(buf: &mut Buffer, x: u16, y: u16, width: usize, edit: &LineEdit, style: ratatui::style::Style) -> u16 {
    if width == 0 {
        return x;
    }
    let (start, caret_col) = edit.window(width);
    let slice: String = edit.text().chars().skip(start).take(width).collect();
    let pad = width.saturating_sub(display_width(&slice));
    buf.set_string(x, y, format!("{slice}{}", ".".repeat(pad)), style);
    // `caret_col` counts window chars; the cursor column is the display
    // width of the chars before it (the two differ only for wide chars).
    let caret_cells = display_width(&slice.chars().take(caret_col).collect::<String>());
    x + caret_cells.min(width - 1) as u16
}

// ---------------------------------------------------------------------
// Reveal helpers (ui-animations; design D3)
//
// Every animated surface is drawn *finished* by its ordinary view function;
// these helpers only decide how much of that finished drawing is on screen
// this frame. The surface is rendered into a scratch buffer pre-filled with
// a sentinel cell, so the cells the view actually touched — and therefore
// the surface's exact bounding rectangle — are known without any per-view
// geometry code, and only the revealed part is copied onto the real buffer.
// Content under the window is the real finished surface, never placeholder
// art, and hit-maps (built from the final layouts in `render`) are
// unaffected. With `now == None` (animations off) or once the duration has
// elapsed, the view draws straight onto the buffer: byte-identical to the
// pre-animation rendering.
// ---------------------------------------------------------------------

/// The marker a scratch buffer starts filled with: a cell still equal to it
/// after a view ran was never touched. No view draws U+0001.
fn sentinel() -> Cell {
    Cell::new("\u{1}")
}

/// The bounding rectangle of the cells `scratch` holds that differ from the
/// sentinel — i.e. everything the view drew — or `None` if it drew nothing.
fn touched_bounds(scratch: &Buffer) -> Option<Rect> {
    let sentinel = sentinel();
    let area = *scratch.area();
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (u16::MAX, u16::MAX, 0u16, 0u16);
    let mut any = false;
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if scratch[(x, y)] != sentinel {
                any = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    any.then(|| Rect { x: min_x, y: min_y, width: max_x - min_x + 1, height: max_y - min_y + 1 })
}

/// Copy the cells of `src` inside `rect` that the view touched onto `dst`.
fn blit(dst: &mut Buffer, src: &Buffer, rect: Rect) {
    let sentinel = sentinel();
    let rect = rect.intersection(*dst.area());
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            let cell = &src[(x, y)];
            if *cell != sentinel {
                dst[(x, y)] = cell.clone();
            }
        }
    }
}

/// Run `draw` for a surface stamped at `stamp` and last-ticked at `now`.
/// While the animation is in flight, `draw` renders into a scratch buffer
/// and `reveal` copies the visible part onto `buf`, given the surface's
/// bounding rectangle and the per-mille progress; otherwise `draw` paints
/// `buf` directly.
fn staged(
    buf: &mut Buffer,
    now: Option<u64>,
    stamp: u64,
    duration_ms: u64,
    draw: impl FnOnce(&mut Buffer),
    reveal: impl FnOnce(&mut Buffer, &Buffer, Rect, u16),
) {
    let Some(now) = now else { return draw(buf) };
    let elapsed = now.saturating_sub(stamp);
    if elapsed >= duration_ms {
        return draw(buf);
    }
    let mut scratch = Buffer::filled(*buf.area(), sentinel());
    draw(&mut scratch);
    if let Some(bounds) = touched_bounds(&scratch) {
        reveal(buf, &scratch, bounds, anim::progress(elapsed, duration_ms));
    }
}

/// Modal dialog entrance: a centered window grows from 2x1 to the dialog's
/// full rectangle over `DIALOG_IRIS_MS`, showing the finished dialog through
/// it (ui-animations "Modal dialogs open with an iris").
fn iris(buf: &mut Buffer, now: Option<u64>, stamp: u64, draw: impl FnOnce(&mut Buffer)) {
    staged(buf, now, stamp, anim::DIALOG_IRIS_MS, draw, |dst, src, bounds, progress| {
        let finish = OverlayRect { x: bounds.x, y: bounds.y, width: bounds.width, height: bounds.height };
        let w = anim::iris_rect(finish, progress);
        blit(dst, src, Rect { x: w.x, y: w.y, width: w.width, height: w.height });
    });
}

/// Pop-up menu entrance: the frame's top row first, item rows added
/// downward, the bottom frame edge tracking the last revealed row so the box
/// is never taller than its final rectangle (ui-animations "Pop-up menus
/// unfold and highlights settle").
fn unfold(buf: &mut Buffer, now: Option<u64>, stamp: u64, draw: impl FnOnce(&mut Buffer)) {
    staged(buf, now, stamp, anim::POPUP_UNFOLD_MS, draw, |dst, src, bounds, progress| {
        if bounds.height < 3 {
            return blit(dst, src, bounds);
        }
        let interior = (bounds.height - 2) as usize;
        let shown = anim::reveal_count(progress, interior) as u16;
        // Top frame row plus the first `shown` item rows, in place.
        blit(dst, src, Rect { height: shown + 1, ..bounds });
        // The bottom frame edge, moved up to sit just under the last row.
        let src_y = bounds.y + bounds.height - 1;
        let dst_y = bounds.y + 1 + shown;
        let sentinel = sentinel();
        for x in bounds.left()..bounds.right() {
            let cell = &src[(x, src_y)];
            if *cell != sentinel {
                dst[(x, dst_y)] = cell.clone();
            }
        }
    });
}

/// F9 bar entrance: columns of the bar row are painted from the left edge
/// (pulldown-menus "Menu bar entrance sweep").
fn sweep(buf: &mut Buffer, now: Option<u64>, stamp: u64, draw: impl FnOnce(&mut Buffer)) {
    staged(buf, now, stamp, anim::MENU_BAR_SWEEP_MS, draw, |dst, src, bounds, progress| {
        let columns = anim::reveal_count(progress, bounds.width as usize) as u16;
        blit(dst, src, Rect { width: columns, ..bounds });
    });
}

/// Elapsed time since `stamp` while animations are on, else `None` (render
/// the final frame). Used by the surfaces that animate per-element rather
/// than through a reveal window (the splash entrance).
fn elapsed_since(now: Option<u64>, stamp: u64) -> Option<u64> {
    now.map(|n| n.saturating_sub(stamp))
}

/// Post-splash panel entrance: the finished screen is revealed top-to-bottom
/// over `PANEL_WIPE_MS`, unrevealed rows drawn in `screen.backdrop`.
/// `stamp == None` means the splash was skipped, so there is no wipe.
fn wipe(
    buf: &mut Buffer,
    now: Option<u64>,
    stamp: Option<u64>,
    theme: &filecommand_core::theme::Theme,
    depth: ColorDepth,
    draw: impl FnOnce(&mut Buffer),
) {
    let Some(stamp) = stamp else { return draw(buf) };
    let area = *buf.area();
    staged(buf, now, stamp, anim::PANEL_WIPE_MS, draw, |dst, src, _bounds, progress| {
        let rows = anim::reveal_count(progress, area.height as usize) as u16;
        let backdrop = role_style(theme, Role::ScreenBackdrop, depth);
        for y in area.top() + rows..area.bottom() {
            dst.set_string(area.x, y, " ".repeat(area.width as usize), backdrop);
        }
        blit(dst, src, Rect { height: rows, ..area });
    });
}

/// `views::render`'s full result: the terminal-cursor position it already
/// returned, plus the [`HitMap`] `input::map_mouse` needs for the *next*
/// frame's mouse events (design D2). Bundled as one struct — rather than
/// widening `render`'s return to a tuple — so a future addition doesn't
/// force every call site to re-destructure.
#[derive(Debug, Clone, Default)]
pub struct RenderOutput {
    /// The real terminal cursor's `(x, y)` position when the current phase
    /// wants one shown (`UiPhase::Editor`, for its caret, or any focused
    /// single-line text field per text-field-editing — the command line
    /// included) — `None` otherwise. See `render`'s own doc comment for why
    /// a stale position must not linger.
    pub cursor: Option<(u16, u16)>,
    pub hitmap: HitMap,
}

/// Render the entire screen for the current `state`. Pure with respect to
/// state — never mutates it, never performs I/O itself. `clock_text` is the
/// already-formatted `h:mm a` wall-clock reading (the TUI reads the real
/// clock; tests pin a fixed string), following the same "input alongside
/// state" pattern as `identity_lines`. `viewer_source` is the open byte
/// window backing an active `UiPhase::Viewer` (design D1 — the TUI owns the
/// `ByteSource`, `core::State` never does); it is ignored in every other
/// phase.
///
/// The frame is drawn in the layers of the single precedence order
/// (`filecommand_core::input_layer`; reducer-overlay-structure design D2/D8),
/// bottom-up: the active screen — the phase, with the overlay in
/// `State::overlay` above it, or the "terminal too small" placeholder in
/// their place below the floor — then the quit-confirmation dialog, then the
/// startup warning, which stays visible over whatever is showing, the splash
/// and the placeholder included. Each layer's arm computes its geometry once
/// (`DialogLayout`), draws from it, and records its clickable cells into the
/// [`HitMap`], so the hit map can never disagree with what is on screen.
///
/// Returns [`RenderOutput`]: the real terminal cursor's `(x, y)` position
/// when the topmost layer wants one shown (`UiPhase::Editor`, for the caret,
/// or a focused single-line text field per text-field-editing) — `None`
/// otherwise, which the caller must treat as "leave the cursor hidden" since
/// a stale position from a previous frame's phase would otherwise linger —
/// alongside the [`HitMap`] `input::map_mouse` needs for the next frame's
/// mouse events (design D2).
pub fn render(
    buf: &mut Buffer,
    area: Rect,
    state: &State,
    depth: ColorDepth,
    identity_lines: &[String; 4],
    clock_text: &str,
    viewer_source: Option<&ByteSource>,
) -> RenderOutput {
    // Resolved once per frame: the highlighted built-in theme while the
    // theme picker is open, else the applied theme (theme-selection "Live
    // theme preview while the picker is open"). Every renderer below —
    // panels, key bar, the picker dialog itself, and any overlay drawn
    // above it — styles itself from this single value so the whole screen
    // repaints consistently as the highlight moves.
    let render_theme = state.render_theme();
    let theme = &render_theme;
    let now = state.anim_now();
    let mut hm = HitMap::default();

    // Below the 60x16 floor the "terminal too small" placeholder is drawn in
    // place of whatever screen is active, which stays intact underneath it
    // and is redrawn as-is once the terminal is enlarged (application-shell
    // "Resize handling with 60x16 hard floor and placeholder"; resize-
    // preserves-session-state design D5). No phase view ever lays out at a
    // sub-floor size, no text field owns the cursor, and nothing of the
    // hidden screen is recorded in the hit map — only the quit dialog's
    // buttons, added below, are clickable there.
    let mut cursor = if state.undersized {
        placeholder::render_placeholder(buf, area, theme, depth);
        None
    } else {
        render_screen(buf, area, state, theme, depth, identity_lines, clock_text, viewer_source, &mut hm)
    };

    // The quit-confirmation dialog is drawn above whatever the screen
    // painted — panels, the viewer, an open menu, or any other modal
    // dialog/overlay — since it lives beside the phase rather than inside
    // it and can open over any of them (application-shell "Quit request
    // keys and confirmation"; design D5). Its iris keys on its own stamp, so
    // it opens over a dialog without making that dialog re-animate
    // (ui-animations "Modal dialogs open with an iris").
    if state.quit_confirm {
        let l = quit_dialog::layout(area);
        iris(buf, now, state.quit_opened_at_ms, |b| quit_dialog::draw_quit_dialog(b, theme, depth, &l));
        hm.dialog_buttons.extend(l.buttons);
        cursor = None;
    }
    // The startup-warning modal is drawn last, over whatever is already
    // painted — it is raised at the very start of a session or after the
    // user-menu file was edited, so it must stay visible regardless of which
    // phase (even the splash screen, or the placeholder) happens to be on
    // screen underneath it (user-menu "Malformed file warns and falls back
    // without overwriting"). The mouse is ignored while it is up, so it
    // records no hits.
    if let Some(Overlay::StartupWarning(message)) = state.overlay() {
        iris(buf, now, state.dialog_opened_at_ms, |b| startup_warning::render_startup_warning(b, area, theme, depth, message));
        cursor = None;
    }
    // While either of those topmost modals is up, it — not any text field
    // underneath — owns the keyboard, so the terminal cursor hides
    // (text-field-editing "Field rendering keeps the caret visible": the
    // cursor shows only while a field is focused).
    RenderOutput { cursor, hitmap: hm }
}

// The screen renderer genuinely needs every one of these inputs (buffer,
// area, state, theme, depth, identity, clock, viewer source, hit map);
// bundling them into a struct would only move the same values one level
// down.
/// The active screen at full size: the phase, and — over the panels — the
/// modal overlay in `State::overlay` at its z-position, then the file-op
/// dialog. Records the clickable regions it draws into `hm`. Returns the
/// terminal cursor position the screen wants, if any.
#[allow(clippy::too_many_arguments)]
fn render_screen(
    buf: &mut Buffer,
    area: Rect,
    state: &State,
    theme: &filecommand_core::theme::Theme,
    depth: ColorDepth,
    identity_lines: &[String; 4],
    clock_text: &str,
    viewer_source: Option<&ByteSource>,
    hm: &mut HitMap,
) -> Option<(u16, u16)> {
    match &state.phase {
        // Splash has nothing clickable.
        UiPhase::Splash { started_at_ms } => {
            let elapsed = elapsed_since(state.anim_now(), *started_at_ms);
            splash::render_splash(buf, area, theme, depth, identity_lines, elapsed);
            None
        }
        // Viewer/Editor's wheel-only handling is dispatched directly by the
        // event loop from `state.phase` (mirroring how it already calls
        // `map_viewer_key`/`map_editor_key` directly rather than through
        // `map_key`), so neither needs a hit map at all.
        UiPhase::Viewer(v) => {
            viewer::render_viewer(buf, area, v, theme, depth, viewer_source);
            None
        }
        UiPhase::Editor(e) => editor::render_editor(buf, area, e, theme, depth),
        UiPhase::Panels | UiPhase::FileOpSetup(_) | UiPhase::FileOpRunning { .. } | UiPhase::FileOpSummary(_) => {
            let l = layout::compute((area.width, area.height), state.split_percent);
            let left_type_ahead = (state.active == PanelSide::Left).then_some(state.quick_search.as_deref()).flatten();
            let right_type_ahead = (state.active == PanelSide::Right).then_some(state.quick_search.as_deref()).flatten();
            // The base screen (panels, clock, command line, key bar) is the
            // surface the post-splash wipe reveals; every overlay below is
            // drawn over it directly (ui-animations "Panels wipe in after
            // the splash").
            let now = state.anim_now();
            // The terminal cursor shows whenever a single-line text field
            // owns the keyboard (text-field-editing "Field rendering keeps
            // the caret visible"): the command line by default, or a
            // dialog's field while one is open. Each text-field view
            // reports its caret position as it draws; the overlays below
            // override the command line's, and an overlay with no field
            // hides the cursor again.
            let mut cmdline_caret = None;
            wipe(buf, now, state.panels_revealed_at_ms, theme, depth, |b| {
                panel::render_panel(
                    b,
                    l.left,
                    &state.left,
                    theme,
                    depth,
                    state.active == PanelSide::Left,
                    identity_lines,
                    &state.right,
                    left_type_ahead,
                    PanelSide::Left,
                    state.drag.as_ref(),
                );
                panel::render_panel(
                    b,
                    l.right,
                    &state.right,
                    theme,
                    depth,
                    state.active == PanelSide::Right,
                    identity_lines,
                    &state.left,
                    right_type_ahead,
                    PanelSide::Right,
                    state.drag.as_ref(),
                );
                // Drawn unconditionally, before the F9 overlay below — the
                // menu bar (when open) paints over the whole top row
                // including this, which is what "hides" it; closing the bar
                // simply stops that overwrite from happening on the next
                // frame. The clock itself is skipped entirely (never
                // partially drawn) when it would collide with the right
                // panel's centered path title (responsive-layout "Chrome
                // degradation").
                if clock_fits_without_colliding(l.right, &state.right, state.active == PanelSide::Right, clock_text) {
                    clock::render_clock(b, l.right, theme, depth, clock_text);
                }
                cmdline_caret = command_line::render_command_line(b, l.cmdline, theme, depth, &state.prompt(), &state.command_line);
                keybar::render_keybar(b, l.keybar, theme, depth, state.drag.is_some());
            });
            // The panel, tab-strip, key-bar, and command-line regions come
            // from the same `layout::compute` the panels were just drawn
            // with (mouse-input "Hit-testing stays in the TUI").
            *hm.panel_mut(PanelSide::Left) = panel::hit_test(l.left, &state.left);
            *hm.panel_mut(PanelSide::Right) = panel::hit_test(l.right, &state.right);
            hm.cmdline = l.cmdline;
            hm.keybar = keybar::hit_slots(l.keybar);

            // The one modal overlay in the slot, drawn at its z-position
            // over the base screen and under the file-op dialog (design D8).
            // The F9 bar overlays the panels' top borders (and the clock)
            // rather than reserving a row of its own; the pop-up menus
            // unfold on their own stamps; every modal dialog opens through
            // the same iris, keyed on the single `dialog_opened_at_ms`
            // stamp. An overlay with a text field hands the cursor to it;
            // any other hides the cursor; the startup warning is drawn by
            // `render` above everything else.
            let opened = state.dialog_opened_at_ms;
            let mut cursor = cmdline_caret;
            match state.overlay() {
                None => {}
                Some(Overlay::Menu(menu)) => {
                    let ml = menubar::layout(area, menu);
                    menubar::draw_menu_bar(buf, theme, depth, menu, now, &ml);
                    hm.menu_titles = ml.titles;
                    hm.menu_items = ml.pulldown.map(|p| p.rows).unwrap_or_default();
                    cursor = None;
                }
                Some(Overlay::DriveSelect(dialog)) => {
                    iris(buf, now, opened, |b| drive_select::render_drive_select(b, area, theme, depth, dialog));
                    cursor = None;
                }
                Some(Overlay::FuzzyJump(dialog)) => {
                    // Ranked with the epoch clock, as the reducer's Enter and
                    // cursor handlers rank, so the drawn order is the
                    // acted-on order (history-persistence-hardening design
                    // D1).
                    let mut fuzzy_caret = None;
                    iris(buf, now, opened, |b| {
                        fuzzy_caret = fuzzy_jump::render_fuzzy_jump(b, area, theme, depth, dialog, &state.dir_history, state.epoch_ms)
                    });
                    cursor = fuzzy_caret;
                }
                Some(Overlay::FindFile(dialog)) => {
                    let mut find_caret = None;
                    iris(buf, now, opened, |b| find_caret = find_file::render_find_file(b, area, theme, depth, dialog));
                    cursor = find_caret;
                }
                Some(Overlay::UserMenu(dialog)) => {
                    user_menu::render_user_menu(buf, area, theme, depth, dialog, &state.user_menu_entries, now);
                    cursor = None;
                }
                Some(Overlay::HistoryDialog(dialog)) => {
                    history_dialog::render_history_dialog(buf, area, theme, depth, dialog, &state.history, now);
                    cursor = None;
                }
                Some(Overlay::ThemePicker(dialog)) => {
                    // The dialog itself styles from the previewed `theme`,
                    // but its active-theme marker stays bound to
                    // `state.theme.name` — the applied theme — even while a
                    // different theme is being previewed (design D2;
                    // theme-selection "Live theme preview while the picker
                    // is open").
                    iris(buf, now, opened, |b| theme_picker::render_theme_picker(b, area, theme, depth, dialog, &state.theme.name));
                    cursor = None;
                }
                Some(Overlay::Help(dialog)) => {
                    iris(buf, now, opened, |b| help::render_help(b, area, theme, depth, dialog, identity_lines));
                    cursor = None;
                }
                Some(Overlay::FileActionMenu(dialog)) => {
                    let fl = file_action_menu::layout(area, dialog);
                    unfold(buf, now, dialog.opened_at_ms, |b| file_action_menu::draw_file_action_menu(b, theme, depth, dialog, now, &fl));
                    hm.file_action_menu_items = fl.rows;
                    cursor = None;
                }
                Some(Overlay::StartupWarning(_)) => cursor = None,
            }

            // The file-operation dialog, above the panels and any overlay.
            // Each arm computes its layout once, draws from it, and records
            // its buttons (design D8).
            match &state.phase {
                UiPhase::FileOpSetup(setup) => {
                    // Exactly one of these two is ever non-empty for a given
                    // `setup`: `delete_confirm::layout` is empty outside
                    // `DeleteConfirm`, `destination_input::layout` outside
                    // the other two variants, and only a drop-initiated
                    // `DestinationInput` carries buttons at all.
                    let dl = delete_confirm::layout(area, setup);
                    let il = destination_input::layout(area, setup);
                    let mut file_op_caret = None;
                    iris(buf, now, opened, |b| {
                        file_op_caret = destination_input::draw_destination_input(b, theme, depth, setup, &il);
                        delete_confirm::draw_delete_confirm(b, theme, depth, &dl);
                    });
                    hm.dialog_buttons = dl.buttons;
                    hm.dialog_buttons.extend(il.buttons);
                    cursor = file_op_caret;
                }
                UiPhase::FileOpRunning { dialog, .. } => match dialog {
                    RunningDialog::Progress { kind, progress } => {
                        let pl = progress_dialog::layout(area, *kind, progress);
                        iris(buf, now, opened, |b| progress_dialog::draw_progress(b, theme, depth, progress, &pl));
                        hm.dialog_buttons = pl.buttons;
                        cursor = None;
                    }
                    RunningDialog::Conflict { info, rename_input, .. } => {
                        let cl = conflict_dialog::layout(area, info, rename_input);
                        let mut file_op_caret = None;
                        iris(buf, now, opened, |b| file_op_caret = conflict_dialog::draw_conflict(b, theme, depth, rename_input, &cl));
                        hm.dialog_buttons = cl.buttons;
                        cursor = file_op_caret;
                    }
                    RunningDialog::Error { info, .. } => {
                        let el = error_dialog::layout(area, info);
                        iris(buf, now, opened, |b| error_dialog::draw_error(b, theme, depth, &el));
                        hm.dialog_buttons = el.buttons;
                        cursor = None;
                    }
                },
                UiPhase::FileOpSummary(skipped) => {
                    let sl = skipped_summary::layout(area, skipped);
                    iris(buf, now, opened, |b| skipped_summary::draw_skipped_summary(b, theme, depth, &sl));
                    hm.dialog_buttons = sl.buttons;
                    cursor = None;
                }
                _ => {}
            }
            cursor
        }
    }
}

/// Whether the clock can render over `right_area`'s top border without
/// touching the right panel's centered path title — computed against the
/// same title string and centering math `panel::render_panel` uses
/// (`panel::panel_title`), so the two never disagree. `false` whenever the
/// clock wouldn't fit at all, matching `clock::render_clock`'s own no-op
/// (responsive-layout "Chrome degradation": "the clock is not drawn at all
/// and the path title renders normally").
fn clock_fits_without_colliding(right_area: Rect, right_panel: &PanelState, active: bool, clock_text: &str) -> bool {
    let clock_w = display_width(clock_text);
    if clock_w == 0 || clock_w > right_area.width as usize {
        return false;
    }
    let title = panel::panel_title(right_panel, active);
    let title_w = display_width(&title);
    let inner_w = (right_area.width as usize).saturating_sub(2);
    let title_x = right_area.x + 1 + (inner_w.saturating_sub(title_w) / 2) as u16;
    let title_end = title_x + title_w as u16;
    let clock_start = right_area.x + right_area.width - clock_w as u16;
    let clock_end = right_area.x + right_area.width;
    !(title_x < clock_end && clock_start < title_end)
}

#[cfg(test)]
mod tests;
