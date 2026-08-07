pub mod cmdline;
pub mod conflict_dialog;
pub mod delete_confirm;
pub mod destination_input;
pub mod error_dialog;
pub mod keybar;
pub mod panel;
pub mod placeholder;
pub mod progress_dialog;
pub mod quit_dialog;
pub mod skipped_summary;
pub mod splash;

use filecommand_core::fs_ops::dialog::RunningDialog;
use filecommand_core::theme::ColorDepth;
use filecommand_core::{PanelSide, State, UiPhase};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::layout;

/// Render the entire screen for the current `state`. Pure with respect to
/// state — never mutates it, never performs I/O.
pub fn render(buf: &mut Buffer, area: Rect, state: &State, depth: ColorDepth, identity_lines: &[String; 4]) {
    match &state.phase {
        UiPhase::Splash { .. } => {
            splash::render_splash(buf, area, &state.theme, depth, identity_lines);
        }
        UiPhase::Placeholder => {
            placeholder::render_placeholder(buf, area, &state.theme, depth);
        }
        UiPhase::Panels
        | UiPhase::QuitConfirm
        | UiPhase::FileOpSetup(_)
        | UiPhase::FileOpRunning { .. }
        | UiPhase::FileOpSummary(_) => {
            let l = layout::compute((area.width, area.height));
            panel::render_panel(buf, l.left, &state.left, &state.theme, depth, state.active == PanelSide::Left);
            panel::render_panel(buf, l.right, &state.right, &state.theme, depth, state.active == PanelSide::Right);
            cmdline::render_command_line(buf, l.cmdline, &state.theme, depth, &state.active_panel().cwd);
            keybar::render_keybar(buf, l.keybar, &state.theme, depth);
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
        }
    }
}
