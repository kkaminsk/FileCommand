//! Snapshot and behavior tests for the ui-animations capability.
//!
//! Animation progress is derived from `State::clock_ms` minus a stamp the
//! reducer recorded, so every frame here is pinned: states are built with
//! `animations = true`, stamped at a fixed `T0`, and rendered at `T0 + t`.
//! Existing snapshot suites build states through `State::empty`, which keeps
//! animations off, so they are untouched by this file.

use std::path::PathBuf;

use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;

use filecommand_core::anim;
use filecommand_core::listing::{DateTime, Entry, EntryKind};
use filecommand_core::menu::MenuState;
use filecommand_core::panel::{ListingProgress, PanelState, SortDirection};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use filecommand_core::{update, Command, State, UiPhase};

use filecommand_tui::style::role_style;
use filecommand_tui::views;

/// The clock reading every animated surface in this file is stamped with.
const T0: u64 = 10_000;
const FIXED_CLOCK_TEXT: &str = "3:04 PM";

fn identity_lines() -> [String; 4] {
    [
        "FileCommand".to_string(),
        "Version 0.1.0".to_string(),
        "Copyright (C) 2026 The FileCommand Authors".to_string(),
        "Inspired by the Norton Commander, 1986-1998".to_string(),
    ]
}

fn complete_panel(cwd: &str, cursor: usize) -> PanelState {
    let date = DateTime { year: 2026, month: 1, day: 2, hour: 3, minute: 4 };
    let mut panel = PanelState::new(PathBuf::from(cwd));
    panel.entries = vec![
        Entry::parent_dir(),
        Entry { name: "docs".into(), kind: EntryKind::Directory, size: 0, modified: Some(date), hidden: false, link: false },
        Entry { name: "Cargo.toml".into(), kind: EntryKind::File, size: 612, modified: Some(date), hidden: false, link: false },
        Entry { name: "readme.txt".into(), kind: EntryKind::File, size: 12_345, modified: Some(date), hidden: false, link: false },
    ];
    panel.progress = ListingProgress::Complete { count: panel.entries.len() };
    panel.sort_direction = SortDirection::Asc;
    panel.cursor = cursor;
    panel
}

/// A panels-phase state with animations enabled and the clock at `T0`.
fn animated_state() -> State {
    State {
        left: complete_panel(r"C:\Users\demo\left", 1),
        right: complete_panel(r"C:\Users\demo\right", 0),
        animations: true,
        clock_ms: T0,
        ..State::empty(Theme::classic())
    }
}

/// `state` with the reducer clock moved to `T0 + t`, as the next `Tick` would.
fn at(state: &State, t: u64) -> State {
    State { clock_ms: T0 + t, ..state.clone() }
}

fn render_buffer(state: &State) -> Buffer {
    render_buffer_with_hitmap(state).0
}

fn render_buffer_with_hitmap(state: &State) -> (Buffer, views::RenderOutput) {
    let lines = identity_lines();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("TestBackend terminal");
    let mut output = None;
    terminal
        .draw(|frame| {
            let area = frame.area();
            output = Some(views::render(frame.buffer_mut(), area, state, ColorDepth::Ansi16, &lines, FIXED_CLOCK_TEXT, None));
        })
        .expect("draw into TestBackend");
    (terminal.backend().buffer().clone(), output.expect("rendered"))
}

fn text(buf: &Buffer) -> String {
    let a = buf.area;
    let mut out = String::new();
    for y in 0..a.height {
        for x in 0..a.width {
            out.push_str(buf[(a.x + x, a.y + y)].symbol());
        }
        out.push('\n');
    }
    out
}

fn render_text(state: &State) -> String {
    text(&render_buffer(state))
}

/// The same state with every animation disabled: the reference "final"
/// rendering an animated state must converge to.
fn finished(state: &State) -> State {
    State { animations: false, ..state.clone() }
}

fn splash_state() -> State {
    State { phase: UiPhase::Splash { started_at_ms: T0 }, ..animated_state() }
}

fn menu_state() -> State {
    let mut menu = MenuState::opened();
    menu.opened_at_ms = T0;
    menu.pulldown_opened_at_ms = T0;
    State { menu: Some(menu), ..animated_state() }
}

/// F7 through the real reducer, so the dialog stamp is whatever production
/// would record rather than a hand-set field.
fn mkdir_dialog_state() -> State {
    let (state, _) = update(animated_state(), Command::RequestMkdir);
    assert!(matches!(state.phase, UiPhase::FileOpSetup(_)));
    assert_eq!(state.dialog_opened_at_ms, T0, "the reducer stamped the open");
    state
}

// ---------------------------------------------------------------------
// Snapshots at pinned clock offsets (tasks 6.4)
// ---------------------------------------------------------------------

#[test]
fn splash_entrance_frames() {
    let state = splash_state();
    insta::assert_snapshot!("splash_entrance_0ms", render_text(&at(&state, 0)));
    insta::assert_snapshot!("splash_entrance_100ms", render_text(&at(&state, 100)));
    insta::assert_snapshot!("splash_entrance_400ms", render_text(&at(&state, 400)));
    insta::assert_snapshot!("splash_entrance_650ms", render_text(&at(&state, 650)));
    insta::assert_snapshot!("splash_entrance_800ms", render_text(&at(&state, 800)));
}

#[test]
fn pulldown_unfolds_top_down() {
    let state = menu_state();
    insta::assert_snapshot!("pulldown_unfold_0ms", render_text(&at(&state, 0)));
    insta::assert_snapshot!("pulldown_unfold_50ms", render_text(&at(&state, 50)));
    insta::assert_snapshot!("pulldown_unfold_100ms", render_text(&at(&state, 100)));
}

#[test]
fn menu_bar_sweeps_in_left_to_right() {
    // Bar only (pull-down collapsed), so the frame isolates the sweep: the
    // left half of the top row is the bar, the right half still shows the
    // panels' top borders and the clock.
    let mut state = menu_state();
    state.menu.as_mut().unwrap().pulldown_open = false;
    insta::assert_snapshot!("menu_bar_sweep_50ms", render_text(&at(&state, 50)));
}

#[test]
fn dialog_opens_through_an_iris() {
    let state = mkdir_dialog_state();
    insta::assert_snapshot!("dialog_iris_0ms", render_text(&at(&state, 0)));
    insta::assert_snapshot!("dialog_iris_50ms", render_text(&at(&state, 50)));
    insta::assert_snapshot!("dialog_iris_100ms", render_text(&at(&state, 100)));
}

#[test]
fn panels_wipe_in_after_the_splash() {
    let state = State { panels_revealed_at_ms: Some(T0), ..animated_state() };
    insta::assert_snapshot!("panel_wipe_75ms", render_text(&at(&state, 75)));
}

// ---------------------------------------------------------------------
// Final frames are byte-identical to the non-animated rendering
// ---------------------------------------------------------------------

#[test]
fn every_surface_converges_to_the_pre_animation_rendering() {
    let surfaces: Vec<(&str, State, u64)> = vec![
        ("splash", splash_state(), anim::SPLASH_ENTRANCE_MS),
        ("menu bar + pull-down", menu_state(), anim::POPUP_UNFOLD_MS.max(anim::MENU_BAR_SWEEP_MS)),
        ("dialog", mkdir_dialog_state(), anim::DIALOG_IRIS_MS),
        ("panel wipe", State { panels_revealed_at_ms: Some(T0), ..animated_state() }, anim::PANEL_WIPE_MS),
    ];
    for (name, state, duration) in surfaces {
        let reference = render_buffer(&finished(&state));
        assert_eq!(render_buffer(&at(&state, duration)), reference, "{name}: not identical at its duration");
        assert_eq!(render_buffer(&at(&state, duration + 5_000)), reference, "{name}: not identical long after");
    }
}

#[test]
fn disabled_animations_render_the_final_frame_at_every_clock_reading() {
    for (name, state) in [("splash", splash_state()), ("menu", menu_state()), ("dialog", mkdir_dialog_state())] {
        let off = finished(&state);
        let reference = render_buffer(&at(&off, 10_000));
        for t in [0, 10, 50, 99] {
            assert_eq!(render_buffer(&at(&off, t)), reference, "{name} with animations off must not depend on the clock (t = {t})");
        }
    }
}

#[test]
fn states_built_without_config_never_animate() {
    // `State::empty` has animations off: an open dialog renders complete even
    // though its stamp equals the clock (existing snapshots rely on this).
    let base = State { left: complete_panel("/l", 0), right: complete_panel("/r", 0), ..State::empty(Theme::classic()) };
    let (state, _) = update(base, Command::RequestMkdir);
    assert!(!state.animations);
    assert_eq!(render_buffer(&state), render_buffer(&State { clock_ms: 5_000, ..state.clone() }));
}

// ---------------------------------------------------------------------
// Behavior
// ---------------------------------------------------------------------

#[test]
fn hit_maps_use_final_geometry_during_the_iris() {
    // A click during the 100 ms iris lands where the button will be: the hit
    // map is built from the finished dialog, never from the growing window.
    let mut state = mkdir_dialog_state();
    state.quit_confirm = true;
    state.quit_opened_at_ms = T0;
    let early = render_buffer_with_hitmap(&at(&state, 0)).1.hitmap;
    let done = render_buffer_with_hitmap(&at(&state, 500)).1.hitmap;
    assert!(!early.dialog_buttons.is_empty(), "the quit dialog's buttons are live from frame one");
    assert_eq!(format!("{early:?}"), format!("{done:?}"));
}

#[test]
fn highlight_flashes_in_the_hotkey_accent_then_settles() {
    let theme = Theme::classic();
    let hotkey = role_style(&theme, Role::MenuHotkey, ColorDepth::Ansi16);
    let highlight = role_style(&theme, Role::MenuHighlight, ColorDepth::Ansi16);
    assert_ne!(hotkey.fg, highlight.fg, "the settle is only visible if the roles differ");

    // The pull-down finished unfolding long ago; then a keypress moves the
    // highlight down one row through the reducer.
    let pressed = T0 + 300;
    let (state, _) = update(State { clock_ms: pressed, ..menu_state() }, Command::MenuSelectNext);
    let moved = state.menu.as_ref().unwrap();
    assert_eq!(moved.selected_at_ms, Some(pressed));
    let row = 2 + moved.selected as u16; // pull-down top frame is row 1, items start at row 2

    // The `Left` pull-down's box starts at column 1 (one left of its title),
    // so column 2 is the first interior cell of the selected row.
    let cell_fg = |since_press: u64| render_buffer(&State { clock_ms: pressed + since_press, ..state.clone() })[(2, row)].fg;
    assert_eq!(cell_fg(10), hotkey.fg.unwrap(), "within 50 ms the new row is in the hotkey accent");
    assert_eq!(cell_fg(anim::HIGHLIGHT_SETTLE_MS - 1), hotkey.fg.unwrap(), "still flashing just before 50 ms");
    assert_eq!(cell_fg(anim::HIGHLIGHT_SETTLE_MS), highlight.fg.unwrap(), "at 50 ms it settles to the normal highlight");
    assert_eq!(cell_fg(5_000), highlight.fg.unwrap(), "and stays settled");
}

#[test]
fn layered_quit_dialog_irises_without_reanimating_the_dialog_beneath() {
    // Help finished opening long ago; the quit prompt opens over it 30 ms ago.
    let (mut state, _) = update(State { clock_ms: T0 - 5_000, ..animated_state() }, Command::HelpOpen);
    assert_eq!(state.dialog_opened_at_ms, T0 - 5_000);
    state.clock_ms = T0;
    let (state, _) = update(state, Command::RequestQuit);
    assert_eq!(state.quit_opened_at_ms, T0);
    assert_eq!(state.dialog_opened_at_ms, T0 - 5_000, "the beneath stamp is untouched");

    let mid = text(&render_buffer(&at(&state, 30)));
    assert!(mid.contains("About FileCommand"), "the Help window beneath stays fully drawn:\n{mid}");

    // And once the iris is done the frame equals the non-animated one.
    assert_eq!(render_buffer(&at(&state, anim::DIALOG_IRIS_MS)), render_buffer(&finished(&state)));
}

/// Bounding box `(x0, y0, x1, y1)` of the cells in `a` that differ from `b`,
/// or `None` when the buffers are identical.
fn diff_bounds(a: &Buffer, b: &Buffer) -> Option<(u16, u16, u16, u16)> {
    let mut bounds: Option<(u16, u16, u16, u16)> = None;
    for y in 0..a.area.height {
        for x in 0..a.area.width {
            if a[(x, y)] != b[(x, y)] {
                bounds = Some(match bounds {
                    None => (x, y, x, y),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                });
            }
        }
    }
    bounds
}

#[test]
fn the_iris_grows_from_the_center_of_the_finished_dialog() {
    let state = mkdir_dialog_state();
    // What is on screen with no dialog at all: the panels the iris opens onto.
    let panels = render_buffer(&State { phase: UiPhase::Panels, ..state.clone() });
    let (fx0, fy0, fx1, fy1) = diff_bounds(&render_buffer(&finished(&state)), &panels).expect("the dialog draws something");
    let (full_w, full_h) = (fx1 - fx0 + 1, fy1 - fy0 + 1);
    let (cx, cy) = ((fx0 + fx1) as i32, (fy0 + fy1) as i32); // doubled centers, to stay in integers

    let mut last_area = 0u32;
    for t in [0u64, 25, 50, 75, 99] {
        let (x0, y0, x1, y1) = diff_bounds(&render_buffer(&at(&state, t)), &panels).unwrap_or_else(|| panic!("a sliver is open at {t} ms"));
        assert!(x0 >= fx0 && y0 >= fy0 && x1 <= fx1 && y1 <= fy1, "the iris stays inside the finished dialog at {t} ms");
        assert!(((x0 + x1) as i32 - cx).abs() <= 1 && ((y0 + y1) as i32 - cy).abs() <= 1, "the window stays centered at {t} ms");
        let area = (x1 - x0 + 1) as u32 * (y1 - y0 + 1) as u32;
        assert!(area >= last_area, "the window never shrinks ({t} ms)");
        last_area = area;
    }
    // Roughly half the dialog's width and height at the midpoint (spec: "a
    // centered window roughly half the dialog's width and height").
    let (x0, y0, x1, y1) = diff_bounds(&render_buffer(&at(&state, 50)), &panels).unwrap();
    let (mid_w, mid_h) = (x1 - x0 + 1, y1 - y0 + 1);
    assert!(mid_w.abs_diff(full_w / 2) <= 2, "mid width {mid_w} vs dialog {full_w}");
    assert!(mid_h.abs_diff(full_h / 2) <= 2, "mid height {mid_h} vs dialog {full_h}");
    assert!(last_area < full_w as u32 * full_h as u32, "still smaller than the dialog just before 100 ms");
}

/// The screen column (not byte offset — box glyphs are three bytes each) at
/// which `needle` starts in `row`.
fn column_of(row: &str, needle: &str) -> u16 {
    row[..row.find(needle).expect("needle is in the row")].chars().count() as u16
}

/// The pop-up unfold invariants, checked against any pop-up's finished
/// rectangle: at every step the visible box is no taller than the finished
/// one, always closes with the bottom frame edge directly under its last
/// row, grows monotonically, and is complete at `POPUP_UNFOLD_MS`.
fn assert_unfolds_top_down(name: &str, state: &State) {
    let panels = render_buffer(&State { menu: None, user_menu: None, file_action_menu: None, ..state.clone() });
    let (_, fy0, _, fy1) = diff_bounds(&render_buffer(&finished(state)), &panels).unwrap_or_else(|| panic!("{name}: draws nothing"));
    let full_h = fy1 - fy0 + 1;

    let mut last_h = 0;
    for t in [0u64, 20, 40, 50, 60, 80, 99] {
        let buf = render_buffer(&at(state, t));
        let (_, y0, _, y1) = diff_bounds(&buf, &panels).unwrap_or_else(|| panic!("{name}: nothing visible at {t} ms"));
        assert_eq!(y0, fy0, "{name}: the top frame row is first and never moves ({t} ms)");
        let h = y1 - y0 + 1;
        assert!(h <= full_h, "{name}: {h} rows at {t} ms exceeds the finished {full_h}");
        assert!(h >= last_h, "{name}: the box never shrinks ({t} ms)");
        assert!(h >= 2, "{name}: at least the top and bottom frame edges ({t} ms)");
        // The last visible row is the bottom frame edge (single or double line).
        let bottom: String = (0..80u16).map(|x| buf[(x, y1)].symbol().to_string()).collect();
        assert!(bottom.contains('\u{2514}') || bottom.contains('\u{255A}'), "{name}: bottom edge missing at {t} ms: `{bottom}`");
        last_h = h;
    }
    assert!(last_h < full_h, "{name}: still unfolding just before {} ms", anim::POPUP_UNFOLD_MS);
    assert_eq!(render_buffer(&at(state, anim::POPUP_UNFOLD_MS)), render_buffer(&finished(state)), "{name}: complete at the unfold duration");
}

#[test]
fn user_menu_and_action_menu_unfold_like_the_pulldowns() {
    use filecommand_core::config::UserMenuEntry;

    let mut base = animated_state();
    base.user_menu_entries = (1..=6).map(|i| UserMenuEntry { label: format!("Entry {i}"), command: format!("cmd{i}") }).collect();
    let (user_menu, _) = update(base.clone(), Command::UserMenuOpen);
    assert_eq!(user_menu.user_menu.as_ref().unwrap().opened_at_ms, T0);
    assert_unfolds_top_down("F2 user menu", &user_menu);

    // Enter on a file opens the action menu (the cursor starts on `docs`; move to a file).
    let mut base = animated_state();
    base.left.cursor = 2; // Cargo.toml
    let (action_menu, _) = update(base, Command::Enter);
    assert_eq!(action_menu.file_action_menu.as_ref().expect("Enter on a file opens the menu").opened_at_ms, T0);
    assert_unfolds_top_down("Enter action menu", &action_menu);
}

#[test]
fn user_menu_and_action_menu_highlights_settle() {
    use filecommand_core::config::UserMenuEntry;

    let theme = Theme::classic();
    let hotkey = role_style(&theme, Role::MenuHotkey, ColorDepth::Ansi16).fg.unwrap();
    let highlight = role_style(&theme, Role::MenuHighlight, ColorDepth::Ansi16).fg.unwrap();
    let pressed = T0 + 300; // well after the unfold finished

    let mut base = State { clock_ms: T0, ..animated_state() };
    base.user_menu_entries = (1..=3).map(|i| UserMenuEntry { label: format!("Entry {i}"), command: format!("c{i}") }).collect();
    let (state, _) = update(base, Command::UserMenuOpen);
    let (state, _) = update(State { clock_ms: pressed, ..state }, Command::UserMenuMove(1));
    assert_eq!(state.user_menu.as_ref().unwrap().selected_at_ms, Some(pressed));
    // Find the highlighted row by its label: "Entry 2".
    let fg_of_entry_2 = |t: u64| {
        let buf = render_buffer(&State { clock_ms: pressed + t, ..state.clone() });
        let rows = text(&buf);
        let (y, row) = rows.lines().enumerate().find(|(_, r)| r.contains("Entry 2")).expect("Entry 2 is listed");
        buf[(column_of(row, "Entry 2"), y as u16)].fg
    };
    assert_eq!(fg_of_entry_2(10), hotkey, "user menu: flashing in the hotkey accent");
    assert_eq!(fg_of_entry_2(anim::HIGHLIGHT_SETTLE_MS), highlight, "user menu: settled");

    let mut base = State { clock_ms: T0, ..animated_state() };
    base.left.cursor = 2;
    let (state, _) = update(base, Command::Enter);
    let (state, _) = update(State { clock_ms: pressed, ..state }, Command::FileActionMenuMove(1));
    let second = state.file_action_menu.as_ref().unwrap().entries[1].label();
    let fg_of_second = |t: u64| {
        let buf = render_buffer(&State { clock_ms: pressed + t, ..state.clone() });
        let rows = text(&buf);
        let (y, row) = rows.lines().enumerate().find(|(_, r)| r.contains(second) && r.contains('\u{2551}')).expect("entry is listed");
        buf[(column_of(row, second), y as u16)].fg
    };
    assert_eq!(fg_of_second(10), hotkey, "action menu: flashing in the hotkey accent");
    assert_eq!(fg_of_second(anim::HIGHLIGHT_SETTLE_MS), highlight, "action menu: settled");
}

#[test]
fn the_scratch_sentinel_never_reaches_the_screen() {
    // The reveal helpers draw into a scratch buffer pre-filled with U+0001 and
    // copy only touched cells. A view that recolored cells without writing a
    // symbol would leak that marker; this fails loudly if one ever does.
    let mut user_menu_state = animated_state();
    user_menu_state.user_menu_entries = vec![filecommand_core::config::UserMenuEntry { label: "Entry".into(), command: "c".into() }];
    let (user_menu_state, _) = update(user_menu_state, Command::UserMenuOpen);
    let mut action_menu_state = animated_state();
    action_menu_state.left.cursor = 2;
    let (action_menu_state, _) = update(action_menu_state, Command::Enter);
    let (help_state, _) = update(animated_state(), Command::HelpOpen);
    let (quit_state, _) = update(animated_state(), Command::RequestQuit);

    let surfaces = [
        ("splash", splash_state()),
        ("menu", menu_state()),
        ("mkdir dialog", mkdir_dialog_state()),
        ("user menu", user_menu_state),
        ("action menu", action_menu_state),
        ("help", help_state),
        ("quit", quit_state),
        ("panel wipe", State { panels_revealed_at_ms: Some(T0), ..animated_state() }),
    ];
    for (name, state) in surfaces {
        for t in (0..=200).step_by(10) {
            let rendered = render_text(&at(&state, t));
            assert!(!rendered.contains('\u{1}'), "{name}: sentinel leaked onto the screen at {t} ms");
        }
    }
}
