//! The pure data-flow core: `State`, `Command`, `Effect`, and `update`.
//!
//! `update(state, command) -> (state, Vec<Effect>)` is the single path all
//! state mutations flow through — key-derived commands and worker-produced
//! events alike. It performs no I/O, spawns no threads, and reads no clock;
//! callers supply the current time via [`Command::Tick`].

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::config;
use crate::drives::{self, DriveSelect};
use crate::external_editor::{self, EditorInvocation};
use crate::fs_ops::dialog::{FileOpSetup, RunningDialog};
use crate::fs_ops::{ConflictChoice, ConflictInfo, ErrorChoice, ErrorInfo, Job, JobKind, JobOutcome, ProgressInfo, SkippedItem, SourceItem};
use crate::info::InfoValues;
use crate::listing::{Entry, EntryKind, SortMode};
use crate::menu::{MenuAction, MenuId, MenuState};
use crate::panel::{CursorMove, DisplayMode, PanelState};
use crate::shell::{self, ShellConfig};
use crate::theme::Theme;
use crate::viewer::ViewerState;

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
///
/// The F9 menu and the drive-select dialog deliberately live *beside* the
/// phase (as `State::menu` / `State::drive_select`) rather than inside it:
/// they overlay the panels without replacing whatever phase is underneath.
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
    /// The F3 viewer, open full-screen in place of the panels (viewer:
    /// Frame-less full-screen chrome — "Viewer owns focus while open").
    Viewer(ViewerState),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    pub left: PanelState,
    pub right: PanelState,
    pub active: PanelSide,
    /// The live command-line buffer. Printable keys land here whenever a
    /// panel is focused and no dialog or quick-search owns them.
    pub command_line: String,
    /// Executed commands, oldest first.
    pub history: Vec<String>,
    /// Where Up/Down recall currently sits in `history`; `None` means "not
    /// recalling", which is the state every edit resets to.
    pub history_cursor: Option<usize>,
    /// The §4.7 type-ahead quick-search pattern. While this is `Some`, it —
    /// not the command line — consumes plain printable keys. Exactly one
    /// typing sink ever sees a given key.
    pub quick_search: Option<String>,
    /// The F9 menu overlay, or `None` when the bar is closed.
    pub menu: Option<MenuState>,
    /// The Alt+F1/F2 drive-select dialog, or `None` when closed.
    pub drive_select: Option<DriveSelect>,
    /// Shell program + PATHEXT, snapshotted from config/environment at
    /// startup so `update` stays a pure function of `State`.
    pub shell: ShellConfig,
    /// The `editor =` command, snapshotted from config at startup. `None`
    /// means unset (external-editor: Config-driven external editor
    /// command).
    pub editor: Option<String>,
    pub phase: UiPhase,
    pub theme: Theme,
    pub term_size: (u16, u16),
    /// Monotonic source for request/generation ids (`PanelState::info_request`,
    /// `DriveSelect::generation`) that let a worker reply be matched against
    /// the request that's still current, so an out-of-order completion from
    /// a superseded request is dropped instead of silently overwriting a
    /// fresher answer. See [`State::next_request_id`].
    pub request_seq: u64,
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

    /// The command-line prompt for the active panel, e.g. `C:\NORTON>`. It
    /// follows focus and directory changes because it is derived, never
    /// stored.
    pub fn prompt(&self) -> String {
        format!("{}>", self.active_panel().cwd.display())
    }

    /// A bare state with empty panels — the base every test builds on, and
    /// the shape `initial` fills in.
    pub fn empty(theme: Theme) -> State {
        State {
            left: PanelState::new(PathBuf::from("/")),
            right: PanelState::new(PathBuf::from("/")),
            active: PanelSide::Left,
            command_line: String::new(),
            history: Vec::new(),
            history_cursor: None,
            quick_search: None,
            menu: None,
            drive_select: None,
            shell: ShellConfig::default(),
            editor: None,
            phase: UiPhase::Panels,
            theme,
            term_size: (MIN_COLS, MIN_ROWS),
            request_seq: 0,
        }
    }

    fn too_small(size: (u16, u16)) -> bool {
        size.0 < MIN_COLS || size.1 < MIN_ROWS
    }

    /// Mint a new, never-repeated request id. Used to tag an outstanding
    /// async request (an Info query, a drive-label fetch) so that when its
    /// answer comes back, `update` can tell a still-current request from
    /// one a newer request has since superseded.
    fn next_request_id(&mut self) -> u64 {
        self.request_seq += 1;
        self.request_seq
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
            phase,
            term_size,
            ..State::empty(theme)
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
    /// Re-read a panel's directory (Ctrl+R). Also the recovery action
    /// offered after a listing failure, but available at any time.
    RereadPanel(PanelSide),

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

    // Command line.
    CommandLineChar(char),
    CommandLineBackspace,
    /// Esc: clears the buffer, which is the explicit mechanism that hands
    /// Up/Down back to the panel cursor.
    CommandLineClear,
    CommandLineHistoryPrev,
    CommandLineHistoryNext,
    /// Ctrl+Enter — paste the cursor entry's file name.
    PasteCursorName,
    /// Ctrl+] — paste the cursor entry's full path.
    PasteCursorPath,
    /// Ctrl+O — drop to the host terminal's scrollback.
    ShowScrollback,

    // Type-ahead quick search (§4.7), which competes with the command line
    // for printable keys.
    QuickSearchStart(char),
    QuickSearchChar(char),
    QuickSearchBackspace,
    QuickSearchEnd,

    // Sort modes (Ctrl+F3..Ctrl+F7).
    SetSortMode { side: PanelSide, mode: SortMode },

    // Ctrl+P inline quick filter (§4.7), scoped to the active panel.
    QuickFilterStart,
    QuickFilterChar(char),
    QuickFilterBackspace,
    QuickFilterEnd,

    // Panel tabs (§4.1/§4.7), scoped to the active panel.
    /// Ctrl+T.
    OpenTab,
    /// Ctrl+W.
    CloseTab,
    /// Alt+1..9 (one-based).
    SwitchTab(usize),

    // F9 menu overlay.
    MenuOpen,
    MenuClose,
    /// Esc: closes the pull-down but leaves the bar open; a second Esc
    /// closes the bar.
    MenuCollapse,
    MenuSelectPrev,
    MenuSelectNext,
    MenuPrevMenu,
    MenuNextMenu,
    MenuHotkey(char),
    MenuActivate,

    // Drive select (Alt+F1/F2).
    OpenDriveSelect(PanelSide),
    DriveListReady { target: PanelSide, drives: Vec<char> },
    DriveSelectMove(isize),
    DriveSelectConfirm,
    DriveSelectCancel,

    // Info display mode (Ctrl+L).
    ToggleInfoMode(PanelSide),

    // F3 viewer (M4).
    /// Open the viewer on the file under the active panel's cursor.
    RequestViewer,
    /// F10 in the viewer: close it and return focus to the panels.
    ViewerClose,
    /// F4 in the viewer: toggle text/hex mode.
    ViewerToggleMode,
    /// F2 in the viewer: toggle wrap/unwrap.
    ViewerToggleWrap,
    /// Move the top-of-screen anchor to this byte offset (clamped to the
    /// file length). Used for both forward paging and the backward-scan
    /// result, which the caller computes via [`crate::viewer::backward`]
    /// before issuing this command, keeping `update` itself I/O-free.
    ViewerSetTop(u64),
    /// Set the unwrap-mode horizontal scroll, in display columns.
    ViewerSetHScroll(usize),
    /// F7 in the viewer: open the search prompt.
    ViewerSearchStart,
    ViewerSearchChar(char),
    ViewerSearchBackspace,
    ViewerSearchCancel,
    /// Enter on the search prompt: run the search (via
    /// `Effect::RunViewerSearch`) for the typed pattern.
    ViewerSearchConfirm,

    // F4 external editor (M4).
    /// Launch the configured external editor on the file under the active
    /// panel's cursor.
    RequestExternalEditor,

    // Worker-produced events, re-entering through the same `update` path.
    ListingChunk { panel: PanelSide, entries: Vec<Entry> },
    ListingComplete { panel: PanelSide, total: usize },
    ListingFailed { panel: PanelSide, message: String },
    JobProgress(ProgressInfo),
    JobConflict(ConflictInfo),
    JobError(ErrorInfo),
    JobDone { outcome: JobOutcome, source_dir: PathBuf, dest_dir: PathBuf },
    DriveLabelResolved { target: PanelSide, letter: char, label: Option<String>, generation: u64 },
    InfoResolved { panel: PanelSide, path: PathBuf, request: u64, values: InfoValues },
    /// Reply to `Effect::OpenViewer`: the file opened at `file_len` bytes
    /// (viewer: Instant open).
    ViewerOpened { path: PathBuf, file_len: u64 },
    /// Reply to `Effect::OpenViewer`: the file could not be opened or
    /// mapped (§7 error policy — surfaced as an inline error, never a
    /// panic).
    ViewerOpenFailed { message: String },
    /// Reply to `Effect::RunViewerSearch` (viewer: F7 streaming search).
    /// `request` echoes the id `Effect::RunViewerSearch` was issued with, so
    /// a reply from a search superseded by a since-closed/reopened viewer
    /// session (`ViewerState::search_request`) is recognized as stale and
    /// dropped rather than applied to whatever viewer session happens to be
    /// open when it arrives.
    ViewerSearchResult { offset: Option<u64>, match_range: Option<(u64, u64)>, request: u64 },
    /// Reply to `Effect::RunExternalEditor` when the editor could not be
    /// spawned (external-editor: Editor spawn errors do not crash the app).
    ExternalEditorSpawnFailed { message: String },
}

/// A side-effect request. `update` only ever returns these; it never
/// performs them. The TUI event loop executes them (spawning worker
/// threads, suspending the terminal, exiting the process, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    StartListing { panel: PanelSide, path: PathBuf },
    Quit,
    RunJob(Job),
    CancelJob,
    SendConflictReply(ConflictChoice),
    SendErrorReply(ErrorChoice),
    /// Suspend the TUI, run this invocation on the real terminal, wait for a
    /// keypress, then restore and redraw.
    /// The invocation to run, plus the panel side to re-read once the TUI
    /// resumes — the shell spawn is inherently a TUI-side effect, so the
    /// re-read command has to travel with it rather than reaching `update`
    /// on its own (§ "Run command" — "the active panel is re-read").
    RunShellCommand(shell::Invocation, PanelSide),
    /// Leave the alternate screen to expose the host terminal's scrollback
    /// until any key is pressed.
    ShowScrollback,
    /// Rewrite `history.json` atomically with these entries.
    PersistHistory(Vec<String>),
    /// Read the logical-drive bitmask (cheap, synchronous) and feed the
    /// letters back as `DriveListReady` before the next paint.
    EnumerateDrives(PanelSide),
    /// Fetch one drive's volume label on a worker thread. `generation`
    /// travels with the result so a reply from a since-superseded dialog
    /// session can be told apart from the current one.
    FetchDriveLabel { target: PanelSide, letter: char, generation: u64 },
    /// Gather the Info panel's async values on a worker thread. `request`
    /// travels with the result so a reply from a since-superseded query can
    /// be told apart from the current one.
    QueryInfo { panel: PanelSide, path: PathBuf, request: u64 },
    /// Open `path` for the F3 viewer: map/open it and report back its
    /// length via `Command::ViewerOpened`, or the failure via
    /// `Command::ViewerOpenFailed`. Cheap enough to run synchronously
    /// before the next repaint, exactly like `EnumerateDrives` — O(1) per
    /// the "instant open" requirement, never a worker-thread round trip
    /// (viewer: Instant open).
    OpenViewer { path: PathBuf },
    /// Run the F7 streaming search from `start_offset` for `pattern`
    /// against `path`, reporting the match back via
    /// `Command::ViewerSearchResult` (viewer: F7 streaming search).
    /// `request` must be echoed back unchanged in the `ViewerSearchResult`
    /// reply so a stale reply can be told apart from the current search.
    RunViewerSearch { path: PathBuf, start_offset: u64, pattern: Vec<u8>, request: u64 },
    /// Suspend the TUI, launch the external editor per `EditorInvocation`,
    /// wait for it to exit, then restore and re-read the panel side that
    /// owned the cursor entry — the same suspend/restore seam
    /// `RunShellCommand` uses (design D7; external-editor: TUI suspend and
    /// restore, Synchronous wait and panel re-read).
    RunExternalEditor(EditorInvocation, PanelSide),
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

    // Async drive/Info results are applied only when they still describe
    // what is on screen; a stale answer is dropped, never rendered.
    match cmd {
        Command::DriveLabelResolved { target, letter, label, generation } => {
            if let Some(dialog) = &mut state.drive_select {
                if dialog.target == target && dialog.generation == generation {
                    dialog.apply_label(letter, label);
                }
            }
            return (state, effects);
        }
        Command::InfoResolved { panel, path, request, values } => {
            let p = state.panel_mut(panel);
            if p.display_mode == DisplayMode::Info && p.cwd == path && p.info_request == Some(request) {
                p.info = values;
            }
            return (state, effects);
        }
        _ => {}
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

    // The F3 viewer replaces the panels full-screen and owns input focus
    // while open (viewer: Frame-less full-screen chrome — "Viewer owns
    // focus while open"), so its commands are handled uniformly here.
    if matches!(state.phase, UiPhase::Viewer(_)) {
        effects.extend(handle_viewer(&mut state, cmd));
        return (state, effects);
    }

    // The drive-select dialog and the F9 menu are modal over the panels:
    // while one is open it claims the keys it understands.
    if state.drive_select.is_some() && is_drive_select_command(&cmd) {
        effects.extend(handle_drive_select(&mut state, cmd));
        return (state, effects);
    }
    if state.menu.is_some() && is_menu_command(&cmd) {
        let follow_up = handle_menu(&mut state, cmd);
        if let Some(next) = follow_up {
            // An activated item re-enters `update` as the command it stands
            // for, so a menu action and its keyboard shortcut share one
            // implementation.
            let (state, more) = update(state, next);
            return (state, more);
        }
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
            Command::RereadPanel(side) => {
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

            Command::CommandLineChar(c) => {
                state.command_line.push(c);
                state.history_cursor = None;
            }
            Command::CommandLineBackspace => {
                state.command_line.pop();
                state.history_cursor = None;
            }
            Command::CommandLineClear => {
                state.command_line.clear();
                state.history_cursor = None;
            }
            Command::CommandLineHistoryPrev => recall_history(&mut state, -1),
            Command::CommandLineHistoryNext => recall_history(&mut state, 1),
            Command::PasteCursorName => paste_cursor_entry(&mut state, false),
            Command::PasteCursorPath => paste_cursor_entry(&mut state, true),
            Command::ShowScrollback => effects.push(Effect::ShowScrollback),

            Command::QuickSearchStart(c) => {
                let mut pattern = String::new();
                pattern.push(c);
                jump_to_prefix(&mut state, &pattern);
                state.quick_search = Some(pattern);
            }
            Command::QuickSearchChar(c) => {
                if let Some(mut pattern) = state.quick_search.take() {
                    pattern.push(c);
                    jump_to_prefix(&mut state, &pattern);
                    state.quick_search = Some(pattern);
                }
            }
            Command::QuickSearchBackspace => {
                if let Some(mut pattern) = state.quick_search.take() {
                    pattern.pop();
                    // An empty pattern stays active rather than exiting
                    // type-ahead, and the cursor holds its position rather
                    // than re-jumping (type-ahead-jump "Backspace on a
                    // single-character pattern").
                    if !pattern.is_empty() {
                        jump_to_prefix(&mut state, &pattern);
                    }
                    state.quick_search = Some(pattern);
                }
            }
            Command::QuickSearchEnd => state.quick_search = None,

            Command::SetSortMode { side, mode } => state.panel_mut(side).set_sort_mode(mode),

            Command::QuickFilterStart => state.panel_mut(state.active).activate_quick_filter(),
            Command::QuickFilterChar(c) => state.panel_mut(state.active).quick_filter_push(c),
            Command::QuickFilterBackspace => state.panel_mut(state.active).quick_filter_backspace(),
            Command::QuickFilterEnd => state.panel_mut(state.active).clear_quick_filter(),

            Command::OpenTab => state.panel_mut(state.active).open_tab(),
            Command::CloseTab => state.panel_mut(state.active).close_tab(),
            Command::SwitchTab(n) => state.panel_mut(state.active).switch_tab(n),

            Command::MenuOpen => state.menu = Some(MenuState::opened()),
            Command::OpenDriveSelect(side) => effects.push(Effect::EnumerateDrives(side)),
            Command::DriveListReady { target, drives } => {
                let current = drives::drive_letter_of(&state.panel(target).cwd);
                let generation = state.next_request_id();
                for letter in &drives {
                    effects.push(Effect::FetchDriveLabel { target, letter: *letter, generation });
                }
                let mut dialog = DriveSelect::new(target, drives, current);
                dialog.generation = generation;
                state.drive_select = Some(dialog);
            }
            Command::ToggleInfoMode(side) => effects.extend(toggle_info_mode(&mut state, side)),

            Command::RequestViewer => effects.extend(handle_request_viewer(&mut state)),
            Command::ViewerOpened { path, file_len } => {
                // A prior failed F3/F4 attempt may have left `last_error`
                // set on this panel; a successful open has clearly moved
                // past it, so the mini-status line must not keep showing
                // the stale message (§7 error policy — errors are surfaced
                // until superseded by success, not left to linger).
                state.panel_mut(state.active).last_error = None;
                state.phase = UiPhase::Viewer(ViewerState::new(path, file_len));
            }
            Command::ViewerOpenFailed { message } => state.panel_mut(state.active).last_error = Some(message),
            Command::RequestExternalEditor => effects.extend(handle_request_external_editor(&mut state)),
            Command::ExternalEditorSpawnFailed { message } => state.panel_mut(state.active).last_error = Some(message),

            Command::Tick(_) => {}
            Command::ConfirmQuit | Command::CancelQuit | Command::Resize(..) => unreachable!("handled above"),
            _ => {}
        },
        UiPhase::FileOpSetup(_) | UiPhase::FileOpRunning { .. } | UiPhase::FileOpSummary(_) | UiPhase::Viewer(_) => {
            unreachable!("handled above")
        }
    }

    (state, effects)
}

// ---------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------

/// Walk `history` by `delta` (-1 = older, +1 = newer) into the buffer.
///
/// Stepping past the newest entry stops recalling but deliberately leaves
/// the buffer as it is: Esc, not Down, is the documented way to release
/// Up/Down back to the panel cursor.
fn recall_history(state: &mut State, delta: isize) {
    if state.history.is_empty() {
        return;
    }
    let last = state.history.len() - 1;
    let next = match (state.history_cursor, delta) {
        (None, d) if d < 0 => Some(last),
        (None, _) => None,
        (Some(i), d) if d < 0 => Some(i.saturating_sub(1)),
        (Some(i), _) if i < last => Some(i + 1),
        (Some(_), _) => None,
    };
    match next {
        Some(i) => {
            state.command_line = state.history[i].clone();
            state.history_cursor = Some(i);
        }
        None => state.history_cursor = None,
    }
}

/// Ctrl+Enter / Ctrl+]: append the cursor entry's name or full path,
/// space-separating it from whatever is already typed.
fn paste_cursor_entry(state: &mut State, full_path: bool) {
    let panel = state.active_panel();
    let Some(entry) = panel.selected() else { return };
    let text = if full_path {
        panel.cwd.join(&entry.name).display().to_string()
    } else {
        entry.name.to_string_lossy().into_owned()
    };
    if !state.command_line.is_empty() && !state.command_line.ends_with(' ') {
        state.command_line.push(' ');
    }
    state.command_line.push_str(&text);
    state.history_cursor = None;
}

/// Run the typed line: record it in history, clear the buffer, and hand the
/// TUI a fully-formed invocation to spawn with the terminal suspended.
fn run_command_line(state: &mut State) -> Vec<Effect> {
    let text = state.command_line.trim().to_string();
    if text.is_empty() {
        return vec![];
    }
    state.command_line.clear();
    state.history_cursor = None;

    // `cd` cannot work through the shell — each command runs in a fresh
    // child, so its working directory dies with it. NC navigates the panel
    // instead, which is also how a UNC path is entered by hand.
    if let Some(target) = parse_cd(&text) {
        config::push_history(&mut state.history, &text);
        let side = state.active;
        let mut effects = vec![Effect::PersistHistory(state.history.clone())];
        if let Some(path) = resolve_cd_target(&state.panel(side).cwd, &target) {
            effects.extend(begin_listing(state, side, path));
        }
        return effects;
    }

    config::push_history(&mut state.history, &text);
    let side = state.active;
    let cwd = state.active_panel().cwd.clone();
    vec![
        Effect::RunShellCommand(shell::build_command(state.shell.shell.as_deref(), &text, &cwd), side),
        Effect::PersistHistory(state.history.clone()),
    ]
}

/// The target of a `cd <path>` line, or `None` if this isn't a `cd`.
pub fn parse_cd(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let rest = trimmed.strip_prefix("cd ").or_else(|| trimmed.strip_prefix("CD ")).or_else(|| trimmed.strip_prefix("Cd "))?;
    let target = rest.trim().trim_matches('"');
    if target.is_empty() {
        None
    } else {
        Some(target.to_string())
    }
}

/// Resolve a `cd` target against `cwd`. Absolute paths, UNC paths, and bare
/// drive letters (`D:`) are taken as-is; anything else is relative.
pub fn resolve_cd_target(cwd: &Path, target: &str) -> Option<PathBuf> {
    if target == ".." {
        return crate::panel::parent_path(cwd);
    }
    if target == "." {
        return Some(cwd.to_path_buf());
    }
    let path = Path::new(target);
    if drives::is_unc_path(path) || path.is_absolute() {
        return Some(path.to_path_buf());
    }
    // `D:` on its own means that drive's root, not a relative path.
    let mut chars = target.chars();
    if let (Some(letter), Some(':'), None) = (chars.next(), chars.next(), chars.next()) {
        if letter.is_ascii_alphabetic() {
            return Some(drives::drive_root(letter));
        }
    }
    Some(cwd.join(target))
}

/// Move the cursor to the first entry matching the type-ahead `pattern`
/// (`quicksearch::type_ahead_match`). A pattern that matches nothing leaves
/// the cursor where it is (type-ahead-jump "Alt+letter with no matching
/// entry", "Extended pattern no longer matches").
fn jump_to_prefix(state: &mut State, pattern: &str) {
    let side = state.active;
    let found = crate::quicksearch::type_ahead_match(&state.panel(side).entries, pattern);
    if let Some(index) = found {
        let panel = state.panel_mut(side);
        panel.cursor = index;
        panel.cursor_user_moved = true;
    }
}

// ---------------------------------------------------------------------
// Info mode
// ---------------------------------------------------------------------

fn toggle_info_mode(state: &mut State, side: PanelSide) -> Vec<Effect> {
    let entering_info = state.panel(side).display_mode == DisplayMode::Full;
    if entering_info {
        let request = state.next_request_id();
        let panel = state.panel_mut(side);
        panel.display_mode = DisplayMode::Info;
        // Every value starts pending; the worker fills them in place.
        panel.info = InfoValues::default();
        panel.info_request = Some(request);
        vec![Effect::QueryInfo { panel: side, path: panel.cwd.clone(), request }]
    } else {
        let panel = state.panel_mut(side);
        panel.display_mode = DisplayMode::Full;
        panel.info_request = None;
        vec![]
    }
}

// ---------------------------------------------------------------------
// F3 viewer (M4)
// ---------------------------------------------------------------------

/// F3: open the viewer on the file under the active panel's cursor. A no-op
/// on a directory (including `..`) or an empty panel; opening itself is an
/// `Effect` because it touches the filesystem, which `update` never does
/// directly (viewer: Instant open).
fn handle_request_viewer(state: &mut State) -> Vec<Effect> {
    let side = state.active;
    let Some(entry) = state.panel(side).selected() else { return vec![] };
    if entry.is_dir_like() {
        return vec![];
    }
    let path = state.panel(side).cwd.join(&entry.name);
    vec![Effect::OpenViewer { path }]
}

/// Drive every command while the viewer is open (gated in [`update`] by
/// `UiPhase::Viewer`). Byte-level work (backward scan, search) is never
/// performed here — it requires file I/O that `update` must stay free of —
/// so `ViewerSetTop` carries an already-computed offset and
/// `ViewerSearchConfirm` only kicks off `Effect::RunViewerSearch`, whose
/// result later re-enters as `Command::ViewerSearchResult` (design D1).
fn handle_viewer(state: &mut State, cmd: Command) -> Vec<Effect> {
    let mut effects = Vec::new();
    let UiPhase::Viewer(mut viewer) = std::mem::replace(&mut state.phase, UiPhase::Panels) else {
        unreachable!("handle_viewer only called when phase is Viewer");
    };
    match cmd {
        Command::ViewerClose => return effects,
        Command::ViewerToggleMode => viewer.toggle_mode(),
        Command::ViewerToggleWrap => viewer.toggle_wrap(),
        Command::ViewerSetTop(offset) => viewer.set_top_offset(offset),
        Command::ViewerSetHScroll(col) => viewer.h_scroll = col,
        Command::ViewerSearchStart => viewer.search_input = Some(String::new()),
        Command::ViewerSearchChar(c) => {
            if let Some(input) = &mut viewer.search_input {
                input.push(c);
            }
        }
        Command::ViewerSearchBackspace => {
            if let Some(input) = &mut viewer.search_input {
                input.pop();
            }
        }
        Command::ViewerSearchCancel => viewer.search_input = None,
        Command::ViewerSearchConfirm => {
            if let Some(pattern) = viewer.search_input.take().filter(|p| !p.is_empty()) {
                let pattern_bytes = pattern.into_bytes();
                viewer.search_pattern = Some(pattern_bytes.clone());
                let path = viewer.path.clone();
                let start_offset = viewer.top_offset;
                // A fresh request id per search, mirroring
                // `PanelState::info_request` — lets a reply be matched
                // against the search that's still outstanding, so an
                // out-of-order or superseded-session reply is dropped
                // rather than applied (viewer: F7 streaming search
                // staleness).
                let request = state.next_request_id();
                viewer.search_request = Some(request);
                state.phase = UiPhase::Viewer(viewer);
                effects.push(Effect::RunViewerSearch { path, start_offset, pattern: pattern_bytes, request });
                return effects;
            }
        }
        // Only apply a reply whose id matches this session's outstanding
        // search. A mismatch means either a stale reply from a search this
        // same session has since superseded, or one from a viewer session
        // that has since closed and been reopened (a fresh `ViewerState`
        // starts with `search_request: None`, which can never equal a real
        // request id) — either way it is silently dropped instead of
        // jumping the user to a bogus offset with a phantom match
        // highlight.
        Command::ViewerSearchResult { offset: Some(offset), match_range, request } if viewer.search_request == Some(request) => {
            viewer.set_top_offset(offset);
            viewer.last_match = match_range;
        }
        _ => {}
    }
    state.phase = UiPhase::Viewer(viewer);
    effects
}

// ---------------------------------------------------------------------
// F4 external editor (M4)
// ---------------------------------------------------------------------

/// F4 from a panel: resolve the cursor entry against the configured
/// `editor =` command and either dispatch the spawn effect or surface why
/// not (external-editor: F4 launches the editor, Config-driven external
/// editor command).
fn handle_request_external_editor(state: &mut State) -> Vec<Effect> {
    let side = state.active;
    let panel = state.panel(side);
    let Some(entry) = panel.selected() else { return vec![] };
    let is_dir = entry.is_dir_like();
    let name = entry.name.clone();
    let cwd = panel.cwd.clone();
    match external_editor::resolve(state.editor.as_deref(), &cwd, &name, is_dir) {
        Ok(invocation) => {
            // Mirrors the `Command::ViewerOpened` clear: a prior failed F3/F4
            // attempt may have left `last_error` set, and successfully
            // dispatching the editor spawn has clearly moved past it — the
            // mini-status line must not keep showing the stale message while
            // the editor runs (§7 error policy). The eventual successful
            // return also clears it again via `Command::RereadPanel` ->
            // `begin_new_listing`, but that round trip only happens once the
            // editor process exits, so this clears it immediately rather
            // than leaving the stale message up for the whole edit session.
            state.panel_mut(side).last_error = None;
            vec![Effect::RunExternalEditor(invocation, side)]
        }
        Err(external_editor::TargetError::Unconfigured) => {
            state.panel_mut(side).last_error = Some(external_editor::NO_EDITOR_CONFIGURED_MESSAGE.to_string());
            vec![]
        }
        // The directory case is silently ignored, matching how F5/F6/F8
        // are no-ops with nothing eligible selected.
        Err(external_editor::TargetError::IsDirectory) => vec![],
    }
}

// ---------------------------------------------------------------------
// Drive select
// ---------------------------------------------------------------------

fn is_drive_select_command(cmd: &Command) -> bool {
    matches!(cmd, Command::DriveSelectMove(_) | Command::DriveSelectConfirm | Command::DriveSelectCancel)
}

fn handle_drive_select(state: &mut State, cmd: Command) -> Vec<Effect> {
    match cmd {
        Command::DriveSelectMove(delta) => {
            if let Some(dialog) = &mut state.drive_select {
                dialog.move_selection(delta);
            }
            vec![]
        }
        Command::DriveSelectCancel => {
            // The target panel keeps its current directory.
            state.drive_select = None;
            vec![]
        }
        Command::DriveSelectConfirm => {
            let Some(dialog) = state.drive_select.take() else { return vec![] };
            let Some(letter) = dialog.selected_letter() else { return vec![] };
            // An unavailable drive is not special-cased here: the listing
            // read fails on the worker thread and surfaces through
            // `ListingFailed` as the panel's inline error state.
            let path = drives::drive_root(letter);
            begin_listing(state, dialog.target, path)
        }
        _ => vec![],
    }
}

// ---------------------------------------------------------------------
// F9 menu
// ---------------------------------------------------------------------

fn is_menu_command(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::MenuOpen
            | Command::MenuClose
            | Command::MenuCollapse
            | Command::MenuSelectPrev
            | Command::MenuSelectNext
            | Command::MenuPrevMenu
            | Command::MenuNextMenu
            | Command::MenuHotkey(_)
            | Command::MenuActivate
    )
}

/// Drive the menu state machine. Returns the command an activated item
/// stands for, which the caller re-enters `update` with.
fn handle_menu(state: &mut State, cmd: Command) -> Option<Command> {
    let active_side = state.active;
    let menu = state.menu.as_mut()?;
    match cmd {
        Command::MenuOpen | Command::MenuClose => state.menu = None,
        Command::MenuCollapse => {
            // Esc closes the pull-down first, leaving the bar open with its
            // title still highlighted; a second Esc closes the bar.
            if menu.pulldown_open {
                menu.pulldown_open = false;
            } else {
                state.menu = None;
            }
        }
        Command::MenuSelectPrev => menu.move_selection(-1),
        Command::MenuSelectNext => menu.move_selection(1),
        Command::MenuPrevMenu => {
            let target = menu.active.prev();
            menu.go_to(target);
        }
        Command::MenuNextMenu => {
            let target = menu.active.next();
            menu.go_to(target);
        }
        Command::MenuHotkey(c) => {
            if let Some(id) = MenuId::from_hotkey(c) {
                menu.go_to(id);
            }
        }
        Command::MenuActivate => {
            let item = menu.selected_item()?;
            let side = menu.active.target_side(active_side);
            let action = item.action;
            // Activating an item closes the whole overlay, restoring the top
            // row and the clock, before the action runs.
            state.menu = None;
            return menu_action_command(action, side);
        }
        _ => {}
    }
    None
}

/// The command a menu item stands for. Disabled items never reach here, so
/// `Unimplemented` maps to nothing.
pub fn menu_action_command(action: MenuAction, side: PanelSide) -> Option<Command> {
    match action {
        MenuAction::ToggleInfoMode => Some(Command::ToggleInfoMode(side)),
        MenuAction::SortBy(mode) => Some(Command::SetSortMode { side, mode }),
        MenuAction::Reread => Some(Command::RereadPanel(side)),
        MenuAction::DriveSelect => Some(Command::OpenDriveSelect(side)),
        MenuAction::Copy => Some(Command::RequestCopy),
        MenuAction::Move => Some(Command::RequestMove),
        MenuAction::Mkdir => Some(Command::RequestMkdir),
        MenuAction::Delete => Some(Command::RequestDelete),
        MenuAction::SelectGroup => Some(Command::GroupSelectAll),
        MenuAction::DeselectGroup => Some(Command::GroupDeselectAll),
        MenuAction::InvertSelection => Some(Command::InvertSelection),
        MenuAction::PanelsOnOff => Some(Command::ShowScrollback),
        MenuAction::Quit => Some(Command::RequestQuit),
        MenuAction::Unimplemented => None,
    }
}

// ---------------------------------------------------------------------
// File operations (M2)
// ---------------------------------------------------------------------

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

/// Enter: run the typed command line if there is one, otherwise act on the
/// entry under the cursor — descend into a directory, or spawn an
/// executable target through the same suspended-shell path a typed command
/// uses.
fn handle_enter(state: &mut State) -> Vec<Effect> {
    if !state.command_line.trim().is_empty() {
        return run_command_line(state);
    }
    let side = state.active;
    let Some(entry) = state.panel(side).selected() else { return vec![] };
    match entry.kind {
        EntryKind::File => {
            let name = entry.name.to_string_lossy().into_owned();
            if !shell::is_executable_name(&name, &state.shell.pathext) {
                return vec![];
            }
            let cwd = state.panel(side).cwd.clone();
            // Quoted so a name with spaces reaches the shell as one token.
            let text = format!("\"{name}\"");
            vec![Effect::RunShellCommand(shell::build_command(state.shell.shell.as_deref(), &text, &cwd), side)]
        }
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
    let mut effects = vec![Effect::StartListing { panel: side, path: path.clone() }];
    // A panel sitting in Info mode needs its drive/directory figures
    // re-gathered for wherever it just landed. A fresh request id is
    // minted even when the path is unchanged (e.g. a re-read), so an
    // answer to a since-superseded query for the same path is still
    // recognized as stale and dropped.
    if state.panel(side).display_mode == DisplayMode::Info {
        let request = state.next_request_id();
        state.panel_mut(side).info_request = Some(request);
        effects.push(Effect::QueryInfo { panel: side, path, request });
    }
    effects
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
mod tests;
