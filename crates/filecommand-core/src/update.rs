//! The pure data-flow core: `State`, `Command`, `Effect`, and `update`.
//!
//! `update(state, command) -> (state, Vec<Effect>)` is the single path all
//! state mutations flow through — key-derived commands and worker-produced
//! events alike. It performs no I/O, spawns no threads, and reads no clock;
//! callers supply the current time via [`Command::Tick`].

use std::path::PathBuf;

use crate::listing::Entry;
use crate::panel::{CursorMove, PanelState};
use crate::theme::Theme;

pub const MIN_COLS: u16 = 80;
pub const MIN_ROWS: u16 = 24;
pub const SPLASH_MIN_HOLD_MS: u64 = 800;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PanelSide {
    Left,
    Right,
}

impl PanelSide {
    pub fn toggle(self) -> PanelSide {
        match self {
            PanelSide::Left => PanelSide::Right,
            PanelSide::Right => PanelSide::Left,
        }
    }
}

/// Top-level UI phase. Governs how commands are interpreted by `update`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiPhase {
    Splash { started_at_ms: u64 },
    Panels,
    QuitConfirm,
    /// Terminal is below the 80x24 minimum. Growing back always resolves to
    /// `Panels`, never back to `Splash`.
    Placeholder,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    pub left: PanelState,
    pub right: PanelState,
    pub active: PanelSide,
    pub command_line: String,
    pub phase: UiPhase,
    pub theme: Theme,
    pub term_size: (u16, u16),
}

impl State {
    pub fn panel(&self, side: PanelSide) -> &PanelState {
        match side {
            PanelSide::Left => &self.left,
            PanelSide::Right => &self.right,
        }
    }

    pub fn panel_mut(&mut self, side: PanelSide) -> &mut PanelState {
        match side {
            PanelSide::Left => &mut self.left,
            PanelSide::Right => &mut self.right,
        }
    }

    pub fn active_panel(&self) -> &PanelState {
        self.panel(self.active)
    }

    fn too_small(size: (u16, u16)) -> bool {
        size.0 < MIN_COLS || size.1 < MIN_ROWS
    }

    /// Build the initial state and the effects needed to kick off both
    /// panels' first directory listings. `show_splash` should already
    /// account for both the config's `splash` flag and `--nosplash`
    /// (flag wins).
    pub fn initial(
        theme: Theme,
        term_size: (u16, u16),
        now_ms: u64,
        left_cwd: PathBuf,
        right_cwd: PathBuf,
        show_splash: bool,
    ) -> (State, Vec<Effect>) {
        let phase = if Self::too_small(term_size) {
            UiPhase::Placeholder
        } else if show_splash {
            UiPhase::Splash { started_at_ms: now_ms }
        } else {
            UiPhase::Panels
        };
        let state = State {
            left: PanelState::new(left_cwd.clone()),
            right: PanelState::new(right_cwd.clone()),
            active: PanelSide::Left,
            command_line: String::new(),
            phase,
            theme,
            term_size,
        };
        let effects = vec![
            Effect::StartListing { panel: PanelSide::Left, path: left_cwd },
            Effect::StartListing { panel: PanelSide::Right, path: right_cwd },
        ];
        (state, effects)
    }
}

/// Key-derived and worker-produced inputs to `update`. Both flow through the
/// same path — worker events are not special-cased at the top level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    MoveCursor(CursorMove),
    ToggleActivePanel,
    Enter,
    ParentDir,
    RequestQuit,
    ConfirmQuit,
    CancelQuit,
    Resize(u16, u16),
    /// Current time in ms, supplied by the TUI's injected `Clock`.
    Tick(u64),

    // Worker-produced events, re-entering through the same `update` path.
    ListingChunk { panel: PanelSide, entries: Vec<Entry> },
    ListingComplete { panel: PanelSide, total: usize },
    ListingFailed { panel: PanelSide, message: String },
}

/// A side-effect request. `update` only ever returns these; it never
/// performs them. The TUI event loop executes them (spawning worker
/// threads, exiting the process, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    StartListing { panel: PanelSide, path: PathBuf },
    Quit,
}

/// The pure state transition. Equal `(state, command)` always yields equal
/// `(state, Vec<Effect>)`.
pub fn update(mut state: State, cmd: Command) -> (State, Vec<Effect>) {
    let mut effects = Vec::new();

    // Resize is handled uniformly regardless of phase.
    if let Command::Resize(w, h) = cmd {
        state.term_size = (w, h);
        let too_small = State::too_small((w, h));
        state.phase = match (&state.phase, too_small) {
            (UiPhase::Placeholder, false) => UiPhase::Panels,
            (UiPhase::Placeholder, true) => UiPhase::Placeholder,
            (_, true) => UiPhase::Placeholder,
            (other, false) => other.clone(),
        };
        return (state, effects);
    }

    match &state.phase {
        UiPhase::Splash { started_at_ms } => match cmd {
            Command::Tick(now) => {
                if now.saturating_sub(*started_at_ms) >= SPLASH_MIN_HOLD_MS {
                    state.phase = UiPhase::Panels;
                }
            }
            Command::ListingChunk { .. } | Command::ListingComplete { .. } | Command::ListingFailed { .. } => {
                apply_listing_event(&mut state, cmd);
            }
            _ => {
                // Any other key-derived command dismisses the splash
                // immediately; the command itself is consumed here and
                // never reaches panel/command-line handling.
                state.phase = UiPhase::Panels;
            }
        },
        UiPhase::Placeholder => match cmd {
            Command::ListingChunk { .. } | Command::ListingComplete { .. } | Command::ListingFailed { .. } => {
                apply_listing_event(&mut state, cmd);
            }
            _ => {}
        },
        UiPhase::QuitConfirm => match cmd {
            Command::ConfirmQuit => effects.push(Effect::Quit),
            Command::CancelQuit => state.phase = UiPhase::Panels,
            Command::ListingChunk { .. } | Command::ListingComplete { .. } | Command::ListingFailed { .. } => {
                apply_listing_event(&mut state, cmd);
            }
            _ => {}
        },
        UiPhase::Panels => match cmd {
            Command::MoveCursor(m) => state.panel_mut(state.active).move_cursor(m),
            Command::ToggleActivePanel => state.active = state.active.toggle(),
            Command::Enter => effects.extend(handle_enter(&mut state)),
            Command::ParentDir => {
                let side = state.active;
                effects.extend(handle_parent(&mut state, side));
            }
            Command::RequestQuit => state.phase = UiPhase::QuitConfirm,
            Command::Tick(_) => {}
            Command::ListingChunk { .. } | Command::ListingComplete { .. } | Command::ListingFailed { .. } => {
                apply_listing_event(&mut state, cmd);
            }
            Command::ConfirmQuit | Command::CancelQuit | Command::Resize(..) => unreachable!("handled above"),
        },
    }

    (state, effects)
}

fn handle_enter(state: &mut State) -> Vec<Effect> {
    use crate::listing::EntryKind;
    let side = state.active;
    let Some(entry) = state.panel(side).selected() else { return vec![] };
    match entry.kind {
        EntryKind::File => vec![],
        EntryKind::ParentDir => handle_parent(state, side),
        EntryKind::Directory => {
            let new_dir = state.panel(side).cwd.join(&entry.name);
            begin_listing(state, side, new_dir)
        }
    }
}

fn handle_parent(state: &mut State, side: PanelSide) -> Vec<Effect> {
    match crate::panel::parent_path(&state.panel(side).cwd) {
        Some(parent) => begin_listing(state, side, parent),
        None => vec![],
    }
}

fn begin_listing(state: &mut State, side: PanelSide, path: PathBuf) -> Vec<Effect> {
    state.panel_mut(side).begin_new_listing(path.clone());
    vec![Effect::StartListing { panel: side, path }]
}

fn apply_listing_event(state: &mut State, cmd: Command) {
    match cmd {
        Command::ListingChunk { panel, entries } => {
            let p = state.panel_mut(panel);
            for e in entries {
                p.insert_streamed(e);
            }
            if let crate::panel::ListingProgress::Streaming { count } = &mut p.progress {
                *count = p.entries.len();
            }
        }
        Command::ListingComplete { panel, total } => {
            let p = state.panel_mut(panel);
            p.progress = crate::panel::ListingProgress::Complete { count: total };
            p.clamp_cursor();
        }
        Command::ListingFailed { panel, message } => {
            let p = state.panel_mut(panel);
            p.progress = crate::panel::ListingProgress::Complete { count: p.entries.len() };
            p.last_error = Some(message);
        }
        _ => unreachable!("apply_listing_event only called for listing events"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listing::{Entry, EntryKind};
    use std::ffi::OsString;

    fn test_state(phase: UiPhase) -> State {
        State {
            left: PanelState::new(PathBuf::from("/left")),
            right: PanelState::new(PathBuf::from("/right")),
            active: PanelSide::Left,
            command_line: String::new(),
            phase,
            theme: Theme::classic(),
            term_size: (80, 24),
        }
    }

    #[test]
    fn update_is_deterministic() {
        let s1 = test_state(UiPhase::Panels);
        let s2 = test_state(UiPhase::Panels);
        let (r1, e1) = update(s1, Command::ToggleActivePanel);
        let (r2, e2) = update(s2, Command::ToggleActivePanel);
        assert_eq!(r1, r2);
        assert_eq!(e1, e2);
    }

    #[test]
    fn directory_read_returns_intent_effect_without_io() {
        let mut state = test_state(UiPhase::Panels);
        state.left.entries = vec![Entry { name: OsString::from("sub"), kind: EntryKind::Directory, size: 0, modified: None }];
        let (state, effects) = update(state, Command::Enter);
        assert_eq!(effects, vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left/sub") }]);
        // The panel's own entries were reset locally — no filesystem was touched.
        assert!(state.left.entries.is_empty());
        assert!(matches!(state.left.progress, crate::panel::ListingProgress::Streaming { count: 0 }));
    }

    #[test]
    fn worker_events_reenter_through_update() {
        let state = test_state(UiPhase::Panels);
        let entries = vec![Entry { name: OsString::from("a"), kind: EntryKind::File, size: 1, modified: None }];
        let (state, effects) = update(state, Command::ListingChunk { panel: PanelSide::Left, entries: entries.clone() });
        assert!(effects.is_empty());
        assert_eq!(state.left.entries, entries);
        let (state, effects) = update(state, Command::ListingComplete { panel: PanelSide::Left, total: 1 });
        assert!(effects.is_empty());
        assert_eq!(state.left.progress, crate::panel::ListingProgress::Complete { count: 1 });
    }

    #[test]
    fn toggle_active_panel_is_exclusive() {
        let state = test_state(UiPhase::Panels);
        assert_eq!(state.active, PanelSide::Left);
        let (state, _) = update(state, Command::ToggleActivePanel);
        assert_eq!(state.active, PanelSide::Right);
        let (state, _) = update(state, Command::ToggleActivePanel);
        assert_eq!(state.active, PanelSide::Left);
    }

    #[test]
    fn enter_on_parent_dir_navigates_up_and_resets_cursor() {
        let mut state = test_state(UiPhase::Panels);
        state.left.cwd = PathBuf::from("/a/b");
        state.left.entries = vec![Entry::parent_dir()];
        state.left.cursor = 0;
        let (state, effects) = update(state, Command::Enter);
        assert_eq!(effects, vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/a") }]);
        assert_eq!(state.left.cursor, 0);
    }

    #[test]
    fn parent_nav_at_root_is_no_op() {
        let mut state = test_state(UiPhase::Panels);
        state.left.cwd = PathBuf::from("/");
        let (state, effects) = update(state, Command::ParentDir);
        assert!(effects.is_empty());
        assert_eq!(state.left.cwd, PathBuf::from("/"));
    }

    #[test]
    fn f10_raises_quit_confirm_and_confirm_quits() {
        let state = test_state(UiPhase::Panels);
        let (state, effects) = update(state, Command::RequestQuit);
        assert_eq!(state.phase, UiPhase::QuitConfirm);
        assert!(effects.is_empty());
        let (state, effects) = update(state, Command::ConfirmQuit);
        assert_eq!(effects, vec![Effect::Quit]);
        let _ = state;
    }

    #[test]
    fn quit_confirm_cancel_returns_to_panels() {
        let state = test_state(UiPhase::QuitConfirm);
        let (state, effects) = update(state, Command::CancelQuit);
        assert_eq!(state.phase, UiPhase::Panels);
        assert!(effects.is_empty());
    }

    #[test]
    fn splash_dismissed_immediately_by_key_before_min_hold() {
        let state = test_state(UiPhase::Splash { started_at_ms: 1_000 });
        let (state, effects) = update(state, Command::ToggleActivePanel);
        assert_eq!(state.phase, UiPhase::Panels);
        // The dismissing key was consumed, not forwarded: active panel must
        // still be Left (unaffected by the ToggleActivePanel command).
        assert_eq!(state.active, PanelSide::Left);
        assert!(effects.is_empty());
    }

    #[test]
    fn splash_auto_dismisses_after_min_hold_via_tick() {
        let state = test_state(UiPhase::Splash { started_at_ms: 1_000 });
        let (state, _) = update(state, Command::Tick(1_799));
        assert!(matches!(state.phase, UiPhase::Splash { .. }));
        let (state, _) = update(state, Command::Tick(1_800));
        assert_eq!(state.phase, UiPhase::Panels);
    }

    #[test]
    fn placeholder_below_min_and_grows_back_to_panels_never_splash() {
        let state = test_state(UiPhase::Splash { started_at_ms: 0 });
        let (state, _) = update(state, Command::Resize(40, 10));
        assert_eq!(state.phase, UiPhase::Placeholder);
        let (state, _) = update(state, Command::Resize(80, 24));
        assert_eq!(state.phase, UiPhase::Panels);
    }

    #[test]
    fn listing_chunk_during_splash_still_updates_panel_state() {
        let state = test_state(UiPhase::Splash { started_at_ms: 0 });
        let entries = vec![Entry { name: OsString::from("a"), kind: EntryKind::File, size: 0, modified: None }];
        let (state, _) = update(state, Command::ListingChunk { panel: PanelSide::Left, entries: entries.clone() });
        assert_eq!(state.left.entries, entries);
        assert!(matches!(state.phase, UiPhase::Splash { .. }));
    }

    #[test]
    fn initial_builds_start_listing_effects_for_both_panels() {
        let (state, effects) = State::initial(Theme::classic(), (80, 24), 0, PathBuf::from("/l"), PathBuf::from("/r"), true);
        assert_eq!(state.phase, UiPhase::Splash { started_at_ms: 0 });
        assert_eq!(
            effects,
            vec![
                Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/l") },
                Effect::StartListing { panel: PanelSide::Right, path: PathBuf::from("/r") },
            ]
        );
    }

    #[test]
    fn initial_below_min_size_skips_straight_to_placeholder() {
        let (state, _) = State::initial(Theme::classic(), (40, 10), 0, PathBuf::from("/l"), PathBuf::from("/r"), true);
        assert_eq!(state.phase, UiPhase::Placeholder);
    }

    #[test]
    fn initial_without_splash_starts_at_panels() {
        let (state, _) = State::initial(Theme::classic(), (80, 24), 0, PathBuf::from("/l"), PathBuf::from("/r"), false);
        assert_eq!(state.phase, UiPhase::Panels);
    }
}
