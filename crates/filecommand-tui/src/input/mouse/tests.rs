use std::ffi::OsString;
use std::path::PathBuf;

use ratatui::layout::Rect;

use filecommand_core::dialogs::{FileActionMenuState, ThemePickerState, UserMenuState};
use filecommand_core::drives::DriveSelect;
use filecommand_core::find_file::FindFileState;
use filecommand_core::fs_ops::dialog::FileOpSetup;
use filecommand_core::fs_ops::JobKind;
use filecommand_core::menu::{MenuId, MenuState};
use filecommand_core::quicksearch::FuzzyJumpState;
use filecommand_core::theme::Theme;
use filecommand_core::update::{ButtonId, ClickMods, DropTarget};
use filecommand_core::{Command, PanelSide, State, UiPhase};

use super::*;
use crate::hitmap::{HitMap, PanelHits};

fn test_state() -> State {
    State::empty(Theme::classic())
}

fn ev(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
    MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::NONE }
}

fn ev_mods(kind: MouseEventKind, x: u16, y: u16, modifiers: KeyModifiers) -> MouseEvent {
    MouseEvent { kind, column: x, row: y, modifiers }
}

/// A hit map with one entry row (`"a.txt"` at (2, 3)) on the left panel, a
/// blank body rect covering the rest of that panel, a keybar slot 5 at
/// (10, 20), and a menu title `Files` at (5, 0) — enough surface for every
/// panels-context test below.
fn sample_hitmap() -> HitMap {
    let mut hm = HitMap::default();
    *hm.panel_mut(PanelSide::Left) = PanelHits {
        area: Rect { x: 0, y: 0, width: 20, height: 10 },
        title: Rect { x: 0, y: 0, width: 20, height: 1 },
        rows: vec![(Rect { x: 2, y: 3, width: 10, height: 1 }, OsString::from("a.txt"))],
        ..Default::default()
    };
    *hm.panel_mut(PanelSide::Right) = PanelHits {
        area: Rect { x: 20, y: 0, width: 20, height: 10 },
        title: Rect { x: 20, y: 0, width: 20, height: 1 },
        rows: vec![],
        ..Default::default()
    };
    hm.keybar = vec![(Rect { x: 10, y: 20, width: 5, height: 1 }, 5)];
    hm.menu_titles = vec![(Rect { x: 5, y: 0, width: 5, height: 1 }, MenuId::Files)];
    hm.dialog_buttons = vec![(Rect { x: 1, y: 1, width: 4, height: 1 }, ButtonId::QuitYes), (Rect { x: 6, y: 1, width: 3, height: 1 }, ButtonId::QuitNo)];
    hm
}

/// A plain Down+Up on the same cell with no modifier is a click.
#[test]
fn plain_click_on_an_entry_row_produces_click_entry() {
    let hitmap = sample_hitmap();
    let state = test_state();
    let mut tracker = MouseTracker::new();
    assert_eq!(map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state), None, "Down alone dispatches nothing");
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, Some(Command::ClickEntry { side: PanelSide::Left, name: OsString::from("a.txt"), mods: ClickMods::Plain }));
}

#[test]
fn ctrl_click_on_an_entry_row_produces_ctrl_click_entry() {
    let hitmap = sample_hitmap();
    let state = test_state();
    let mut tracker = MouseTracker::new();
    map_mouse(ev_mods(MouseEventKind::Down(MouseButton::Left), 3, 3, KeyModifiers::CONTROL), &hitmap, &mut tracker, &state);
    let cmd = map_mouse(ev_mods(MouseEventKind::Up(MouseButton::Left), 3, 3, KeyModifiers::CONTROL), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, Some(Command::ClickEntry { side: PanelSide::Left, name: OsString::from("a.txt"), mods: ClickMods::Ctrl }));
}

/// mouse-input "Double-click acts as Enter": a second Down+Up on the same
/// row within the double-click window.
#[test]
fn second_click_within_the_window_is_enter() {
    let hitmap = sample_hitmap();
    let mut state = test_state();
    state.clock_ms = 1_000;
    let mut tracker = MouseTracker::new();

    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    let first = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    assert!(matches!(first, Some(Command::ClickEntry { .. })), "the first click still focuses+moves");

    state.clock_ms += 200; // inside DOUBLE_CLICK_MS
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    let second = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    assert_eq!(second, Some(Command::Enter));
}

#[test]
fn a_click_after_the_double_click_window_expires_is_not_enter() {
    let hitmap = sample_hitmap();
    let mut state = test_state();
    state.clock_ms = 1_000;
    let mut tracker = MouseTracker::new();

    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);

    state.clock_ms += DOUBLE_CLICK_MS + 1;
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    let second = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    assert!(matches!(second, Some(Command::ClickEntry { .. })), "outside the window it's just another click");
}

/// design D3 "Ctrl-click-without-movement detection": a Ctrl+click never
/// arms a double-click, so a following plain click on the same row is a
/// fresh click, not Enter.
#[test]
fn ctrl_click_does_not_arm_a_double_click() {
    let hitmap = sample_hitmap();
    let mut state = test_state();
    state.clock_ms = 1_000;
    let mut tracker = MouseTracker::new();

    map_mouse(ev_mods(MouseEventKind::Down(MouseButton::Left), 3, 3, KeyModifiers::CONTROL), &hitmap, &mut tracker, &state);
    map_mouse(ev_mods(MouseEventKind::Up(MouseButton::Left), 3, 3, KeyModifiers::CONTROL), &hitmap, &mut tracker, &state);

    state.clock_ms += 50;
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, Some(Command::ClickEntry { side: PanelSide::Left, name: OsString::from("a.txt"), mods: ClickMods::Plain }));
}

/// A press that drags to a different cell before release is not a click —
/// mouse-panel-drag's territory: a press on an entry row that moves begins
/// and then completes a drag instead (mouse-drag "Drag lifecycle").
#[test]
fn a_drag_before_release_begins_and_completes_a_drag_not_a_click() {
    let hitmap = sample_hitmap();
    let state = test_state();
    let mut tracker = MouseTracker::new();
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    let begin = map_mouse(ev(MouseEventKind::Drag(MouseButton::Left), 4, 3), &hitmap, &mut tracker, &state);
    assert_eq!(begin, Some(Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy }));
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 4, 3), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, Some(Command::DragDrop { op: JobKind::Copy }), "not a click; a completed drag instead");
}

// ---------------------------------------------------------------------
// mouse-panel-drag: drag lifecycle, verb selection, and de-duplication
// (tasks.md 2.1).
// ---------------------------------------------------------------------

/// A press that never moves is untouched by any of this — still a plain
/// click, exactly as mouse-basics pinned it.
#[test]
fn a_press_that_never_moves_is_still_a_plain_click() {
    let hitmap = sample_hitmap();
    let state = test_state();
    let mut tracker = MouseTracker::new();
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, Some(Command::ClickEntry { side: PanelSide::Left, name: OsString::from("a.txt"), mods: ClickMods::Plain }));
}

/// A press that starts on blank panel area and then moves never begins a
/// drag (mouse-drag "Drag lifecycle": only "a press on an entry row"
/// qualifies) — and releasing it is a no-op, exactly like the old
/// (pre-drag) "moved press" behaviour.
#[test]
fn a_press_on_blank_area_that_moves_never_begins_a_drag() {
    let hitmap = sample_hitmap();
    let state = test_state();
    let mut tracker = MouseTracker::new();
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 15, 8), &hitmap, &mut tracker, &state);
    let drag_event = map_mouse(ev(MouseEventKind::Drag(MouseButton::Left), 16, 8), &hitmap, &mut tracker, &state);
    assert_eq!(drag_event, None);
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 16, 8), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, None);
}

/// mouse-drag "Drag lifecycle": dragging the pressed entry over the other
/// panel's blank area resolves that panel as a `PanelDir` target once the
/// pointer actually gets there.
#[test]
fn dragging_over_the_other_panel_resolves_a_panel_dir_target() {
    let hitmap = sample_hitmap();
    let state = test_state();
    let mut tracker = MouseTracker::new();
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    map_mouse(ev(MouseEventKind::Drag(MouseButton::Left), 4, 3), &hitmap, &mut tracker, &state); // crosses the threshold: DragBegin
    let cmd = map_mouse(ev(MouseEventKind::Drag(MouseButton::Left), 25, 5), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, Some(Command::DragOver { op: JobKind::Copy, target: Some(DropTarget::PanelDir(PanelSide::Right)) }));
}

/// An unchanged target/verb across successive `Drag` events is
/// de-duplicated — no repeated `DragOver` (mouse-drag "de-duplicated so an
/// unchanged target doesn't re-emit every event").
#[test]
fn an_unchanged_drag_target_does_not_re_emit_drag_over() {
    let hitmap = sample_hitmap();
    let state = test_state();
    let mut tracker = MouseTracker::new();
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    map_mouse(ev(MouseEventKind::Drag(MouseButton::Left), 4, 3), &hitmap, &mut tracker, &state); // DragBegin
    let first = map_mouse(ev(MouseEventKind::Drag(MouseButton::Left), 25, 5), &hitmap, &mut tracker, &state);
    assert!(matches!(first, Some(Command::DragOver { .. })));
    let second = map_mouse(ev(MouseEventKind::Drag(MouseButton::Left), 26, 6), &hitmap, &mut tracker, &state); // still over the right panel's blank area
    assert_eq!(second, None, "the target didn't change, so nothing re-emits");
}

/// mouse-drag "Verb selection": Shift+left-button drag proposes Move.
#[test]
fn shift_drag_proposes_move() {
    let hitmap = sample_hitmap();
    let state = test_state();
    let mut tracker = MouseTracker::new();
    map_mouse(ev_mods(MouseEventKind::Down(MouseButton::Left), 3, 3, KeyModifiers::SHIFT), &hitmap, &mut tracker, &state);
    let begin = map_mouse(ev_mods(MouseEventKind::Drag(MouseButton::Left), 4, 3, KeyModifiers::SHIFT), &hitmap, &mut tracker, &state);
    assert_eq!(begin, Some(Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Move }));
}

/// mouse-drag "Verb selection": Ctrl+left-button drag still proposes Copy —
/// Ctrl never proposes Move.
#[test]
fn ctrl_drag_still_proposes_copy() {
    let hitmap = sample_hitmap();
    let state = test_state();
    let mut tracker = MouseTracker::new();
    map_mouse(ev_mods(MouseEventKind::Down(MouseButton::Left), 3, 3, KeyModifiers::CONTROL), &hitmap, &mut tracker, &state);
    let begin = map_mouse(ev_mods(MouseEventKind::Drag(MouseButton::Left), 4, 3, KeyModifiers::CONTROL), &hitmap, &mut tracker, &state);
    assert_eq!(begin, Some(Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy }));
}

/// mouse-drag "Verb selection" / "Right-button drag proposes Move": a
/// right-button press that moves before release begins a Move-proposing
/// drag instead of opening the action menu.
#[test]
fn right_button_press_that_moves_begins_a_move_drag_instead_of_the_action_menu() {
    let hitmap = sample_hitmap();
    let state = test_state();
    let mut tracker = MouseTracker::new();
    map_mouse(ev(MouseEventKind::Down(MouseButton::Right), 3, 3), &hitmap, &mut tracker, &state);
    let begin = map_mouse(ev(MouseEventKind::Drag(MouseButton::Right), 4, 3), &hitmap, &mut tracker, &state);
    assert_eq!(begin, Some(Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Move }));
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Right), 4, 3), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, Some(Command::DragDrop { op: JobKind::Move }));
}

#[test]
fn click_on_blank_panel_area_focuses_only() {
    let hitmap = sample_hitmap();
    let state = test_state();
    let mut tracker = MouseTracker::new();
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 15, 8), &hitmap, &mut tracker, &state);
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 15, 8), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, Some(Command::FocusPanel(PanelSide::Left)));
}

#[test]
fn click_on_keybar_slot_dispatches_keybar_press() {
    let hitmap = sample_hitmap();
    let state = test_state();
    let mut tracker = MouseTracker::new();
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 11, 20), &hitmap, &mut tracker, &state);
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 11, 20), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, Some(Command::KeybarPress(5)));
}

#[test]
fn click_on_a_menu_title_opens_the_pulldown() {
    let hitmap = sample_hitmap();
    let state = test_state();
    let mut tracker = MouseTracker::new();
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 6, 0), &hitmap, &mut tracker, &state);
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 6, 0), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, Some(Command::MenuTitleClick(MenuId::Files)));
}

/// mouse-input "Right-click opens the action menu" / mouse-panel-drag: a
/// right-button press that never moves before release still opens the
/// action menu, now resolved on `Up` rather than `Down` — deferred so a
/// right-button press that *does* move first can become a drag instead
/// (mouse-drag "Verb selection": "a right-button drag ... SHALL propose
/// Move").
#[test]
fn right_click_on_an_entry_row_opens_the_action_menu() {
    let hitmap = sample_hitmap();
    let state = test_state();
    let mut tracker = MouseTracker::new();
    assert_eq!(map_mouse(ev(MouseEventKind::Down(MouseButton::Right), 3, 3), &hitmap, &mut tracker, &state), None, "Down alone dispatches nothing");
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Right), 3, 3), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, Some(Command::OpenActionMenuAt { side: PanelSide::Left, name: OsString::from("a.txt") }));
}

#[test]
fn right_click_off_any_row_does_nothing() {
    let hitmap = sample_hitmap();
    let state = test_state();
    let mut tracker = MouseTracker::new();
    map_mouse(ev(MouseEventKind::Down(MouseButton::Right), 15, 8), &hitmap, &mut tracker, &state);
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Right), 15, 8), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, None);
}

/// A right-button press breaks any in-progress left-click double-click
/// chain immediately on `Down`, even though opening the action menu itself
/// is now deferred to `Up`.
#[test]
fn right_click_clears_the_double_click_chain() {
    let hitmap = sample_hitmap();
    let mut state = test_state();
    state.clock_ms = 1_000;
    let mut tracker = MouseTracker::new();

    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);

    state.clock_ms += 50;
    map_mouse(ev(MouseEventKind::Down(MouseButton::Right), 3, 3), &hitmap, &mut tracker, &state);

    state.clock_ms += 50; // still inside DOUBLE_CLICK_MS of the first left click
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    assert!(matches!(cmd, Some(Command::ClickEntry { .. })), "the right-click in between must have broken the chain");
}

/// mouse-input "Mouse is honoured only where the key would be": a right-click
/// while a pull-down is open does nothing — panel rows are not honoured at
/// all in that context.
#[test]
fn right_click_while_a_pulldown_is_open_does_nothing() {
    let hitmap = sample_hitmap();
    let mut state = test_state();
    state.menu = Some(MenuState::for_menu(MenuId::Files));
    let mut tracker = MouseTracker::new();
    let cmd = map_mouse(ev(MouseEventKind::Down(MouseButton::Right), 3, 3), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, None);
}

#[test]
fn wheel_over_the_left_panel_scrolls_it_down_three_rows() {
    let hitmap = sample_hitmap();
    let state = test_state();
    let mut tracker = MouseTracker::new();
    let cmd = map_mouse(ev(MouseEventKind::ScrollDown, 5, 5), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, Some(Command::ScrollPanel { side: PanelSide::Left, delta: 3 }));
}

#[test]
fn wheel_over_the_right_panel_scrolls_it_up_three_rows() {
    let hitmap = sample_hitmap();
    let state = test_state();
    let mut tracker = MouseTracker::new();
    let cmd = map_mouse(ev(MouseEventKind::ScrollUp, 25, 5), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, Some(Command::ScrollPanel { side: PanelSide::Right, delta: -3 }));
}

#[test]
fn wheel_outside_both_panels_does_nothing() {
    let hitmap = sample_hitmap();
    let state = test_state();
    let mut tracker = MouseTracker::new();
    let cmd = map_mouse(ev(MouseEventKind::ScrollDown, 5, 20), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, None);
}

/// mouse-input "Mouse is honoured only where the key would be": every
/// overlay not in the gating table ignores mouse entirely, regardless of
/// what the hit map contains.
#[test]
fn an_ignored_overlay_returns_none_even_over_a_hit_row() {
    let hitmap = sample_hitmap();
    let mut state = test_state();
    state.help = Some(filecommand_core::dialogs::HelpState::new());
    let mut tracker = MouseTracker::new();
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, None);
}

/// mouse-input "Running job accepts Cancel only" / "Dialog button": a
/// quit-confirm click on a button rect activates it.
#[test]
fn quit_confirm_click_on_yes_activates_the_button() {
    let hitmap = sample_hitmap();
    let mut state = test_state();
    state.quit_confirm = true;
    let mut tracker = MouseTracker::new();
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 2, 1), &hitmap, &mut tracker, &state);
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 2, 1), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, Some(Command::DialogButtonClick(ButtonId::QuitYes)));
}

#[test]
fn quit_confirm_click_off_any_button_does_nothing() {
    let hitmap = sample_hitmap();
    let mut state = test_state();
    state.quit_confirm = true;
    let mut tracker = MouseTracker::new();
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 15, 15), &hitmap, &mut tracker, &state);
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 15, 15), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, None);
}

/// mouse-input "Key bar, menu bar, pull-down items, and dialog buttons are
/// clickable": an open pull-down's item click activates it; a click
/// elsewhere closes the bar.
#[test]
fn pulldown_item_click_dispatches_menu_item_click() {
    let mut hitmap = sample_hitmap();
    hitmap.menu_items = vec![(Rect { x: 6, y: 2, width: 8, height: 1 }, 2)];
    let mut state = test_state();
    state.menu = Some(MenuState::for_menu(MenuId::Files));
    let mut tracker = MouseTracker::new();
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 8, 2), &hitmap, &mut tracker, &state);
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 8, 2), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, Some(Command::MenuItemClick(2)));
}

#[test]
fn pulldown_click_elsewhere_closes_the_bar() {
    let mut hitmap = sample_hitmap();
    hitmap.menu_items = vec![(Rect { x: 6, y: 2, width: 8, height: 1 }, 2)];
    let mut state = test_state();
    state.menu = Some(MenuState::for_menu(MenuId::Files));
    let mut tracker = MouseTracker::new();
    // Lands on the left panel's entry row — while a pull-down is open,
    // panel rows are not honoured at all; the click just closes the bar.
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, Some(Command::MenuClose));
}

// ---------------------------------------------------------------------
// Mode-gating table (design D5; mouse-input "Mouse is honoured only where
// the key would be"). The tests above already cover `Panels`
// (`map_panels`), `PulldownOpen` (`map_pulldown`), and two `DialogButtons`/
// `Ignored` cases (`quit_confirm`, an open Help window) — the tests below
// round out every remaining arm of `context()` so the whole table is
// exercised, not just a sample of it.
// ---------------------------------------------------------------------

/// A file-op setup dialog (e.g. the destination-input/rename/delete-confirm
/// text dialogs) honours button clicks only — a panel row is inert while
/// one is open.
#[test]
fn file_op_setup_phase_honours_dialog_buttons_only() {
    let hitmap = sample_hitmap();
    let mut state = test_state();
    state.phase = UiPhase::FileOpSetup(FileOpSetup::RenameInput {
        source_dir: PathBuf::new(),
        original_name: OsString::from("a.txt"),
        is_dir: false,
        input: String::new(),
    });
    let mut tracker = MouseTracker::new();

    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    let panel_click = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    assert_eq!(panel_click, None, "panel rows are not honoured while a file-op dialog is open");

    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 2, 1), &hitmap, &mut tracker, &state);
    let button_click = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 2, 1), &hitmap, &mut tracker, &state);
    assert_eq!(button_click, Some(Command::DialogButtonClick(ButtonId::QuitYes)), "but its own button rect still is");
}

/// `UiPhase::FileOpSummary` reaches the same `DialogButtons` context.
#[test]
fn file_op_summary_phase_honours_dialog_buttons_only() {
    let hitmap = sample_hitmap();
    let mut state = test_state();
    state.phase = UiPhase::FileOpSummary(Vec::new());
    let mut tracker = MouseTracker::new();

    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    let panel_click = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    assert_eq!(panel_click, None);

    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 2, 1), &hitmap, &mut tracker, &state);
    let button_click = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 2, 1), &hitmap, &mut tracker, &state);
    assert_eq!(button_click, Some(Command::DialogButtonClick(ButtonId::QuitYes)));
}

/// Every overlay/phase not listed in the mode-gating table ignores mouse
/// input outright, regardless of what the hit map contains — a shared
/// assertion the rest of this block drives with each remaining case.
fn assert_ignores_mouse_over_a_hit_row(state: State) {
    let hitmap = sample_hitmap();
    let mut tracker = MouseTracker::new();
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, None);
}

#[test]
fn drive_select_overlay_ignores_mouse() {
    let mut state = test_state();
    state.drive_select = Some(DriveSelect::new(PanelSide::Left, vec!['C'], Some('C')));
    assert_ignores_mouse_over_a_hit_row(state);
}

#[test]
fn fuzzy_jump_overlay_ignores_mouse() {
    let mut state = test_state();
    state.fuzzy_jump = Some(FuzzyJumpState::new());
    assert_ignores_mouse_over_a_hit_row(state);
}

#[test]
fn find_file_overlay_ignores_mouse() {
    let mut state = test_state();
    state.find_file = Some(FindFileState::new(PathBuf::from(r"C:\")));
    assert_ignores_mouse_over_a_hit_row(state);
}

#[test]
fn user_menu_overlay_ignores_mouse() {
    let mut state = test_state();
    state.user_menu = Some(UserMenuState::new());
    assert_ignores_mouse_over_a_hit_row(state);
}

#[test]
fn theme_picker_overlay_ignores_mouse() {
    let mut state = test_state();
    let name = state.theme.name.clone();
    state.theme_picker = Some(ThemePickerState::open(&name));
    assert_ignores_mouse_over_a_hit_row(state);
}

#[test]
fn startup_warning_overlay_ignores_mouse() {
    let mut state = test_state();
    state.startup_warning = Some("malformed usermenu.toml".to_string());
    assert_ignores_mouse_over_a_hit_row(state);
}

#[test]
fn file_action_menu_overlay_ignores_mouse() {
    let mut state = test_state();
    state.file_action_menu = Some(FileActionMenuState::new(OsString::from("a.txt"), false));
    assert_ignores_mouse_over_a_hit_row(state);
}

#[test]
fn splash_phase_ignores_mouse() {
    let mut state = test_state();
    state.phase = UiPhase::Splash { started_at_ms: 0 };
    assert_ignores_mouse_over_a_hit_row(state);
}

#[test]
fn placeholder_phase_ignores_mouse() {
    let mut state = test_state();
    state.phase = UiPhase::Placeholder;
    assert_ignores_mouse_over_a_hit_row(state);
}

/// The viewer/editor phases are never actually routed through `map_mouse`
/// in production (the event loop dispatches their wheel-only handling
/// directly from `state.phase` — see this module's doc comment); this just
/// pins the defensive `Context::Ignored` arm so a future refactor that
/// *did* start calling `map_mouse` here wouldn't silently start honouring
/// panel-row clicks underneath a full-screen viewer/editor.
#[test]
fn viewer_phase_ignores_map_mouse_if_ever_called_directly() {
    let mut state = test_state();
    state.phase = UiPhase::Viewer(filecommand_core::viewer::ViewerState::new(PathBuf::from("f.txt"), 0));
    assert_ignores_mouse_over_a_hit_row(state);
}

#[test]
fn editor_phase_ignores_map_mouse_if_ever_called_directly() {
    let mut state = test_state();
    state.phase = UiPhase::Editor(filecommand_core::editor::EditorState::from_bytes(PathBuf::from("f.txt"), b""));
    assert_ignores_mouse_over_a_hit_row(state);
}

#[test]
fn reset_clears_an_in_progress_press_and_the_double_click_chain() {
    let hitmap = sample_hitmap();
    let mut state = test_state();
    state.clock_ms = 1_000;
    let mut tracker = MouseTracker::new();
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);

    tracker.reset();
    state.clock_ms += 10; // well inside the double-click window, if it survived
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    assert!(matches!(cmd, Some(Command::ClickEntry { .. })), "reset() must break the double-click chain");
}
