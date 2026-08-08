pub mod clock;
pub mod command_line;
pub mod conflict_dialog;
pub mod delete_confirm;
pub mod destination_input;
pub mod drive_select;
pub mod editor;
pub mod error_dialog;
pub mod find_file;
pub mod fuzzy_jump;
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
pub mod tab_strip;
pub mod user_menu;
pub mod viewer;

use filecommand_core::fs_ops::dialog::RunningDialog;
use filecommand_core::theme::ColorDepth;
use filecommand_core::viewer::ByteSource;
use filecommand_core::{PanelSide, State, UiPhase};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::layout;

/// Render the entire screen for the current `state`. Pure with respect to
/// state — never mutates it, never performs I/O itself. `clock_text` is the
/// already-formatted `h:mm a` wall-clock reading (the TUI reads the real
/// clock; tests pin a fixed string), following the same "input alongside
/// state" pattern as `identity_lines`. `viewer_source` is the open byte
/// window backing an active `UiPhase::Viewer` (design D1 — the TUI owns the
/// `ByteSource`, `core::State` never does); it is ignored in every other
/// phase.
///
/// Returns the real terminal cursor's `(x, y)` position when the current
/// phase wants one shown (only `UiPhase::Editor`, for the caret) — `None`
/// otherwise, which the caller must treat as "leave the cursor hidden"
/// since a stale position from a previous frame's phase would otherwise
/// linger.
pub fn render(
    buf: &mut Buffer,
    area: Rect,
    state: &State,
    depth: ColorDepth,
    identity_lines: &[String; 4],
    clock_text: &str,
    viewer_source: Option<&ByteSource>,
) -> Option<(u16, u16)> {
    match &state.phase {
        UiPhase::Splash { .. } => {
            splash::render_splash(buf, area, &state.theme, depth, identity_lines);
            None
        }
        UiPhase::Placeholder => {
            placeholder::render_placeholder(buf, area, &state.theme, depth);
            None
        }
        UiPhase::Viewer(v) => {
            viewer::render_viewer(buf, area, v, &state.theme, depth, viewer_source);
            None
        }
        UiPhase::Editor(e) => editor::render_editor(buf, area, e, &state.theme, depth),
        UiPhase::Panels
        | UiPhase::QuitConfirm
        | UiPhase::FileOpSetup(_)
        | UiPhase::FileOpRunning { .. }
        | UiPhase::FileOpSummary(_) => {
            let l = layout::compute((area.width, area.height));
            let left_type_ahead = (state.active == PanelSide::Left).then_some(state.quick_search.as_deref()).flatten();
            let right_type_ahead = (state.active == PanelSide::Right).then_some(state.quick_search.as_deref()).flatten();
            panel::render_panel(
                buf,
                l.left,
                &state.left,
                &state.theme,
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
                &state.theme,
                depth,
                state.active == PanelSide::Right,
                identity_lines,
                &state.left,
                right_type_ahead,
            );
            // Drawn unconditionally, before the F9 overlay below — the menu
            // bar (when open) paints over the whole top row including this,
            // which is what "hides" it; closing the bar simply stops that
            // overwrite from happening on the next frame.
            clock::render_clock(buf, l.right, &state.theme, depth, clock_text);
            command_line::render_command_line(buf, l.cmdline, &state.theme, depth, &state.prompt(), &state.command_line);
            keybar::render_keybar(buf, l.keybar, &state.theme, depth);

            // The F9 bar overlays the panels' top borders (and the clock)
            // rather than reserving a row of its own.
            if let Some(menu) = &state.menu {
                menubar::render_menu_bar(buf, area, &state.theme, depth, menu);
            }
            if let Some(dialog) = &state.drive_select {
                drive_select::render_drive_select(buf, area, &state.theme, depth, dialog);
            }
            if let Some(dialog) = &state.fuzzy_jump {
                fuzzy_jump::render_fuzzy_jump(buf, area, &state.theme, depth, dialog, &state.dir_history, state.clock_ms);
            }
            if let Some(dialog) = &state.find_file {
                find_file::render_find_file(buf, area, &state.theme, depth, dialog);
            }
            if let Some(dialog) = &state.user_menu {
                user_menu::render_user_menu(buf, area, &state.theme, depth, dialog, &state.user_menu_entries);
            }
            if let Some(dialog) = &state.help {
                help::render_help(buf, area, &state.theme, depth, dialog, identity_lines);
            }

            match &state.phase {
                UiPhase::QuitConfirm => quit_dialog::render_quit_dialog(buf, area, &state.theme, depth),
                UiPhase::FileOpSetup(setup) => {
                    destination_input::render_destination_input(buf, area, &state.theme, depth, setup);
                    delete_confirm::render_delete_confirm(buf, area, &state.theme, depth, setup);
                }
                UiPhase::FileOpRunning { dialog, .. } => match dialog {
                    RunningDialog::Progress { kind, progress } => {
                        progress_dialog::render_progress(buf, area, &state.theme, depth, *kind, progress);
                    }
                    RunningDialog::Conflict { info, rename_input, .. } => {
                        conflict_dialog::render_conflict(buf, area, &state.theme, depth, info, rename_input);
                    }
                    RunningDialog::Error { info, .. } => {
                        error_dialog::render_error(buf, area, &state.theme, depth, info);
                    }
                },
                UiPhase::FileOpSummary(skipped) => {
                    skipped_summary::render_skipped_summary(buf, area, &state.theme, depth, skipped);
                }
                _ => {}
            }
            None
        }
    }
}
