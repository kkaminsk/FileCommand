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

use filecommand_core::fs_ops::dialog::RunningDialog;
use filecommand_core::listing::display_width;
use filecommand_core::panel::PanelState;
use filecommand_core::theme::ColorDepth;
use filecommand_core::viewer::ByteSource;
use filecommand_core::{PanelSide, State, UiPhase};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::hitmap::HitMap;
use crate::layout;

/// `views::render`'s full result: the terminal-cursor position it already
/// returned, plus the [`HitMap`] `input::map_mouse` needs for the *next*
/// frame's mouse events (design D2). Bundled as one struct — rather than
/// widening `render`'s return to a tuple — so a future addition doesn't
/// force every call site to re-destructure.
#[derive(Debug, Clone, Default)]
pub struct RenderOutput {
    /// The real terminal cursor's `(x, y)` position when the current phase
    /// wants one shown (only `UiPhase::Editor`, for the caret) — `None`
    /// otherwise. See `render`'s own doc comment for why a stale position
    /// must not linger.
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
/// when the current phase wants one shown (only `UiPhase::Editor`, for the
/// caret) — `None` otherwise, which the caller must treat as "leave the
/// cursor hidden" since a stale position from a previous frame's phase
/// would otherwise linger — alongside the [`HitMap`] `input::map_mouse`
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
    if state.quit_confirm {
        quit_dialog::render_quit_dialog(buf, area, &render_theme, depth);
    }
    // The startup-warning modal is drawn last, over whatever the current
    // phase already painted — it can only ever be raised at the very start
    // of a session (currently: a malformed `usermenu.toml`), so it must stay
    // visible regardless of which phase (even the splash screen) happens to
    // be on screen underneath it (user-menu "Malformed file warns and falls
    // back without overwriting").
    if let Some(message) = &state.startup_warning {
        startup_warning::render_startup_warning(buf, area, &render_theme, depth, message);
    }
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
            match &state.phase {
                UiPhase::FileOpSetup(setup) => hm.dialog_buttons = delete_confirm::hit_buttons(area, setup),
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
        UiPhase::Splash { .. } => {
            splash::render_splash(buf, area, theme, depth, identity_lines);
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
            panel::render_panel(
                buf,
                l.left,
                &state.left,
                theme,
                depth,
                state.active == PanelSide::Left,
                identity_lines,
                &state.right,
                left_type_ahead,
            );
            panel::render_panel(
                buf,
                l.right,
                &state.right,
                theme,
                depth,
                state.active == PanelSide::Right,
                identity_lines,
                &state.left,
                right_type_ahead,
            );
            // Drawn unconditionally, before the F9 overlay below — the menu
            // bar (when open) paints over the whole top row including this,
            // which is what "hides" it; closing the bar simply stops that
            // overwrite from happening on the next frame. The clock itself
            // is skipped entirely (never partially drawn) when it would
            // collide with the right panel's centered path title
            // (responsive-layout "Chrome degradation").
            if clock_fits_without_colliding(l.right, &state.right, state.active == PanelSide::Right, clock_text) {
                clock::render_clock(buf, l.right, theme, depth, clock_text);
            }
            command_line::render_command_line(buf, l.cmdline, theme, depth, &state.prompt(), &state.command_line);
            keybar::render_keybar(buf, l.keybar, theme, depth);

            // The F9 bar overlays the panels' top borders (and the clock)
            // rather than reserving a row of its own.
            if let Some(menu) = &state.menu {
                menubar::render_menu_bar(buf, area, theme, depth, menu);
            }
            if let Some(dialog) = &state.drive_select {
                drive_select::render_drive_select(buf, area, theme, depth, dialog);
            }
            if let Some(dialog) = &state.fuzzy_jump {
                fuzzy_jump::render_fuzzy_jump(buf, area, theme, depth, dialog, &state.dir_history, state.clock_ms);
            }
            if let Some(dialog) = &state.find_file {
                find_file::render_find_file(buf, area, theme, depth, dialog);
            }
            if let Some(dialog) = &state.user_menu {
                user_menu::render_user_menu(buf, area, theme, depth, dialog, &state.user_menu_entries);
            }
            if let Some(dialog) = &state.theme_picker {
                // The dialog itself styles from the previewed `theme`, but
                // its active-theme marker stays bound to `state.theme.name`
                // — the applied theme — even while a different theme is
                // being previewed (design D2; theme-selection "Live theme
                // preview while the picker is open").
                theme_picker::render_theme_picker(buf, area, theme, depth, dialog, &state.theme.name);
            }
            if let Some(dialog) = &state.help {
                help::render_help(buf, area, theme, depth, dialog, identity_lines);
            }
            if let Some(dialog) = &state.file_action_menu {
                file_action_menu::render_file_action_menu(buf, area, theme, depth, dialog);
            }

            match &state.phase {
                UiPhase::FileOpSetup(setup) => {
                    destination_input::render_destination_input(buf, area, theme, depth, setup);
                    delete_confirm::render_delete_confirm(buf, area, theme, depth, setup);
                }
                UiPhase::FileOpRunning { dialog, .. } => match dialog {
                    RunningDialog::Progress { kind, progress } => {
                        progress_dialog::render_progress(buf, area, theme, depth, *kind, progress);
                    }
                    RunningDialog::Conflict { info, rename_input, .. } => {
                        conflict_dialog::render_conflict(buf, area, theme, depth, info, rename_input);
                    }
                    RunningDialog::Error { info, .. } => {
                        error_dialog::render_error(buf, area, theme, depth, info);
                    }
                },
                UiPhase::FileOpSummary(skipped) => {
                    skipped_summary::render_skipped_summary(buf, area, theme, depth, skipped);
                }
                _ => {}
            }
            None
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
