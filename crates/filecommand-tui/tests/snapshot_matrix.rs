//! A pinned matrix of renders at the sizes the degraded band spans — the
//! 60x16 hard floor, two points inside the degraded band, and the 80x24
//! nominal size plus a wide 120x30 terminal — covering a Full-mode panel,
//! a Brief-mode panel, the F-key bar alone, and a clamped dialog (the Help
//! window, since it has an explicit spec'd preferred/minimum size)
//! (responsive-layout "Degraded band and per-panel breakpoints", "Unified
//! overlay geometry", "F-key bar degradation forms").

use std::ffi::OsString;
use std::path::PathBuf;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use filecommand_core::dialogs::HelpState;
use filecommand_core::listing::{DateTime, Entry, EntryKind};
use filecommand_core::panel::{DisplayMode, ListingProgress, PanelState};
use filecommand_core::theme::{ColorDepth, Theme};

use filecommand_tui::views;

/// The degraded-band matrix: the 60x16 floor, two points inside the band,
/// the 80x24 nominal size, and a wide terminal.
const SIZES: [(u16, u16); 4] = [(60, 16), (70, 20), (80, 24), (120, 30)];

fn fixed_identity() -> [String; 4] {
    [
        "FileCommand".to_string(),
        "Version 0.1.0".to_string(),
        "Copyright (C) 2026 The FileCommand Authors".to_string(),
        "Inspired by the Norton Commander, 1986-1998".to_string(),
    ]
}

fn fixed_date() -> DateTime {
    DateTime { year: 2026, month: 1, day: 2, hour: 3, minute: 4 }
}

fn sample_panel(display_mode: DisplayMode) -> PanelState {
    let mut panel = PanelState::new(PathBuf::from(r"C:\Users\demo\projects\filecommand"));
    panel.entries = vec![
        Entry::parent_dir(),
        Entry { name: OsString::from("docs"), kind: EntryKind::Directory, size: 0, modified: Some(fixed_date()) },
        Entry { name: OsString::from("Cargo.toml"), kind: EntryKind::File, size: 612, modified: Some(fixed_date()) },
        Entry { name: OsString::from("readme.txt"), kind: EntryKind::File, size: 12_345, modified: Some(fixed_date()) },
    ];
    panel.progress = ListingProgress::Complete { count: panel.entries.len() };
    panel.display_mode = display_mode;
    panel.cursor = 1;
    panel
}

fn buffer_to_text(buf: &Buffer, area: Rect) -> String {
    let mut out = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            out.push_str(buf[(area.x + x, area.y + y)].symbol());
        }
        out.push('\n');
    }
    out
}

/// Renders just the left panel at its *real* split-derived width (the
/// default 50/50 split) rather than the full terminal width — this is
/// what actually exercises the column-ladder/Brief-formula degradation at
/// each matrix size, since a panel is always narrower than the terminal.
fn render_panel_at(display_mode: DisplayMode, size: (u16, u16)) -> String {
    let l = filecommand_tui::layout::compute(size, 50);
    let area = Rect { x: 0, y: 0, width: l.left.width, height: l.left.height };
    let mut buf = Buffer::empty(area);
    let panel = sample_panel(display_mode);
    let opposite = sample_panel(DisplayMode::Full);
    let theme = Theme::classic();
    views::panel::render_panel(&mut buf, area, &panel, &theme, ColorDepth::Ansi16, true, &fixed_identity(), &opposite, None);
    buffer_to_text(&buf, area)
}

fn render_keybar_at(width: u16) -> String {
    let area = Rect { x: 0, y: 0, width, height: 1 };
    let mut buf = Buffer::empty(area);
    views::keybar::render_keybar(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16);
    buffer_to_text(&buf, area)
}

fn render_help_at(size: (u16, u16)) -> String {
    let area = Rect { x: 0, y: 0, width: size.0, height: size.1 };
    let mut buf = Buffer::empty(area);
    views::help::render_help(&mut buf, area, &Theme::classic(), ColorDepth::Ansi16, &HelpState::new(), &fixed_identity());
    buffer_to_text(&buf, area)
}

#[test]
fn full_mode_panel_across_the_degraded_band() {
    for (w, h) in SIZES {
        let text = render_panel_at(DisplayMode::Full, (w, h));
        insta::assert_snapshot!(format!("matrix_full_panel_{w}x{h}"), text);
    }
}

#[test]
fn brief_mode_panel_across_the_degraded_band() {
    for (w, h) in SIZES {
        let text = render_panel_at(DisplayMode::Brief, (w, h));
        insta::assert_snapshot!(format!("matrix_brief_panel_{w}x{h}"), text);
    }
}

#[test]
fn keybar_across_the_degraded_band() {
    for (w, _h) in SIZES {
        let text = render_keybar_at(w);
        insta::assert_snapshot!(format!("matrix_keybar_{w}"), text);
    }
}

#[test]
fn help_window_across_the_degraded_band() {
    for (w, h) in SIZES {
        let text = render_help_at((w, h));
        insta::assert_snapshot!(format!("matrix_help_window_{w}x{h}"), text);
    }
}

/// Every size in the matrix is at or above the 60x16 hard floor and never
/// panics — the actual "at or above the floor" boundary is covered by
/// `application-shell`'s own tests, this just guards the matrix itself.
#[test]
fn every_matrix_size_is_at_or_above_the_floor() {
    for (w, h) in SIZES {
        assert!(w >= 60 && h >= 16, "matrix size {w}x{h} is below the 60x16 floor");
    }
}
