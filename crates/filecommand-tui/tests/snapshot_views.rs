//! Snapshot tests for the renderers, using ratatui's `TestBackend` so no
//! real terminal is required. State is fully pinned (fixed theme, fixed
//! entries/dates/sizes, fixed identity lines, fixed terminal size) so
//! output is deterministic across runs and locales.

use std::path::PathBuf;

use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;

use filecommand_core::listing::{DateTime, Entry, EntryKind};
use filecommand_core::panel::{ListingProgress, PanelState, SortDirection};
use filecommand_core::theme::{ColorDepth, Theme};
use filecommand_core::{PanelSide, State, UiPhase};

use filecommand_tui::views;

/// The identity lines used on the real splash screen are pinned here rather
/// than derived from `env!("CARGO_PKG_VERSION")`/the current year, so the
/// snapshot doesn't change every time the crate version or the calendar
/// does.
fn pinned_identity_lines() -> [String; 4] {
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

fn fixed_entries() -> Vec<Entry> {
    vec![
        Entry::parent_dir(),
        Entry { name: "docs".into(), kind: EntryKind::Directory, size: 0, modified: Some(fixed_date()) },
        Entry { name: "Cargo.toml".into(), kind: EntryKind::File, size: 612, modified: Some(fixed_date()) },
        Entry { name: "readme.txt".into(), kind: EntryKind::File, size: 12_345, modified: Some(fixed_date()) },
    ]
}

fn complete_panel(cwd: &str, cursor: usize) -> PanelState {
    let mut panel = PanelState::new(PathBuf::from(cwd));
    panel.entries = fixed_entries();
    panel.progress = ListingProgress::Complete { count: panel.entries.len() };
    panel.sort_direction = SortDirection::Asc;
    panel.cursor = cursor;
    panel
}

fn streaming_panel(cwd: &str, count: usize) -> PanelState {
    let mut panel = PanelState::new(PathBuf::from(cwd));
    panel.entries = fixed_entries().into_iter().take(count).collect();
    panel.progress = ListingProgress::Streaming { count };
    panel
}

fn base_state(phase: UiPhase, theme: Theme) -> State {
    State {
        left: complete_panel(r"C:\Users\demo\left", 1),
        right: complete_panel(r"C:\Users\demo\right", 0),
        active: PanelSide::Left,
        command_line: String::new(),
        phase,
        theme,
        term_size: (80, 24),
    }
}

/// Render `state` at `width`x`height` through a real ratatui `TestBackend`
/// and return the plain-text glyph grid (styling is checked separately by
/// direct role/style assertions, not by the text snapshot).
fn render_to_text(width: u16, height: u16, state: &State, depth: ColorDepth) -> String {
    let identity_lines = pinned_identity_lines();
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("TestBackend terminal");
    terminal
        .draw(|frame| {
            let area = frame.area();
            views::render(frame.buffer_mut(), area, state, depth, &identity_lines);
        })
        .expect("draw into TestBackend");
    buffer_to_text(terminal.backend().buffer())
}

fn buffer_to_text(buf: &Buffer) -> String {
    let area = buf.area;
    let mut out = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            out.push_str(buf[(area.x + x, area.y + y)].symbol());
        }
        out.push('\n');
    }
    out
}

#[test]
fn snapshot_full_panels_active_left_inactive_right() {
    let state = base_state(UiPhase::Panels, Theme::classic());
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    insta::assert_snapshot!("full_panels_active_left", text);
}

#[test]
fn snapshot_full_panels_active_right_inactive_left() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.active = PanelSide::Right;
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    insta::assert_snapshot!("full_panels_active_right", text);
}

#[test]
fn snapshot_splash_nc_classic() {
    let state = base_state(UiPhase::Splash { started_at_ms: 0 }, Theme::classic());
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    insta::assert_snapshot!("splash_nc_classic", text);
}

#[test]
fn snapshot_splash_nc_mono() {
    let state = base_state(UiPhase::Splash { started_at_ms: 0 }, Theme::mono());
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    insta::assert_snapshot!("splash_nc_mono", text);
}

#[test]
fn snapshot_terminal_too_small_placeholder() {
    let state = base_state(UiPhase::Placeholder, Theme::classic());
    let text = render_to_text(40, 10, &state, ColorDepth::Ansi16);
    insta::assert_snapshot!("placeholder_too_small", text);
}

#[test]
fn snapshot_fkey_bar() {
    let state = base_state(UiPhase::Panels, Theme::classic());
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    let last_line = text.lines().last().unwrap_or_default();
    insta::assert_snapshot!("fkey_bar_last_row", last_line);
}

#[test]
fn snapshot_streaming_ministatus() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.left = streaming_panel(r"C:\Users\demo\left", 2);
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    insta::assert_snapshot!("streaming_ministatus_reading_n", text);
}

#[test]
fn quit_confirm_dialog_renders_over_panels() {
    let state = base_state(UiPhase::QuitConfirm, Theme::classic());
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(text.contains("Quit FileCommand?"));
    insta::assert_snapshot!("quit_confirm_dialog", text);
}

#[test]
fn active_panel_title_uses_active_role_inactive_uses_inactive_role() {
    use filecommand_tui::style::role_style;
    let theme = Theme::classic();
    let active_style = role_style(&theme, filecommand_core::theme::Role::PanelTitleActive, ColorDepth::Ansi16);
    let inactive_style = role_style(&theme, filecommand_core::theme::Role::PanelTitleInactive, ColorDepth::Ansi16);
    assert_ne!(active_style, inactive_style);
}
