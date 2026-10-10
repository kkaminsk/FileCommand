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
use filecommand_core::viewer::ByteSource;
use filecommand_core::{PanelSide, State, UiPhase};
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;

use crate::hitmap::HitMap;
use crate::layout;
use crate::style::role_style;

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
// art, and hit-maps (built from final geometry in `build_hitmap`) are
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
/// Returns [`RenderOutput`]: the real terminal cursor's `(x, y)` position
/// when the current phase wants one shown (`UiPhase::Editor`, for the
/// caret, or a focused single-line text field per text-field-editing) —
/// `None` otherwise, which the caller must treat as "leave the cursor
/// hidden" since a stale position from a previous frame's phase would
/// otherwise linger — alongside the [`HitMap`] `input::map_mouse`
/// needs for the next frame's mouse events (design D2; this is the same
/// return value the M4 caret position already was, just extended rather
/// than replaced).
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
    let cursor = render_phase(buf, area, state, &render_theme, depth, identity_lines, clock_text, viewer_source);
    // The quit-confirmation dialog is drawn above whatever the current phase
    // painted — panels, the viewer, an open menu, or any other modal
    // dialog/overlay — since it lives beside the phase rather than inside
    // it and can open over any of them (application-shell "Quit request
    // keys and confirmation"; design D5).
    // Its iris keys on its own stamp, so it opens over a dialog without
    // making that dialog re-animate (ui-animations "Modal dialogs open with
    // an iris").
    let now = state.anim_now();
    if state.quit_confirm {
        iris(buf, now, state.quit_opened_at_ms, |b| quit_dialog::render_quit_dialog(b, area, &render_theme, depth));
    }
    // The startup-warning modal is drawn last, over whatever the current
    // phase already painted — it can only ever be raised at the very start
    // of a session (currently: a malformed `usermenu.toml`), so it must stay
    // visible regardless of which phase (even the splash screen) happens to
    // be on screen underneath it (user-menu "Malformed file warns and falls
    // back without overwriting").
    if let Some(message) = &state.startup_warning {
        iris(buf, now, state.dialog_opened_at_ms, |b| startup_warning::render_startup_warning(b, area, &render_theme, depth, message));
    }
    // While either of those topmost modals is up, it — not any text field
    // underneath — owns the keyboard, so the terminal cursor hides
    // (text-field-editing "Field rendering keeps the caret visible": the
    // cursor shows only while a field is focused).
    let cursor = if state.quit_confirm || state.startup_warning.is_some() { None } else { cursor };
    RenderOutput { cursor, hitmap: build_hitmap(area, state) }
}

/// Builds the frame's [`HitMap`] by asking each relevant view module for the
/// clickable regions it just drew — `panel::hit_test`, `keybar::hit_slots`,
/// `menubar::hit_titles`/`hit_items`, and each open dialog's own
/// `hit_buttons` — mirroring `render_phase`'s own "what's open" dispatch so
/// the two can never disagree about what's actually on screen this frame
/// (mouse-input "Hit-testing stays in the TUI"; design D2). Never draws
/// anything itself.
fn build_hitmap(area: Rect, state: &State) -> HitMap {
    let mut hm = HitMap::default();
    match &state.phase {
        UiPhase::Panels | UiPhase::FileOpSetup(_) | UiPhase::FileOpRunning { .. } | UiPhase::FileOpSummary(_) => {
            let l = layout::compute((area.width, area.height), state.split_percent);
            *hm.panel_mut(PanelSide::Left) = panel::hit_test(l.left, &state.left);
            *hm.panel_mut(PanelSide::Right) = panel::hit_test(l.right, &state.right);
            hm.cmdline = l.cmdline;
            hm.keybar = keybar::hit_slots(l.keybar);
            if let Some(menu) = &state.menu {
                hm.menu_titles = menubar::hit_titles(area);
                hm.menu_items = menubar::hit_items(area, menu);
            }
            if let Some(dialog) = &state.file_action_menu {
                hm.file_action_menu_items = file_action_menu::hit_items(area, dialog);
            }
            match &state.phase {
                UiPhase::FileOpSetup(setup) => {
                    // Exactly one of these is ever non-empty for a given
                    // `setup` — `delete_confirm::hit_buttons` no-ops outside
                    // `DeleteConfirm`, `destination_input::hit_buttons`
                    // outside a drop-initiated `DestinationInput` — so
                    // concatenating both is simply "whichever dialog is
                    // actually showing its buttons this frame."
                    hm.dialog_buttons = delete_confirm::hit_buttons(area, setup);
                    hm.dialog_buttons.extend(destination_input::hit_buttons(area, setup));
                }
                UiPhase::FileOpRunning { dialog, .. } => {
                    hm.dialog_buttons = match dialog {
                        RunningDialog::Progress { .. } => progress_dialog::hit_buttons(area),
                        RunningDialog::Conflict { rename_input, .. } => conflict_dialog::hit_buttons(area, rename_input),
                        RunningDialog::Error { info, .. } => error_dialog::hit_buttons(area, info),
                    };
                }
                UiPhase::FileOpSummary(skipped) => hm.dialog_buttons = skipped_summary::hit_buttons(area, skipped),
                _ => {}
            }
        }
        // Splash/Placeholder have nothing clickable; Viewer/Editor's wheel-
        // only handling is dispatched directly by the event loop from
        // `state.phase` (mirroring how it already calls `map_viewer_key`/
        // `map_editor_key` directly rather than through `map_key`), so
        // neither needs a hit map at all.
        _ => {}
    }
    // The quit-confirmation dialog is drawn above whatever the phase
    // painted (see `render`, above) and is reachable from every context, so
    // its buttons are recorded the same way, independent of the match above.
    if state.quit_confirm {
        hm.dialog_buttons.extend(quit_dialog::hit_buttons(area));
    }
    hm
}

fn render_phase(
    buf: &mut Buffer,
    area: Rect,
    state: &State,
    theme: &filecommand_core::theme::Theme,
    depth: ColorDepth,
    identity_lines: &[String; 4],
    clock_text: &str,
    viewer_source: Option<&ByteSource>,
) -> Option<(u16, u16)> {
    match &state.phase {
        UiPhase::Splash { started_at_ms } => {
            let elapsed = elapsed_since(state.anim_now(), *started_at_ms);
            splash::render_splash(buf, area, theme, depth, identity_lines, elapsed);
            None
        }
        UiPhase::Placeholder => {
            placeholder::render_placeholder(buf, area, theme, depth);
            None
        }
        UiPhase::Viewer(v) => {
            viewer::render_viewer(buf, area, v, theme, depth, viewer_source);
            None
        }
        UiPhase::Editor(e) => editor::render_editor(buf, area, e, theme, depth),
        UiPhase::Panels
        | UiPhase::FileOpSetup(_)
        | UiPhase::FileOpRunning { .. }
        | UiPhase::FileOpSummary(_) => {
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

            // The F9 bar overlays the panels' top borders (and the clock)
            // rather than reserving a row of its own. It sweeps in and its
            // pull-down unfolds inside `render_menu_bar`.
            if let Some(menu) = &state.menu {
                menubar::render_menu_bar(buf, area, theme, depth, menu, now);
            }
            // Every modal dialog below opens through the same iris, keyed on
            // the single `dialog_opened_at_ms` stamp — only one of them is
            // ever visible at a time (ui-animations "Modal dialogs open with
            // an iris").
            let opened = state.dialog_opened_at_ms;
            if let Some(dialog) = &state.drive_select {
                iris(buf, now, opened, |b| drive_select::render_drive_select(b, area, theme, depth, dialog));
            }
            let mut fuzzy_caret = None;
            if let Some(dialog) = &state.fuzzy_jump {
                iris(buf, now, opened, |b| fuzzy_caret = fuzzy_jump::render_fuzzy_jump(b, area, theme, depth, dialog, &state.dir_history, state.clock_ms));
            }
            let mut find_caret = None;
            if let Some(dialog) = &state.find_file {
                iris(buf, now, opened, |b| find_caret = find_file::render_find_file(b, area, theme, depth, dialog));
            }
            if let Some(dialog) = &state.user_menu {
                user_menu::render_user_menu(buf, area, theme, depth, dialog, &state.user_menu_entries, now);
            }
            if let Some(dialog) = &state.theme_picker {
                // The dialog itself styles from the previewed `theme`, but
                // its active-theme marker stays bound to `state.theme.name`
                // — the applied theme — even while a different theme is
                // being previewed (design D2; theme-selection "Live theme
                // preview while the picker is open").
                iris(buf, now, opened, |b| theme_picker::render_theme_picker(b, area, theme, depth, dialog, &state.theme.name));
            }
            if let Some(dialog) = &state.help {
                iris(buf, now, opened, |b| help::render_help(b, area, theme, depth, dialog, identity_lines));
            }
            if let Some(dialog) = &state.file_action_menu {
                file_action_menu::render_file_action_menu(buf, area, theme, depth, dialog, now);
            }

            let mut file_op_caret = None;
            match &state.phase {
                UiPhase::FileOpSetup(setup) => iris(buf, now, opened, |b| {
                    file_op_caret = destination_input::render_destination_input(b, area, theme, depth, setup);
                    delete_confirm::render_delete_confirm(b, area, theme, depth, setup);
                }),
                UiPhase::FileOpRunning { dialog, .. } => match dialog {
                    RunningDialog::Progress { kind, progress } => {
                        iris(buf, now, opened, |b| progress_dialog::render_progress(b, area, theme, depth, *kind, progress));
                    }
                    RunningDialog::Conflict { info, rename_input, .. } => {
                        iris(buf, now, opened, |b| file_op_caret = conflict_dialog::render_conflict(b, area, theme, depth, info, rename_input));
                    }
                    RunningDialog::Error { info, .. } => {
                        iris(buf, now, opened, |b| error_dialog::render_error(b, area, theme, depth, info));
                    }
                },
                UiPhase::FileOpSummary(skipped) => {
                    iris(buf, now, opened, |b| skipped_summary::render_skipped_summary(b, area, theme, depth, skipped));
                }
                _ => {}
            }

            // The command line's caret is the default; any overlay above
            // either hands the cursor to its own text field or hides it
            // (text-field-editing "Field rendering keeps the caret
            // visible": "SHALL show the terminal cursor at the caret's
            // screen position while the field is focused" — and only then).
            let mut cursor = cmdline_caret;
            if state.menu.is_some()
                || state.drive_select.is_some()
                || state.user_menu.is_some()
                || state.theme_picker.is_some()
                || state.help.is_some()
                || state.file_action_menu.is_some()
            {
                cursor = None;
            }
            if state.fuzzy_jump.is_some() {
                cursor = fuzzy_caret;
            }
            if state.find_file.is_some() {
                cursor = find_caret;
            }
            match &state.phase {
                UiPhase::FileOpSetup(_) | UiPhase::FileOpRunning { .. } | UiPhase::FileOpSummary(_) => cursor = file_op_caret,
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
