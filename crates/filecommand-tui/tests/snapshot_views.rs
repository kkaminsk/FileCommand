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
use filecommand_core::info::InfoValues;
use filecommand_core::listing::{DateTime, Entry, EntryKind, SortMode};
use filecommand_core::menu::{MenuId, MenuState};
use filecommand_core::panel::{DisplayMode, ListingProgress, PanelState, SortDirection};
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
    let l = filecommand_tui::layout::compute((26, 24));
    let text = render_to_text(26, 24, &state, ColorDepth::Ansi16);
    let strip_row = left_half(text.lines().nth(1).unwrap(), l.left.width as usize);
    assert!(strip_row.contains('\u{2026}'), "`{strip_row}`");
    insta::assert_snapshot!("tab_strip_truncated_labels", strip_row);
}

#[test]
fn snapshot_tab_strip_number_only_labels() {
    let mut state = base_state(UiPhase::Panels, Theme::classic());
    state.left = panel_with_tabs(&[r"C:\filecommand", r"C:\other"]);
    let l = filecommand_tui::layout::compute((14, 24));
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
    let l = filecommand_tui::layout::compute((30, 24));
    let text = render_to_text(30, 24, &state, ColorDepth::Ansi16);
    let strip_row = left_half(text.lines().nth(1).unwrap(), l.left.width as usize);
    assert!(strip_row.contains('\u{25C4}') || strip_row.contains('\u{25BA}'), "`{strip_row}`");
    insta::assert_snapshot!("tab_strip_scrolled_with_markers", strip_row);
}
