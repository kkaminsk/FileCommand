//! The main panel F-key bar and the shared widest-form-that-fits selection
//! machinery every F-key bar in the app (this one, plus the viewer's and
//! editor's) is built on.
//!
//! Rather than a single fixed label table that silently stops drawing
//! wherever it runs out of width (which used to let a slot start drawing
//! and then get cut off mid-label), each bar declares a small ladder of
//! complete, self-consistent forms — full labels, short labels,
//! numbers-only — and renders whichever is the widest one that fits
//! (responsive-layout "F-key bar degradation forms"; design D5).

use filecommand_core::listing::display_width;
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

/// The main panel bar's full form: `1Help 2Menu 3View 4Edit 5Copy 6RenMov
/// 7Mkdir 8Delete 9PullDn 10Quit` (67 columns; responsive-layout "F-key bar
/// degradation forms").
const KEYS_FULL: &[(&str, &str)] = &[
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

/// The main panel bar's short form: `1Hlp 2Mnu 3Vew 4Edt 5Cpy 6Ren 7Mkd
/// 8Del 9Pdn 10Qit` (50 columns).
const KEYS_SHORT: &[(&str, &str)] = &[
    ("1", "Hlp"),
    ("2", "Mnu"),
    ("3", "Vew"),
    ("4", "Edt"),
    ("5", "Cpy"),
    ("6", "Ren"),
    ("7", "Mkd"),
    ("8", "Del"),
    ("9", "Pdn"),
    ("10", "Qit"),
];

/// The main panel bar's numbers-only form: `1 2 3 4 5 6 7 8 9 10` (20
/// columns) — defensive headroom below the 60-column floor, where the
/// short form already fits.
const KEYS_NUMBERS_ONLY: &[(&str, &str)] = &[
    ("1", ""),
    ("2", ""),
    ("3", ""),
    ("4", ""),
    ("5", ""),
    ("6", ""),
    ("7", ""),
    ("8", ""),
    ("9", ""),
    ("10", ""),
];

/// The display-column width a bar built from `pairs` would occupy: every
/// slot's number plus label, plus one separating space between slots (none
/// after the last).
pub fn form_width(pairs: &[(&str, &str)]) -> u16 {
    let mut w = 0usize;
    for (i, (num, label)) in pairs.iter().enumerate() {
        if i > 0 {
            w += 1;
        }
        w += display_width(num) + display_width(label);
    }
    w as u16
}

/// The widest form in `forms` (given widest-first) whose `form_width` fits
/// `width`, or the last (narrowest) form if none do — so a bar always
/// renders *something* complete rather than nothing, even below every
/// form's declared width (responsive-layout "F-key bar degradation
/// forms").
pub fn choose_form<'a>(width: u16, forms: &[&'a [(&'a str, &'a str)]]) -> &'a [(&'a str, &'a str)] {
    forms.iter().find(|f| form_width(f) <= width).copied().unwrap_or(forms[forms.len() - 1])
}

/// Render `pairs` (a form already chosen by [`choose_form`]) left to right,
/// number in `Role::KeybarNumber`, label in `Role::KeybarLabel`, one space
/// between slots. The chosen form is guaranteed to fit by construction, so
/// the per-slot bounds check here is a last-resort defensive measure, not
/// the mechanism that picks what fits — no slot is ever partially drawn
/// (responsive-layout "F-key bar degradation forms": "No mid-label
/// truncation").
pub fn render_bar(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, pairs: &[(&str, &str)]) {
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
    for (i, (num, label)) in pairs.iter().enumerate() {
        if i > 0 {
            x += 1; // separating space, already painted with label_style
        }
        let slot_w = display_width(num) + display_width(label);
        if x + slot_w as u16 > right_edge {
            break;
        }
        buf.set_string(x, area.y, num, number_style);
        x += display_width(num) as u16;
        if !label.is_empty() {
            buf.set_string(x, area.y, label, label_style);
            x += display_width(label) as u16;
        }
    }
}

/// The main panel F-key bar: `1Help 2Menu 3View 4Edit 5Copy 6RenMov 7Mkdir
/// 8Delete 9PullDn 10Quit`, degrading through the short and numbers-only
/// forms as `area` narrows — or, for the duration of an in-progress mouse
/// drag (`dragging`), the drag relabel `Drop=Copy  Shift/RightBtn=Move
/// Esc=Cancel` instead (mouse-panel-drag "Drag feedback"), vanishing back to
/// the normal F-key forms the instant `dragging` is `false` again.
pub fn render_keybar(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, dragging: bool) {
    if dragging {
        render_drag_keybar(buf, area, theme, depth);
        return;
    }
    let form = choose_form(area.width, &[KEYS_FULL, KEYS_SHORT, KEYS_NUMBERS_ONLY]);
    render_bar(buf, area, theme, depth, form);
}

/// The drag relabel's `(key, label)` slots, rendered with a 2-space
/// separator between slots (unlike the F-key bar's single space) to match
/// the spec's literal `Drop=Copy  Shift/RightBtn=Move  Esc=Cancel` string
/// exactly (mouse-panel-drag "Drag feedback").
const DRAG_KEYS: &[(&str, &str)] = &[("Drop", "=Copy"), ("Shift/RightBtn", "=Move"), ("Esc", "=Cancel")];

/// Renders `DRAG_KEYS` left to right with the drag relabel's 2-space
/// separator, in the same `Role::KeybarNumber`/`Role::KeybarLabel` roles and
/// defensive per-slot bounds check `render_bar` uses — no slot is ever
/// partially drawn.
fn render_drag_keybar(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth) {
    if area.height == 0 {
        return;
    }
    let number_style = role_style(theme, Role::KeybarNumber, depth);
    let label_style = role_style(theme, Role::KeybarLabel, depth);
    buf.set_string(area.x, area.y, " ".repeat(area.width as usize), label_style);

    let mut x = area.x;
    let right_edge = area.x + area.width;
    for (i, (key, label)) in DRAG_KEYS.iter().enumerate() {
        if i > 0 {
            x += 2;
        }
        let slot_w = display_width(key) + display_width(label);
        if x + slot_w as u16 > right_edge {
            break;
        }
        buf.set_string(x, area.y, *key, number_style);
        x += display_width(key) as u16;
        buf.set_string(x, area.y, *label, label_style);
        x += display_width(label) as u16;
    }
}

/// The clickable slot rects `render_keybar` currently draws at `area`, each
/// paired with its F-key number (1..=10, `10` for F10 — mouse-input "Key
/// bar, menu bar, pull-down items, and dialog buttons are clickable").
/// Mirrors `choose_form`/`render_bar`'s exact placement (same form choice,
/// same running `x`) so a click can never land on a slot that isn't
/// actually drawn there.
pub fn hit_slots(area: Rect) -> Vec<(Rect, u8)> {
    let form = choose_form(area.width, &[KEYS_FULL, KEYS_SHORT, KEYS_NUMBERS_ONLY]);
    let mut out = Vec::with_capacity(form.len());
    let mut x = area.x;
    let right_edge = area.x + area.width;
    for (i, (num, label)) in form.iter().enumerate() {
        if i > 0 {
            x += 1;
        }
        let slot_w = (display_width(num) + display_width(label)) as u16;
        if x + slot_w > right_edge {
            break;
        }
        if let Ok(n) = num.parse::<u8>() {
            out.push((Rect { x, y: area.y, width: slot_w, height: 1 }, n));
        }
        x += slot_w;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;

    fn render(width: u16) -> String {
        let area = Rect { x: 0, y: 0, width, height: 1 };
        let mut buf = Buffer::empty(area);
        render_keybar(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, false);
        (0..width).map(|x| buf[(x, 0)].symbol()).collect()
    }

    #[test]
    fn keybar_text_matches_spec_string() {
        let line = render(80);
        assert!(line.trim_end().starts_with("1Help 2Menu 3View 4Edit 5Copy 6RenMov 7Mkdir 8Delete 9PullDn 10Quit"));
    }

    #[test]
    fn full_form_widths_match_the_spec() {
        assert_eq!(form_width(KEYS_FULL), 67);
        assert_eq!(form_width(KEYS_SHORT), 50);
        assert_eq!(form_width(KEYS_NUMBERS_ONLY), 20);
    }

    #[test]
    fn full_form_at_nominal_width() {
        let line = render(80);
        assert_eq!(line.trim_end(), "1Help 2Menu 3View 4Edit 5Copy 6RenMov 7Mkdir 8Delete 9PullDn 10Quit");
    }

    #[test]
    fn short_form_at_the_60_column_floor() {
        let line = render(60);
        assert_eq!(line.trim_end(), "1Hlp 2Mnu 3Vew 4Edt 5Cpy 6Ren 7Mkd 8Del 9Pdn 10Qit");
    }

    #[test]
    fn numbers_only_below_the_short_forms_width() {
        let line = render(30);
        assert_eq!(line.trim_end(), "1 2 3 4 5 6 7 8 9 10");
    }

    #[test]
    fn no_form_is_ever_partially_drawn_at_a_boundary_width() {
        // 66 is one column short of the full form (67); the short form (50)
        // must render in full, never a truncated full form.
        let line = render(66);
        assert_eq!(line.trim_end(), "1Hlp 2Mnu 3Vew 4Edt 5Cpy 6Ren 7Mkd 8Del 9Pdn 10Qit");

        // 49 is one column short of the short form (50); numbers-only (20)
        // must render in full instead.
        let line = render(49);
        assert_eq!(line.trim_end(), "1 2 3 4 5 6 7 8 9 10");
    }

    #[test]
    fn even_below_every_forms_width_something_complete_still_renders() {
        // Below the narrowest form's own declared width (20), choose_form
        // still falls back to it rather than an even-narrower non-existent
        // form; render_bar's defensive per-slot check then draws as many
        // complete numbers-only slots as actually fit, never a partial one
        // (e.g. never a lone "1" of "10").
        assert_eq!(choose_form(10, &[KEYS_FULL, KEYS_SHORT, KEYS_NUMBERS_ONLY]), KEYS_NUMBERS_ONLY);
        let line = render(10);
        assert_eq!(line.trim_end(), "1 2 3 4 5");
    }

    // -----------------------------------------------------------------
    // mouse-panel-drag: the drag relabel (tasks.md 2.2).
    // -----------------------------------------------------------------

    fn render_dragging(width: u16) -> String {
        let area = Rect { x: 0, y: 0, width, height: 1 };
        let mut buf = Buffer::empty(area);
        render_keybar(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, true);
        (0..width).map(|x| buf[(x, 0)].symbol()).collect()
    }

    #[test]
    fn dragging_relabels_the_key_bar_to_the_exact_spec_string() {
        let line = render_dragging(80);
        assert_eq!(line.trim_end(), "Drop=Copy  Shift/RightBtn=Move  Esc=Cancel");
    }

    #[test]
    fn not_dragging_still_renders_the_ordinary_f_key_bar() {
        // The relabel must vanish the instant `dragging` is false again —
        // no lingering drag treatment (mouse-drag "Feedback ends with the
        // drag").
        let line = render(80);
        assert_eq!(line.trim_end(), "1Help 2Menu 3View 4Edit 5Copy 6RenMov 7Mkdir 8Delete 9PullDn 10Quit");
    }
}
