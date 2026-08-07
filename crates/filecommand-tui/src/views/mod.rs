pub mod cmdline;
pub mod keybar;
pub mod panel;
pub mod placeholder;
pub mod quit_dialog;
pub mod splash;

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
        UiPhase::Panels | UiPhase::QuitConfirm => {
            let l = layout::compute((area.width, area.height));
            panel::render_panel(buf, l.left, &state.left, &state.theme, depth, state.active == PanelSide::Left);
            panel::render_panel(buf, l.right, &state.right, &state.theme, depth, state.active == PanelSide::Right);
            cmdline::render_command_line(buf, l.cmdline, &state.theme, depth, &state.active_panel().cwd);
            keybar::render_keybar(buf, l.keybar, &state.theme, depth);
            if matches!(state.phase, UiPhase::QuitConfirm) {
                quit_dialog::render_quit_dialog(buf, area, &state.theme, depth);
            }
        }
    }
}
