//! The F9 menu bar and its pull-downs.
//!
//! The bar is a full-width overlay on the top screen row, replacing the
//! panels' top borders and the clock while it is open. Pull-downs are
//! single-line CP437-framed boxes hanging directly below their title.

use filecommand_core::dialogs::clamp_overlay_dim;
use filecommand_core::listing::{display_width, pad_to_width, truncate_with_ellipsis};
use filecommand_core::menu::{entries, MenuEntry, MenuId, MenuState, ALL_MENUS};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::style::role_style;

/// Columns before the first title and between titles, matching the NC bar
/// `  Left     Files     Commands     Options     Right  `.
const LEAD: u16 = 2;
const GAP: u16 = 5;

/// Padding inside a pull-down between its frame and the item text.
const ITEM_PAD: usize = 1;
/// Minimum columns between an item's label and its shortcut hint.
const SHORTCUT_GAP: usize = 2;
/// Pull-down minimum interior width/height — small enough that a box is
/// still legible when clamped (responsive-layout "Unified overlay
/// geometry").
const MIN_INNER_W: u16 = 10;
const MIN_BOX_H: u16 = 3;

/// Where each menu title starts on the bar. Shared by the bar renderer and
/// the pull-down renderer so a box always hangs under its own title.
pub fn title_positions() -> Vec<(MenuId, u16)> {
    let mut out = Vec::with_capacity(ALL_MENUS.len());
    let mut x = LEAD;
    for id in ALL_MENUS {
        out.push((id, x));
        x += id.title().chars().count() as u16 + GAP;
    }
    out
}

pub fn render_menu_bar(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, menu: &MenuState) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let bar_style = role_style(theme, Role::MenuBar, depth);
    let highlight_style = role_style(theme, Role::MenuHighlight, depth);
    let hotkey_style = role_style(theme, Role::MenuHotkey, depth);

    let y = area.y;
    buf.set_string(area.x, y, " ".repeat(area.width as usize), bar_style);

    for (id, x) in title_positions() {
        let title = id.title();
        let x = area.x + x;
        if x >= area.x + area.width {
            break;
        }
        let active = id == menu.active;
        let style = if active { highlight_style } else { bar_style };
        buf.set_string(x, y, clip(title, (area.x + area.width - x) as usize), style);
        // The hotkey letter stays bright-yellow on the cyan bar, but an
        // active title is a solid white-on-black block: recoloring one
        // letter inside it would break the highlight.
        if !active {
            let letter = &title[..id.hotkey().len_utf8()];
            buf.set_string(x, y, letter, hotkey_style);
        }
    }

    if menu.pulldown_open {
        render_pulldown(buf, area, theme, depth, menu);
    }
}

/// One rendered pull-down row: its text and which style it takes. When
/// `inner_w` clamps below the row's natural width, the label+shortcut
/// combination truncates with `…` rather than silently overflowing
/// (responsive-layout "Unified overlay geometry").
fn item_rows(id: MenuId, inner_w: usize) -> Vec<Option<String>> {
    entries(id)
        .iter()
        .map(|entry| match entry {
            MenuEntry::Separator => None,
            MenuEntry::Item(item) => {
                let pad = " ".repeat(ITEM_PAD);
                let text = if item.shortcut.is_empty() {
                    format!("{pad}{}", item.label)
                } else {
                    let used = ITEM_PAD * 2 + display_width(item.label) + display_width(item.shortcut);
                    let spacer = " ".repeat(inner_w.saturating_sub(used).max(SHORTCUT_GAP));
                    format!("{pad}{}{spacer}{}{pad}", item.label, item.shortcut)
                };
                Some(pad_to_width(&truncate_with_ellipsis(&text, inner_w), inner_w))
            }
        })
        .collect()
}

/// The inner width a menu's pull-down needs to fit its widest row.
fn pulldown_inner_width(id: MenuId) -> usize {
    entries(id)
        .iter()
        .filter_map(|entry| match entry {
            MenuEntry::Item(item) => {
                let mut w = ITEM_PAD * 2 + display_width(item.label);
                if !item.shortcut.is_empty() {
                    w += SHORTCUT_GAP + display_width(item.shortcut);
                }
                Some(w)
            }
            MenuEntry::Separator => None,
        })
        .max()
        .unwrap_or(4)
}

fn render_pulldown(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, menu: &MenuState) {
    let body_style = role_style(theme, Role::MenuBody, depth);
    let highlight_style = role_style(theme, Role::MenuHighlight, depth);
    let disabled_style = role_style(theme, Role::MenuDisabled, depth);

    let id = menu.active;
    let content_rows = entries(id).len() as u16;
    // Width and height each clamp independently via the unified overlay
    // rule's per-dimension helper (responsive-layout "Unified overlay
    // geometry"); rows available is the terminal height minus the bar's
    // own row.
    let avail_h = area.height.saturating_sub(1);
    let box_w = clamp_overlay_dim(pulldown_inner_width(id) as u16 + 2, MIN_INNER_W + 2, area.width);
    let box_h = clamp_overlay_dim(content_rows + 2, MIN_BOX_H, avail_h);
    let inner_w = box_w.saturating_sub(2) as usize;
    // Frame top + frame bottom take two of the rows we have; if there
    // isn't room for even an empty framed box, draw nothing rather than a
    // single overlapping border row.
    if box_h < 2 {
        return;
    }
    let visible = box_h.saturating_sub(2) as usize;

    let title_x = title_positions().iter().find(|(m, _)| *m == id).map(|(_, x)| *x).unwrap_or(LEAD);
    // Hang the box under its title, shifted one column left so the title
    // sits over the box's interior rather than its frame — and shifted
    // further left just enough to stay on screen, rather than centering
    // (responsive-layout "Unified overlay geometry": "Pull-down box shifts
    // left instead of centering").
    let mut x = area.x + title_x.saturating_sub(1);
    let right_edge = area.x + area.width;
    if x + box_w > right_edge {
        x = right_edge.saturating_sub(box_w);
    }
    let y = area.y + 1;

    let rows = item_rows(id, inner_w);

    buf.set_string(x, y, format!("\u{250C}{}\u{2510}", "\u{2500}".repeat(inner_w)), body_style);
    for (i, row) in rows.iter().take(visible).enumerate() {
        let ry = y + 1 + i as u16;
        match row {
            None => {
                buf.set_string(x, ry, format!("\u{251C}{}\u{2524}", "\u{2500}".repeat(inner_w)), body_style);
            }
            Some(text) => {
                buf.set_string(x, ry, "\u{2502}", body_style);
                let enabled = matches!(entries(id).get(i), Some(MenuEntry::Item(item)) if item.is_enabled());
                let style = if i == menu.selected && enabled {
                    highlight_style
                } else if enabled {
                    body_style
                } else {
                    disabled_style
                };
                buf.set_string(x + 1, ry, text, style);
                buf.set_string(x + 1 + inner_w as u16, ry, "\u{2502}", body_style);
            }
        }
    }
    buf.set_string(x, y + 1 + visible as u16, format!("\u{2514}{}\u{2518}", "\u{2500}".repeat(inner_w)), body_style);
}

/// The clickable menu-title rects `render_menu_bar` currently draws across
/// the top row of `area` (mouse-input "Key bar, menu bar, pull-down items,
/// and dialog buttons are clickable"). Mirrors `title_positions`' exact
/// placement, same as the renderer itself does.
pub fn hit_titles(area: Rect) -> Vec<(Rect, MenuId)> {
    if area.height == 0 || area.width == 0 {
        return vec![];
    }
    let mut out = Vec::with_capacity(ALL_MENUS.len());
    for (id, x) in title_positions() {
        let x = area.x + x;
        if x >= area.x + area.width {
            break;
        }
        let w = (id.title().chars().count() as u16).min(area.x + area.width - x);
        out.push((Rect { x, y: area.y, width: w, height: 1 }, id));
    }
    out
}

/// The open pull-down's clickable item rects at `area` (the same full-
/// screen `area` `render_menu_bar`/`render_pulldown` are given), indexed by
/// position in `menu::entries(menu.active)` so the index matches exactly
/// what `MenuState::selected` would land on — separators simply have no
/// entry here. Mirrors `render_pulldown`'s box-geometry math (`box_w`,
/// `box_h`, `x`, `y`, `visible`) so an item rect can never land somewhere
/// the box isn't actually drawn. Empty when the pull-down is closed.
pub fn hit_items(area: Rect, menu: &MenuState) -> Vec<(Rect, usize)> {
    if !menu.pulldown_open {
        return vec![];
    }
    let id = menu.active;
    let content_rows = entries(id).len() as u16;
    let avail_h = area.height.saturating_sub(1);
    let box_w = clamp_overlay_dim(pulldown_inner_width(id) as u16 + 2, MIN_INNER_W + 2, area.width);
    let box_h = clamp_overlay_dim(content_rows + 2, MIN_BOX_H, avail_h);
    if box_h < 2 {
        return vec![];
    }
    let inner_w = box_w.saturating_sub(2);
    let visible = box_h.saturating_sub(2) as usize;

    let title_x = title_positions().iter().find(|(m, _)| *m == id).map(|(_, x)| *x).unwrap_or(LEAD);
    let mut x = area.x + title_x.saturating_sub(1);
    let right_edge = area.x + area.width;
    if x + box_w > right_edge {
        x = right_edge.saturating_sub(box_w);
    }
    let y = area.y + 1;

    let mut out = Vec::new();
    for (i, entry) in entries(id).iter().take(visible).enumerate() {
        if matches!(entry, MenuEntry::Item(_)) {
            let ry = y + 1 + i as u16;
            out.push((Rect { x: x + 1, y: ry, width: inner_w, height: 1 }, i));
        }
    }
    out
}

fn clip(s: &str, max_w: usize) -> String {
    let mut out = String::new();
    let mut acc = 0usize;
    for ch in s.chars() {
        let cw = display_width(&ch.to_string());
        if acc + cw > max_w {
            break;
        }
        out.push(ch);
        acc += cw;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(menu: &MenuState, width: u16, height: u16) -> Vec<String> {
        let area = Rect { x: 0, y: 0, width, height };
        let mut buf = Buffer::empty(area);
        render_menu_bar(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, menu);
        (0..height).map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect()).collect()
    }

    /// Column (not byte) index of the first/last `needle` in a row. Box
    /// glyphs are three bytes each, so `str::find` would report a byte
    /// offset far past the real screen column.
    fn col_of(row: &str, needle: char) -> Option<usize> {
        row.chars().position(|c| c == needle)
    }

    fn last_col_of(row: &str, needle: char) -> Option<usize> {
        row.chars().enumerate().filter(|(_, c)| *c == needle).map(|(i, _)| i).last()
    }

    #[test]
    fn bar_lists_the_five_titles_in_order() {
        let mut menu = MenuState::opened();
        menu.pulldown_open = false;
        let bar = &render(&menu, 80, 1)[0];
        let positions: Vec<usize> = ["Left", "Files", "Commands", "Options", "Right"]
            .iter()
            .map(|t| bar.find(t).unwrap_or_else(|| panic!("`{t}` missing from `{bar}`")))
            .collect();
        for w in positions.windows(2) {
            assert!(w[0] < w[1], "titles must appear in bar order");
        }
    }

    #[test]
    fn pulldown_hangs_below_its_own_title() {
        let menu = MenuState::for_menu(MenuId::Files);
        let rows = render(&menu, 80, 24);
        let title_x = rows[0].find("Files").unwrap();
        let box_x = col_of(&rows[1], '\u{250C}').unwrap();
        assert_eq!(box_x, title_x - 1, "the box frames the title's column");
    }

    #[test]
    fn pulldown_is_single_line_framed_with_cp437_glyphs() {
        let rows = render(&MenuState::for_menu(MenuId::Options), 80, 24);
        assert!(rows[1].contains('\u{250C}') && rows[1].contains('\u{2510}'));
        assert!(rows.iter().any(|r| r.contains('\u{2514}') && r.contains('\u{2518}')));
        assert!(rows.iter().any(|r| r.contains('\u{2502}')));
    }

    #[test]
    fn separator_rows_are_drawn_with_the_horizontal_rule_glyph() {
        let rows = render(&MenuState::for_menu(MenuId::Files), 80, 24);
        // FILES_MENU puts a separator after "Attributes" (row index 7).
        assert!(
            rows.iter().any(|r| r.contains('\u{251C}') && r.contains('\u{2524}')),
            "expected a `├──┤` separator row in:\n{}",
            rows.join("\n")
        );
    }

    #[test]
    fn every_item_and_shortcut_is_listed() {
        let rows = render(&MenuState::for_menu(MenuId::Files), 80, 24).join("\n");
        for label in ["View", "Edit", "Copy", "Rename/Move", "Make directory", "Delete", "Attributes", "Quit"] {
            assert!(rows.contains(label), "`{label}` missing from the Files pull-down");
        }
        assert!(rows.contains("F5"), "shortcut hints are rendered");
    }

    #[test]
    fn only_cp437_heritage_glyphs_are_emitted() {
        let rows = render(&MenuState::for_menu(MenuId::Left), 80, 24).join("");
        for ch in rows.chars() {
            assert!(
                ch.is_ascii() || "\u{250C}\u{2510}\u{2514}\u{2518}\u{2502}\u{2500}\u{251C}\u{2524}".contains(ch),
                "non-CP437 glyph `{ch}` (U+{:04X}) in the menu",
                ch as u32
            );
        }
    }

    #[test]
    fn a_closed_pulldown_leaves_the_rows_below_the_bar_untouched() {
        let mut menu = MenuState::opened();
        menu.pulldown_open = false;
        let rows = render(&menu, 80, 5);
        assert!(rows[1..].iter().all(|r| r.trim().is_empty()), "nothing is drawn below a closed pull-down");
    }

    #[test]
    fn the_last_menu_s_pulldown_stays_on_screen() {
        // `Right`'s title sits far along the bar; its box must be nudged
        // left rather than drawn off the edge.
        let menu = MenuState::for_menu(MenuId::Right);
        let rows = render(&menu, 50, 24);
        let box_x = col_of(&rows[1], '\u{250C}').expect("the box is drawn");
        let box_end = last_col_of(&rows[1], '\u{2510}').expect("the box closes");
        assert!(box_end < 50, "box right edge {box_end} ran off a 50-column screen");
        assert!(box_x < box_end);
    }

    #[test]
    fn a_short_screen_drops_the_pulldown_rather_than_overflowing() {
        // Two rows leaves no room for even an empty framed box under the
        // bar — a genuinely zero-space case, not one the unified overlay
        // rule's "always clamp and draw something" applies to.
        let rows = render(&MenuState::for_menu(MenuId::Files), 80, 2);
        assert!(rows[1].trim().is_empty());
    }

    #[test]
    fn a_narrow_screen_clamps_and_truncates_rather_than_dropping() {
        // With *some* room (unlike the zero-space case above), the pull-
        // down clamps to what's available and truncates item text with
        // `…`, rather than disappearing (responsive-layout "Unified
        // overlay geometry").
        let rows = render(&MenuState::for_menu(MenuId::Commands), 20, 24);
        assert!(rows[1].contains('\u{250C}') && rows[1].contains('\u{2510}'), "the box still draws: `{}`", rows[1]);
        let joined = rows.join("\n");
        assert!(joined.contains('\u{2026}'), "the widest item must truncate with an ellipsis:\n{joined}");
    }

    #[test]
    fn pulldown_width_fits_the_widest_item() {
        let inner = pulldown_inner_width(MenuId::Commands);
        let widest = display_width("Compare directories") + ITEM_PAD * 2;
        assert!(inner >= widest, "inner width {inner} cannot fit `Compare directories` ({widest})");
    }
}
