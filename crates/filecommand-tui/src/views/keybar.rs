//! Static F-key bar: `1Help 2Menu 3View 4Edit 5Copy 6RenMov 7Mkdir 8Delete
//! 9PullDn 10Quit`, numbers and labels in distinct theme roles.

use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

const KEYS: &[(&str, &str)] = &[
    ("1", "Help"),
    ("2", "Menu"),
    ("3", "View"),
    ("4", "Edit"),
    ("5", "Copy"),
    ("6", "RenMov"),
    ("7", "Mkdir"),
    ("8", "Delete"),
    ("9", "PullDn"),
    ("10", "Quit"),
];

pub fn render_keybar(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth) {
    if area.height == 0 {
        return;
    }
    let number_style = role_style(theme, Role::KeybarNumber, depth);
    let label_style = role_style(theme, Role::KeybarLabel, depth);

    // Fill the whole row with the label background first so trailing space
    // and inter-group gaps pick up the correct bg.
    buf.set_string(area.x, area.y, " ".repeat(area.width as usize), label_style);

    let mut x = area.x;
    let right_edge = area.x + area.width;
    for (i, (num, label)) in KEYS.iter().enumerate() {
        if i > 0 {
            if x >= right_edge {
                break;
            }
            x += 1; // separating space, already painted with label_style
        }
        if x >= right_edge {
            break;
        }
        buf.set_string(x, area.y, num, number_style);
        x += num.chars().count() as u16;
        if x >= right_edge {
            break;
        }
        buf.set_string(x, area.y, label, label_style);
        x += label.chars().count() as u16;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;

    #[test]
    fn keybar_text_matches_spec_string() {
        let theme = Theme::classic();
        let area = Rect { x: 0, y: 0, width: 80, height: 1 };
        let mut buf = Buffer::empty(area);
        render_keybar(&mut buf, area, &theme, ColorDepth::Ansi16);
        let mut line = String::new();
        for x in 0..80 {
            line.push_str(buf[(x, 0)].symbol());
        }
        assert!(line.trim_end().starts_with("1Help 2Menu 3View 4Edit 5Copy 6RenMov 7Mkdir 8Delete 9PullDn 10Quit"));
    }
}
