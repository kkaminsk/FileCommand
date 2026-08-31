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
use filecommand_core::fs_ops::JobKind;
use filecommand_core::update::{ClickMods, DropTarget};
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
    /// The verb/target last sent via `DragBegin`/`DragOver` for the
    /// in-progress drag (mouse-panel-drag "Drag lifecycle"; design D4): also
    /// doubles as "is a drag actually under way right now", since it is
    /// `Some` only between a real `DragBegin` and the drag ending (drop,
    /// cancel, or `reset()`) — a moved press that never started on an entry
    /// row (so no `DragBegin` was ever sent) leaves this `None` for the
    /// whole gesture. Comparing against it before each `DragOver` is what
    /// keeps an unchanged target from re-emitting every event (mouse-drag:
    /// "de-duplicated so an unchanged target doesn't re-emit every event").
    drag_sent: Option<(JobKind, Option<DropTarget>)>,
}

#[derive(Debug, Clone)]
struct PressState {
    x: u16,
    y: u16,
    button: MouseButton,
    ctrl: bool,
    /// Set once `Drag` reports the pointer over a different cell than where
    /// it went down — mouse-panel-drag's ≥ 1 cell threshold (design D2)
    /// reuses this verbatim rather than adding a coarser one. A moved press
    /// resolves as a drag gesture on `Up` (or, if it started on an entry
    /// row, already mid-drag by then via `DragBegin`/`DragOver`), never a
    /// click or a Ctrl-toggle (design D3: "Ctrl-click-without-movement
    /// detection").
    moved: bool,
    /// The entry (if any) the press landed on, resolved once at press time
    /// against the same hit map a drag beginning later needs — a drag can
    /// only ever begin "on an entry row" (mouse-drag "Drag lifecycle"), so a
    /// press on blank panel area, a title, or a tab/tree-node hit never
    /// starts one, however far it moves.
    origin_entry: Option<(PanelSide, OsString)>,
}

impl MouseTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Discard all in-progress press/double-click/drag state. Called at
    /// every `TerminalGuard::resume()` call site (design D1).
    pub fn reset(&mut self) {
        self.press = None;
        self.last_click = None;
        self.drag_sent = None;
    }

    fn begin_press(&mut self, x: u16, y: u16, button: MouseButton, ctrl: bool, origin_entry: Option<(PanelSide, OsString)>) {
        self.press = Some(PressState { x, y, button, ctrl, moved: false, origin_entry });
    }

    /// Marks the current press as moved once its cell differs from where it
    /// went down, returning whether *this* call is the one that made that
    /// transition (`false` -> `true`) — the exact event mouse-panel-drag's
    /// "moved at least one cell" threshold is crossed on, and so the one
    /// `resolve_panels_drag` must answer with `DragBegin` rather than
    /// `DragOver`.
    fn note_drag(&mut self, x: u16, y: u16, button: MouseButton) -> bool {
        if let Some(p) = &mut self.press {
            if p.button == button && (p.x != x || p.y != y) {
                let just_crossed = !p.moved;
                p.moved = true;
                return just_crossed;
            }
        }
        false
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
            tracker.begin_press(event.column, event.row, MouseButton::Left, false, None);
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
            tracker.begin_press(event.column, event.row, MouseButton::Left, false, None);
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
            let origin = press_origin(event.column, event.row, hitmap);
            tracker.begin_press(event.column, event.row, MouseButton::Left, ctrl, origin);
            None
        }
        MouseEventKind::Drag(MouseButton::Left) => resolve_panels_drag(event, hitmap, tracker, state, MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left) => {
            let press = tracker.take_press(MouseButton::Left)?;
            if press.moved {
                return resolve_drag_release(event, tracker, MouseButton::Left);
            }
            resolve_panels_click(event.column, event.row, press, hitmap, tracker, state)
        }
        // A right-button press is deferred exactly like a left-button one
        // now: only an `Up` that never moved resolves as opening the action
        // menu (mouse-input "Right-click opens the action menu"); movement
        // before release turns it into a Move-proposing drag instead
        // (mouse-drag "Verb selection": "a right-button drag ... SHALL
        // propose Move") — there is no way to know which of the two a press
        // will become until it either releases or moves. Any in-progress
        // left-button double-click chain is broken immediately on `Down`,
        // matching the immediate-on-`Down` behaviour this replaces.
        MouseEventKind::Down(MouseButton::Right) => {
            tracker.last_click = None;
            let origin = press_origin(event.column, event.row, hitmap);
            tracker.begin_press(event.column, event.row, MouseButton::Right, false, origin);
            None
        }
        MouseEventKind::Drag(MouseButton::Right) => resolve_panels_drag(event, hitmap, tracker, state, MouseButton::Right),
        MouseEventKind::Up(MouseButton::Right) => {
            let press = tracker.take_press(MouseButton::Right)?;
            if press.moved {
                return resolve_drag_release(event, tracker, MouseButton::Right);
            }
            resolve_right_click(event.column, event.row, hitmap, tracker)
        }
        MouseEventKind::ScrollDown => resolve_wheel(event.column, event.row, 3, hitmap),
        MouseEventKind::ScrollUp => resolve_wheel(event.column, event.row, -3, hitmap),
        _ => None,
    }
}

/// The entry (if any) under `(x, y)` in either panel's currently-drawn rows,
/// resolved once at press time so a drag beginning later already knows
/// which side/entry to freeze (mouse-drag "Drag lifecycle": items are
/// "the pressed entry", named here before any movement has happened).
fn press_origin(x: u16, y: u16, hitmap: &HitMap) -> Option<(PanelSide, OsString)> {
    for side in [PanelSide::Left, PanelSide::Right] {
        if let Some(name) = find_hit(&hitmap.panel(side).rows, x, y) {
            return Some((side, name));
        }
    }
    None
}

/// A `Drag` event over the panels (design D2/D4): the first one to cross
/// the ≥ 1 cell threshold begins the drag (`Command::DragBegin`), scoped to
/// the entry the press landed on — a no-op if the press didn't start on an
/// entry row, since mouse-drag's "Drag lifecycle" only ever begins "on an
/// entry row". Every later `Drag` event recomputes the proposed verb and a
/// fresh geometric target and, if either changed since the last one sent,
/// re-emits `Command::DragOver`, de-duplicated via `tracker.drag_sent`
/// (mouse-drag: "de-duplicated so an unchanged target doesn't re-emit every
/// event").
fn resolve_panels_drag(event: MouseEvent, hitmap: &HitMap, tracker: &mut MouseTracker, state: &State, button: MouseButton) -> Option<Command> {
    let just_crossed = tracker.note_drag(event.column, event.row, button);
    let press = tracker.press.as_ref()?;
    if press.button != button {
        return None;
    }
    let (side, name) = press.origin_entry.clone()?;
    let op = propose_verb(event.modifiers, button);
    if just_crossed {
        tracker.drag_sent = Some((op, None));
        return Some(Command::DragBegin { side, name, op });
    }
    let target = resolve_drop_target(event.column, event.row, hitmap, state);
    let candidate = (op, target);
    if tracker.drag_sent.as_ref() == Some(&candidate) {
        return None;
    }
    tracker.drag_sent = Some(candidate.clone());
    Some(Command::DragOver { op: candidate.0, target: candidate.1 })
}

/// The button released after the press had moved: ends the drag with
/// `Command::DragDrop` if one had actually begun — `tracker.drag_sent` is
/// `Some` only for the life of a real drag (see its own doc comment) — or
/// does nothing if the moved press never started on an entry row in the
/// first place (mouse-drag "Release on an invalid spot" only applies to a
/// real drag; a moved press with no drag is simply not a click, exactly as
/// before mouse-panel-drag). The verb is recomputed fresh from this release
/// event's own modifiers/button, never reused from the last `DragOver`
/// (mouse-drag "Verb selection": "recomputed ... of each drag and release
/// event").
fn resolve_drag_release(event: MouseEvent, tracker: &mut MouseTracker, button: MouseButton) -> Option<Command> {
    tracker.last_click = None;
    tracker.drag_sent.take()?;
    let op = propose_verb(event.modifiers, button);
    Some(Command::DragDrop { op })
}

/// mouse-drag "Verb selection" / design D1/D2: a plain or Ctrl-modified
/// left-button drag proposes Copy; a Shift-modified left-button drag or any
/// right-button drag proposes Move — Ctrl never proposes Move. Recomputed
/// fresh from this event's own modifiers/button every time it's called,
/// never cached across events, since no key event exists for a bare
/// modifier press or release (design D2).
fn propose_verb(modifiers: KeyModifiers, button: MouseButton) -> JobKind {
    if button == MouseButton::Right || modifiers.contains(KeyModifiers::SHIFT) {
        JobKind::Move
    } else {
        JobKind::Copy
    }
}

/// The raw geometric hit at `(x, y)` translated into core's `DropTarget`
/// vocabulary — never validated here (mouse-drag "Valid drop targets" is
/// entirely `core::update`'s job by design; this only reports what's
/// physically under the pointer this frame). Checked per side in the same
/// rows-then-tree-nodes-then-tabs-then-area/title order every side offers,
/// mirroring `resolve_panels_click`'s own per-side loop below.
fn resolve_drop_target(x: u16, y: u16, hitmap: &HitMap, state: &State) -> Option<DropTarget> {
    for side in [PanelSide::Left, PanelSide::Right] {
        let panel_hits = hitmap.panel(side);
        if let Some(name) = find_hit(&panel_hits.rows, x, y) {
            return Some(resolve_row_target(state, side, name));
        }
        if let Some(path) = find_hit(&panel_hits.tree_nodes, x, y) {
            return Some(DropTarget::TreeNode { side, path });
        }
        if let Some(index) = find_hit(&panel_hits.tabs, x, y) {
            return Some(DropTarget::Tab { side, index });
        }
        if hitmap::hit(panel_hits.area, x, y) || hitmap::hit(panel_hits.title, x, y) {
            return Some(DropTarget::PanelDir(side));
        }
    }
    None
}

/// A row hit resolves to `SubDir` (a subdirectory or the `..` row) or
/// `PanelDir` (any non-directory row — per `DropTarget::PanelDir`'s own doc
/// comment, "its title, blank body area, or any non-directory row all
/// resolve to this same variant") depending on what the panel's own,
/// already-loaded listing says about that name right now — the hit map
/// itself carries no entry-kind information, only the name.
fn resolve_row_target(state: &State, side: PanelSide, name: OsString) -> DropTarget {
    let is_dir_like = state.panel(side).entries.iter().find(|e| e.name == name).map(|e| e.is_dir_like()).unwrap_or(false);
    if is_dir_like {
        DropTarget::SubDir { side, name }
    } else {
        DropTarget::PanelDir(side)
    }
}

fn resolve_panels_click(x: u16, y: u16, press: PressState, hitmap: &HitMap, tracker: &mut MouseTracker, state: &State) -> Option<Command> {
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
