//! The pure data-flow core: `State`, `Command`, `Effect`, and `update`.
//!
//! `update(state, command) -> (state, Vec<Effect>)` is the single path all
//! state mutations flow through — key-derived commands and worker-produced
//! events alike. It performs no I/O, spawns no threads, and reads no clock;
//! callers supply the current time via [`Command::Tick`].

use std::ffi::OsString;
use std::path::PathBuf;

use crate::fs_ops::dialog::{FileOpSetup, RunningDialog};
use crate::fs_ops::{ConflictChoice, ConflictInfo, ErrorChoice, ErrorInfo, Job, JobKind, JobOutcome, ProgressInfo, SkippedItem, SourceItem};
use crate::listing::{Entry, EntryKind};
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
    /// Gathering input before a Copy/Move/Mkdir/Delete job is dispatched.
    FileOpSetup(FileOpSetup),
    /// A job is running on the worker thread; `dialog` shows progress, or a
    /// conflict/error the worker is blocked waiting on.
    FileOpRunning { source_dir: PathBuf, dest_dir: PathBuf, dialog: RunningDialog },
    /// End-of-job summary, shown only when the job skipped 1+ items.
    FileOpSummary(Vec<SkippedItem>),
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
    /// Re-read the active panel's directory — the recovery action offered
    /// after a listing failure, but harmless (and available) at any time.
    RetryListing,

    // Selection (Ins/+/-/*).
    ToggleSelectAtCursor,
    GroupSelectAll,
    GroupDeselectAll,
    InvertSelection,

    // F5/F6/F7/F8: enter the corresponding file-op setup dialog. No-ops if
    // there is nothing selected (Mkdir is always available).
    RequestCopy,
    RequestMove,
    RequestMkdir,
    RequestDelete,

    // FileOpSetup dialog interaction.
    FileOpInputChar(char),
    FileOpInputBackspace,
    FileOpConfirm,
    FileOpCancel,

    // FileOpRunning dialog interaction.
    FileOpConflictChoice(ConflictChoice),
    FileOpBeginRename,
    FileOpErrorChoice(ErrorChoice),
    FileOpCancelJob,

    // Worker-produced events, re-entering through the same `update` path.
    ListingChunk { panel: PanelSide, entries: Vec<Entry> },
    ListingComplete { panel: PanelSide, total: usize },
    ListingFailed { panel: PanelSide, message: String },
    JobProgress(ProgressInfo),
    JobConflict(ConflictInfo),
    JobError(ErrorInfo),
    JobDone { outcome: JobOutcome, source_dir: PathBuf, dest_dir: PathBuf },
}

/// A side-effect request. `update` only ever returns these; it never
/// performs them. The TUI event loop executes them (spawning worker
/// threads, exiting the process, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    StartListing { panel: PanelSide, path: PathBuf },
    Quit,
    RunJob(Job),
    CancelJob,
    SendConflictReply(ConflictChoice),
    SendErrorReply(ErrorChoice),
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

    // Listing events fold in identically regardless of phase — a background
    // listing thread has no notion of which dialog (if any) is on screen.
    if matches!(cmd, Command::ListingChunk { .. } | Command::ListingComplete { .. } | Command::ListingFailed { .. }) {
        apply_listing_event(&mut state, cmd);
        return (state, effects);
    }

    // File-op setup/running/summary phases (and the job events that drive
    // them) are handled uniformly here, independent of the
    // Splash/Placeholder/QuitConfirm/Panels phases below.
    if matches!(state.phase, UiPhase::FileOpSetup(_) | UiPhase::FileOpRunning { .. } | UiPhase::FileOpSummary(_))
        || matches!(cmd, Command::JobProgress(_) | Command::JobConflict(_) | Command::JobError(_) | Command::JobDone { .. })
    {
        effects.extend(handle_file_op(&mut state, cmd));
        return (state, effects);
    }

    match &state.phase {
        UiPhase::Splash { started_at_ms } => match cmd {
            Command::Tick(now) => {
                if now.saturating_sub(*started_at_ms) >= SPLASH_MIN_HOLD_MS {
                    state.phase = UiPhase::Panels;
                }
            }
            _ => {
                // Any other key-derived command dismisses the splash
                // immediately; the command itself is consumed here and
                // never reaches panel/command-line handling.
                state.phase = UiPhase::Panels;
            }
        },
        UiPhase::Placeholder => {}
        UiPhase::QuitConfirm => match cmd {
            Command::ConfirmQuit => effects.push(Effect::Quit),
            Command::CancelQuit => state.phase = UiPhase::Panels,
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
            Command::RetryListing => {
                let side = state.active;
                let path = state.panel(side).cwd.clone();
                effects.extend(begin_listing(&mut state, side, path));
            }
            Command::ToggleSelectAtCursor => state.panel_mut(state.active).toggle_selection_and_advance(),
            Command::GroupSelectAll => state.panel_mut(state.active).select_matching("*"),
            Command::GroupDeselectAll => state.panel_mut(state.active).deselect_matching("*"),
            Command::InvertSelection => state.panel_mut(state.active).invert_selection(),
            Command::RequestCopy => enter_file_op_setup(&mut state, JobKind::Copy),
            Command::RequestMove => enter_file_op_setup(&mut state, JobKind::Move),
            Command::RequestMkdir => enter_file_op_setup(&mut state, JobKind::Mkdir),
            Command::RequestDelete => enter_delete_confirm(&mut state),
            Command::Tick(_) => {}
            Command::ConfirmQuit | Command::CancelQuit | Command::Resize(..) => unreachable!("handled above"),
            _ => {}
        },
        UiPhase::FileOpSetup(_) | UiPhase::FileOpRunning { .. } | UiPhase::FileOpSummary(_) => unreachable!("handled above"),
    }

    (state, effects)
}

/// Which selectable entry(ies) an F5/F6/F8 request applies to: the explicit
/// selection if non-empty, else the single entry under the cursor.
fn active_selection_sources(state: &State) -> Vec<SourceItem> {
    let panel = state.active_panel();
    if !panel.selected.is_empty() {
        return panel
            .entries
            .iter()
            .filter(|e| panel.selected.contains(&e.name))
            .map(|e| SourceItem { original_name: e.name.clone(), path: panel.cwd.join(&e.name), is_dir: e.is_dir_like() })
            .collect();
    }
    match panel.selected() {
        Some(e) if e.kind != EntryKind::ParentDir => {
            vec![SourceItem { original_name: e.name.clone(), path: panel.cwd.join(&e.name), is_dir: e.is_dir_like() }]
        }
        _ => vec![],
    }
}

/// F5/F6/F7: enter the destination/name-input setup dialog. A no-op for
/// Copy/Move when there is nothing selected; Mkdir is always available.
fn enter_file_op_setup(state: &mut State, kind: JobKind) {
    let side = state.active;
    let source_dir = state.panel(side).cwd.clone();
    if kind == JobKind::Mkdir {
        state.phase = UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources: vec![], source_dir, input: String::new() });
        return;
    }
    let sources = active_selection_sources(state);
    if sources.is_empty() {
        return;
    }
    let prefill = state.panel(side.toggle()).cwd.display().to_string();
    state.phase = UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, source_dir, input: prefill });
}

/// F8: enter the delete-confirmation dialog. A no-op when there is nothing
/// selected.
fn enter_delete_confirm(state: &mut State) {
    let side = state.active;
    let source_dir = state.panel(side).cwd.clone();
    let sources = active_selection_sources(state);
    if sources.is_empty() {
        return;
    }
    let needs_second_confirm = sources.iter().any(|s| s.is_dir);
    state.phase = UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { sources, source_dir, needs_second_confirm, confirmed_once: false });
}

fn panels_matching(state: &State, dirs: &[&PathBuf]) -> Vec<PanelSide> {
    let mut out = Vec::new();
    for side in [PanelSide::Left, PanelSide::Right] {
        if dirs.iter().any(|d| &state.panel(side).cwd == *d) {
            out.push(side);
        }
    }
    out
}

fn handle_file_op(state: &mut State, cmd: Command) -> Vec<Effect> {
    let mut effects = Vec::new();
    match std::mem::replace(&mut state.phase, UiPhase::Panels) {
        UiPhase::FileOpSetup(setup) => {
            state.phase = handle_file_op_setup(setup, cmd, &mut effects);
        }
        UiPhase::FileOpRunning { source_dir, dest_dir, dialog } => {
            state.phase = handle_file_op_running(state, source_dir, dest_dir, dialog, cmd, &mut effects);
        }
        UiPhase::FileOpSummary(skipped) => {
            state.phase = match cmd {
                Command::FileOpConfirm | Command::FileOpCancel => UiPhase::Panels,
                _ => UiPhase::FileOpSummary(skipped),
            };
        }
        // A Job* event arrived outside FileOpRunning — shouldn't happen
        // given the single-active-job design, but drop it defensively
        // rather than lose the phase we were actually in.
        other => state.phase = other,
    }
    effects
}

fn handle_file_op_setup(setup: FileOpSetup, cmd: Command, effects: &mut Vec<Effect>) -> UiPhase {
    match setup {
        FileOpSetup::DestinationInput { kind, sources, source_dir, mut input } => match cmd {
            Command::FileOpInputChar(c) => {
                input.push(c);
                UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, source_dir, input })
            }
            Command::FileOpInputBackspace => {
                input.pop();
                UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, source_dir, input })
            }
            Command::FileOpCancel => UiPhase::Panels,
            Command::FileOpConfirm => {
                let trimmed = input.trim();
                if trimmed.is_empty() {
                    return UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, source_dir, input });
                }
                let job = match kind {
                    JobKind::Mkdir => Job {
                        kind,
                        sources: vec![],
                        source_dir: source_dir.clone(),
                        dest_dir: source_dir.clone(),
                        new_dir_name: Some(OsString::from(trimmed)),
                    },
                    _ => Job { kind, sources, source_dir: source_dir.clone(), dest_dir: PathBuf::from(trimmed), new_dir_name: None },
                };
                let running = UiPhase::FileOpRunning {
                    source_dir: job.source_dir.clone(),
                    dest_dir: job.dest_dir.clone(),
                    dialog: RunningDialog::Progress { kind: job.kind, progress: ProgressInfo::starting(0, 0) },
                };
                effects.push(Effect::RunJob(job));
                running
            }
            _ => UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, source_dir, input }),
        },
        FileOpSetup::DeleteConfirm { sources, source_dir, needs_second_confirm, confirmed_once } => match cmd {
            Command::FileOpCancel => UiPhase::Panels,
            Command::FileOpConfirm => {
                if needs_second_confirm && !confirmed_once {
                    UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { sources, source_dir, needs_second_confirm, confirmed_once: true })
                } else {
                    let job =
                        Job { kind: JobKind::Delete, sources, source_dir: source_dir.clone(), dest_dir: source_dir.clone(), new_dir_name: None };
                    let running = UiPhase::FileOpRunning {
                        source_dir: job.source_dir.clone(),
                        dest_dir: job.dest_dir.clone(),
                        dialog: RunningDialog::Progress { kind: JobKind::Delete, progress: ProgressInfo::starting(0, 0) },
                    };
                    effects.push(Effect::RunJob(job));
                    running
                }
            }
            _ => UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { sources, source_dir, needs_second_confirm, confirmed_once }),
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_file_op_running(
    state: &mut State,
    source_dir: PathBuf,
    dest_dir: PathBuf,
    dialog: RunningDialog,
    cmd: Command,
    effects: &mut Vec<Effect>,
) -> UiPhase {
    match cmd {
        Command::FileOpCancelJob => {
            effects.push(Effect::CancelJob);
            UiPhase::FileOpRunning { source_dir, dest_dir, dialog }
        }
        Command::FileOpConflictChoice(choice) => match dialog {
            RunningDialog::Conflict { kind, progress, .. } => {
                effects.push(Effect::SendConflictReply(choice));
                UiPhase::FileOpRunning { source_dir, dest_dir, dialog: RunningDialog::Progress { kind, progress } }
            }
            other => UiPhase::FileOpRunning { source_dir, dest_dir, dialog: other },
        },
        Command::FileOpBeginRename => match dialog {
            RunningDialog::Conflict { kind, info, progress, .. } => UiPhase::FileOpRunning {
                source_dir,
                dest_dir,
                dialog: RunningDialog::Conflict { kind, info, progress, rename_input: Some(String::new()) },
            },
            other => UiPhase::FileOpRunning { source_dir, dest_dir, dialog: other },
        },
        Command::FileOpInputChar(c) => match dialog {
            RunningDialog::Conflict { kind, info, progress, rename_input: Some(mut s) } => {
                s.push(c);
                UiPhase::FileOpRunning { source_dir, dest_dir, dialog: RunningDialog::Conflict { kind, info, progress, rename_input: Some(s) } }
            }
            other => UiPhase::FileOpRunning { source_dir, dest_dir, dialog: other },
        },
        Command::FileOpInputBackspace => match dialog {
            RunningDialog::Conflict { kind, info, progress, rename_input: Some(mut s) } => {
                s.pop();
                UiPhase::FileOpRunning { source_dir, dest_dir, dialog: RunningDialog::Conflict { kind, info, progress, rename_input: Some(s) } }
            }
            other => UiPhase::FileOpRunning { source_dir, dest_dir, dialog: other },
        },
        Command::FileOpConfirm => match dialog {
            RunningDialog::Conflict { kind, progress, rename_input: Some(name), .. } if !name.trim().is_empty() => {
                effects.push(Effect::SendConflictReply(ConflictChoice::Rename(OsString::from(name))));
                UiPhase::FileOpRunning { source_dir, dest_dir, dialog: RunningDialog::Progress { kind, progress } }
            }
            other => UiPhase::FileOpRunning { source_dir, dest_dir, dialog: other },
        },
        Command::FileOpCancel => match dialog {
            RunningDialog::Conflict { kind, info, progress, rename_input: Some(_) } => UiPhase::FileOpRunning {
                source_dir,
                dest_dir,
                dialog: RunningDialog::Conflict { kind, info, progress, rename_input: None },
            },
            other => UiPhase::FileOpRunning { source_dir, dest_dir, dialog: other },
        },
        Command::FileOpErrorChoice(choice) => match dialog {
            RunningDialog::Error { kind, progress, .. } => {
                effects.push(Effect::SendErrorReply(choice));
                UiPhase::FileOpRunning { source_dir, dest_dir, dialog: RunningDialog::Progress { kind, progress } }
            }
            other => UiPhase::FileOpRunning { source_dir, dest_dir, dialog: other },
        },
        Command::JobProgress(info) => match dialog {
            RunningDialog::Progress { kind, .. } => {
                UiPhase::FileOpRunning { source_dir, dest_dir, dialog: RunningDialog::Progress { kind, progress: info } }
            }
            other => UiPhase::FileOpRunning { source_dir, dest_dir, dialog: other },
        },
        Command::JobConflict(info) => {
            let kind = dialog.kind();
            let progress = dialog.progress().clone();
            UiPhase::FileOpRunning { source_dir, dest_dir, dialog: RunningDialog::Conflict { kind, info, progress, rename_input: None } }
        }
        Command::JobError(info) => {
            let kind = dialog.kind();
            let progress = dialog.progress().clone();
            UiPhase::FileOpRunning { source_dir, dest_dir, dialog: RunningDialog::Error { kind, info, progress } }
        }
        Command::JobDone { outcome, .. } => {
            let skipped = match outcome {
                JobOutcome::Completed { skipped } => skipped,
                JobOutcome::Cancelled { skipped } => skipped,
            };
            for side in panels_matching(state, &[&source_dir, &dest_dir]) {
                let path = state.panel(side).cwd.clone();
                effects.extend(begin_listing(state, side, path));
            }
            if skipped.is_empty() {
                UiPhase::Panels
            } else {
                UiPhase::FileOpSummary(skipped)
            }
        }
        _ => UiPhase::FileOpRunning { source_dir, dest_dir, dialog },
    }
}

fn handle_enter(state: &mut State) -> Vec<Effect> {
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
            p.reconcile_selection();
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

    fn file_entry(name: &str, size: u64) -> Entry {
        Entry { name: OsString::from(name), kind: EntryKind::File, size, modified: None }
    }

    fn dir_entry(name: &str) -> Entry {
        Entry { name: OsString::from(name), kind: EntryKind::Directory, size: 0, modified: None }
    }

    #[test]
    fn request_copy_with_no_selection_and_no_cursor_entry_is_noop() {
        let state = test_state(UiPhase::Panels);
        let (state, effects) = update(state, Command::RequestCopy);
        assert_eq!(state.phase, UiPhase::Panels);
        assert!(effects.is_empty());
    }

    #[test]
    fn request_copy_uses_cursor_entry_when_nothing_explicitly_selected() {
        let mut state = test_state(UiPhase::Panels);
        state.left.entries = vec![file_entry("a.txt", 10)];
        let (state, _) = update(state, Command::RequestCopy);
        match state.phase {
            UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, .. }) => {
                assert_eq!(kind, JobKind::Copy);
                assert_eq!(sources.len(), 1);
                assert_eq!(sources[0].original_name, OsString::from("a.txt"));
            }
            other => panic!("expected FileOpSetup::DestinationInput, got {other:?}"),
        }
    }

    #[test]
    fn request_copy_prefills_destination_with_opposite_panel_cwd() {
        let mut state = test_state(UiPhase::Panels);
        state.left.entries = vec![file_entry("a.txt", 10)];
        let (state, _) = update(state, Command::RequestCopy);
        match state.phase {
            UiPhase::FileOpSetup(FileOpSetup::DestinationInput { input, .. }) => {
                assert_eq!(input, PathBuf::from("/right").display().to_string());
            }
            other => panic!("expected FileOpSetup::DestinationInput, got {other:?}"),
        }
    }

    #[test]
    fn request_copy_uses_explicit_selection_over_cursor() {
        let mut state = test_state(UiPhase::Panels);
        state.left.entries = vec![file_entry("a.txt", 1), file_entry("b.txt", 2)];
        state.left.selected.insert(OsString::from("b.txt"));
        let (state, _) = update(state, Command::RequestCopy);
        match state.phase {
            UiPhase::FileOpSetup(FileOpSetup::DestinationInput { sources, .. }) => {
                assert_eq!(sources.len(), 1);
                assert_eq!(sources[0].original_name, OsString::from("b.txt"));
            }
            other => panic!("expected FileOpSetup::DestinationInput, got {other:?}"),
        }
    }

    #[test]
    fn request_mkdir_is_always_available_even_with_empty_panel() {
        let state = test_state(UiPhase::Panels);
        let (state, _) = update(state, Command::RequestMkdir);
        assert!(matches!(state.phase, UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind: JobKind::Mkdir, .. })));
    }

    #[test]
    fn destination_input_typing_and_confirm_starts_job() {
        let mut state = test_state(UiPhase::Panels);
        state.left.entries = vec![file_entry("a.txt", 1)];
        let (state, _) = update(state, Command::RequestCopy);
        let (state, _) = update(state, Command::FileOpInputBackspace); // backspace on prefilled input
        let (state, _) = update(state, Command::FileOpInputChar('X'));
        let (state, effects) = update(state, Command::FileOpConfirm);
        match &state.phase {
            UiPhase::FileOpRunning { source_dir, dest_dir, dialog: RunningDialog::Progress { kind, .. } } => {
                assert_eq!(*kind, JobKind::Copy);
                assert_eq!(source_dir, &PathBuf::from("/left"));
                assert!(dest_dir.to_string_lossy().ends_with('X'));
            }
            other => panic!("expected FileOpRunning Progress, got {other:?}"),
        }
        assert!(matches!(effects.as_slice(), [Effect::RunJob(_)]));
    }

    #[test]
    fn destination_input_confirm_with_empty_input_is_noop() {
        let mut state = test_state(UiPhase::Panels);
        state.left.entries = vec![file_entry("a.txt", 1)];
        let (state, _) = update(state, Command::RequestMkdir);
        let (state, effects) = update(state, Command::FileOpConfirm);
        assert!(effects.is_empty());
        assert!(matches!(state.phase, UiPhase::FileOpSetup(FileOpSetup::DestinationInput { .. })));
    }

    #[test]
    fn escape_cancels_destination_input_back_to_panels() {
        let mut state = test_state(UiPhase::Panels);
        state.left.entries = vec![file_entry("a.txt", 1)];
        let (state, _) = update(state, Command::RequestCopy);
        let (state, effects) = update(state, Command::FileOpCancel);
        assert_eq!(state.phase, UiPhase::Panels);
        assert!(effects.is_empty());
    }

    #[test]
    fn delete_confirm_requires_second_confirmation_for_a_directory() {
        let mut state = test_state(UiPhase::Panels);
        state.left.entries = vec![dir_entry("sub")];
        let (state, effects) = update(state, Command::RequestDelete);
        assert!(effects.is_empty());
        assert!(matches!(
            state.phase,
            UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { needs_second_confirm: true, confirmed_once: false, .. })
        ));

        let (state, effects) = update(state, Command::FileOpConfirm);
        assert!(effects.is_empty(), "first confirm just arms the second confirmation");
        assert!(matches!(
            state.phase,
            UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { needs_second_confirm: true, confirmed_once: true, .. })
        ));

        let (state, effects) = update(state, Command::FileOpConfirm);
        assert!(matches!(effects.as_slice(), [Effect::RunJob(Job { kind: JobKind::Delete, .. })]));
        assert!(matches!(state.phase, UiPhase::FileOpRunning { .. }));
    }

    #[test]
    fn delete_confirm_single_file_needs_only_one_confirmation() {
        let mut state = test_state(UiPhase::Panels);
        state.left.entries = vec![file_entry("a.txt", 1)];
        let (state, _) = update(state, Command::RequestDelete);
        let (state, effects) = update(state, Command::FileOpConfirm);
        assert!(matches!(effects.as_slice(), [Effect::RunJob(_)]));
        assert!(matches!(state.phase, UiPhase::FileOpRunning { .. }));
    }

    fn running_progress_state(kind: JobKind, source_dir: &str, dest_dir: &str) -> State {
        let mut state = test_state(UiPhase::FileOpRunning {
            source_dir: PathBuf::from(source_dir),
            dest_dir: PathBuf::from(dest_dir),
            dialog: RunningDialog::Progress { kind, progress: ProgressInfo::starting(3, 30) },
        });
        state.left.cwd = PathBuf::from(source_dir);
        state.right.cwd = PathBuf::from(dest_dir);
        state
    }

    #[test]
    fn job_progress_event_updates_progress_dialog() {
        let state = running_progress_state(JobKind::Copy, "/left", "/right");
        let info = ProgressInfo { files_done: 1, files_total: 3, bytes_done: 10, bytes_total: 30, current_file: OsString::from("a.txt") };
        let (state, effects) = update(state, Command::JobProgress(info.clone()));
        assert!(effects.is_empty());
        match state.phase {
            UiPhase::FileOpRunning { dialog: RunningDialog::Progress { progress, .. }, .. } => assert_eq!(progress, info),
            other => panic!("expected Progress dialog, got {other:?}"),
        }
    }

    #[test]
    fn job_conflict_event_switches_to_conflict_dialog_and_reply_returns_to_progress() {
        let state = running_progress_state(JobKind::Copy, "/left", "/right");
        let info = ConflictInfo {
            source_name: OsString::from("a.txt"),
            source_size: 1,
            source_modified: None,
            target_path: PathBuf::from("/right/a.txt"),
            target_size: 2,
            target_modified: None,
        };
        let (state, _) = update(state, Command::JobConflict(info.clone()));
        assert!(matches!(state.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Conflict { .. }, .. }));

        let (state, effects) = update(state, Command::FileOpConflictChoice(ConflictChoice::Overwrite));
        assert_eq!(effects, vec![Effect::SendConflictReply(ConflictChoice::Overwrite)]);
        assert!(matches!(state.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Progress { .. }, .. }));
    }

    #[test]
    fn conflict_rename_flow_composes_a_name_then_replies() {
        let state = running_progress_state(JobKind::Copy, "/left", "/right");
        let info = ConflictInfo {
            source_name: OsString::from("a.txt"),
            source_size: 1,
            source_modified: None,
            target_path: PathBuf::from("/right/a.txt"),
            target_size: 2,
            target_modified: None,
        };
        let (state, _) = update(state, Command::JobConflict(info));
        let (state, _) = update(state, Command::FileOpBeginRename);
        let (state, _) = update(state, Command::FileOpInputChar('b'));
        let (state, _) = update(state, Command::FileOpInputChar('.'));
        let (state, _) = update(state, Command::FileOpInputChar('c'));
        let (state, effects) = update(state, Command::FileOpConfirm);
        assert_eq!(effects, vec![Effect::SendConflictReply(ConflictChoice::Rename(OsString::from("b.c")))]);
        assert!(matches!(state.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Progress { .. }, .. }));
    }

    #[test]
    fn job_error_event_switches_to_error_dialog_and_reply_returns_to_progress() {
        let state = running_progress_state(JobKind::Delete, "/left", "/left");
        let info = ErrorInfo { path: PathBuf::from("/left/a.txt"), message: "permission denied".to_string() };
        let (state, _) = update(state, Command::JobError(info));
        assert!(matches!(state.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Error { .. }, .. }));
        let (state, effects) = update(state, Command::FileOpErrorChoice(ErrorChoice::Skip));
        assert_eq!(effects, vec![Effect::SendErrorReply(ErrorChoice::Skip)]);
        assert!(matches!(state.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Progress { .. }, .. }));
    }

    #[test]
    fn job_cancel_sends_cancel_effect_without_leaving_progress_dialog() {
        let state = running_progress_state(JobKind::Copy, "/left", "/right");
        let (state, effects) = update(state, Command::FileOpCancelJob);
        assert_eq!(effects, vec![Effect::CancelJob]);
        assert!(matches!(state.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Progress { .. }, .. }));
    }

    #[test]
    fn job_done_with_no_skips_rereads_matching_panels_and_returns_to_panels() {
        let state = running_progress_state(JobKind::Copy, "/left", "/right");
        let (state, effects) = update(
            state,
            Command::JobDone { outcome: JobOutcome::Completed { skipped: vec![] }, source_dir: PathBuf::from("/left"), dest_dir: PathBuf::from("/right") },
        );
        assert_eq!(state.phase, UiPhase::Panels);
        assert_eq!(
            effects,
            vec![
                Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left") },
                Effect::StartListing { panel: PanelSide::Right, path: PathBuf::from("/right") },
            ]
        );
    }

    #[test]
    fn job_done_with_skips_shows_summary_instead_of_panels() {
        let mut state = test_state(UiPhase::FileOpRunning {
            source_dir: PathBuf::from("/left"),
            dest_dir: PathBuf::from("/left"),
            dialog: RunningDialog::Progress { kind: JobKind::Delete, progress: ProgressInfo::starting(3, 30) },
        });
        state.left.cwd = PathBuf::from("/left");
        state.right.cwd = PathBuf::from("/right"); // unaffected panel: must not be re-read
        let skipped = vec![SkippedItem { path: PathBuf::from("/left/a.txt"), reason: "denied".to_string() }];
        let (state, effects) = update(
            state,
            Command::JobDone { outcome: JobOutcome::Completed { skipped: skipped.clone() }, source_dir: PathBuf::from("/left"), dest_dir: PathBuf::from("/left") },
        );
        assert_eq!(state.phase, UiPhase::FileOpSummary(skipped));
        assert_eq!(effects, vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left") }]);

        let (state, _) = update(state, Command::FileOpConfirm);
        assert_eq!(state.phase, UiPhase::Panels);
    }

    #[test]
    fn job_done_only_rereads_panels_whose_cwd_matches_source_or_dest() {
        let mut state = test_state(UiPhase::FileOpRunning {
            source_dir: PathBuf::from("/left"),
            dest_dir: PathBuf::from("/somewhere/else"),
            dialog: RunningDialog::Progress { kind: JobKind::Copy, progress: ProgressInfo::starting(1, 1) },
        });
        state.left.cwd = PathBuf::from("/left");
        state.right.cwd = PathBuf::from("/right"); // does not match dest_dir
        let (_, effects) = update(
            state,
            Command::JobDone {
                outcome: JobOutcome::Completed { skipped: vec![] },
                source_dir: PathBuf::from("/left"),
                dest_dir: PathBuf::from("/somewhere/else"),
            },
        );
        assert_eq!(effects, vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left") }]);
    }

    #[test]
    fn listing_complete_reconciles_selection_against_fresh_entries() {
        let mut state = test_state(UiPhase::Panels);
        state.left.selected.insert(OsString::from("gone.txt"));
        state.left.selected.insert(OsString::from("stays.txt"));
        state.left.entries = vec![file_entry("stays.txt", 1)];
        let (state, _) = update(state, Command::ListingComplete { panel: PanelSide::Left, total: 1 });
        assert_eq!(state.left.selected, std::collections::HashSet::from([OsString::from("stays.txt")]));
    }

    #[test]
    fn selection_commands_operate_on_active_panel() {
        let mut state = test_state(UiPhase::Panels);
        state.left.entries = vec![file_entry("a.txt", 1), file_entry("b.txt", 2)];
        let (state, _) = update(state, Command::ToggleSelectAtCursor);
        assert!(state.left.selected.contains(&OsString::from("a.txt")));
        let (state, _) = update(state, Command::InvertSelection);
        assert_eq!(state.left.selected, std::collections::HashSet::from([OsString::from("b.txt")]));
        let (state, _) = update(state, Command::GroupSelectAll);
        assert_eq!(state.left.selected.len(), 2);
        let (state, _) = update(state, Command::GroupDeselectAll);
        assert!(state.left.selected.is_empty());
    }

    #[test]
    fn retry_listing_reissues_start_listing_for_active_panel() {
        let mut state = test_state(UiPhase::Panels);
        state.left.last_error = Some("boom".to_string());
        state.left.cwd = PathBuf::from("/left");
        let (state, effects) = update(state, Command::RetryListing);
        assert_eq!(effects, vec![Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/left") }]);
        assert!(state.left.last_error.is_none());
    }
}
