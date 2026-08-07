//! Static command-line prompt row: `C:\PATH>_` — display only, no shell
//! execution in M1.

use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use std::path::Path;

use crate::style::role_style;
use filecommand_core::listing::pad_to_width;

pub fn render_command_line(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, cwd: &Path) {
    if area.height == 0 {
        return;
    }
    let style = role_style(theme, Role::CommandLine, depth);
    let text = format!("{}>_", cwd.display());
    buf.set_string(area.x, area.y, pad_to_width(&text, area.width as usize), style);
}
