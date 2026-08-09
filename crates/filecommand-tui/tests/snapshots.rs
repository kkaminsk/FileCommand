//! Rendering snapshot tests: ratatui `TestBackend` + insta, with a fixed
//! time/size/locale (no real clock, no real filesystem — panel contents are
//! constructed by hand so output is fully deterministic).

use std::ffi::OsString;
use std::path::PathBuf;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use filecommand_core::listing::{DateTime, Entry, EntryKind};
use filecommand_core::panel::{ListingProgress, PanelState};
use filecommand_core::theme::{ColorDepth, Theme};
use filecommand_core::{State, UiPhase};

use filecommand_tui::views;

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

fn fixed_identity() -> [String; 4] {
    [
        "FileCommand".to_string(),
        "Version 0.1.0".to_string(),
        "Copyright (C) 2026 The FileCommand Authors".to_string(),
        "Inspired by the Norton Commander, 1986-1998".to_string(),
    ]
}

fn sample_entries() -> Vec<Entry> {
    vec![
        Entry::parent_dir(),
        Entry {
            name: OsString::from("Documents"),
            kind: EntryKind::Directory,
            size: 0,
            modified: Some(DateTime { year: 2026, month: 8, day: 7, hour: 14, minute: 36 }),
        },
        Entry {
            name: OsString::from("notes.txt"),
            kind: EntryKind::File,
            size: 2048,
            modified: Some(DateTime { year: 2026, month: 8, day: 7, hour: 9, minute: 5 }),
        },
    ]
}

fn sample_state(theme: Theme) -> State {
    let mut left = PanelState::new(PathBuf::from(r"C:\Users\demo"));
    left.entries = sample_entries();
    left.progress = ListingProgress::Complete { count: left.entries.len() };
    left.cursor = 1;

    let mut right = PanelState::new(PathBuf::from(r"C:\Users\demo\Documents"));
    right.progress = ListingProgress::Streaming { count: 12_345 };

    State { left, right, ..State::empty(theme) }
}

/// Pinned so the clock widget's top-right-corner glyphs don't make every
/// snapshot in this file non-deterministic.
const FIXED_CLOCK_TEXT: &str = "3:04 PM";

fn render(state: &State, size: (u16, u16)) -> String {
    let area = Rect { x: 0, y: 0, width: size.0, height: size.1 };
    let mut buf = Buffer::empty(area);
    views::render(&mut buf, area, state, ColorDepth::Ansi16, &fixed_identity(), FIXED_CLOCK_TEXT, None);
    buffer_to_text(&buf, area)
}

#[test]
fn full_panel_active_and_inactive_nc_classic() {
    let state = sample_state(Theme::classic());
    insta::assert_snapshot!("full_panel_active_inactive_nc_classic", render(&state, (80, 24)));
}

/// A second, independent guard against the same regression `insta`
/// snapshots protect against — a plain `assert_eq!` against a literal
/// golden string, which (unlike an `insta` snapshot) cannot be
/// silently re-accepted with `cargo insta review`/`--accept` without a
/// deliberate source edit. Pins the exact 80x24 nominal-size rendering
/// the column-ladder (D3) and Brief-formula (D4) anchors both promise is
/// byte-identical to the pre-responsive-layout output (panel-navigation
/// "Full display mode layout"; additional-panel-modes "Brief display
/// mode"; responsive-layout task 6.3).
#[test]
fn full_panel_at_80x24_matches_the_pinned_golden_string() {
    let state = sample_state(Theme::classic());
    let text = render(&state, (80, 24));

    let header_row = text.lines().nth(1).unwrap();
    assert_eq!(
        header_row,
        "\u{2551}Name\u{2193}        \u{2502} Size    Date    Time   \u{2551}\u{2551}Name\u{2193}        \u{2502} Size    Date    Time   \u{2551}",
        "the Full-mode column header at the 80x24 nominal size changed (panel-navigation \"Full display mode layout\", D3 anchor)"
    );
    let first_entry_row = text.lines().nth(2).unwrap();
    assert_eq!(
        first_entry_row,
        "\u{2551}..           \u{2502} \u{25B6}UP--DI                \u{2551}\u{2551}                                      \u{2551}",
        "the Full-mode entry row at the 80x24 nominal size changed"
    );
    let last_line = text.lines().last().unwrap().trim_end();
    assert_eq!(last_line, "1Help 2Menu 3View 4Edit 5Copy 6RenMov 7Mkdir 8Delete 9PullDn 10Quit", "the full F-key bar form changed at 80 columns");
}

#[test]
fn streaming_ministatus_shows_reading_count() {
    let state = sample_state(Theme::classic());
    let text = render(&state, (80, 24));
    assert!(text.contains("Reading\u{2026} 12,345"), "expected streaming mini-status in:\n{text}");
}

#[test]
fn splash_nc_classic() {
    let state = State { phase: UiPhase::Splash { started_at_ms: 0 }, ..State::empty(Theme::classic()) };
    insta::assert_snapshot!("splash_nc_classic", render(&state, (80, 24)));
}

#[test]
fn splash_nc_mono() {
    let state = State { phase: UiPhase::Splash { started_at_ms: 0 }, ..State::empty(Theme::mono()) };
    insta::assert_snapshot!("splash_nc_mono", render(&state, (80, 24)));
}

#[test]
fn terminal_too_small_placeholder() {
    let state = State { phase: UiPhase::Placeholder, term_size: (40, 10), ..State::empty(Theme::classic()) };
    insta::assert_snapshot!("terminal_too_small_placeholder", render(&state, (40, 10)));
}

#[test]
fn splash_renders_at_60x16() {
    // startup-splash "Splash renders in the degraded band": the 48x9 box
    // fits at every supported size down to the 60x16 floor.
    let state = State { phase: UiPhase::Splash { started_at_ms: 0 }, ..State::empty(Theme::classic()) };
    insta::assert_snapshot!("splash_at_60x16_floor", render(&state, (60, 16)));
}

#[test]
fn placeholder_below_the_floor() {
    // application-shell "Placeholder below the floor": one column below
    // the new 60x16 floor still shows the placeholder, naming 60x16.
    let state = State { phase: UiPhase::Placeholder, term_size: (59, 16), ..State::empty(Theme::classic()) };
    let text = render(&state, (59, 16));
    assert!(text.contains("60x16"), "expected the placeholder to name the 60x16 floor:\n{text}");
    insta::assert_snapshot!("placeholder_below_the_60x16_floor", text);
}

#[test]
fn f_key_bar_static_labels() {
    let theme = Theme::classic();
    let area = Rect { x: 0, y: 0, width: 80, height: 1 };
    let mut buf = Buffer::empty(area);
    views::keybar::render_keybar(&mut buf, area, &theme, ColorDepth::Ansi16);
    insta::assert_snapshot!("f_key_bar", buffer_to_text(&buf, area));
}
