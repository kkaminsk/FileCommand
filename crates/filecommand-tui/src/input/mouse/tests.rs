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
    hm.dialog_buttons =
        vec![(Rect { x: 1, y: 1, width: 4, height: 1 }, ButtonId::QuitYes), (Rect { x: 6, y: 1, width: 3, height: 1 }, ButtonId::QuitNo)];
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
    state.open_overlay(Overlay::Menu(MenuState::for_menu(MenuId::Files)));
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
    state.open_overlay(Overlay::Help(filecommand_core::dialogs::HelpState::new()));
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
    state.open_overlay(Overlay::Menu(MenuState::for_menu(MenuId::Files)));
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
    state.open_overlay(Overlay::Menu(MenuState::for_menu(MenuId::Files)));
    let mut tracker = MouseTracker::new();
    // Lands on the left panel's entry row — while a pull-down is open,
    // panel rows are not honoured at all; the click just closes the bar.
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, Some(Command::MenuClose));
}

/// mouse-input "File-action menu entries are clickable": a click on an
/// open file-action menu's row dispatches the click command — the pull-
/// down's `pulldown_item_click_dispatches_menu_item_click`, mirrored.
#[test]
fn file_action_menu_item_click_dispatches_file_action_menu_item_click() {
    let mut hitmap = sample_hitmap();
    hitmap.file_action_menu_items = vec![(Rect { x: 6, y: 2, width: 8, height: 1 }, 2)];
    let mut state = test_state();
    state.open_overlay(Overlay::FileActionMenu(FileActionMenuState::new(OsString::from("a.txt"), false)));
    let mut tracker = MouseTracker::new();
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 8, 2), &hitmap, &mut tracker, &state);
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 8, 2), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, Some(Command::FileActionMenuItemClick(2)));
}

/// mouse-input "File-action menu entries are clickable": a click outside
/// the open menu closes it with no action taken — the pull-down's
/// `pulldown_click_elsewhere_closes_the_bar`, mirrored.
#[test]
fn file_action_menu_click_elsewhere_closes_it() {
    let mut hitmap = sample_hitmap();
    hitmap.file_action_menu_items = vec![(Rect { x: 6, y: 2, width: 8, height: 1 }, 2)];
    let mut state = test_state();
    state.open_overlay(Overlay::FileActionMenu(FileActionMenuState::new(OsString::from("a.txt"), false)));
    let mut tracker = MouseTracker::new();
    // Lands on the left panel's entry row — while the file-action menu is
    // open, panel rows are not honoured at all; the click just closes it.
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, Some(Command::FileActionMenuCancel));
}

// ---------------------------------------------------------------------
// Mode-gating table (design D5; mouse-input "Mouse is honoured only where
// the key would be"). The tests above already cover `Panels`
// (`map_panels`), `PulldownOpen` (`map_pulldown`), `FileActionMenuOpen`
// (`map_file_action_menu`), and two `DialogButtons`/`Ignored` cases
// (`quit_confirm`, an open Help window) — the tests below round out every
// remaining arm of `context()` so the whole table is exercised, not just a
// sample of it.
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
        input: filecommand_core::line_edit::LineEdit::default(),
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
    state.open_overlay(Overlay::DriveSelect(DriveSelect::new(PanelSide::Left, vec!['C'], Some('C'))));
    assert_ignores_mouse_over_a_hit_row(state);
}

#[test]
fn fuzzy_jump_overlay_ignores_mouse() {
    let mut state = test_state();
    state.open_overlay(Overlay::FuzzyJump(FuzzyJumpState::new()));
    assert_ignores_mouse_over_a_hit_row(state);
}

#[test]
fn find_file_overlay_ignores_mouse() {
    let mut state = test_state();
    state.open_overlay(Overlay::FindFile(FindFileState::new(PathBuf::from(r"C:\"))));
    assert_ignores_mouse_over_a_hit_row(state);
}

#[test]
fn user_menu_overlay_ignores_mouse() {
    let mut state = test_state();
    state.open_overlay(Overlay::UserMenu(UserMenuState::new()));
    assert_ignores_mouse_over_a_hit_row(state);
}

/// command-line "Command history dialog (Alt+F8)": "The dialog SHALL NOT
/// respond to the mouse (it joins the unsupported-overlay set)."
#[test]
fn history_dialog_overlay_ignores_mouse() {
    let mut state = test_state();
    state.open_overlay(Overlay::HistoryDialog(filecommand_core::dialogs::HistoryDialogState::new()));
    assert_ignores_mouse_over_a_hit_row(state);
}

#[test]
fn theme_picker_overlay_ignores_mouse() {
    let mut state = test_state();
    let name = state.theme.name.clone();
    state.open_overlay(Overlay::ThemePicker(ThemePickerState::open(&name)));
    assert_ignores_mouse_over_a_hit_row(state);
}

#[test]
fn startup_warning_overlay_ignores_mouse() {
    let mut state = test_state();
    state.set_startup_warning("malformed usermenu.toml".to_string());
    assert_ignores_mouse_over_a_hit_row(state);
}

// The file-action menu is no longer a genuinely-ignored overlay — see
// `file_action_menu_item_click_dispatches_file_action_menu_item_click` and
// `file_action_menu_click_elsewhere_closes_it` above, which cover its
// `Context::FileActionMenuOpen` arm instead.

#[test]
fn splash_phase_ignores_mouse() {
    let mut state = test_state();
    state.phase = UiPhase::Splash { started_at_ms: 0 };
    assert_ignores_mouse_over_a_hit_row(state);
}

/// application-shell "Mouse input is ignored while undersized": the
/// placeholder is `State::undersized` beside the phase now, not a phase, so
/// it must ignore the mouse over *every* hidden screen (resize-preserves-
/// session-state design D3).
#[test]
fn undersized_ignores_mouse_over_every_phase() {
    let phases = [
        UiPhase::Panels,
        UiPhase::Splash { started_at_ms: 0 },
        UiPhase::Viewer(filecommand_core::viewer::ViewerState::new(PathBuf::from("f.txt"), 0)),
        UiPhase::Editor(filecommand_core::editor::EditorState::from_bytes(PathBuf::from("f.txt"), b"").expect("valid UTF-8 fixture")),
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm {
            sources: vec![],
            source_dir: PathBuf::from("/left"),
            needs_second_confirm: false,
            confirmed_once: false,
            hidden_by_filter: 0,
        }),
        UiPhase::FileOpSummary(vec![]),
    ];
    for phase in phases {
        let mut state = test_state();
        state.undersized = true;
        state.phase = phase;
        assert_ignores_mouse_over_a_hit_row(state);
    }
    // Overlays beside the phase are hidden too — a menu's title must not be
    // clickable under the placeholder.
    let mut state = test_state();
    state.undersized = true;
    state.open_overlay(Overlay::Menu(MenuState::opened()));
    assert_ignores_mouse_over_a_hit_row(state);
    let mut state = test_state();
    state.undersized = true;
    state.open_overlay(Overlay::FileActionMenu(FileActionMenuState::new(OsString::from("a.txt"), false)));
    assert_ignores_mouse_over_a_hit_row(state);
}

/// application-shell "Quit dialog buttons stay clickable over the
/// placeholder": `quit_confirm` is checked before `undersized` in
/// `context`, so a click on the dialog's No button still maps.
#[test]
fn undersized_quit_dialog_buttons_stay_clickable() {
    let hitmap = sample_hitmap();
    let mut state = test_state();
    state.undersized = true;
    state.quit_confirm = true;
    let mut tracker = MouseTracker::new();
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 7, 1), &hitmap, &mut tracker, &state);
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 7, 1), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, Some(Command::DialogButtonClick(ButtonId::QuitNo)));
    // Off the buttons, nothing underneath is reachable.
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    let cmd = map_mouse(ev(MouseEventKind::Up(MouseButton::Left), 3, 3), &hitmap, &mut tracker, &state);
    assert_eq!(cmd, None);
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
    state.phase = UiPhase::Editor(filecommand_core::editor::EditorState::from_bytes(PathBuf::from("f.txt"), b"").expect("valid UTF-8 fixture"));
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

// ---------------------------------------------------------------------
// reducer-overlay-structure: click targets come from the rendered geometry
// (mouse-input "Click targets match the rendered geometry"; tasks 5.5, 6.8)
// ---------------------------------------------------------------------

/// Render `state` at `size` and return the frame's painted text and the hit
/// map `map_mouse` would use for the next frame's events.
fn rendered(state: &State, size: (u16, u16)) -> (Vec<String>, HitMap) {
    let area = Rect { x: 0, y: 0, width: size.0, height: size.1 };
    let mut buf = ratatui::buffer::Buffer::empty(area);
    let identity = ["FileCommand".to_string(), "v0".to_string(), "test".to_string(), "fixture".to_string()];
    let out = crate::views::render(&mut buf, area, state, filecommand_core::theme::ColorDepth::Ansi16, &identity, "3:04 PM", None);
    let rows = (0..size.1).map(|y| (0..size.0).map(|x| buf[(x, y)].symbol()).collect::<String>()).collect();
    (rows, out.hitmap)
}

/// A left Down+Up on one cell.
fn click_at(hitmap: &HitMap, state: &State, x: u16, y: u16) -> Option<Command> {
    let mut tracker = MouseTracker::new();
    map_mouse(ev(MouseEventKind::Down(MouseButton::Left), x, y), hitmap, &mut tracker, state);
    map_mouse(ev(MouseEventKind::Up(MouseButton::Left), x, y), hitmap, &mut tracker, state)
}

/// The screen column range `needle` occupies on `row`, as display columns.
fn span_of(row: &str, needle: &str) -> std::ops::Range<u16> {
    let byte = row.find(needle).unwrap_or_else(|| panic!("{needle:?} is painted on {row:?}"));
    let start = filecommand_core::listing::display_width(&row[..byte]) as u16;
    start..start + filecommand_core::listing::display_width(needle) as u16
}

fn sized_state(size: (u16, u16), phase: UiPhase) -> State {
    let mut state = test_state();
    state.term_size = size;
    state.phase = phase;
    state
}

/// mouse-input "Delete confirmation buttons at the size floor": a click on
/// any cell of the drawn `(Y)es` confirms and on any cell of `(N)o`
/// cancels; the frame, the truncated question, and the spaces between the
/// labels do nothing.
#[test]
fn delete_confirm_buttons_at_the_size_floor_match_the_drawn_labels() {
    let name = "n".repeat(200);
    let setup = FileOpSetup::DeleteConfirm {
        sources: vec![filecommand_core::fs_ops::SourceItem {
            original_name: OsString::from(&name),
            path: PathBuf::from("/left").join(&name),
            is_dir: false,
        }],
        source_dir: PathBuf::from("/left"),
        needs_second_confirm: false,
        confirmed_once: false,
        hidden_by_filter: 0,
    };
    let state = sized_state((60, 16), UiPhase::FileOpSetup(setup));
    let (rows, hm) = rendered(&state, (60, 16));
    let question_y = rows.iter().position(|r| r.contains("Delete n") && r.contains('\u{2026}')).expect("the question is truncated with …") as u16;
    let buttons_y = rows.iter().position(|r| r.contains("(Y)es")).expect("the buttons row") as u16;
    let yes = span_of(&rows[buttons_y as usize], "(Y)es");
    let no = span_of(&rows[buttons_y as usize], "(N)o");
    for x in yes.clone() {
        assert_eq!(click_at(&hm, &state, x, buttons_y), Some(Command::DialogButtonClick(ButtonId::DeleteConfirmYes)), "cell {x}");
    }
    for x in no.clone() {
        assert_eq!(click_at(&hm, &state, x, buttons_y), Some(Command::DialogButtonClick(ButtonId::DeleteConfirmNo)), "cell {x}");
    }
    for x in yes.end..no.start {
        assert_eq!(click_at(&hm, &state, x, buttons_y), None, "the gap between the labels at {x}");
    }
    assert_eq!(click_at(&hm, &state, yes.start, question_y), None, "the question row");
    assert_eq!(click_at(&hm, &state, yes.start - 1, buttons_y), None, "left of the first label");
    let frame_y = buttons_y + 1;
    assert_eq!(click_at(&hm, &state, yes.start, frame_y), None, "the bottom frame");
    assert_eq!(click_at(&hm, &state, 0, 0), None, "the panels underneath are not live");
}

/// mouse-input "Quit dialog hotkey cells": the drawn `Y` confirms, the
/// drawn `N` cancels, every other cell of the dialog does nothing — at 80x24
/// and at the 60x16 floor.
#[test]
fn quit_dialog_hotkey_cells_match_the_drawn_message() {
    for size in [(80, 24), (60, 16)] {
        let mut state = sized_state(size, UiPhase::Panels);
        state.quit_confirm = true;
        let (rows, hm) = rendered(&state, size);
        let y = rows.iter().position(|r| r.contains("Quit FileCommand? (Y/N)")).expect("the message row") as u16;
        let row = &rows[y as usize];
        let y_col = span_of(row, "(Y/N)").start + 1;
        let n_col = y_col + 2;
        assert_eq!(click_at(&hm, &state, y_col, y), Some(Command::DialogButtonClick(ButtonId::QuitYes)), "{size:?}");
        assert_eq!(click_at(&hm, &state, n_col, y), Some(Command::DialogButtonClick(ButtonId::QuitNo)), "{size:?}");
        let left = span_of(row, "Quit");
        for x in left.start.saturating_sub(1)..left.end {
            assert_eq!(click_at(&hm, &state, x, y), None, "{size:?}: a message cell at {x}");
        }
        assert_eq!(click_at(&hm, &state, y_col + 1, y), None, "{size:?}: the slash between Y and N");
        assert_eq!(click_at(&hm, &state, y_col, y - 1), None, "{size:?}: the top frame");
        assert_eq!(click_at(&hm, &state, y_col, y + 1), None, "{size:?}: the bottom frame");
    }
}

/// mouse-input "Progress dialog Cancel follows a resize": after the
/// terminal shrinks from 120x40 to 80x24 the button is where the new frame
/// draws it, and a cell it occupied before — but no longer does — is dead.
#[test]
fn progress_cancel_follows_a_resize() {
    let phase = UiPhase::FileOpRunning {
        source_dir: PathBuf::from("/left"),
        dest_dir: PathBuf::from("/right"),
        dialog: filecommand_core::fs_ops::dialog::RunningDialog::Progress {
            kind: JobKind::Copy,
            progress: filecommand_core::fs_ops::ProgressInfo::starting(3, 30),
        },
    };
    let large = sized_state((120, 40), phase.clone());
    let (large_rows, _) = rendered(&large, (120, 40));
    let large_y = large_rows.iter().position(|r| r.contains("[ Cancel ]")).expect("the button at 120x40") as u16;
    let large_x = span_of(&large_rows[large_y as usize], "[ Cancel ]");

    let (small, _) = filecommand_core::update(large, Command::Resize(80, 24));
    let (small_rows, hm) = rendered(&small, (80, 24));
    let small_y = small_rows.iter().position(|r| r.contains("[ Cancel ]")).expect("the button at 80x24") as u16;
    let small_x = span_of(&small_rows[small_y as usize], "[ Cancel ]");
    assert_ne!((large_x.start, large_y), (small_x.start, small_y), "the dialog moved with the resize");
    for x in small_x.clone() {
        assert_eq!(click_at(&hm, &small, x, small_y), Some(Command::DialogButtonClick(ButtonId::ProgressCancel)), "cell {x} of the button now");
    }
    // The old position is off the 80x24 screen or on a dead cell either way.
    let old = (large_x.start, large_y);
    if old.0 < 80 && old.1 < 24 {
        assert_eq!(click_at(&hm, &small, old.0, old.1), None, "where the button was drawn before the resize");
    }
    assert_eq!(click_at(&hm, &small, small_x.start, small_y + 1), None, "the frame below the button");
    assert_eq!(click_at(&hm, &small, small_x.end, small_y), None, "just past the label");
}

/// 6.8 "One precedence order everywhere": for states covering every
/// `InputLayer` and every overlay variant, the key router, the mouse
/// context, and the frame's hit map all treat the layer `input_layer` names
/// as the one that owns input.
#[test]
fn key_routing_mouse_context_and_the_hit_map_agree_with_input_layer() {
    use filecommand_core::dialogs::{HelpState, HistoryDialogState};
    use filecommand_core::input_layer;
    use filecommand_core::InputLayer;

    use crate::input::map_key;

    let esc = crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Esc, KeyModifiers::NONE);
    let letter = crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Char('a'), KeyModifiers::NONE);
    let keys = filecommand_core::config::Keys::default();
    let size = (80, 24);
    let delete_confirm = UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm {
        sources: vec![],
        source_dir: PathBuf::from("/left"),
        needs_second_confirm: false,
        confirmed_once: false,
        hidden_by_filter: 0,
    });
    let overlays: Vec<(&str, Overlay)> = vec![
        ("menu", Overlay::Menu(MenuState::for_menu(MenuId::Files))),
        ("drive select", Overlay::DriveSelect(DriveSelect::new(PanelSide::Left, vec!['C'], Some('C')))),
        ("fuzzy jump", Overlay::FuzzyJump(FuzzyJumpState::new())),
        ("find file", Overlay::FindFile(FindFileState::new(PathBuf::from("/left")))),
        ("user menu", Overlay::UserMenu(UserMenuState::new())),
        ("history dialog", Overlay::HistoryDialog(HistoryDialogState::new())),
        ("theme picker", Overlay::ThemePicker(ThemePickerState::open("nc-classic"))),
        ("file-action menu", Overlay::FileActionMenu(FileActionMenuState::new(OsString::from("a.txt"), false))),
        ("help", Overlay::Help(HelpState::new())),
        ("startup warning", Overlay::StartupWarning("usermenu.toml is malformed".to_string())),
    ];

    let mut cases: Vec<(String, State)> = vec![
        ("panels".to_string(), sized_state(size, UiPhase::Panels)),
        ("splash".to_string(), sized_state(size, UiPhase::Splash { started_at_ms: 0 })),
        ("file-op dialog".to_string(), sized_state(size, delete_confirm.clone())),
        ("viewer".to_string(), sized_state(size, UiPhase::Viewer(filecommand_core::viewer::ViewerState::new(PathBuf::from("f.txt"), 0)))),
        (
            "editor".to_string(),
            sized_state(
                size,
                UiPhase::Editor(filecommand_core::editor::EditorState::from_bytes(PathBuf::from("f.txt"), b"x").expect("valid UTF-8 fixture")),
            ),
        ),
    ];
    for (name, overlay) in &overlays {
        let mut state = sized_state(size, UiPhase::Panels);
        state.open_overlay(overlay.clone());
        cases.push((format!("{name} over the panels"), state));
    }
    let mut quit_over_menu = sized_state(size, UiPhase::Panels);
    quit_over_menu.open_overlay(Overlay::Menu(MenuState::for_menu(MenuId::Files)));
    quit_over_menu.quit_confirm = true;
    cases.push(("quit over the menu".to_string(), quit_over_menu));
    let mut quit_over_dialog = sized_state(size, delete_confirm);
    quit_over_dialog.quit_confirm = true;
    cases.push(("quit over a file-op dialog".to_string(), quit_over_dialog));
    for undersized_over in ["panels", "menu over the panels", "startup warning over the panels", "file-op dialog", "viewer", "editor"] {
        let mut state = cases.iter().find(|(n, _)| n == undersized_over).map(|(_, s)| s.clone()).expect("a base case");
        state.term_size = (40, 10);
        state.undersized = true;
        cases.push((format!("undersized {undersized_over}"), state));
    }

    for (name, state) in cases {
        let layer = input_layer(&state);
        let size = state.term_size;
        let (_, hm) = rendered(&state, size);
        let has_quit_buttons = hm.dialog_buttons.iter().any(|(_, id)| matches!(id, ButtonId::QuitYes | ButtonId::QuitNo));
        let has_dialog_buttons = hm.dialog_buttons.iter().any(|(_, id)| !matches!(id, ButtonId::QuitYes | ButtonId::QuitNo));
        let page = 5;
        let on_esc = map_key(esc, &state, page, &keys);
        let on_letter = map_key(letter, &state, page, &keys);
        match layer {
            InputLayer::QuitConfirm => {
                assert!(matches!(context(&state), Context::DialogButtons), "{name}");
                assert!(has_quit_buttons, "{name}: the quit dialog's buttons are recorded");
                assert_eq!(on_esc, Some(Command::CancelQuit), "{name}");
                assert_eq!(on_letter, None, "{name}: a letter never reaches the layers below");
            }
            InputLayer::Undersized => {
                assert!(matches!(context(&state), Context::Ignored), "{name}");
                assert!(
                    !has_quit_buttons && !has_dialog_buttons && hm.menu_titles.is_empty() && hm.keybar.is_empty(),
                    "{name}: nothing hidden is clickable"
                );
                assert_eq!(on_letter, None, "{name}");
                let expected = if matches!(state.phase, UiPhase::Editor(_)) { None } else { Some(Command::RequestQuit) };
                assert_eq!(on_esc, expected, "{name}");
            }
            InputLayer::FileOp => {
                assert!(matches!(context(&state), Context::DialogButtons), "{name}");
                assert!(has_dialog_buttons && !has_quit_buttons, "{name}: the dialog's own buttons are recorded");
                assert_eq!(on_esc, Some(Command::FileOpCancel), "{name}");
            }
            InputLayer::Viewer | InputLayer::Editor => {
                assert!(matches!(context(&state), Context::Ignored), "{name}");
                assert!(
                    hm.dialog_buttons.is_empty() && hm.keybar.is_empty() && hm.panel(PanelSide::Left).rows.is_empty(),
                    "{name}: nothing is clickable"
                );
                assert_eq!(on_esc, None, "{name}: the event loop maps these keys directly");
            }
            InputLayer::Overlay => {
                let overlay = state.overlay().expect("the overlay layer has a slot");
                match overlay {
                    Overlay::Menu(_) => {
                        assert!(matches!(context(&state), Context::PulldownOpen), "{name}");
                        assert!(!hm.menu_titles.is_empty() && !hm.menu_items.is_empty(), "{name}: the bar and its rows are recorded");
                        assert_eq!(on_esc, Some(Command::MenuCollapse), "{name}");
                    }
                    Overlay::FileActionMenu(_) => {
                        assert!(matches!(context(&state), Context::FileActionMenuOpen), "{name}");
                        assert!(!hm.file_action_menu_items.is_empty(), "{name}: its rows are recorded");
                        assert_eq!(on_esc, Some(Command::FileActionMenuCancel), "{name}");
                    }
                    _ => {
                        assert!(matches!(context(&state), Context::Ignored), "{name}");
                        assert!(hm.menu_titles.is_empty() && hm.file_action_menu_items.is_empty(), "{name}: no menu rows are recorded");
                        let esc_cmd = on_esc.clone().unwrap_or_else(|| panic!("{name}: Esc is the overlay's own cancel"));
                        assert!(overlay.claims(&esc_cmd), "{name}: {esc_cmd:?} belongs to the open overlay");
                        assert_ne!(esc_cmd, Command::RequestQuit, "{name}");
                    }
                }
                assert!(!has_dialog_buttons && !has_quit_buttons, "{name}: no dialog buttons");
                if let Some(cmd) = &on_letter {
                    assert!(overlay.claims(cmd), "{name}: {cmd:?} is the overlay's own, never a panel command");
                }
            }
            InputLayer::Panels => {
                assert!(matches!(context(&state), Context::Panels), "{name}");
                assert!(
                    !hm.keybar.is_empty() && hm.dialog_buttons.is_empty() && hm.menu_titles.is_empty(),
                    "{name}: the panels and key bar are live, nothing modal is"
                );
                assert_eq!(on_esc, Some(Command::RequestQuit), "{name}");
            }
            InputLayer::Splash => {
                assert!(matches!(context(&state), Context::Ignored), "{name}");
                assert!(hm.keybar.is_empty() && hm.dialog_buttons.is_empty(), "{name}: the splash has nothing clickable");
            }
        }
    }
}
