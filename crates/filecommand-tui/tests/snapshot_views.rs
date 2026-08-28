//! Snapshot tests for the renderers, using ratatui's `TestBackend` so no
//! real terminal is required. State is fully pinned (fixed theme, fixed
//! entries/dates/sizes, fixed identity lines, fixed terminal size) so
//! output is deterministic across runs and locales.

use std::io::Write;
use std::path::PathBuf;

use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;

use filecommand_core::drives::DriveSelect;
use filecommand_core::editor::EditorState;
use filecommand_core::git_info::{FileStatus, GitInfo};
use filecommand_core::info::InfoValues;
use filecommand_core::listing::{DateTime, Entry, EntryKind, SortMode};
use filecommand_core::menu::{MenuId, MenuState};
use filecommand_core::panel::{ClipboardFeedback, DisplayMode, ListingProgress, PanelState, SortDirection, TreeState};
use filecommand_core::theme::{ColorDepth, Theme};
use filecommand_core::viewer::{ByteSource, ViewMode, ViewerState};
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

/// Pinned so the clock widget's top-right-corner glyphs don't make every
/// full-screen snapshot in this file non-deterministic.
const FIXED_CLOCK_TEXT: &str = "3:04 PM";

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
        phase,
        ..State::empty(theme)
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
            views::render(frame.buffer_mut(), area, state, depth, &identity_lines, FIXED_CLOCK_TEXT, None);
        })
        .expect("draw into TestBackend");
    buffer_to_text(terminal.backend().buffer())
}

/// Same as [`render_to_text`], but also threads through the F3 viewer's
/// open byte window — needed only while `state.phase` is `UiPhase::Viewer`.
fn render_viewer_to_text(width: u16, height: u16, state: &State, source: Option<&ByteSource>) -> String {
    let identity_lines = pinned_identity_lines();
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("TestBackend terminal");
    terminal
        .draw(|frame| {
            let area = frame.area();
            views::render(frame.buffer_mut(), area, state, ColorDepth::Ansi16, &identity_lines, FIXED_CLOCK_TEXT, source);
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
    // Post-quit-keys: the dialog is an overlay beside the phase
    // (`State::quit_confirm: bool`), not a `UiPhase::QuitConfirm` variant —
    // it is drawn on top of whatever `state.phase` painted underneath it
    // (application-shell "Quit request keys and confirmation"; design D5).
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.quit_confirm = true;
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(text.contains("Quit FileCommand?"));
    insta::assert_snapshot!("quit_confirm_dialog", text);
}

#[test]
fn quit_confirm_dialog_renders_above_the_viewer() {
    // task 4.5: the dialog can be raised while the F3 viewer is open — it
    // lives beside the phase, not inside it, so it must paint over whatever
    // the viewer drew underneath (application-shell "Quit request keys and
    // confirmation"; design D5).
    let source = temp_viewer_file("quit-over-viewer", b"The quick brown fox\njumps over the lazy dog.\n");
    let mut state = viewer_state("sample.txt", &source, ViewMode::Text);
    state.quit_confirm = true;
    let text = render_viewer_to_text(80, 24, &state, Some(&source));
    assert!(text.contains("Quit FileCommand?"), "the dialog must be drawn over the viewer");
    insta::assert_snapshot!("quit_confirm_dialog_over_viewer", text);
}

#[test]
fn quit_confirm_dialog_renders_above_an_open_pull_down_menu() {
    // task 4.5: the dialog can be raised while an F9 pull-down menu is open
    // — same topmost-overlay contract as above the viewer.
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.menu = Some(MenuState::for_menu(MenuId::Files));
    state.quit_confirm = true;
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(text.contains("Quit FileCommand?"), "the dialog must be drawn over the open pull-down");
    insta::assert_snapshot!("quit_confirm_dialog_over_pulldown_menu", text);
}

#[test]
fn active_panel_title_uses_active_role_inactive_uses_inactive_role() {
    use filecommand_tui::style::role_style;
    let theme = Theme::classic();
    let active_style = role_style(&theme, filecommand_core::theme::Role::PanelTitleActive, ColorDepth::Ansi16);
    let inactive_style = role_style(&theme, filecommand_core::theme::Role::PanelTitleInactive, ColorDepth::Ansi16);
    assert_ne!(active_style, inactive_style);
}

// ---------------------------------------------------------------------
// M3
// ---------------------------------------------------------------------

/// The command-line row is the second from the bottom (above the F-key bar).
fn command_line_row(text: &str) -> &str {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len() - 2]
}

#[test]
fn snapshot_command_line_with_prompt_and_typed_text() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.command_line = "dir *.txt".to_string();
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    let row = command_line_row(&text);
    assert!(row.contains(r"C:\Users\demo\left>dir *.txt"), "prompt + buffer in `{row}`");
    insta::assert_snapshot!("command_line_with_prompt", row);
}

#[test]
fn command_line_prompt_follows_the_active_panel() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.active = PanelSide::Right;
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(command_line_row(&text).contains(r"C:\Users\demo\right>"));
}

#[test]
fn snapshot_command_line_recalling_history() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.history = vec!["cd docs".to_string(), "type readme.txt".to_string()];
    state.command_line = "type readme.txt".to_string();
    state.history_cursor = Some(1);
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    insta::assert_snapshot!("command_line_history_recall", command_line_row(&text));
}

#[test]
fn the_clock_is_drawn_over_the_right_end_of_the_right_panel_s_top_border() {
    let state = base_state(UiPhase::Panels, Theme::classic());
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    let top = text.lines().next().unwrap();
    assert!(top.ends_with(FIXED_CLOCK_TEXT), "`{top}`");
}

#[test]
fn the_f9_bar_hides_the_clock_and_closing_it_restores_the_clock() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    let without_menu = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(without_menu.lines().next().unwrap().contains(FIXED_CLOCK_TEXT), "clock shows with the bar closed");

    state.menu = Some(MenuState::opened());
    let with_menu = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(!with_menu.lines().next().unwrap().contains(FIXED_CLOCK_TEXT), "clock is hidden while F9 is open");

    // Esc-ing the bar closed (state.menu back to None) restores it — same
    // as any other frame where the bar isn't open.
    state.menu = None;
    let restored = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(restored.lines().next().unwrap().contains(FIXED_CLOCK_TEXT), "clock is restored once the bar closes");
}

#[test]
fn the_clock_hides_entirely_rather_than_colliding_with_a_long_path_title() {
    // A long right-panel path pushes the centered title's span far enough
    // right that it would overlap the clock's fixed-width slot in the
    // corner (responsive-layout "Chrome degradation": "the clock is not
    // drawn at all and the path title renders normally").
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    let long_cwd = format!(r"C:\{}", "a".repeat(31));
    state.right = complete_panel(&long_cwd, 0);
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    let top = text.lines().next().unwrap();
    assert!(!top.contains(FIXED_CLOCK_TEXT), "clock must not partially or fully draw over the title: `{top}`");
    assert!(top.contains(&long_cwd[..10]), "the path title still renders normally: `{top}`");
}

#[test]
fn snapshot_menu_bar_with_left_pulldown_open() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.menu = Some(MenuState::opened());
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(text.lines().next().unwrap().contains("Left"));
    insta::assert_snapshot!("menu_bar_left_pulldown", text);
}

#[test]
fn snapshot_menu_bar_with_files_pulldown_open() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.menu = Some(MenuState::for_menu(MenuId::Files));
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    insta::assert_snapshot!("menu_bar_files_pulldown", text);
}

#[test]
fn snapshot_menu_bar_with_pulldown_closed() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    let mut menu = MenuState::opened();
    menu.pulldown_open = false;
    state.menu = Some(menu);
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    insta::assert_snapshot!("menu_bar_no_pulldown", text.lines().next().unwrap());
}

#[test]
fn the_menu_bar_replaces_the_panels_top_border_row() {
    let plain = render_to_text(80, 24, &base_state(UiPhase::Panels, Theme::classic()), ColorDepth::Ansi16);
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.menu = Some(MenuState::opened());
    let with_menu = render_to_text(80, 24, &state, ColorDepth::Ansi16);

    assert!(plain.lines().next().unwrap().contains('\u{2554}'), "normally the top row is panel border");
    let top = with_menu.lines().next().unwrap();
    assert!(!top.contains('\u{2554}'), "the bar takes the whole top row: `{top}`");
    for title in ["Left", "Files", "Commands", "Options", "Right"] {
        assert!(top.contains(title));
    }
}

#[test]
fn snapshot_drive_select_dialog_labels_pending() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.drive_select = Some(DriveSelect::new(PanelSide::Left, vec!['A', 'C', 'D', 'Z'], Some('C')));
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    insta::assert_snapshot!("drive_select_labels_pending", text);
}

#[test]
fn snapshot_drive_select_dialog_labels_resolved() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    let mut dialog = DriveSelect::new(PanelSide::Left, vec!['A', 'C', 'D', 'Z'], Some('C'));
    dialog.apply_label('C', Some("OS".to_string()));
    dialog.apply_label('D', Some("DATA".to_string()));
    dialog.apply_label('Z', Some("net".to_string()));
    // A: has no media, so its fetch never resolves and its column stays
    // blank — which must not hold up the rest of the dialog.
    state.drive_select = Some(dialog);
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    insta::assert_snapshot!("drive_select_labels_resolved", text);
}

fn info_panel_state(values: InfoValues) -> State {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    let mut panel = complete_panel(r"C:\Users\demo\left", 1);
    panel.display_mode = DisplayMode::Info;
    panel.info = values;
    state.left = panel;
    state
}

#[test]
fn snapshot_info_panel_with_pending_values() {
    let text = render_to_text(80, 24, &info_panel_state(InfoValues::default()), ColorDepth::Ansi16);
    assert!(text.contains('\u{2026}'), "unresolved values render as `…`");
    insta::assert_snapshot!("info_panel_pending", text);
}

#[test]
fn snapshot_info_panel_with_resolved_values() {
    let values = InfoValues {
        memory_bytes: Some(8_589_934_592),
        drive_total: Some(511_000_000_000),
        drive_free: Some(123_456_789),
        volume_label: Some("OS".to_string()),
        serial: Some("1A2B-3C4D".to_string()),
        file_count: Some(42),
        dir_count: Some(7),
    };
    let text = render_to_text(80, 24, &info_panel_state(values), ColorDepth::Ansi16);
    assert!(!text.contains('\u{2026}'), "no placeholder survives once every value resolved");
    insta::assert_snapshot!("info_panel_resolved", text);
}

#[test]
fn info_mode_leaves_the_opposite_panel_listing_normally() {
    let text = render_to_text(80, 24, &info_panel_state(InfoValues::default()), ColorDepth::Ansi16);
    assert!(text.contains("Cargo.toml"), "the right panel still lists its entries");
    assert!(text.contains("Volume label"), "the left panel is in Info mode");
}

#[test]
fn snapshot_header_sort_arrow_per_mode() {
    let mut rendered = String::new();
    for mode in [SortMode::Name, SortMode::Extension, SortMode::Size, SortMode::Time, SortMode::Unsorted] {
        let mut state = base_state(UiPhase::Panels, Theme::classic());
        state.left.sort_mode = mode;
        let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
        // The header is the second row; take the left panel's half.
        let header: String = text.lines().nth(1).unwrap().chars().take(40).collect();
        rendered.push_str(&format!("{mode:?}\n{header}\n"));
    }
    insta::assert_snapshot!("header_sort_arrows", rendered);
}

#[test]
fn the_sort_arrow_marks_only_the_active_sort_column() {
    let header_for = |mode: SortMode| {
        let mut state = base_state(UiPhase::Panels, Theme::classic());
        state.left.sort_mode = mode;
        let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
        text.lines().nth(1).unwrap().chars().take(40).collect::<String>()
    };

    let name = header_for(SortMode::Name);
    assert!(name.contains("Name\u{2193}"), "`{name}`");
    assert_eq!(name.matches('\u{2193}').count(), 1, "exactly one column carries the arrow: `{name}`");

    let size = header_for(SortMode::Size);
    assert!(size.contains("Size\u{2193}"), "`{size}`");
    assert!(!size.contains("Name\u{2193}"), "the arrow left the Name column: `{size}`");

    let time = header_for(SortMode::Time);
    assert!(time.contains("Date\u{2193}"), "`{time}`");

    let unsorted = header_for(SortMode::Unsorted);
    assert!(!unsorted.contains('\u{2193}') && !unsorted.contains('\u{2191}'), "Unsorted shows no arrow: `{unsorted}`");
}

#[test]
fn a_descending_sort_flips_the_arrow() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.left.sort_mode = SortMode::Name;
    state.left.sort_direction = SortDirection::Desc;
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    let header: String = text.lines().nth(1).unwrap().chars().take(40).collect();
    assert!(header.contains("Name\u{2191}"), "`{header}`");
}

// ---------------------------------------------------------------------
// M4: F3 viewer
// ---------------------------------------------------------------------

fn temp_viewer_file(name: &str, contents: &[u8]) -> ByteSource {
    let dir = std::env::temp_dir().join(format!("filecommand-tui-viewer-snapshot-{}-{}", std::process::id(), name));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("file.bin");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(contents).unwrap();
    f.flush().unwrap();
    ByteSource::open(&path).unwrap()
}

fn viewer_state(filename: &str, source: &ByteSource, mode: ViewMode) -> State {
    let mut viewer = ViewerState::new(PathBuf::from(filename), source.len());
    viewer.mode = mode;
    State { phase: UiPhase::Viewer(viewer), ..State::empty(Theme::classic()) }
}

#[test]
fn snapshot_viewer_text_mode() {
    let source = temp_viewer_file("text-mode", b"The quick brown fox\njumps over the lazy dog.\n");
    let state = viewer_state("sample.txt", &source, ViewMode::Text);
    let text = render_viewer_to_text(80, 24, &state, Some(&source));
    insta::assert_snapshot!("viewer_text_mode", text);
}

#[test]
fn snapshot_viewer_hex_mode() {
    let source = temp_viewer_file("hex-mode", b"Hello, FileCommand! \x00\xff\x1b");
    let state = viewer_state("sample.bin", &source, ViewMode::Hex);
    let text = render_viewer_to_text(80, 24, &state, Some(&source));
    insta::assert_snapshot!("viewer_hex_mode", text);
}

#[test]
fn viewer_keybar_label_swaps_hex_and_ascii_by_mode() {
    let source = temp_viewer_file("keybar-swap", b"data");

    let text_state = viewer_state("sample.txt", &source, ViewMode::Text);
    let text_row = render_viewer_to_text(80, 24, &text_state, Some(&source));
    let last = text_row.lines().last().unwrap();
    assert!(last.trim_end().starts_with("1Help 2Unwrap 4Hex 7Search 10Quit"), "`{last}`");
    insta::assert_snapshot!("viewer_keybar_hex_label", last);

    let hex_state = viewer_state("sample.bin", &source, ViewMode::Hex);
    let hex_row = render_viewer_to_text(80, 24, &hex_state, Some(&source));
    let last = hex_row.lines().last().unwrap();
    assert!(last.trim_end().starts_with("1Help 2Unwrap 4ASCII 7Search 10Quit"), "`{last}`");
    insta::assert_snapshot!("viewer_keybar_ascii_label", last);
}

#[test]
fn snapshot_viewer_search_match_highlight() {
    let source = temp_viewer_file("match-highlight", b"the quick brown fox jumps over the lazy dog\n");
    let mut viewer = ViewerState::new(PathBuf::from("sample.txt"), source.len());
    viewer.last_match = Some((4, 9)); // "quick"
    let state = State { phase: UiPhase::Viewer(viewer), ..State::empty(Theme::classic()) };
    let text = render_viewer_to_text(80, 24, &state, Some(&source));
    insta::assert_snapshot!("viewer_search_match_highlight", text);
}

#[test]
fn viewer_replaces_the_panels_full_screen() {
    let source = temp_viewer_file("full-screen", b"content\n");
    let state = viewer_state("sample.txt", &source, ViewMode::Text);
    let text = render_viewer_to_text(80, 24, &state, Some(&source));
    assert!(!text.contains('\u{2554}'), "no panel border while the viewer is open:\n{text}");
    assert!(text.contains("sample.txt"), "the header shows the open file:\n{text}");
}

// ---------------------------------------------------------------------
// M5: F4 built-in editor
// ---------------------------------------------------------------------

fn editor_state(editor: EditorState) -> State {
    State { phase: UiPhase::Editor(editor), ..State::empty(Theme::classic()) }
}

#[test]
fn snapshot_editor_chrome_unmodified() {
    let editor = EditorState::from_bytes(PathBuf::from(r"C:\notes.txt"), b"The quick brown fox\njumps over the lazy dog.\n");
    let state = editor_state(editor);
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    insta::assert_snapshot!("editor_chrome_unmodified", text);
}

#[test]
fn snapshot_editor_header_position_and_modified_indicator() {
    // 440 lines ("line one".."line 440") plus a trailing empty line from the
    // final newline (matching how `logical_lines`/`from_bytes` model every
    // other file in this codebase), caret on line 12 column 8 (both
    // 1-based, matching the spec's header example), buffer modified.
    let content: String = (1..=440).map(|i| format!("line {i}\n")).collect();
    let mut editor = EditorState::from_bytes(PathBuf::from(r"C:\big.txt"), content.as_bytes());
    editor.caret.line = 11;
    editor.caret.col = 7;
    editor.type_char('!');
    editor.caret.line = 11;
    editor.caret.col = 7;
    let state = editor_state(editor);
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    let header = text.lines().next().unwrap();
    assert!(header.contains("Edit: C:\\big.txt *"), "`{header}`");
    assert!(header.contains("Line 12/441   Col 8"), "`{header}`");
    insta::assert_snapshot!("editor_header_position_and_modified", header);
}

#[test]
fn snapshot_editor_overwrite_indicator() {
    let mut editor = EditorState::from_bytes(PathBuf::from("f.txt"), b"abc\n");
    editor.toggle_mode();
    let state = editor_state(editor);
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    let header = text.lines().next().unwrap();
    assert!(header.trim_end().ends_with("Ovr"), "`{header}`");
    insta::assert_snapshot!("editor_overwrite_indicator", header);
}

#[test]
fn snapshot_editor_keybar() {
    let editor = EditorState::from_bytes(PathBuf::from("f.txt"), b"abc\n");
    let state = editor_state(editor);
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    let last = text.lines().last().unwrap();
    assert!(last.trim_end().starts_with("1Help 2Save 3Mark 4Replac 5 6 7Search 8 9 10Quit"), "`{last}`");
    insta::assert_snapshot!("editor_keybar", last);
}

#[test]
fn snapshot_editor_marked_selection() {
    let mut editor = EditorState::from_bytes(PathBuf::from("f.txt"), b"alpha\nbeta\ngamma\ndelta\n");
    editor.start_mark();
    editor.move_down();
    let state = editor_state(editor);
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    insta::assert_snapshot!("editor_marked_selection", text);
}

#[test]
fn snapshot_editor_save_on_exit_confirm() {
    let mut editor = EditorState::from_bytes(PathBuf::from(r"C:\notes.txt"), b"abc\n");
    editor.type_char('!');
    editor.quit_confirm = true;
    let state = editor_state(editor);
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(text.contains("Save changes to notes.txt?"), "{text}");
    insta::assert_snapshot!("editor_save_on_exit_confirm", text);
}

#[test]
fn editor_replaces_the_panels_full_screen() {
    let editor = EditorState::from_bytes(PathBuf::from("f.txt"), b"content\n");
    let state = editor_state(editor);
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(!text.contains('\u{2554}'), "no panel border while the editor is open:\n{text}");
    assert!(text.contains("f.txt"), "the header shows the open file:\n{text}");
}

// ---------------------------------------------------------------------
// M5: panel tabs — tab strip
// ---------------------------------------------------------------------

fn panel_with_tabs(cwds: &[&str]) -> PanelState {
    let mut panel = complete_panel(cwds[0], 0);
    for cwd in &cwds[1..] {
        panel.open_tab();
        panel.begin_new_listing(PathBuf::from(cwd));
        panel.entries = fixed_entries();
        panel.progress = ListingProgress::Complete { count: panel.entries.len() };
    }
    panel.switch_tab(1);
    panel
}

#[test]
fn snapshot_tab_strip_hidden_with_a_single_tab() {
    let state = base_state(UiPhase::Panels, Theme::classic());
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    // The left panel's top border stays at row 0 — no strip row inserted.
    assert!(text.lines().next().unwrap().starts_with('\u{2554}'), "{}", text.lines().next().unwrap());
}

#[test]
fn snapshot_tab_strip_full_labels() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.left = panel_with_tabs(&[r"C:\alpha", r"C:\beta"]);
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    let strip_row = text.lines().nth(1).unwrap();
    assert!(strip_row.contains("1:ALPHA"), "`{strip_row}`");
    assert!(strip_row.contains("2:BETA"), "`{strip_row}`");
    insta::assert_snapshot!("tab_strip_full_labels", strip_row);
}

#[test]
fn snapshot_tab_strip_body_shrinks_by_one_row() {
    let one_tab = base_state(UiPhase::Panels, Theme::classic());
    let one_tab_text = render_to_text(80, 24, &one_tab, ColorDepth::Ansi16);
    let mut two_tabs = base_state(UiPhase::Panels, Theme::classic());
    two_tabs.left = panel_with_tabs(&[r"C:\alpha", r"C:\beta"]);
    let two_tabs_text = render_to_text(80, 24, &two_tabs, ColorDepth::Ansi16);
    // Both variants fill the same 24 rows; the strip trades one entry row
    // for itself rather than growing the panel (panel-tabs "Strip appears
    // and reclaims a row with two tabs").
    assert_eq!(one_tab_text.lines().count(), two_tabs_text.lines().count());
}

/// The first `width` *characters* (not bytes — a tab-strip row can contain
/// multi-byte box-drawing glyphs) of a rendered row, isolating the left
/// panel's half of a two-panel row.
fn left_half(row: &str, width: usize) -> String {
    row.chars().take(width).collect()
}

#[test]
fn snapshot_tab_strip_truncated_labels() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.left = panel_with_tabs(&[r"C:\filecommand", r"C:\b"]);
    let l = filecommand_tui::layout::compute((26, 24), 50);
    let text = render_to_text(26, 24, &state, ColorDepth::Ansi16);
    let strip_row = left_half(text.lines().nth(1).unwrap(), l.left.width as usize);
    assert!(strip_row.contains('\u{2026}'), "`{strip_row}`");
    insta::assert_snapshot!("tab_strip_truncated_labels", strip_row);
}

#[test]
fn snapshot_tab_strip_number_only_labels() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.left = panel_with_tabs(&[r"C:\filecommand", r"C:\other"]);
    let l = filecommand_tui::layout::compute((14, 24), 50);
    let text = render_to_text(14, 24, &state, ColorDepth::Ansi16);
    let strip_row = left_half(text.lines().nth(1).unwrap(), l.left.width as usize);
    assert!(!strip_row.contains(':'), "`{strip_row}`");
    insta::assert_snapshot!("tab_strip_number_only_labels", strip_row);
}

#[test]
fn snapshot_tab_strip_scrolled_with_overflow_markers() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    let cwds: Vec<String> = (1..=12).map(|i| format!(r"C:\dir{i}")).collect();
    let cwd_refs: Vec<&str> = cwds.iter().map(String::as_str).collect();
    state.left = panel_with_tabs(&cwd_refs);
    state.left.switch_tab(6); // land somewhere in the middle
    let l = filecommand_tui::layout::compute((30, 24), 50);
    let text = render_to_text(30, 24, &state, ColorDepth::Ansi16);
    let strip_row = left_half(text.lines().nth(1).unwrap(), l.left.width as usize);
    assert!(strip_row.contains('\u{25C4}') || strip_row.contains('\u{25BA}'), "`{strip_row}`");
    insta::assert_snapshot!("tab_strip_scrolled_with_markers", strip_row);
}

// ---------------------------------------------------------------------
// M5: Brief, Tree, Quick View panel modes; git branch suffix + marker
// column (additional-panel-modes; git-info)
// ---------------------------------------------------------------------

#[test]
fn snapshot_brief_mode_three_name_columns() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    let mut panel = complete_panel(r"C:\Users\demo\left", 0);
    panel.display_mode = DisplayMode::Brief;
    panel.entries = std::iter::once(Entry::parent_dir())
        .chain((0..8).map(|i| Entry { name: format!("f{i}.txt").into(), kind: EntryKind::File, size: 0, modified: None }))
        .collect();
    state.left = panel;
    // A short panel (6 body rows, after the command line and F-key bar
    // reserve their own rows) so the 9 entries visibly wrap into a second
    // column rather than all fitting in one (additional-panel-modes "Brief
    // mode renders three name-only columns").
    let l = filecommand_tui::layout::compute((80, 10), 50);
    let text = render_to_text(80, 10, &state, ColorDepth::Ansi16);
    let first_body_row = left_half(text.lines().nth(1).unwrap(), l.left.width as usize);
    assert!(!first_body_row.contains("Size"), "Brief mode has no Size/Date/Time header: `{first_body_row}`");
    assert!(first_body_row.contains("\u{25B6}UP--DIR\u{25C4}"), "`{first_body_row}`");
    assert!(first_body_row.contains("f5.txt"), "the 7th entry wraps into the second column, on the same row: `{first_body_row}`");
    insta::assert_snapshot!("brief_mode_three_columns", text);
}

fn tree_panel(prior_mode: DisplayMode, cursor: usize) -> PanelState {
    let mut panel = complete_panel(r"C:\Users\demo\left", 0);
    let mut tree = TreeState::new(PathBuf::from(r"C:\"), prior_mode);
    tree.insert_children(
        &PathBuf::from(r"C:\"),
        vec![
            Entry { name: "Alpha".into(), kind: EntryKind::Directory, size: 0, modified: None },
            Entry { name: "Beta".into(), kind: EntryKind::Directory, size: 0, modified: None },
        ],
    );
    tree.cursor = cursor;
    panel.display_mode = DisplayMode::Tree;
    panel.tree = Some(tree);
    panel
}

#[test]
fn snapshot_tree_mode_header_root_and_branch_glyphs() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.left = tree_panel(DisplayMode::Full, 2); // highlight "Beta"
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    let l = filecommand_tui::layout::compute((80, 24), 50);

    let header_row = left_half(text.lines().nth(1).unwrap(), l.left.width as usize);
    assert!(header_row.contains("Tree"), "`{header_row}`");

    let root_row = left_half(text.lines().nth(2).unwrap(), l.left.width as usize);
    assert!(root_row.contains(r"C:\"), "`{root_row}`");

    assert!(text.contains("ALPHA"), "{text}");
    assert!(text.contains("BETA"), "{text}");
    assert!(text.contains('\u{251C}') || text.contains('\u{2514}'), "branch glyphs present:\n{text}");

    let bottom_row = text.lines().nth(l.left.height as usize - 1).unwrap();
    assert!(bottom_row.contains(r"C:\Beta"), "mini-status shows the highlighted directory's full path: `{bottom_row}`");
    insta::assert_snapshot!("tree_mode_branches", text);
}

fn temp_quick_view_file(name: &str, contents: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("filecommand-tui-quickview-snapshot-{}-{}", std::process::id(), name));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("preview.txt");
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn quick_view_previews_the_opposite_cursor_file() {
    // Not an `insta` snapshot: the real on-disk temp directory the preview
    // file lives in (needed so `ByteSource::open` resolves) bakes the
    // machine-specific temp path into the right panel's title, which would
    // make a full-screen snapshot non-deterministic across machines/users.
    // The behavior itself — wrap-on text-head preview, mini-status name +
    // size — is still fully exercised via direct assertions.
    let file_path = temp_quick_view_file("text", b"Hello from quick view\nsecond line\n");

    let mut state = base_state(UiPhase::Panels, Theme::classic());
    let mut left = complete_panel(r"C:\Users\demo\left", 0);
    left.display_mode = DisplayMode::QuickView;
    state.left = left;

    let mut right = PanelState::new(file_path.parent().unwrap().to_path_buf());
    right.entries = vec![Entry { name: "preview.txt".into(), kind: EntryKind::File, size: 34, modified: None }];
    right.progress = ListingProgress::Complete { count: 1 };
    right.cursor = 0;
    state.right = right;

    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    let top = text.lines().next().unwrap();
    assert!(top.contains("Quick view"), "`{top}`");
    assert!(text.contains("Hello from quick view"), "the previewed file's head is rendered like the viewer's text mode:\n{text}");
    assert!(text.contains("second line"), "{text}");
    let bottom_row = text.lines().nth(21).unwrap();
    assert!(bottom_row.contains("preview.txt") && bottom_row.contains('3') && bottom_row.contains('4'), "mini-status shows the previewed file's name and size: `{bottom_row}`");
}

#[test]
fn snapshot_quick_view_shows_sub_dir_indicator_for_a_directory_cursor() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    let mut left = complete_panel(r"C:\Users\demo\left", 0);
    left.display_mode = DisplayMode::QuickView;
    state.left = left;

    let mut right = PanelState::new(PathBuf::from(r"C:\Users\demo\right"));
    right.entries = vec![Entry { name: "docs".into(), kind: EntryKind::Directory, size: 0, modified: None }];
    right.progress = ListingProgress::Complete { count: 1 };
    right.cursor = 0;
    state.right = right;

    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(text.contains("\u{25B6}SUB-DIR\u{25C4}"), "{text}");
    insta::assert_snapshot!("quick_view_sub_dir_indicator", text);
}

#[test]
fn snapshot_git_branch_suffix_and_marker_column() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    let mut left = complete_panel(r"C:\Users\demo\left", 1);
    left.git_info = GitInfo {
        branch: Some("main".to_string()),
        statuses: std::collections::HashMap::from([
            (std::ffi::OsString::from("Cargo.toml"), FileStatus::Modified),
            (std::ffi::OsString::from("readme.txt"), FileStatus::Untracked),
        ]),
    };
    state.left = left;
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);

    let top = text.lines().next().unwrap();
    assert!(top.contains("(main)"), "branch suffix on the active panel's top border: `{top}`");

    // Rows: 0 = top border, 1 = header, 2 = "..", 3 = "docs", 4 =
    // "Cargo.toml", 5 = "readme.txt" — the marker cell is the column
    // immediately after the left frame vertical.
    let marker_col: Vec<char> = text.lines().skip(2).take(4).map(|line| line.chars().nth(1).unwrap()).collect();
    assert_eq!(marker_col, vec![' ', ' ', 'M', '?'], "blank/blank/modified/untracked markers: {marker_col:?}");
    insta::assert_snapshot!("git_branch_suffix_and_marker_column", text);
}

#[test]
fn snapshot_git_info_absent_reserves_no_marker_column() {
    // Outside a repository (the default `GitInfo::none()`), the layout must
    // be pixel-identical to the pre-git-info M1-M4 rendering — no reserved
    // blank column (git-info "Single-reflow appearance with nothing
    // reserved while pending").
    let state = base_state(UiPhase::Panels, Theme::classic());
    let with_no_git = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    let plain = render_to_text(80, 24, &base_state(UiPhase::Panels, Theme::classic()), ColorDepth::Ansi16);
    assert_eq!(with_no_git, plain);
    assert!(!with_no_git.lines().next().unwrap().contains('('), "no branch suffix outside a repo");
}

// ---------------------------------------------------------------------
// M5: quick-filter/type-ahead mini-status, and the fuzzy-jump, find-file,
// user-menu, and Help/About dialogs (§8; quick-filter, type-ahead-jump,
// fuzzy-jump, find-file, user-menu, help-and-about)
// ---------------------------------------------------------------------

/// The active panel's mini-status row — its own bottom border, which for
/// the standard 80x24 layout is screen row 21 (0-indexed): panels occupy
/// rows 0..22 of the 24-row screen (`layout::compute` reserves the last 2
/// for the command line + F-key bar).
fn mini_status_row(text: &str) -> &str {
    text.lines().nth(21).unwrap()
}

#[test]
fn snapshot_quick_filter_mini_status_replaces_normal_content() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.left.quick_filter = Some("rep".to_string());
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    let row = mini_status_row(&text);
    assert!(row.contains("rep"), "`{row}`");
    insta::assert_snapshot!("quick_filter_mini_status", row);
}

#[test]
fn quick_filter_narrows_the_full_mode_body_to_matching_entries() {
    // `fixed_entries()` is `..`, `docs`, `Cargo.toml`, `readme.txt`; "read"
    // matches only `readme.txt`, so the narrowed body must show `..` and
    // `readme.txt` and hide `docs`/`Cargo.toml`, contradicting the pre-fix
    // behavior where every entry stayed visible and only the cursor moved
    // (quick-filter "the panel body is narrowed to entries whose displayed
    // name contains the pattern").
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.left.quick_filter = Some("read".to_string());
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    let left_half: String = text.lines().map(|l| l.chars().take(40).collect::<String>()).collect::<Vec<_>>().join("\n");
    assert!(left_half.contains(".."), "`..` must always stay visible: {left_half}");
    assert!(left_half.contains("readme.txt"), "the matching entry must stay visible: {left_half}");
    assert!(!left_half.contains("docs"), "a non-matching entry must be hidden: {left_half}");
    assert!(!left_half.contains("Cargo.toml"), "a non-matching entry must be hidden: {left_half}");
}

#[test]
fn quick_filter_leaves_the_opposite_panels_body_unfiltered() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.left.quick_filter = Some("read".to_string());
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    let right_half: String = text.lines().map(|l| l.chars().skip(40).collect::<String>()).collect::<Vec<_>>().join("\n");
    assert!(right_half.contains("docs"), "the opposite panel's body must stay unfiltered: {right_half}");
    assert!(right_half.contains("Cargo.toml"), "the opposite panel's body must stay unfiltered: {right_half}");
}

#[test]
fn quick_filter_narrows_the_brief_mode_body_to_matching_entries() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.left.display_mode = filecommand_core::panel::DisplayMode::Brief;
    state.left.quick_filter = Some("read".to_string());
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    let left_half: String = text.lines().map(|l| l.chars().take(40).collect::<String>()).collect::<Vec<_>>().join("\n");
    assert!(left_half.contains("readme.txt"), "the matching entry must stay visible: {left_half}");
    assert!(!left_half.contains("docs"), "a non-matching entry must be hidden in Brief mode too: {left_half}");
    assert!(!left_half.contains("Cargo.toml"), "a non-matching entry must be hidden in Brief mode too: {left_half}");
}

#[test]
fn snapshot_type_ahead_mini_status_replaces_normal_content() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.quick_search = Some("re".to_string());
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    let row = mini_status_row(&text);
    assert!(row.contains("re"), "`{row}`");
    insta::assert_snapshot!("type_ahead_mini_status", row);
}

#[test]
fn type_ahead_mini_status_only_affects_the_active_panel() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.active = PanelSide::Left;
    state.quick_search = Some("zz".to_string());
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    // Row 21's left half is the active (left) panel's mini-status; its
    // right half belongs to the inactive right panel, which must show its
    // ordinary status instead of the pattern.
    let row = mini_status_row(&text);
    let chars: Vec<char> = row.chars().collect();
    let mid = chars.len() / 2;
    let left_half: String = chars[..mid].iter().collect();
    let right_half: String = chars[mid..].iter().collect();
    assert!(left_half.contains("zz"));
    assert!(!right_half.contains("zz"));
}

// ---------------------------------------------------------------------
// clipboard-file-export: mini-status clipboard feedback (clipboard-export
// "Clipboard feedback")
// ---------------------------------------------------------------------

#[test]
fn snapshot_clipboard_feedback_mini_status_success() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.left.clipboard_feedback =
        Some(ClipboardFeedback { message: "3 files copied to clipboard".to_string(), is_error: false, expires_at_ms: 3_000 });
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    let row = mini_status_row(&text);
    assert!(row.contains("3 files copied to clipboard"), "`{row}`");
    insta::assert_snapshot!("clipboard_feedback_mini_status_success", row);
}

#[test]
fn snapshot_clipboard_feedback_mini_status_error() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.left.clipboard_feedback =
        Some(ClipboardFeedback { message: "Clipboard busy — try again".to_string(), is_error: true, expires_at_ms: 3_000 });
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    let row = mini_status_row(&text);
    assert!(row.contains("Clipboard busy"), "`{row}`");
    insta::assert_snapshot!("clipboard_feedback_mini_status_error", row);
}

#[test]
fn clipboard_feedback_takes_precedence_over_the_quick_filter_mini_status() {
    // Ctrl+C is matched over the panels ahead of the quick-filter's
    // plain-char capture (`input::map_panel_key`), so both can be live in
    // the same frame; the feedback must win (clipboard-export "Clipboard
    // feedback").
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.left.quick_filter = Some("rep".to_string());
    state.left.clipboard_feedback =
        Some(ClipboardFeedback { message: "1 file copied to clipboard".to_string(), is_error: false, expires_at_ms: 3_000 });
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    let row = mini_status_row(&text);
    assert!(row.contains("1 file copied to clipboard"), "`{row}`");
    assert!(!row.contains("rep"), "`{row}`");
}

#[test]
fn clipboard_feedback_error_uses_a_different_style_than_success() {
    use filecommand_tui::style::role_style;
    let theme = Theme::classic();
    let success_style = role_style(&theme, filecommand_core::theme::Role::PanelMinistatus, ColorDepth::Ansi16);
    let error_style = role_style(&theme, filecommand_core::theme::Role::DialogError, ColorDepth::Ansi16);
    assert_ne!(success_style, error_style);
}

#[test]
fn snapshot_fuzzy_jump_dialog() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.dir_history = vec![
        filecommand_core::quicksearch::FrecencyEntry { path: PathBuf::from(r"C:\Users\demo\Documents"), visit_count: 5, last_visited_ms: 1_000 },
        filecommand_core::quicksearch::FrecencyEntry { path: PathBuf::from(r"C:\Users\demo\Downloads"), visit_count: 1, last_visited_ms: 500 },
    ];
    state.fuzzy_jump = Some(filecommand_core::quicksearch::FuzzyJumpState::new());
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(text.contains("Fuzzy jump"));
    assert!(text.contains("Documents") && text.contains("Downloads"));
    insta::assert_snapshot!("fuzzy_jump_dialog", text);
}

#[test]
fn snapshot_find_file_dialog_with_streamed_results() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    let mut dialog = filecommand_core::find_file::FindFileState::new(PathBuf::from(r"C:\Users\demo\left"));
    dialog.push('r');
    dialog.push('e');
    dialog.submit(1);
    dialog.push_match(
        1,
        filecommand_core::listing::FindMatch {
            relative_path: PathBuf::from(r"docs\readme.txt"),
            entry: Entry { name: "readme.txt".into(), kind: EntryKind::File, size: 12_345, modified: Some(fixed_date()) },
        },
    );
    dialog.mark_done(1);
    state.find_file = Some(dialog);
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(text.contains("Find file"));
    assert!(text.contains(r"docs\readme.txt"));
    insta::assert_snapshot!("find_file_dialog_with_results", text);
}

#[test]
fn snapshot_find_file_dialog_no_matches() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    let mut dialog = filecommand_core::find_file::FindFileState::new(PathBuf::from(r"C:\Users\demo\left"));
    dialog.push('z');
    dialog.submit(1);
    dialog.mark_done(1);
    state.find_file = Some(dialog);
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(text.contains("No matches"));
    insta::assert_snapshot!("find_file_dialog_no_matches", text);
}

#[test]
fn snapshot_user_menu_dialog_with_entries() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.user_menu_entries = vec![
        filecommand_core::config::UserMenuEntry { label: "Compress".to_string(), command: "7z a x.7z".to_string() },
        filecommand_core::config::UserMenuEntry { label: "Backup".to_string(), command: r"robocopy . D:\backup /E".to_string() },
        filecommand_core::config::UserMenuEntry { label: "Checksum".to_string(), command: "certutil -hashfile x SHA256".to_string() },
    ];
    state.user_menu = Some(filecommand_core::dialogs::UserMenuState::new());
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(text.contains("Compress") && text.contains("Backup") && text.contains("Checksum"));
    assert!(!text.contains("robocopy"), "the command string is never shown, only the label");
    assert!(text.contains("Themes"), "the built-in Themes row always renders below the user entries");
    assert!(text.contains('\u{2560}') && text.contains('\u{2563}'), "a separator row sets the built-in entry off from the user entries");
    insta::assert_snapshot!("user_menu_dialog_with_entries", text);
}

#[test]
fn snapshot_user_menu_dialog_empty_state() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.user_menu = Some(filecommand_core::dialogs::UserMenuState::new());
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(text.contains("no entries"));
    assert!(text.contains("Themes"), "the built-in Themes row renders below the placeholder even with no user entries");
    assert!(text.contains('\u{2560}') && text.contains('\u{2563}'), "a separator row sets the built-in entry off from the placeholder");
    insta::assert_snapshot!("user_menu_dialog_empty", text);
}

#[test]
fn snapshot_theme_picker_dialog_with_active_theme_marked() {
    let mut state = base_state(UiPhase::Panels, Theme::terminal_green());
    state.theme_picker = Some(filecommand_core::dialogs::ThemePickerState::open("terminal-green"));
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(text.contains("Themes"));
    for name in filecommand_core::theme::BUILTIN_THEME_NAMES {
        assert!(text.contains(name), "`{name}` missing from the picker");
    }
    let active_row = text.lines().find(|l| l.contains("terminal-green")).expect("terminal-green row present");
    assert!(active_row.contains('*'), "the active theme row carries the marker: `{active_row}`");
    insta::assert_snapshot!("theme_picker_dialog_active_marked", text);
}

// ---------------------------------------------------------------------
// theme-picker-live-preview: live whole-screen preview while the picker
// is open (theme-selection "Live theme preview while the picker is open").
// ---------------------------------------------------------------------

/// Full-frame render through a real `TestBackend`, returning both the
/// plain-text glyph grid and the backing [`Buffer`] so callers can also
/// spot-check styling — needed here because a theme change never changes
/// glyphs, only colors, so the text grid alone can't tell two themes apart.
fn render_to_buffer(width: u16, height: u16, state: &State, depth: ColorDepth) -> Buffer {
    let identity_lines = pinned_identity_lines();
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("TestBackend terminal");
    terminal
        .draw(|frame| {
            let area = frame.area();
            views::render(frame.buffer_mut(), area, state, depth, &identity_lines, FIXED_CLOCK_TEXT, None);
        })
        .expect("draw into TestBackend");
    terminal.backend().buffer().clone()
}

#[test]
fn snapshot_theme_picker_dialog_previews_a_non_active_highlighted_theme() {
    // task 3.1: the active theme is `nc-classic`, but the highlight sits on
    // `purple-lights`. Every surface — not just the picker dialog — must
    // render through `purple-lights`'s role table, while the marker stays
    // on the applied theme's (`nc-classic`) row (theme-selection scenario
    // "Moving the highlight previews the theme").
    use filecommand_core::theme::{BUILTIN_THEME_NAMES, Role};
    use filecommand_tui::style::role_style;

    let mut state = base_state(UiPhase::Panels, Theme::classic());
    let highlight = BUILTIN_THEME_NAMES.iter().position(|n| *n == "purple-lights").unwrap();
    state.theme_picker = Some(filecommand_core::dialogs::ThemePickerState { highlight });

    let buf = render_to_buffer(80, 24, &state, ColorDepth::Ansi16);
    let text = buffer_to_text(&buf);

    // The marker stays on the applied theme's row, not the previewed one
    // (design D2).
    let active_row = text.lines().find(|l| l.contains("nc-classic")).expect("nc-classic row present");
    assert!(active_row.contains('*'), "the marker stays on the applied theme's row: `{active_row}`");
    let previewed_row = text.lines().find(|l| l.contains("purple-lights")).expect("purple-lights row present");
    assert!(!previewed_row.contains('*'), "the previewed (non-applied) row carries no marker: `{previewed_row}`");

    // A theme swap changes colors, not glyphs, so prove the *whole frame*
    // previews `purple-lights` by checking styling directly: the left
    // panel's top-left frame cell (outside the picker dialog entirely)
    // must use `purple-lights`'s `PanelFrame` role, not `nc-classic`'s.
    let purple = Theme::purple_lights();
    let classic = Theme::classic();
    let purple_frame = role_style(&purple, Role::PanelFrame, ColorDepth::Ansi16);
    let classic_frame = role_style(&classic, Role::PanelFrame, ColorDepth::Ansi16);
    assert_ne!(purple_frame, classic_frame, "sanity: the two themes actually differ on PanelFrame");
    assert_eq!(buf[(0, 0)].style().fg, purple_frame.fg, "the panel border previews the highlighted theme, not the applied one");
    assert_eq!(buf[(0, 0)].style().bg, purple_frame.bg, "the panel border previews the highlighted theme, not the applied one");

    insta::assert_snapshot!("theme_picker_dialog_previews_purple_lights", text);
}

#[test]
fn theme_picker_open_with_the_active_theme_highlighted_is_byte_identical_to_the_frame_before_it_opened() {
    // task 3.2 / theme-selection scenario "Opening the picker changes
    // nothing visually": since the highlight starts on the active theme,
    // opening the picker must not repaint anything outside the dialog's
    // own footprint.
    let state = base_state(UiPhase::Panels, Theme::classic());
    let before = render_to_buffer(80, 24, &state, ColorDepth::Ansi16);

    let mut opened = state;
    opened.theme_picker = Some(filecommand_core::dialogs::ThemePickerState::open("nc-classic"));
    let after = render_to_buffer(80, 24, &opened, ColorDepth::Ansi16);

    // Derive the dialog's exact footprint from its own box-drawing corners
    // rather than hardcoding coordinates, so this stays correct however the
    // picker sizes itself. Row 0 is skipped because it's the panels' own
    // top border, which reuses the same corner glyph as the dialog's.
    let after_text = buffer_to_text(&after);
    let lines: Vec<&str> = after_text.lines().collect();
    let y_top = lines.iter().enumerate().skip(1).find(|(_, l)| l.contains('\u{2554}')).map(|(i, _)| i).expect("picker top border present");
    let y_bottom = lines.iter().position(|l| l.contains('\u{255A}')).expect("picker bottom border present");
    let x_left = lines[y_top].chars().position(|c| c == '\u{2554}').expect("picker top-left corner present");
    let x_right = lines[y_top].chars().position(|c| c == '\u{2557}').expect("picker top-right corner present");

    let area = before.area;
    for y in 0..area.height {
        for x in 0..area.width {
            let before_cell = &before[(x, y)];
            let after_cell = &after[(x, y)];
            if before_cell != after_cell {
                // Every diff must fall inside the picker dialog's own
                // footprint — nothing else may repaint.
                let (xu, yu) = (x as usize, y as usize);
                assert!(
                    xu >= x_left && xu <= x_right && yu >= y_top && yu <= y_bottom,
                    "cell ({x},{y}) changed outside the picker dialog's footprint: {before_cell:?} -> {after_cell:?}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------
// visual-themes: representative screens (panels, a dialog, key bar) under
// each of the four new built-in themes (theme-system "New themes satisfy
// validation and swap semantics" — task 4.3).
// ---------------------------------------------------------------------

/// `(theme, snapshot-name fragment)` for each of the four new built-in
/// themes, so the panels/dialog/key-bar trio below runs identically for all
/// four without repeating the theme roster by hand.
fn new_builtin_themes() -> [(Theme, &'static str); 4] {
    [
        (Theme::terminal_green(), "terminal_green"),
        (Theme::purple_lights(), "purple_lights"),
        (Theme::yellow_storm(), "yellow_storm"),
        (Theme::inverted(), "inverted"),
    ]
}

#[test]
fn snapshot_full_panels_under_each_new_theme() {
    for (theme, name) in new_builtin_themes() {
        let state = base_state(UiPhase::Panels, theme);
        let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
        insta::assert_snapshot!(format!("full_panels_{name}"), text);
    }
}

#[test]
fn snapshot_quit_confirm_dialog_under_each_new_theme() {
    for (theme, name) in new_builtin_themes() {
        let mut state = base_state(UiPhase::Panels, theme);
        state.quit_confirm = true;
        let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
        assert!(text.contains("Quit FileCommand?"));
        insta::assert_snapshot!(format!("quit_confirm_dialog_{name}"), text);
    }
}

#[test]
fn snapshot_fkey_bar_under_each_new_theme() {
    for (theme, name) in new_builtin_themes() {
        let state = base_state(UiPhase::Panels, theme);
        let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
        let last_line = text.lines().last().unwrap_or_default();
        insta::assert_snapshot!(format!("fkey_bar_last_row_{name}"), last_line);
    }
}

#[test]
fn snapshot_file_action_menu_dialog_non_executable() {
    // file-action-menu "Menu contents, ordering, and navigation":
    // non-executable targets list View, Edit, Copy, Rename, Move, Delete
    // with View highlighted first, and no Run entry.
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.file_action_menu = Some(filecommand_core::dialogs::FileActionMenuState::new(std::ffi::OsString::from("readme.txt"), false));
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(!text.contains("Run"), "a non-executable target must not list Run");
    assert!(text.contains("View") && text.contains("Edit") && text.contains("Copy"));
    assert!(text.contains("Rename") && text.contains("Move") && text.contains("Delete"));
    insta::assert_snapshot!("file_action_menu_dialog_non_executable", text);
}

#[test]
fn snapshot_file_action_menu_dialog_executable() {
    // file-action-menu "Menu contents, ordering, and navigation": an
    // executable target lists Run first (default-highlighted).
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.file_action_menu = Some(filecommand_core::dialogs::FileActionMenuState::new(std::ffi::OsString::from("setup.exe"), true));
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(text.contains("Run"), "an executable target must list Run");
    let run_pos = text.find("Run").unwrap();
    let view_pos = text.find("View").unwrap();
    assert!(run_pos < view_pos, "Run must render before View");
    insta::assert_snapshot!("file_action_menu_dialog_executable", text);
}

#[test]
fn snapshot_help_window_topic_list() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.help = Some(filecommand_core::dialogs::HelpState::new());
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(text.contains("Help"));
    assert!(text.contains("About FileCommand"));
    assert!(text.contains("FileCommand") && text.contains("Norton Commander"), "shared identity header");
    insta::assert_snapshot!("help_window_topic_list", text);
}

#[test]
fn snapshot_help_window_topic_page() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    let mut help = filecommand_core::dialogs::HelpState::new();
    help.page = Some(1); // Keyboard reference
    state.help = Some(help);
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(text.contains("modifier variants"));
    assert!(!text.contains("About FileCommand"), "the list is replaced by the page");
    insta::assert_snapshot!("help_window_topic_page", text);
}

#[test]
fn snapshot_about_dialog_over_help_window() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    let mut help = filecommand_core::dialogs::HelpState::new();
    help.about_open = true;
    state.help = Some(help);
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(text.contains("License: MIT OR Apache-2.0"));
    assert!(text.contains("OK"));
    insta::assert_snapshot!("about_dialog_over_help_window", text);
}

#[test]
fn snapshot_startup_warning_dialog_overlays_the_panels() {
    // Regression: a malformed usermenu.toml must raise a dismissable modal
    // warning dialog (not a panel mini-status) at startup (user-menu
    // "Malformed file warns and falls back without overwriting").
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.startup_warning = Some("usermenu.toml is malformed; F2 uses the default user menu".to_string());
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(text.contains("Warning"));
    assert!(text.contains("usermenu.toml is malformed"));
    assert!(text.contains("Press any key to continue"));
    insta::assert_snapshot!("startup_warning_dialog", text);
}

#[test]
fn snapshot_startup_warning_dialog_overlays_the_splash_screen_too() {
    // The warning is raised before the event loop starts, so it can coexist
    // with the splash screen — it must still be visible on top of it rather
    // than getting lost underneath.
    let mut state = base_state(UiPhase::Splash { started_at_ms: 0 }, Theme::classic());
    state.startup_warning = Some("usermenu.toml is malformed; F2 uses the default user menu".to_string());
    let text = render_to_text(80, 24, &state, ColorDepth::Ansi16);
    assert!(text.contains("Warning"));
    assert!(text.contains("usermenu.toml is malformed"));
}
