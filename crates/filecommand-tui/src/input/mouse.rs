//! Maps crossterm mouse events to core [`Command`]s, mirroring `map_key`'s
//! shape: pure with respect to state (never mutates it, never performs
//! I/O), but — unlike `map_key` — it also needs a little bookkeeping of its
//! own across calls ([`MouseTracker`]) to tell a click from a drag and to
//! time a double-click, so it takes that as an explicit `&mut` parameter
//! rather than threading it through `State` (mouse-input "Hit-testing stays
//! in the TUI"; design D2/D3).
//!
//! The viewer and built-in editor are *not* handled here: their wheel-only
//! mouse handling needs the open `ByteSource`/editor viewport the way
//! `map_viewer_key`/`map_editor_key` need I/O `map_key` cannot perform, so
//! the event loop dispatches those two phases directly from `state.phase`
//! before ever calling `map_mouse` — the same bypass it already gives their
//! keyboard input (see `app.rs::apply_mouse_batch`).

use std::ffi::OsString;

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use filecommand_core::update::ClickMods;
use filecommand_core::{Command, PanelSide, State, UiPhase};

use crate::hitmap::{self, HitMap};

/// A same-row second click within this many milliseconds of the first acts
/// as Enter (mouse-input "Double-click acts as Enter"; design D3).
const DOUBLE_CLICK_MS: u64 = 400;

/// Press/double-click bookkeeping `map_mouse` carries across calls: which
/// button (if any) is currently held down and where it went down, and the
/// most recently completed click's target, for double-click timing. Reset
/// at every `TerminalGuard::resume()` call site (design D1 risk:
/// "crossterm believes a button is still held after resume") — a stale
/// press surviving a suspended shell/editor/scrollback run could otherwise
/// complete as a click on whatever happens to be under the pointer once the
/// TUI redraws.
#[derive(Debug, Clone, Default)]
pub struct MouseTracker {
    press: Option<PressState>,
    /// The most recently completed left-click's target entry and when it
    /// happened (`State::clock_ms`), used to detect the next click as a
    /// double-click (design D3: "second Down on the same row within ~400
    /// ms"). Cleared on a Ctrl+click, a non-entry click, or once consumed.
    last_click: Option<(PanelSide, OsString, u64)>,
}

#[derive(Debug, Clone, Copy)]
struct PressState {
    x: u16,
    y: u16,
    button: MouseButton,
    ctrl: bool,
    /// Set once `Drag` reports the pointer over a different cell than where
    /// it went down — a moved press resolves as a drag gesture on `Up`
    /// (mouse-panel-drag's territory, a non-goal here), never a click or a
    /// Ctrl-toggle (design D3: "Ctrl-click-without-movement detection").
    moved: bool,
}

impl MouseTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Discard all in-progress press/double-click state. Called at every
    /// `TerminalGuard::resume()` call site (design D1).
    pub fn reset(&mut self) {
        self.press = None;
        self.last_click = None;
    }

    fn begin_press(&mut self, x: u16, y: u16, button: MouseButton, ctrl: bool) {
        self.press = Some(PressState { x, y, button, ctrl, moved: false });
    }

    fn note_drag(&mut self, x: u16, y: u16, button: MouseButton) {
        if let Some(p) = &mut self.press {
            if p.button == button && (p.x != x || p.y != y) {
                p.moved = true;
            }
        }
    }

    /// Take the current press iff it was for `button`; `None` both when
    /// nothing is pressed and when the release button doesn't match the
    /// press (crossterm shouldn't deliver that, but a mismatched release is
    /// safer treated as "nothing to complete" than as a click).
    fn take_press(&mut self, button: MouseButton) -> Option<PressState> {
        match self.press.take() {
            Some(p) if p.button == button => Some(p),
            other => {
                self.press = other;
                None
            }
        }
    }
}

/// Which class of thing mouse events are honoured over right now — the
/// mode-gating table (design D5; mouse-input "Mouse is honoured only where
/// the key would be"). The viewer and built-in editor never reach this: the
/// caller dispatches their wheel-only handling directly from `state.phase`
/// (see the module doc comment).
enum Context {
    /// No overlay `map_mouse` understands owns the keyboard right now —
    /// panels, key bar, and menu-bar titles are all live.
    Panels,
    /// The F9 bar has a pull-down open: only its own titles/items are live;
    /// anything else closes it.
    PulldownOpen,
    /// A modal dialog that accepts button clicks only — file-op setup,
    /// running-job (conflict/error/progress), summary, or the quit-confirm
    /// overlay (which can appear over any of the others, hence checked
    /// first in `context`).
    DialogButtons,
    /// An overlay mouse-basics does not support yet (drive select, fuzzy
    /// jump, find file, user menu, theme picker, help, the file-action
    /// menu, the startup warning) or a phase with nothing clickable
    /// (splash, placeholder) — every event is ignored.
    Ignored,
}

fn context(state: &State) -> Context {
    // The quit-confirmation dialog can open above panels, the viewer, an
    // open menu, or any other modal dialog/overlay (application-shell
    // "Quit request keys and confirmation"), so it is checked before
    // everything else here too, mirroring `core::update`'s own precedence.
    if state.quit_confirm {
        return Context::DialogButtons;
    }
    if state.startup_warning.is_some()
        || state.drive_select.is_some()
        || state.fuzzy_jump.is_some()
        || state.find_file.is_some()
        || state.user_menu.is_some()
        || state.theme_picker.is_some()
        || state.help.is_some()
        || state.file_action_menu.is_some()
    {
        return Context::Ignored;
    }
    match &state.phase {
        UiPhase::FileOpSetup(_) | UiPhase::FileOpRunning { .. } | UiPhase::FileOpSummary(_) => Context::DialogButtons,
        UiPhase::Panels if state.menu.is_some() => Context::PulldownOpen,
        UiPhase::Panels => Context::Panels,
        // Splash/Placeholder have nothing clickable; Viewer/Editor are
        // never routed here at all (see the module doc comment) — kept as
        // an explicit arm rather than a catch-all so a future `UiPhase`
        // variant doesn't silently fall through to "ignored" unnoticed.
        UiPhase::Splash { .. } | UiPhase::Placeholder | UiPhase::Viewer(_) | UiPhase::Editor(_) => Context::Ignored,
    }
}

/// Translate one raw mouse event into a semantic [`Command`], or `None` when
/// this event has no effect — either because nothing under the pointer is
/// clickable, or because the current overlay doesn't accept mouse input at
/// all (mouse-input "Mouse is honoured only where the key would be"). Raw
/// coordinates and `crossterm::event::KeyModifiers` never leave this
/// function's body — every `Command` it returns carries entry names, panel
/// sides, slot numbers, or button identities only (mouse-input "Core
/// receives no coordinates").
pub fn map_mouse(event: MouseEvent, hitmap: &HitMap, tracker: &mut MouseTracker, state: &State) -> Option<Command> {
    match context(state) {
        Context::Ignored => {
            // An overlay that doesn't accept mouse input still shouldn't
            // let an in-progress press survive into whatever context comes
            // next (e.g. Help closing mid-drag): safest to drop it here.
            tracker.press = None;
            None
        }
        Context::DialogButtons => map_dialog_buttons(event, hitmap, tracker),
        Context::PulldownOpen => map_pulldown(event, hitmap, tracker),
        Context::Panels => map_panels(event, hitmap, tracker, state),
    }
}

fn map_dialog_buttons(event: MouseEvent, hitmap: &HitMap, tracker: &mut MouseTracker) -> Option<Command> {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            tracker.begin_press(event.column, event.row, MouseButton::Left, false);
            None
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            tracker.note_drag(event.column, event.row, MouseButton::Left);
            None
        }
        MouseEventKind::Up(MouseButton::Left) => {
            let press = tracker.take_press(MouseButton::Left)?;
            if press.moved {
                return None;
            }
            find_hit(&hitmap.dialog_buttons, event.column, event.row).map(Command::DialogButtonClick)
        }
        _ => None,
    }
}

fn map_pulldown(event: MouseEvent, hitmap: &HitMap, tracker: &mut MouseTracker) -> Option<Command> {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            tracker.begin_press(event.column, event.row, MouseButton::Left, false);
            None
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            tracker.note_drag(event.column, event.row, MouseButton::Left);
            None
        }
        MouseEventKind::Up(MouseButton::Left) => {
            let press = tracker.take_press(MouseButton::Left)?;
            if press.moved {
                return None;
            }
            let (x, y) = (event.column, event.row);
            if let Some(id) = find_hit(&hitmap.menu_titles, x, y) {
                return Some(Command::MenuTitleClick(id));
            }
            if let Some(index) = find_hit(&hitmap.menu_items, x, y) {
                return Some(Command::MenuItemClick(index));
            }
            // mouse-input "Key bar, menu bar, pull-down items, and dialog
            // buttons are clickable": "a click outside an open pull-down
            // SHALL close it".
            Some(Command::MenuClose)
        }
        _ => None,
    }
}

fn map_panels(event: MouseEvent, hitmap: &HitMap, tracker: &mut MouseTracker, state: &State) -> Option<Command> {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
            tracker.begin_press(event.column, event.row, MouseButton::Left, ctrl);
            None
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            tracker.note_drag(event.column, event.row, MouseButton::Left);
            None
        }
        MouseEventKind::Up(MouseButton::Left) => {
            let press = tracker.take_press(MouseButton::Left)?;
            resolve_panels_click(event.column, event.row, press, hitmap, tracker, state)
        }
        // Right-click opens the action menu straight on `Down` — unlike the
        // left button it has no drag or double-click meaning to
        // disambiguate, so there is nothing to wait for a matching `Up` to
        // resolve (mouse-input "Right-click opens the action menu"; design
        // D2/D4). Any in-progress left-button double-click chain is broken,
        // matching every other panels-context click resolution below.
        MouseEventKind::Down(MouseButton::Right) => resolve_right_click(event.column, event.row, hitmap, tracker),
        MouseEventKind::ScrollDown => resolve_wheel(event.column, event.row, 3, hitmap),
        MouseEventKind::ScrollUp => resolve_wheel(event.column, event.row, -3, hitmap),
        _ => None,
    }
}

fn resolve_panels_click(x: u16, y: u16, press: PressState, hitmap: &HitMap, tracker: &mut MouseTracker, state: &State) -> Option<Command> {
    if press.moved {
        // A drag that ends somewhere is `mouse-panel-drag`'s territory
        // (Non-Goal here) — never a click, so it doesn't chain into a
        // double-click either.
        tracker.last_click = None;
        return None;
    }

    if let Some(id) = find_hit(&hitmap.menu_titles, x, y) {
        tracker.last_click = None;
        return Some(Command::MenuTitleClick(id));
    }
    if let Some(slot) = find_hit(&hitmap.keybar, x, y) {
        tracker.last_click = None;
        return Some(Command::KeybarPress(slot));
    }

    for side in [PanelSide::Left, PanelSide::Right] {
        let panel = hitmap.panel(side);
        if let Some(name) = find_hit(&panel.rows, x, y) {
            return Some(resolve_entry_click(side, name, press.ctrl, tracker, state.clock_ms));
        }
        if hitmap::hit(panel.area, x, y) || hitmap::hit(panel.title, x, y) {
            tracker.last_click = None;
            return Some(Command::FocusPanel(side));
        }
    }

    tracker.last_click = None;
    None
}

/// A plain click focuses+moves (`ClickEntry` with `Plain`); a Ctrl+click
/// toggles selection in place (`ClickEntry` with `Ctrl`) and never chains
/// into a double-click; a second plain click on the same entry within
/// [`DOUBLE_CLICK_MS`] of the first is Enter instead (mouse-input "Click
/// focuses and places the cursor", "Ctrl+click toggles selection",
/// "Double-click acts as Enter").
fn resolve_entry_click(side: PanelSide, name: OsString, ctrl: bool, tracker: &mut MouseTracker, now_ms: u64) -> Command {
    if ctrl {
        tracker.last_click = None;
        return Command::ClickEntry { side, name, mods: ClickMods::Ctrl };
    }
    let is_double_click = matches!(&tracker.last_click, Some((s, n, t)) if *s == side && *n == name && now_ms.saturating_sub(*t) <= DOUBLE_CLICK_MS);
    if is_double_click {
        tracker.last_click = None;
        return Command::Enter;
    }
    tracker.last_click = Some((side, name.clone(), now_ms));
    Command::ClickEntry { side, name, mods: ClickMods::Plain }
}

/// A right-click on a panel row moves the cursor to that entry and opens
/// the file-action menu for it; a right-click elsewhere in the panels
/// context has nothing to open (mouse-input "Right-click opens the action
/// menu": "A right-click on an entry row SHALL move the cursor to that
/// entry and open the file-action menu for it"). `core::update`'s own
/// `handle_open_action_menu_at` resolves directory-vs-file menu contents
/// and selection scoping (design D4) — this layer only names the entry.
fn resolve_right_click(x: u16, y: u16, hitmap: &HitMap, tracker: &mut MouseTracker) -> Option<Command> {
    tracker.last_click = None;
    for side in [PanelSide::Left, PanelSide::Right] {
        if let Some(name) = find_hit(&hitmap.panel(side).rows, x, y) {
            return Some(Command::OpenActionMenuAt { side, name });
        }
    }
    None
}

fn resolve_wheel(x: u16, y: u16, delta: isize, hitmap: &HitMap) -> Option<Command> {
    for side in [PanelSide::Left, PanelSide::Right] {
        if hitmap::hit(hitmap.panel(side).area, x, y) {
            return Some(Command::ScrollPanel { side, delta });
        }
    }
    None
}

fn find_hit<T: Clone>(hits: &[(ratatui::layout::Rect, T)], x: u16, y: u16) -> Option<T> {
    hits.iter().find(|(r, _)| hitmap::hit(*r, x, y)).map(|(_, v)| v.clone())
}

#[cfg(test)]
mod tests;
