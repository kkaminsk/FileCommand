//! Startup splash: solid backdrop plus a centered double-line box with the
//! product identity lines — and, when animations are enabled, the entrance
//! that builds that box inside the 800 ms minimum hold (startup-splash
//! "Splash entrance animation").

use filecommand_core::anim;
use filecommand_core::dialogs::overlay_rect;
use filecommand_core::listing::display_width;
use filecommand_core::theme::{ColorDepth, Role, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::style::role_style;

const BOX_W: u16 = 48;
const BOX_H: u16 = 9;

/// The box rows the four identity lines sit on, in line order.
const LINE_ROWS: [u16; 4] = [2, 3, 5, 6];

/// The splash box's geometry, via the unified overlay rule. Preferred and
/// minimum coincide (48×9) — the box never clamps at or above the 60×16
/// floor (startup-splash "Splash renders in the degraded band"; design D1).
///
/// `elapsed` is the time since the splash started on the reducer clock when
/// animations are on, or `None` for the finished box. From
/// `SPLASH_ENTRANCE_MS` on, the animated splash is identical to the static
/// one, so it is the same code path.
pub fn render_splash(buf: &mut Buffer, area: Rect, theme: &Theme, depth: ColorDepth, identity_lines: &[String; 4], elapsed: Option<u64>) {
    let backdrop = role_style(theme, Role::ScreenBackdrop, depth);
    for y in area.y..area.y + area.height {
        buf.set_string(area.x, y, " ".repeat(area.width as usize), backdrop);
    }

    let r = overlay_rect((BOX_W, BOX_H), (BOX_W, BOX_H), (area.width, area.height));
    let box_w = r.width;
    let box_h = r.height;
    let inner_w = box_w.saturating_sub(2) as usize;
    if box_h < 3 {
        return;
    }
    let frame_style = role_style(theme, Role::SplashFrame, depth);
    let title_style = role_style(theme, Role::SplashTitle, depth);
    let version_style = role_style(theme, Role::SplashVersion, depth);
    let text_style = role_style(theme, Role::SplashText, depth);

    let box_x = area.x + r.x;
    let box_y = area.y + r.y;

    if let Some(elapsed) = elapsed.filter(|e| *e < anim::SPLASH_ENTRANCE_MS) {
        let geometry = Geometry { box_x, box_y, box_w, box_h, inner_w };
        let styles = [title_style, version_style, text_style, text_style];
        draw_entrance(buf, &geometry, identity_lines, frame_style, version_style, styles, elapsed);
        return;
    }

    let top = format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(inner_w));
    let bottom = format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(inner_w));
    let blank = format!("\u{2551}{}\u{2551}", " ".repeat(inner_w));

    buf.set_string(box_x, box_y, &top, frame_style);
    buf.set_string(box_x, box_y + 1, &blank, frame_style);
    write_row(buf, box_x, box_y + 2, inner_w, &identity_lines[0], title_style, frame_style);
    write_row(buf, box_x, box_y + 3, inner_w, &identity_lines[1], version_style, frame_style);
    buf.set_string(box_x, box_y + 4, &blank, frame_style);
    write_row(buf, box_x, box_y + 5, inner_w, &identity_lines[2], text_style, frame_style);
    write_row(buf, box_x, box_y + 6, inner_w, &identity_lines[3], text_style, frame_style);
    buf.set_string(box_x, box_y + 7, &blank, frame_style);
    buf.set_string(box_x, box_y + 8, &bottom, frame_style);
}

#[allow(clippy::too_many_arguments)]
fn write_row(
    buf: &mut Buffer,
    box_x: u16,
    y: u16,
    inner_w: usize,
    text: &str,
    text_style: ratatui::style::Style,
    frame_style: ratatui::style::Style,
) {
    buf.set_string(box_x, y, "\u{2551}", frame_style);
    let pad = inner_w.saturating_sub(display_width(text));
    let left = pad / 2;
    let right = pad - left;
    let line = format!("{}{}{}", " ".repeat(left), text, " ".repeat(right));
    buf.set_string(box_x + 1, y, &line, text_style);
    buf.set_string(box_x + 1 + inner_w as u16, y, "\u{2551}", frame_style);
}

/// Where the box sits on screen.
struct Geometry {
    box_x: u16,
    box_y: u16,
    box_w: u16,
    box_h: u16,
    inner_w: usize,
}

/// The frame glyph at box-relative `(dx, dy)` on the perimeter.
fn frame_glyph(g: &Geometry, dx: u16, dy: u16) -> char {
    let (right, bottom) = (g.box_w - 1, g.box_h - 1);
    match (dx, dy) {
        (0, 0) => '\u{2554}',
        (x, 0) if x == right => '\u{2557}',
        (0, y) if y == bottom => '\u{255A}',
        (x, y) if x == right && y == bottom => '\u{255D}',
        (_, 0) => '\u{2550}',
        (_, y) if y == bottom => '\u{2550}',
        _ => '\u{2551}',
    }
}

/// The perimeter cells in clockwise order from the top-left corner: the top
/// row left→right, the right column down, the bottom row right→left, the
/// left column up. `2W + 2H − 4` cells, each exactly once.
fn perimeter(g: &Geometry) -> Vec<(u16, u16)> {
    let (w, h) = (g.box_w, g.box_h);
    let mut cells = Vec::with_capacity(2 * (w as usize + h as usize) - 4);
    cells.extend((0..w).map(|x| (x, 0)));
    cells.extend((1..h).map(|y| (w - 1, y)));
    cells.extend((0..w - 1).rev().map(|x| (x, h - 1)));
    cells.extend((1..h - 1).rev().map(|y| (0, y)));
    cells
}

/// One identity character with the display column it starts at, relative to
/// the line's first cell.
struct LineCell {
    ch: char,
    col: u16,
}

fn line_cells(text: &str) -> Vec<LineCell> {
    let mut col = 0u16;
    text.chars()
        .map(|ch| {
            let cell = LineCell { ch, col };
            col += display_width(&ch.to_string()) as u16;
            cell
        })
        .collect()
}

/// Draw the splash entrance at `elapsed` ms (< `SPLASH_ENTRANCE_MS`): the
/// frame traces in clockwise, each identity line reveals outward from its
/// center in `splash.frame` before settling to its role, and one
/// `splash.version` window sweeps across the product name. Everything is a
/// pure function of `elapsed`.
fn draw_entrance(
    buf: &mut Buffer,
    g: &Geometry,
    identity_lines: &[String; 4],
    frame_style: Style,
    version_style: Style,
    line_styles: [Style; 4],
    elapsed: u64,
) {
    // Frame trace: the first N perimeter cells, clockwise from the top-left.
    let ring = perimeter(g);
    let traced = anim::reveal_count(anim::progress(elapsed, anim::SPLASH_TRACE_MS), ring.len());
    for &(dx, dy) in ring.iter().take(traced) {
        buf.set_string(g.box_x + dx, g.box_y + dy, frame_glyph(g, dx, dy).to_string(), frame_style);
    }
    // The interior joins once the frame is complete, so before that only the
    // trace is on the backdrop and no identity text can show.
    if elapsed < anim::SPLASH_TRACE_MS {
        return;
    }
    for dy in 1..g.box_h - 1 {
        buf.set_string(g.box_x + 1, g.box_y + dy, " ".repeat(g.inner_w), frame_style);
    }

    for (i, text) in identity_lines.iter().enumerate() {
        let line_start = anim::SPLASH_IDENTITY_START_MS + anim::SPLASH_IDENTITY_STAGGER_MS * i as u64;
        if elapsed < line_start {
            continue;
        }
        let cells = line_cells(text);
        let left = g.inner_w.saturating_sub(display_width(text)) / 2;
        let x0 = g.box_x + 1 + left as u16;
        let y = g.box_y + LINE_ROWS[i];
        // Characters appear in order of distance from the line's center, so
        // the line opens outward; `rank` runs from 0 at the center to
        // `n - 1` at the ends and is scaled onto the line's reveal duration,
        // so the outermost characters land exactly as the line completes.
        let n = cells.len();
        let span = n.saturating_sub(1).max(1) as u64;
        for (j, cell) in cells.iter().enumerate() {
            let rank = (2 * j as i64 - (n as i64 - 1)).unsigned_abs();
            let appears = line_start + anim::SPLASH_IDENTITY_LINE_MS * rank / span;
            if elapsed < appears {
                continue;
            }
            let style = if elapsed - appears < anim::SPLASH_IDENTITY_FLASH_MS { frame_style } else { line_styles[i] };
            buf.set_string(x0 + cell.col, y, cell.ch.to_string(), style);
        }
    }

    // Title sweep: a 3-cell `splash.version` window crosses the product name
    // once, restyling the name's own characters as it passes.
    if elapsed >= anim::SPLASH_SWEEP_START_MS {
        let name = &identity_lines[0];
        let cells = line_cells(name);
        let name_w = display_width(name) as u16;
        let left = g.inner_w.saturating_sub(name_w as usize) / 2;
        let x0 = g.box_x + 1 + left as u16;
        let p = anim::progress(elapsed - anim::SPLASH_SWEEP_START_MS, anim::SPLASH_SWEEP_END_MS - anim::SPLASH_SWEEP_START_MS);
        let pos = anim::sweep_pos(p, name_w, anim::SPLASH_SWEEP_WINDOW);
        for cell in &cells {
            let col = cell.col as i32;
            if col >= pos && col < pos + anim::SPLASH_SWEEP_WINDOW as i32 {
                buf.set_string(x0 + cell.col, g.box_y + LINE_ROWS[0], cell.ch.to_string(), version_style);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines() -> [String; 4] {
        [
            "FileCommand".to_string(),
            "Version 0.1.0".to_string(),
            "Copyright (C) 2026 The FileCommand Authors".to_string(),
            "Inspired by the Norton Commander, 1986-1998".to_string(),
        ]
    }

    fn render(elapsed: Option<u64>) -> Buffer {
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let mut buf = Buffer::empty(area);
        render_splash(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, &lines(), elapsed);
        buf
    }

    fn text(buf: &Buffer) -> String {
        let a = buf.area;
        (0..a.height).map(|y| (0..a.width).map(|x| buf[(x, y)].symbol()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    /// Frame glyphs (the double-line set) present in `buf`.
    fn frame_cells(buf: &Buffer) -> usize {
        text(buf).chars().filter(|c| "\u{2554}\u{2557}\u{255A}\u{255D}\u{2550}\u{2551}".contains(*c)).count()
    }

    #[test]
    fn finished_splash_is_identical_with_animations_off_or_after_the_entrance() {
        let still = render(None);
        assert_eq!(render(Some(anim::SPLASH_ENTRANCE_MS)), still);
        assert_eq!(render(Some(800)), still);
        assert_eq!(render(Some(60_000)), still, "stays final forever after");
    }

    #[test]
    fn entrance_starts_on_a_bare_backdrop() {
        let first = render(Some(0));
        assert_eq!(frame_cells(&first), 0, "frame 1 draws no box yet");
        assert!(!text(&first).contains("FileCommand"));
        // ...but the blue backdrop is already solid.
        let backdrop = role_style(&Theme::classic(), Role::ScreenBackdrop, ColorDepth::Ansi16);
        assert_eq!(first[(0, 0)].bg, backdrop.bg.unwrap());
    }

    #[test]
    fn frame_traces_clockwise_from_the_top_left_corner() {
        let perimeter_cells = 2 * (BOX_W as usize + BOX_H as usize) - 4;
        // 100 ms is half the 200 ms trace: half the perimeter, no text yet.
        let half = render(Some(100));
        assert_eq!(frame_cells(&half), perimeter_cells / 2);
        let t = text(&half);
        assert!(!t.contains("FileCommand") && !t.contains("Version"), "no identity text during the trace");

        // The very first cells are the top-left corner and the top edge.
        let r = overlay_rect((BOX_W, BOX_H), (BOX_W, BOX_H), (80, 24));
        let (bx, by) = (r.x, r.y);
        assert_eq!(half[(bx, by)].symbol(), "\u{2554}", "the trace starts at the top-left corner");
        assert_eq!(half[(bx + 1, by)].symbol(), "\u{2550}");
        // Clockwise: the top edge is complete before the left column starts.
        assert_eq!(half[(bx, by + 1)].symbol(), " ", "the left column is traced last");
        assert_eq!(half[(bx + BOX_W - 1, by)].symbol(), "\u{2557}", "the top-right corner is reached next");
    }

    #[test]
    fn frame_is_complete_when_the_trace_ends() {
        let done = render(Some(anim::SPLASH_TRACE_MS));
        let perimeter_cells = 2 * (BOX_W as usize + BOX_H as usize) - 4;
        assert_eq!(frame_cells(&done), perimeter_cells);
    }

    #[test]
    fn identity_lines_reveal_outward_from_their_centers() {
        let t = text(&render(Some(400)));
        assert!(t.contains("FileCommand"), "product name fully revealed by 400 ms");
        assert!(t.contains("Version 0.1.0"), "version line fully revealed by 400 ms");
        assert!(!t.contains("Copyright (C) 2026 The FileCommand Authors"), "copyright still opening");
        assert!(t.contains("Copyright") || t.contains("(C) 2026") || t.contains("The File"), "...but its middle is already showing:\n{t}");
        assert!(!t.contains("Inspired"), "tribute line has not started at 400 ms");

        // The ends of the copyright line are the last to appear: its first
        // character is still missing while a central one is present.
        let r = overlay_rect((BOX_W, BOX_H), (BOX_W, BOX_H), (80, 24));
        let row: String = (r.x + 1..r.x + BOX_W - 1).map(|x| render(Some(400))[(x, r.y + LINE_ROWS[2])].symbol().to_string()).collect();
        assert!(!row.trim_start().starts_with('C'), "left end not yet revealed: `{row}`");
        assert!(!row.trim_end().ends_with('s'), "right end not yet revealed: `{row}`");
    }

    #[test]
    fn newly_revealed_characters_flash_in_the_frame_color_then_settle() {
        let theme = Theme::classic();
        let frame = role_style(&theme, Role::SplashFrame, ColorDepth::Ansi16);
        let title = role_style(&theme, Role::SplashTitle, ColorDepth::Ansi16);
        let r = overlay_rect((BOX_W, BOX_H), (BOX_W, BOX_H), (80, 24));
        let y = r.y + LINE_ROWS[0];
        // "FileCommand" (11 cells) centered in the 46-cell interior.
        let center_x = r.x + 1 + (46 - 11) / 2 + 5;

        // The center character appears at the line's start (200 ms) in the
        // frame role...
        let appearing = render(Some(210));
        assert_eq!(appearing[(center_x, y)].symbol(), "o", "the middle of `FileCommand`");
        assert_eq!(appearing[(center_x, y)].fg, frame.fg.unwrap());
        // ...and has settled to the title role once 66 ms have passed.
        let settled = render(Some(200 + anim::SPLASH_IDENTITY_FLASH_MS + 20));
        assert_eq!(settled[(center_x, y)].fg, title.fg.unwrap());
    }

    #[test]
    fn title_sweep_crosses_the_product_name_in_the_version_color() {
        let theme = Theme::classic();
        let version = role_style(&theme, Role::SplashVersion, ColorDepth::Ansi16);
        let title = role_style(&theme, Role::SplashTitle, ColorDepth::Ansi16);
        assert_ne!(version.fg, title.fg, "the sweep is only visible if the two roles differ");
        let r = overlay_rect((BOX_W, BOX_H), (BOX_W, BOX_H), (80, 24));
        let y = r.y + LINE_ROWS[0];
        let name_x = r.x + 1 + (46 - 11) / 2;

        let swept = |at: u64| -> Vec<usize> {
            let buf = render(Some(at));
            (0..11).filter(|i| buf[(name_x + *i as u16, y)].fg == version.fg.unwrap()).collect()
        };
        assert!(swept(560).is_empty(), "the window starts off the left edge");
        let mid = swept(650);
        assert_eq!(mid.len(), anim::SPLASH_SWEEP_WINDOW as usize, "a 3-cell window part-way across: {mid:?}");
        assert!(mid.windows(2).all(|w| w[1] == w[0] + 1), "contiguous: {mid:?}");
        assert!(swept(759).len() <= 1, "only the window's trailing cell is left over the name by 760 ms");
    }

    #[test]
    fn entrance_never_draws_outside_the_box() {
        let r = overlay_rect((BOX_W, BOX_H), (BOX_W, BOX_H), (80, 24));
        let backdrop = role_style(&Theme::classic(), Role::ScreenBackdrop, ColorDepth::Ansi16);
        for at in (0..anim::SPLASH_ENTRANCE_MS).step_by(20) {
            let buf = render(Some(at));
            for y in 0..24u16 {
                for x in 0..80u16 {
                    let inside = x >= r.x && x < r.x + BOX_W && y >= r.y && y < r.y + BOX_H;
                    if !inside {
                        assert_eq!(buf[(x, y)].symbol(), " ", "stray glyph at ({x},{y}) at {at} ms");
                        assert_eq!(buf[(x, y)].bg, backdrop.bg.unwrap());
                    }
                }
            }
        }
    }
}
