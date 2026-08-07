//! Theme-role-to-ratatui style adapter. Every renderer must resolve colors
//! through [`role_style`] — no hardcoded `Color::*` literals belong in a
//! view module.

use filecommand_core::theme::{Ansi16, ColorDepth, ResolvedColor, Role, Theme};
use ratatui::style::{Color, Style};

fn ansi16_to_color(a: Ansi16) -> Color {
    match a {
        Ansi16::Black => Color::Black,
        Ansi16::Red => Color::Red,
        Ansi16::Green => Color::Green,
        Ansi16::Yellow => Color::Yellow,
        Ansi16::Blue => Color::Blue,
        Ansi16::Magenta => Color::Magenta,
        Ansi16::Cyan => Color::Cyan,
        Ansi16::White => Color::Gray,
        Ansi16::BrightBlack => Color::DarkGray,
        Ansi16::BrightRed => Color::LightRed,
        Ansi16::BrightGreen => Color::LightGreen,
        Ansi16::BrightYellow => Color::LightYellow,
        Ansi16::BrightBlue => Color::LightBlue,
        Ansi16::BrightMagenta => Color::LightMagenta,
        Ansi16::BrightCyan => Color::LightCyan,
        Ansi16::BrightWhite => Color::White,
    }
}

fn resolved_to_color(rc: ResolvedColor) -> Option<Color> {
    match rc {
        ResolvedColor::Inherit => None,
        ResolvedColor::Ansi16(a) => Some(ansi16_to_color(a)),
        ResolvedColor::Rgb(filecommand_core::theme::Rgb(r, g, b)) => Some(Color::Rgb(r, g, b)),
    }
}

/// Resolve a role to a ratatui [`Style`] at the given color depth. This is
/// the only place fg/bg colors are chosen anywhere in the TUI.
pub fn role_style(theme: &Theme, role: Role, depth: ColorDepth) -> Style {
    let spec = theme.get(role);
    let mut style = Style::default();
    if let Some(fg) = resolved_to_color(spec.resolve_fg(depth)) {
        style = style.fg(fg);
    }
    if let Some(bg) = resolved_to_color(spec.resolve_bg(depth)) {
        style = style.bg(bg);
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;
    use filecommand_core::theme::Theme;

    #[test]
    fn resolves_ansi16_role_to_expected_colors() {
        let theme = Theme::classic();
        let style = role_style(&theme, Role::PanelFrame, ColorDepth::Ansi16);
        assert_eq!(style.fg, Some(Color::Cyan));
        assert_eq!(style.bg, Some(Color::Blue));
    }

    #[test]
    fn inherit_fg_leaves_style_fg_unset() {
        let theme = Theme::classic();
        let style = role_style(&theme, Role::ScreenBackdrop, ColorDepth::Ansi16);
        assert_eq!(style.fg, None);
        assert_eq!(style.bg, Some(Color::Blue));
    }
}
