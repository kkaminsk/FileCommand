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
use filecommand_core::{PanelSide, State, UiPhase};

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

    State {
        left,
        right,
        active: PanelSide::Left,
        command_line: String::new(),
        phase: UiPhase::Panels,
        theme,
        term_size: (80, 24),
    }
}

fn render(state: &State, size: (u16, u16)) -> String {
    let area = Rect { x: 0, y: 0, width: size.0, height: size.1 };
    let mut buf = Buffer::empty(area);
    views::render(&mut buf, area, state, ColorDepth::Ansi16, &fixed_identity());
    buffer_to_text(&buf, area)
}

#[test]
fn full_panel_active_and_inactive_nc_classic() {
    let state = sample_state(Theme::classic());
    insta::assert_snapshot!("full_panel_active_inactive_nc_classic", render(&state, (80, 24)));
}

#[test]
fn streaming_ministatus_shows_reading_count() {
    let state = sample_state(Theme::classic());
    let text = render(&state, (80, 24));
    assert!(text.contains("Reading\u{2026} 12,345"), "expected streaming mini-status in:\n{text}");
}

#[test]
fn splash_nc_classic() {
    let state = State {
        left: PanelState::new(PathBuf::from("/")),
        right: PanelState::new(PathBuf::from("/")),
        active: PanelSide::Left,
        command_line: String::new(),
        phase: UiPhase::Splash { started_at_ms: 0 },
        theme: Theme::classic(),
        term_size: (80, 24),
    };
    insta::assert_snapshot!("splash_nc_classic", render(&state, (80, 24)));
}

#[test]
fn splash_nc_mono() {
    let state = State {
        left: PanelState::new(PathBuf::from("/")),
        right: PanelState::new(PathBuf::from("/")),
        active: PanelSide::Left,
        command_line: String::new(),
        phase: UiPhase::Splash { started_at_ms: 0 },
        theme: Theme::mono(),
        term_size: (80, 24),
    };
    insta::assert_snapshot!("splash_nc_mono", render(&state, (80, 24)));
}

#[test]
fn terminal_too_small_placeholder() {
    let state = State {
        left: PanelState::new(PathBuf::from("/")),
        right: PanelState::new(PathBuf::from("/")),
        active: PanelSide::Left,
        command_line: String::new(),
        phase: UiPhase::Placeholder,
        theme: Theme::classic(),
        term_size: (40, 10),
    };
    insta::assert_snapshot!("terminal_too_small_placeholder", render(&state, (40, 10)));
}

#[test]
fn f_key_bar_static_labels() {
    let theme = Theme::classic();
    let area = Rect { x: 0, y: 0, width: 80, height: 1 };
    let mut buf = Buffer::empty(area);
    views::keybar::render_keybar(&mut buf, area, &theme, ColorDepth::Ansi16);
    insta::assert_snapshot!("f_key_bar", buffer_to_text(&buf, area));
}
