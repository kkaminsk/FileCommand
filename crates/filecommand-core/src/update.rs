//! The pure data-flow core: `State`, `Command`, `Effect`, and `update`.
//!
//! `update(state, command) -> (state, Vec<Effect>)` is the single path all
//! state mutations flow through — key-derived commands and worker-produced
//! events alike. It performs no I/O, spawns no threads, and reads no clock;
//! callers supply the current time via [`Command::Tick`].

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::config::{self, UserMenuEntry};
use crate::dialogs::{FileActionMenuEntry, FileActionMenuState, HelpState, ThemePickerState, UserMenuState};
use crate::drives::{self, DriveSelect};
use crate::editor::{EditorMove, EditorState, ReplacePrompt};
use crate::external_editor::{self, EditorInvocation};
use crate::find_file::FindFileState;
use crate::fs_ops::dialog::{FileOpSetup, RunningDialog};
use crate::fs_ops::{ConflictChoice, ConflictInfo, ErrorChoice, ErrorInfo, Job, JobKind, JobOutcome, ProgressInfo, SkippedItem, SourceItem};
use crate::git_info::GitInfo;
use crate::info::InfoValues;
use crate::listing::{Entry, EntryKind, FindMatch, SortMode};
use crate::menu::{MenuAction, MenuId, MenuState};
use crate::panel::{CursorMove, DisplayMode, PanelState, TreeState};
use crate::quicksearch::{self, FrecencyEntry, FuzzyJumpState};
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
    /// The F4 built-in editor, open full-screen in place of the panels and
    /// owning input focus the same way the viewer does (builtin-editor
    /// "Full-screen editor chrome").
    Editor(EditorState),
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
    /// Directory frecency history backing the Ctrl+J fuzzy-jump dialog,
    /// persisted alongside command history in `history.json` (fuzzy-jump
    /// "Directory history persistence"; design D6).
    pub dir_history: Vec<FrecencyEntry>,
    /// The most recent clock reading `Command::Tick` delivered. Used to
    /// timestamp frecency updates so `update` never reads a clock directly
    /// — the TUI's injected `Clock` is still the only source of "now".
    pub clock_ms: u64,
    /// The F2 user menu's entries, snapshotted from `usermenu.toml` at
    /// startup (user-menu "Parse label and command entries from
    /// usermenu.toml").
    pub user_menu_entries: Vec<UserMenuEntry>,
    /// The Ctrl+J fuzzy-jump dialog, or `None` when closed.
    pub fuzzy_jump: Option<FuzzyJumpState>,
    /// The Alt+F7 find-file dialog, or `None` when closed.
    pub find_file: Option<FindFileState>,
    /// The F2 user-menu overlay, or `None` when closed.
    pub user_menu: Option<UserMenuState>,
    /// The Options → Themes picker overlay, or `None` when closed
    /// (theme-selection "Options menu opens the theme picker").
    pub theme_picker: Option<ThemePickerState>,
    /// The Enter-on-file action menu, or `None` when closed. Opened by
    /// `handle_enter` and dismissed by activating an entry or Esc
    /// (file-action-menu "Enter on a file opens the action menu").
    pub file_action_menu: Option<FileActionMenuState>,
    /// The F1 Help window (and, nested within it, the About dialog), or
    /// `None` when closed.
    pub help: Option<HelpState>,
    /// A dismissable startup-warning modal, or `None` when there is nothing
    /// to warn about. Currently raised only for a malformed `usermenu.toml`
    /// (user-menu falls back to default entries without overwriting the
    /// file, per §6) — modeled the same "one-shot `Option<T>` overlay,
    /// dismissed by a dedicated `Command`" way as `drive_select`/
    /// `fuzzy_jump`/`find_file`/`user_menu`/`help`, rather than a panel
    /// mini-status, since it must be visible regardless of which panel (if
    /// either) it concerns (user-menu "Malformed file warns and falls back
    /// without overwriting").
    pub startup_warning: Option<String>,
    /// The quit-confirmation dialog, or `false` when closed. Modeled as a
    /// bare `bool` — like `EditorState::quit_confirm` for the built-in
    /// editor's own F10 quit prompt — rather than an `Option<T>`, since the
    /// dialog carries no state of its own (just a Y/N choice). It lives
    /// beside the phase, not inside it, so it can open above panels, the
    /// viewer, an open menu, or any other modal dialog/overlay without
    /// disturbing whatever is underneath; cancelling clears only this flag,
    /// which is what makes cancel-restores-context exact (application-shell
    /// "Quit request keys and confirmation"; design D5).
    pub quit_confirm: bool,
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
            dir_history: Vec::new(),
            clock_ms: 0,
            user_menu_entries: Vec::new(),
            fuzzy_jump: None,
            find_file: None,
            user_menu: None,
            theme_picker: None,
            file_action_menu: None,
            help: None,
            startup_warning: None,
            quit_confirm: false,
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
        let mut state = State {
            left: PanelState::new(left_cwd.clone()),
            right: PanelState::new(right_cwd.clone()),
            phase,
            term_size,
            clock_ms: now_ms,
            ..State::empty(theme)
        };
        let mut effects = vec![
            Effect::StartListing { panel: PanelSide::Left, path: left_cwd.clone() },
            Effect::StartListing { panel: PanelSide::Right, path: right_cwd.clone() },
        ];
        // Both panels start their git-info query alongside their first
        // listing, exactly as a later navigation does (git-info "Background
        // repository detection").
        effects.push(git_info_query_effect(&mut state, PanelSide::Left, left_cwd));
        effects.push(git_info_query_effect(&mut state, PanelSide::Right, right_cwd));
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

    // Tree display mode (§4.2), scoped to a panel side.
    /// Enter Tree mode on `side`, recording its current display mode so
    /// Enter can restore it, and kicking off the drive root's immediate-
    /// children read (additional-panel-modes "No up-front full-drive
    /// scan"; design D7).
    EnterTreeMode(PanelSide),
    /// Reply to `Effect::ExpandTreeNode`: `path`'s immediate child
    /// directories (empty on a read failure — skipped rather than
    /// aborting, matching `find_in_subtree`'s precedent). Applied only if
    /// `path` still names a not-yet-expanded node in `panel`'s tree, so a
    /// reply for a node the tree no longer has (a since-superseded Tree
    /// session) is silently dropped (additional-panel-modes "Children read
    /// on expand").
    TreeNodeExpanded { panel: PanelSide, path: PathBuf, children: Vec<Entry> },

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

    // F4 built-in editor (M5). `RequestEditor` is the actual F4 keybinding
    // target from a panel: it resolves the external-editor/built-in/size-cap
    // precedence (builtin-editor "Editor invocation and size cap", "External
    // editor takes precedence") and, when it wins, dispatches to the same
    // `handle_request_external_editor` path `RequestExternalEditor` already
    // exercises.
    RequestEditor,
    /// Reply to `Effect::OpenEditor`: the file loaded under the 10 MB cap
    /// (builtin-editor "Small file opens in the editor").
    EditorOpened(Box<EditorState>),
    /// Reply to `Effect::OpenEditor`: the file is `size` bytes, at or above
    /// the cap — redirected to the F3 viewer with a notice (builtin-editor
    /// "Large file redirects to the viewer").
    EditorTooLarge { path: PathBuf, size: u64 },
    /// Reply to `Effect::OpenEditor` when the file could not be opened.
    EditorOpenFailed { message: String },

    // Editor keymap, while `UiPhase::Editor` owns focus.
    EditorChar(char),
    EditorNewline,
    EditorBackspace,
    EditorMove(EditorMove),
    /// Insert key: toggle insert/overwrite text-entry mode.
    EditorToggleMode,
    /// F3: anchor a line selection at the caret.
    EditorMark,
    EditorCut,
    EditorCopy,
    EditorPaste,
    EditorUndo,
    /// F7: open the literal-search prompt.
    EditorSearchStart,
    EditorSearchChar(char),
    EditorSearchBackspace,
    EditorSearchCancel,
    /// Enter on the search prompt: run `EditorState::find_next` for the
    /// typed pattern.
    EditorSearchConfirm,
    /// F4 (while the editor, not a panel, owns focus): open the
    /// search-and-replace prompt's first (pattern) stage.
    EditorReplaceStart,
    EditorReplaceChar(char),
    EditorReplaceBackspace,
    EditorReplaceCancel,
    /// Enter on the replace prompt: advances pattern -> replacement, or —
    /// from the replacement stage — runs `EditorState::replace_first`.
    EditorReplaceConfirm,
    /// F2: save in place.
    EditorSave,
    /// Reply to `Effect::SaveEditor`: the write succeeded. `then_quit`
    /// echoes back whether this save was requested by the save-on-exit
    /// confirm's "Y", in which case the editor closes once applied.
    EditorSaved { editor: Box<EditorState>, then_quit: bool },
    /// Reply to `Effect::SaveEditor`: the write failed — aborts a save-on-
    /// exit quit attempt and surfaces the message inline rather than losing
    /// the buffer (builtin-editor "Modified indicator and save-on-exit
    /// prompt").
    EditorSaveFailed { message: String },
    /// F10: quit if unmodified, else raise the save-on-exit confirm
    /// (builtin-editor "Quitting with unsaved changes prompts").
    EditorRequestQuit,
    /// "Y" on the save-on-exit confirm: save, then close the editor.
    EditorConfirmQuitSave,
    /// "N" on the save-on-exit confirm: close the editor without saving.
    EditorConfirmQuitDiscard,
    /// Esc on the save-on-exit confirm: dismiss it and keep editing.
    EditorCancelQuit,

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
    /// Reply to `Effect::QueryGitInfo`: either a resolved repository's
    /// branch and per-entry statuses, or `GitInfo::none()` for "outside any
    /// repository" and for a timed-out query (git-info "Background
    /// repository detection", "Silent absence on timeout and stale-result
    /// discarding"). `request` is matched against
    /// `PanelState::git_request` the same way `InfoResolved`'s `request` is
    /// matched against `info_request`, so a reply for a directory/generation
    /// the panel has since moved past is dropped.
    GitInfoResolved { panel: PanelSide, path: PathBuf, request: u64, info: GitInfo },
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

    // Ctrl+J fuzzy jump (M5).
    /// Open the dialog (default binding, overridable via `config.toml`).
    FuzzyJumpOpen,
    FuzzyJumpChar(char),
    FuzzyJumpBackspace,
    FuzzyJumpMove(isize),
    /// Enter: navigate the active panel to the highlighted directory and
    /// close the dialog (fuzzy-jump "Enter navigates the active panel").
    FuzzyJumpConfirm,
    /// Esc: close without navigating.
    FuzzyJumpCancel,

    // Alt+F7 find file (M5).
    /// Open the dialog, rooted at the active panel's current directory
    /// (find-file "Find-file invocation").
    FindFileOpen,
    FindFileChar(char),
    FindFileBackspace,
    /// Enter on the pattern input: kick off `Effect::FindInSubtree`.
    FindFileSubmit,
    /// Reply to `Effect::FindInSubtree`: one matched entry, streamed as the
    /// walk finds it. `request` is matched against `FindFileState::request`
    /// so a reply from an abandoned/superseded search is dropped (find-file
    /// "Non-blocking search with static progress").
    FindFileMatch { request: u64, m: FindMatch },
    /// Reply to `Effect::FindInSubtree`: the walk has finished.
    FindFileSearchDone { request: u64 },
    FindFileMove(isize),
    /// Enter on a result: navigate the active panel's current tab in place
    /// and close the dialog (find-file "Navigate to a chosen result").
    FindFileConfirm,
    /// Esc: close without navigating, abandoning any in-progress search
    /// (find-file "Dismiss the find-file dialog").
    FindFileCancel,

    // F2 user menu (M5).
    UserMenuOpen,
    UserMenuMove(isize),
    /// Enter: run the highlighted entry's command via the shell passthrough
    /// in the active panel's directory, then close the menu (user-menu "Run
    /// the selected entry's command via the shell in the active panel
    /// directory").
    UserMenuConfirm,
    /// Esc: close without running anything.
    UserMenuCancel,

    // Options -> Themes picker (visual-themes).
    /// Opens the picker with the active theme's row pre-highlighted.
    /// Reached only via `MenuAction::OpenThemes` (Options → Themes), like
    /// `UserMenuOpen`/`HelpOpen` above.
    ThemePickerOpen,
    ThemePickerMove(isize),
    /// Enter: apply the highlighted theme immediately (switches
    /// `state.theme` in the same reducer step), persist it to
    /// `config.toml`, and close the dialog (theme-selection "Picker
    /// navigation, apply, and cancel").
    ThemePickerConfirm,
    /// Esc: close without changing the active theme or touching
    /// `config.toml`.
    ThemePickerCancel,

    // F1 Help window + About dialog (M5).
    HelpOpen,
    HelpMove(isize),
    /// Enter / the `Help` button: opens the highlighted topic's page, or
    /// the About dialog for the special first entry.
    HelpActivate,
    /// Esc / the `Cancel` button: dismisses the About dialog back to the
    /// list, or a topic page back to the list, or closes the window
    /// entirely from the list.
    HelpCancel,

    // Panel display-mode switches reachable from the Left/Right menu (M5;
    // design D7). `Info` and `Tree` keep their own dedicated commands
    // (`ToggleInfoMode`, `EnterTreeMode`) since each carries extra
    // bookkeeping (an Info query, a Tree session) this generic command
    // deliberately does not.
    SetDisplayMode { side: PanelSide, mode: DisplayMode },

    /// Dismiss the startup-warning modal (any key, per the one-shot dialogs'
    /// existing Esc/Enter-dismiss convention) (user-menu "Malformed file
    /// warns and falls back without overwriting").
    DismissStartupWarning,

    // Enter-on-file action menu (file-action-menu).
    /// Up/Down: move the highlight, clamped at both ends.
    FileActionMenuMove(isize),
    /// Enter: activate the highlighted entry and close the menu.
    FileActionMenuConfirm,
    /// Esc: close the menu with no action taken.
    FileActionMenuCancel,
    /// A first-letter hotkey: activate the matching entry directly, exactly
    /// as if it had been highlighted and confirmed (file-action-menu
    /// "First-letter hotkey activates directly").
    FileActionMenuHotkey(char),
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
    /// Rewrite `history.json` atomically with the command history and
    /// directory frecency together (fuzzy-jump "Directory history
    /// persistence"; design D6).
    PersistHistory(config::HistoryFile),
    /// Rewrite `config.toml`'s `theme =` key atomically after the Options →
    /// Themes picker applies a theme (theme-selection "Applied theme
    /// persists to configuration").
    PersistTheme(String),
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
    /// Run `git_info::query` for `path` on a dedicated worker thread —
    /// never the shared Info worker, since a slow status call on one panel
    /// must not head-of-line-block the other (design D3) — and report the
    /// result back via `Command::GitInfoResolved`. `request` travels with
    /// the result so a stale or timed-out reply can be told apart from the
    /// query that's still current (git-info "Background repository
    /// detection", "Pathspec-scoped status queries").
    QueryGitInfo { panel: PanelSide, path: PathBuf, request: u64 },
    /// Read `path`'s immediate child directories for Tree-mode expansion
    /// (`listing::list_child_dirs`) on a worker thread, reporting them back
    /// via `Command::TreeNodeExpanded` — one directory's worth of I/O per
    /// call, never a recursive scan (additional-panel-modes "Children read
    /// on expand"; design D7).
    ExpandTreeNode { panel: PanelSide, path: PathBuf },
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
    /// Open `path` for the F4 built-in editor: stat it and, under the 10 MB
    /// cap, read and decode it in full, reporting back via
    /// `Command::EditorOpened`/`EditorTooLarge`/`EditorOpenFailed`. Run
    /// synchronously before the next repaint rather than on a worker
    /// thread — the same "cheap enough for the input path" precedent
    /// `Effect::OpenViewer` sets, extended here to the whole-file read the
    /// 10 MB cap explicitly bounds (design D1).
    OpenEditor { path: PathBuf },
    /// Write `editor`'s buffer to disk via `EditorState::save`, reporting
    /// the post-save state back via `Command::EditorSaved` (or the failure
    /// via `Command::EditorSaveFailed`). `then_quit` travels through
    /// unchanged so the reply knows whether to close the editor once the
    /// write lands (builtin-editor "Save in place …", design D9).
    SaveEditor { editor: Box<EditorState>, then_quit: bool },
    /// Walk `root`'s subtree for entries whose name contains `pattern` on a
    /// worker thread, streaming each match back via `Command::FindFileMatch`
    /// as it's found and finishing with `Command::FindFileSearchDone` —
    /// never blocking the UI event loop (find-file "Non-blocking search
    /// with static progress"). `request` travels with every reply so a
    /// stale/abandoned search's results are dropped once the dialog has
    /// moved on.
    FindInSubtree { root: PathBuf, pattern: String, request: u64 },
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

    // Every `Tick` updates the clock reading `begin_listing` timestamps
    // frecency visits with, regardless of phase — mirrored generically here
    // rather than duplicated in every phase arm that might navigate.
    if let Command::Tick(now) = cmd {
        state.clock_ms = now;
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
        Command::GitInfoResolved { panel, path, request, info } => {
            let p = state.panel_mut(panel);
            if p.cwd == path && p.git_request == Some(request) {
                p.git_info = info;
            }
            return (state, effects);
        }
        Command::TreeNodeExpanded { panel, path, children } => {
            if let Some(tree) = state.panel_mut(panel).tree.as_mut() {
                tree.insert_children(&path, children);
            }
            return (state, effects);
        }
        _ => {}
    }

    // The quit-confirmation dialog is a modal overlay beside the phase, like
    // the M5 dialogs below, but uniquely reachable from *every* context —
    // panels, the viewer, an open menu, and every other modal dialog/
    // overlay, including while a file operation is running — so it is
    // handled here, ahead of the phase-specific short-circuits that follow,
    // rather than gated behind any one of them (application-shell "Quit
    // request keys and confirmation"; design D5).
    if let Command::RequestQuit = cmd {
        state.quit_confirm = true;
        return (state, effects);
    }
    if state.quit_confirm {
        match cmd {
            Command::ConfirmQuit => {
                // Confirming while a job is running aborts it first, through
                // the same cancel path the Progress dialog's own
                // `Command::FileOpCancelJob` uses (`Effect::CancelJob`),
                // before quitting (design D3).
                if matches!(state.phase, UiPhase::FileOpRunning { .. }) {
                    effects.push(Effect::CancelJob);
                }
                state.quit_confirm = false;
                effects.push(Effect::Quit);
                return (state, effects);
            }
            Command::CancelQuit => {
                // Clearing only this flag — never touching `state.phase` or
                // any other overlay field — is what makes cancel restore the
                // prior context exactly: a still-open viewer, menu, or
                // dialog, and untouched command-line/quick-filter/type-ahead
                // state (application-shell "Quit request keys and
                // confirmation").
                state.quit_confirm = false;
                return (state, effects);
            }
            _ => {}
        }
    }

    // File-op setup/running/summary phases (and the job events that drive
    // them) are handled uniformly here, independent of the
    // Splash/Placeholder/Panels phases below.
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

    // The F4 built-in editor, likewise, replaces the panels full-screen and
    // owns input focus while open (builtin-editor "Full-screen editor
    // chrome").
    if matches!(state.phase, UiPhase::Editor(_)) {
        effects.extend(handle_editor(&mut state, cmd));
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
    // The M5 dialogs (fuzzy jump, find file, user menu, Help/About) are
    // likewise modal overlays beside the phase, each claiming only the
    // commands it understands while open — the same shape as the two blocks
    // above.
    if state.fuzzy_jump.is_some() && is_fuzzy_jump_command(&cmd) {
        effects.extend(handle_fuzzy_jump(&mut state, cmd));
        return (state, effects);
    }
    if state.find_file.is_some() && is_find_file_command(&cmd) {
        effects.extend(handle_find_file(&mut state, cmd));
        return (state, effects);
    }
    if state.user_menu.is_some() && is_user_menu_command(&cmd) {
        effects.extend(handle_user_menu(&mut state, cmd));
        return (state, effects);
    }
    if state.theme_picker.is_some() && is_theme_picker_command(&cmd) {
        effects.extend(handle_theme_picker(&mut state, cmd));
        return (state, effects);
    }
    if state.file_action_menu.is_some() && is_file_action_menu_command(&cmd) {
        effects.extend(handle_file_action_menu(&mut state, cmd));
        return (state, effects);
    }
    if state.help.is_some() && is_help_command(&cmd) {
        effects.extend(handle_help(&mut state, cmd));
        return (state, effects);
    }
    if state.startup_warning.is_some() && matches!(cmd, Command::DismissStartupWarning) {
        state.startup_warning = None;
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
        UiPhase::Panels => match cmd {
            Command::MoveCursor(m) => {
                // A movement key exits type-ahead (if active) *and* is
                // still applied to the panel cursor as a normal movement,
                // in the same keystroke — the `input/` mapper emits exactly
                // this one command for a movement key while type-ahead owns
                // the keyboard, relying on this side effect to also end the
                // mode (type-ahead-jump "A movement key exits type-ahead
                // and is applied to the panel"; design D5).
                state.quick_search = None;
                let side = state.active;
                if state.panel(side).display_mode == DisplayMode::Tree {
                    effects.extend(handle_tree_cursor_move(&mut state, side, m));
                } else {
                    state.panel_mut(side).move_cursor(m);
                }
            }
            Command::ToggleActivePanel => state.active = state.active.toggle(),
            Command::Enter => effects.extend(handle_enter(&mut state)),
            Command::ParentDir => {
                let side = state.active;
                effects.extend(handle_parent(&mut state, side));
            }
            // `Command::RequestQuit` is handled globally, above, regardless
            // of phase — it never reaches this arm.
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

            Command::EnterTreeMode(side) => effects.extend(enter_tree_mode(&mut state, side)),
            Command::SetDisplayMode { side, mode } => {
                let p = state.panel_mut(side);
                p.tree = None;
                p.info_request = None;
                p.display_mode = mode;
                // A quick filter narrowed the panel's prior display mode; a
                // stale pattern must not linger invisibly into Brief/Tree/
                // Info mode, whose renderers don't surface it the same way
                // Full mode does (quick-filter "Substring narrowing as the
                // pattern is typed").
                p.clear_quick_filter();
            }

            Command::FuzzyJumpOpen => state.fuzzy_jump = Some(FuzzyJumpState::new()),
            Command::FindFileOpen => {
                let root = state.active_panel().cwd.clone();
                state.find_file = Some(FindFileState::new(root));
            }
            Command::UserMenuOpen => state.user_menu = Some(UserMenuState::new()),
            Command::ThemePickerOpen => state.theme_picker = Some(ThemePickerState::open(&state.theme.name)),
            Command::HelpOpen => state.help = Some(HelpState::new()),

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

            Command::RequestEditor => effects.extend(handle_request_editor(&mut state)),
            Command::EditorOpened(editor) => {
                // Mirrors the `Command::ViewerOpened` clear: a prior failed
                // F3/F4 attempt may have left `last_error` set (§7 error
                // policy).
                state.panel_mut(state.active).last_error = None;
                state.phase = UiPhase::Editor(*editor);
            }
            Command::EditorTooLarge { path, size: _ } => {
                // builtin-editor "Large file redirects to the viewer": the
                // notice lands on the panel's mini-status (the same surface
                // `Command::ViewerOpenFailed` uses) since the viewer chrome
                // itself has no room reserved for a banner; it's visible the
                // moment the user returns from the viewer to the panels.
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
                state.panel_mut(state.active).last_error =
                    Some(format!("{name} is too large for the editor (10 MB limit) — opened in the viewer"));
                effects.push(Effect::OpenViewer { path });
            }
            Command::EditorOpenFailed { message } => state.panel_mut(state.active).last_error = Some(message),

            Command::Tick(_) => {}
            Command::ConfirmQuit | Command::CancelQuit | Command::Resize(..) => unreachable!("handled above"),
            _ => {}
        },
        UiPhase::FileOpSetup(_) | UiPhase::FileOpRunning { .. } | UiPhase::FileOpSummary(_) | UiPhase::Viewer(_) | UiPhase::Editor(_) => {
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
        let mut effects =
            vec![Effect::PersistHistory(config::HistoryFile { commands: state.history.clone(), directories: state.dir_history.clone() })];
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
        Effect::PersistHistory(config::HistoryFile { commands: state.history.clone(), directories: state.dir_history.clone() }),
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
/// entry", "Extended pattern no longer matches"). Input routing normally
/// keeps type-ahead and the Ctrl+P quick filter mutually exclusive, but if
/// both are ever active at once the jump must still only land within
/// `visible_indices()` — otherwise the cursor could land on an entry the
/// active filter hides (quick-filter "Navigation is restricted to matching
/// entries").
fn jump_to_prefix(state: &mut State, pattern: &str) {
    let side = state.active;
    let panel = state.panel(side);
    let found = if panel.quick_filter.is_some() {
        let visible = panel.visible_indices();
        let visible_entries: Vec<Entry> = visible.iter().map(|&i| panel.entries[i].clone()).collect();
        crate::quicksearch::type_ahead_match(&visible_entries, pattern).map(|pos| visible[pos])
    } else {
        crate::quicksearch::type_ahead_match(&panel.entries, pattern)
    };
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
        // Same reasoning as `SetDisplayMode`/`EnterTreeMode`: Ctrl+L is a
        // second path into Info mode, and a quick filter narrowing the
        // prior Full-mode list must not linger invisibly here either
        // (quick-filter "Substring narrowing as the pattern is typed").
        panel.clear_quick_filter();
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
// F4 built-in editor (M5)
// ---------------------------------------------------------------------

/// F4 from a panel: the external editor takes precedence when configured
/// (builtin-editor "External editor takes precedence") — reusing
/// `handle_request_external_editor`'s own entry/directory resolution rather
/// than duplicating it. Otherwise, on a file (not `..`, not a directory),
/// kick off the size-gated open; the size cap itself is enforced by
/// `EditorState::open` on the TUI side once `Effect::OpenEditor` runs
/// (builtin-editor "Editor invocation and size cap").
fn handle_request_editor(state: &mut State) -> Vec<Effect> {
    if state.editor.is_some() {
        return handle_request_external_editor(state);
    }
    let side = state.active;
    let panel = state.panel(side);
    let Some(entry) = panel.selected() else { return vec![] };
    if entry.is_dir_like() {
        return vec![];
    }
    let path = panel.cwd.join(&entry.name);
    state.panel_mut(side).last_error = None;
    vec![Effect::OpenEditor { path }]
}

/// The editor body's visible row count for a given terminal height: the
/// header and F-key bar each reserve one row, exactly like the viewer's
/// `body_rows` (kept as a one-line duplicate here rather than a cross-crate
/// dependency, since `core::update` needs it for `ensure_caret_visible` and
/// cannot depend on `filecommand-tui`).
fn editor_viewport(term_size: (u16, u16)) -> (usize, usize) {
    (term_size.1.saturating_sub(2).max(1) as usize, term_size.0.max(1) as usize)
}

/// Drive every command while the editor is open (gated in [`update`] by
/// `UiPhase::Editor`). Saving is the one operation here that needs I/O
/// (`EditorState::save` writes to disk), so it is dispatched as
/// `Effect::SaveEditor` and applied only once `Command::EditorSaved`/
/// `EditorSaveFailed` reply — mirroring how `handle_viewer` defers the
/// viewer's own I/O to effects and re-enters through their replies (design
/// D1/D2).
fn handle_editor(state: &mut State, cmd: Command) -> Vec<Effect> {
    let mut effects = Vec::new();
    let UiPhase::Editor(mut editor) = std::mem::replace(&mut state.phase, UiPhase::Panels) else {
        unreachable!("handle_editor only called when phase is Editor");
    };
    let (rows, cols) = editor_viewport(state.term_size);
    match cmd {
        Command::EditorChar(c) => editor.type_char(c),
        Command::EditorNewline => editor.insert_newline(),
        Command::EditorBackspace => editor.backspace(),
        Command::EditorMove(m) => match m {
            EditorMove::Left => editor.move_left(),
            EditorMove::Right => editor.move_right(),
            EditorMove::Up => editor.move_up(),
            EditorMove::Down => editor.move_down(),
            EditorMove::Home => editor.move_home(),
            EditorMove::End => editor.move_end(),
            EditorMove::PageUp(rows) => editor.move_page_up(rows),
            EditorMove::PageDown(rows) => editor.move_page_down(rows),
        },
        Command::EditorToggleMode => editor.toggle_mode(),
        Command::EditorMark => editor.start_mark(),
        Command::EditorCut => editor.cut_selection(),
        Command::EditorCopy => editor.copy_selection(),
        Command::EditorPaste => editor.paste(),
        Command::EditorUndo => editor.undo(),

        Command::EditorSearchStart => editor.search_prompt = Some(String::new()),
        Command::EditorSearchChar(c) => {
            if let Some(p) = &mut editor.search_prompt {
                p.push(c);
            }
        }
        Command::EditorSearchBackspace => {
            if let Some(p) = &mut editor.search_prompt {
                p.pop();
            }
        }
        Command::EditorSearchCancel => editor.search_prompt = None,
        Command::EditorSearchConfirm => {
            if let Some(pattern) = editor.search_prompt.take() {
                if !pattern.is_empty() {
                    editor.find_next(&pattern);
                }
            }
        }

        Command::EditorReplaceStart => editor.replace_prompt = Some(ReplacePrompt::Pattern(String::new())),
        Command::EditorReplaceChar(c) => {
            if let Some(prompt) = &mut editor.replace_prompt {
                match prompt {
                    ReplacePrompt::Pattern(p) => p.push(c),
                    ReplacePrompt::Replacement { replacement, .. } => replacement.push(c),
                }
            }
        }
        Command::EditorReplaceBackspace => {
            if let Some(prompt) = &mut editor.replace_prompt {
                match prompt {
                    ReplacePrompt::Pattern(p) => {
                        p.pop();
                    }
                    ReplacePrompt::Replacement { replacement, .. } => {
                        replacement.pop();
                    }
                }
            }
        }
        Command::EditorReplaceCancel => editor.replace_prompt = None,
        // Enter: the pattern stage advances to the replacement stage (an
        // empty pattern instead cancels the prompt outright — nothing to
        // search for); the replacement stage performs the replacement
        // (builtin-editor "Replace substitutes a match").
        Command::EditorReplaceConfirm => {
            if let Some(prompt) = editor.replace_prompt.take() {
                match prompt {
                    ReplacePrompt::Pattern(pattern) if !pattern.is_empty() => {
                        editor.replace_prompt = Some(ReplacePrompt::Replacement { pattern, replacement: String::new() });
                    }
                    ReplacePrompt::Pattern(_) => {}
                    ReplacePrompt::Replacement { pattern, replacement } => {
                        editor.replace_first(&pattern, &replacement);
                    }
                }
            }
        }

        Command::EditorSave => {
            effects.push(Effect::SaveEditor { editor: Box::new(editor.clone()), then_quit: false });
        }
        Command::EditorSaved { editor: saved, then_quit } => {
            if then_quit {
                // Phase was already reset to `Panels` by the `mem::replace`
                // above; leaving it as-is is the exit.
                return effects;
            }
            editor = *saved;
            editor.quit_confirm = false;
        }
        Command::EditorSaveFailed { message } => {
            editor.save_error = Some(message);
            editor.quit_confirm = false;
        }

        Command::EditorRequestQuit => {
            if editor.is_modified() {
                editor.quit_confirm = true;
            } else {
                // Unmodified: exits directly (builtin-editor "Quitting an
                // unmodified buffer exits directly") — phase stays `Panels`.
                return effects;
            }
        }
        Command::EditorConfirmQuitSave => {
            effects.push(Effect::SaveEditor { editor: Box::new(editor.clone()), then_quit: true });
        }
        Command::EditorConfirmQuitDiscard => return effects,
        Command::EditorCancelQuit => editor.quit_confirm = false,

        _ => {}
    }
    editor.ensure_caret_visible(rows, cols);
    state.phase = UiPhase::Editor(editor);
    effects
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
        MenuAction::SetDisplayMode(mode) => Some(Command::SetDisplayMode { side, mode }),
        MenuAction::EnterTree => Some(Command::EnterTreeMode(side)),
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
        MenuAction::FindFile => Some(Command::FindFileOpen),
        MenuAction::FuzzyJump => Some(Command::FuzzyJumpOpen),
        MenuAction::Quit => Some(Command::RequestQuit),
        MenuAction::OpenThemes => Some(Command::ThemePickerOpen),
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
    if kind == JobKind::Mkdir {
        let side = state.active;
        let source_dir = state.panel(side).cwd.clone();
        state.phase = UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources: vec![], source_dir, input: String::new() });
        return;
    }
    let sources = active_selection_sources(state);
    if sources.is_empty() {
        return;
    }
    enter_file_op_setup_for_sources(state, kind, sources);
}

/// Shared by `enter_file_op_setup` (F5/F6, selection- or cursor-scoped) and
/// the file-action menu's Copy/Move (cursor-entry-only, D3) — the setup
/// dialog itself doesn't care which chose its `sources`.
fn enter_file_op_setup_for_sources(state: &mut State, kind: JobKind, sources: Vec<SourceItem>) {
    let side = state.active;
    let source_dir = state.panel(side).cwd.clone();
    let prefill = state.panel(side.toggle()).cwd.display().to_string();
    state.phase = UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, source_dir, input: prefill });
}

/// F8: enter the delete-confirmation dialog. A no-op when there is nothing
/// selected.
fn enter_delete_confirm(state: &mut State) {
    let sources = active_selection_sources(state);
    if sources.is_empty() {
        return;
    }
    enter_delete_confirm_for_sources(state, sources);
}

/// Shared by `enter_delete_confirm` (F8) and the file-action menu's Delete
/// (cursor-entry-only, D3).
fn enter_delete_confirm_for_sources(state: &mut State, sources: Vec<SourceItem>) {
    let side = state.active;
    let source_dir = state.panel(side).cwd.clone();
    let needs_second_confirm = sources.iter().any(|s| s.is_dir);
    state.phase = UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { sources, source_dir, needs_second_confirm, confirmed_once: false });
}

/// The file-action menu's Rename: enter the in-place rename input dialog,
/// pre-filled with the target's current name (file-action-menu "In-place
/// Rename": "pre-filled with the target entry's current name").
fn enter_rename_input(state: &mut State, original_name: OsString, is_dir: bool) {
    let side = state.active;
    let source_dir = state.panel(side).cwd.clone();
    let input = original_name.to_string_lossy().into_owned();
    state.phase = UiPhase::FileOpSetup(FileOpSetup::RenameInput { source_dir, original_name, is_dir, input });
}

// ---------------------------------------------------------------------
// Enter-on-file action menu (file-action-menu)
// ---------------------------------------------------------------------

fn is_file_action_menu_command(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::FileActionMenuMove(_) | Command::FileActionMenuConfirm | Command::FileActionMenuCancel | Command::FileActionMenuHotkey(_)
    )
}

/// Drive the Enter-on-file action menu (gated in [`update`] by
/// `state.file_action_menu`) — navigation, dismissal, and activation
/// (file-action-menu "Menu contents, ordering, and navigation").
fn handle_file_action_menu(state: &mut State, cmd: Command) -> Vec<Effect> {
    if state.file_action_menu.is_none() {
        return vec![];
    }
    match cmd {
        Command::FileActionMenuMove(delta) => {
            state.file_action_menu.as_mut().unwrap().move_cursor(delta);
            vec![]
        }
        Command::FileActionMenuCancel => {
            state.file_action_menu = None;
            vec![]
        }
        Command::FileActionMenuConfirm => {
            let menu = state.file_action_menu.take().unwrap();
            let target_name = menu.target_name.clone();
            activate_file_action(state, menu.selected(), target_name)
        }
        Command::FileActionMenuHotkey(c) => {
            let menu = state.file_action_menu.as_ref().unwrap();
            let action = menu.hotkey_action(c);
            match action {
                Some(action) => {
                    let target_name = menu.target_name.clone();
                    state.file_action_menu = None;
                    activate_file_action(state, action, target_name)
                }
                None => vec![],
            }
        }
        _ => vec![],
    }
}

/// Route an activated menu entry into its existing capability flow, applied
/// to `target_name` — the entry the menu was opened on, captured in
/// `FileActionMenuState` at open time and threaded through here rather than
/// re-read from the panel's live cursor, which can drift while the menu (a
/// modal overlay) is open: a background `ListingChunk`/`ListingComplete` is
/// applied unconditionally regardless of any open modal and resets the
/// cursor to row 0 whenever the panel hasn't seen a user-driven move since
/// the listing began (file-action-menu "Menu actions route to existing
/// flows").
fn activate_file_action(state: &mut State, action: FileActionMenuEntry, target_name: OsString) -> Vec<Effect> {
    let side = state.active;
    match action {
        // Run: the same suspended-shell spawn path a typed command line
        // uses, and the only way to reach it now that Enter no longer
        // spawns an executable directly (command-line: "Enter on an
        // executable opens the menu instead of spawning").
        FileActionMenuEntry::Run => {
            let Some(_) = named_entry_source(state, side, &target_name) else { return vec![] };
            let name = target_name.to_string_lossy().into_owned();
            let cwd = state.panel(side).cwd.clone();
            // Quoted so a name with spaces reaches the shell as one token.
            let text = format!("\"{name}\"");
            vec![Effect::RunShellCommand(shell::build_command(state.shell.shell.as_deref(), &text, &cwd), side)]
        }
        // View/Edit: the exact F3/F4 core paths, unchanged.
        FileActionMenuEntry::View => handle_request_viewer(state),
        FileActionMenuEntry::Edit => handle_request_editor(state),
        // Copy/Move/Delete: the existing F5/F6/F8 setup dialogs, scoped to
        // the menu's target entry only — never the panel's selection (D3).
        FileActionMenuEntry::Copy => {
            if let Some(source) = named_entry_source(state, side, &target_name) {
                enter_file_op_setup_for_sources(state, JobKind::Copy, vec![source]);
            }
            vec![]
        }
        FileActionMenuEntry::Move => {
            if let Some(source) = named_entry_source(state, side, &target_name) {
                enter_file_op_setup_for_sources(state, JobKind::Move, vec![source]);
            }
            vec![]
        }
        FileActionMenuEntry::Delete => {
            if let Some(source) = named_entry_source(state, side, &target_name) {
                enter_delete_confirm_for_sources(state, vec![source]);
            }
            vec![]
        }
        // Rename: new in-place variant (file-action-menu "In-place Rename").
        FileActionMenuEntry::Rename => {
            if let Some(source) = named_entry_source(state, side, &target_name) {
                enter_rename_input(state, source.original_name, source.is_dir);
            }
            vec![]
        }
    }
}

/// The panel entry on `side` named `name` as a single-item `SourceItem`,
/// ignoring the panel's cursor position and selection entirely — used only
/// by the file-action menu, which targets the entry it was opened on
/// (captured by name in `FileActionMenuState`) regardless of any cursor
/// drift from a background listing refresh or any multi-entry selection
/// (design D3; file-action-menu "Enter on a file opens the action menu":
/// "SHALL NOT consume or alter the multi-entry selection"). Returns `None`
/// if the named entry is no longer present (e.g. deleted or renamed away by
/// something else while the menu was open) or is a directory-like pseudo
/// entry, matching the existing not-found-is-a-no-op behavior of every
/// other dialog-open path.
fn named_entry_source(state: &State, side: PanelSide, name: &OsStr) -> Option<SourceItem> {
    let panel = state.panel(side);
    let entry = panel.entries.iter().find(|e| e.name == name)?;
    if entry.kind == EntryKind::ParentDir {
        return None;
    }
    Some(SourceItem { original_name: entry.name.clone(), path: panel.cwd.join(&entry.name), is_dir: entry.is_dir_like() })
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
        FileOpSetup::RenameInput { source_dir, original_name, is_dir, mut input } => match cmd {
            Command::FileOpInputChar(c) => {
                input.push(c);
                UiPhase::FileOpSetup(FileOpSetup::RenameInput { source_dir, original_name, is_dir, input })
            }
            Command::FileOpInputBackspace => {
                input.pop();
                UiPhase::FileOpSetup(FileOpSetup::RenameInput { source_dir, original_name, is_dir, input })
            }
            Command::FileOpCancel => UiPhase::Panels,
            Command::FileOpConfirm => {
                let trimmed = input.trim();
                if trimmed.is_empty() {
                    return UiPhase::FileOpSetup(FileOpSetup::RenameInput { source_dir, original_name, is_dir, input });
                }
                let source = SourceItem { original_name: original_name.clone(), path: source_dir.join(&original_name), is_dir };
                let job = Job {
                    kind: JobKind::Rename,
                    sources: vec![source],
                    source_dir: source_dir.clone(),
                    dest_dir: source_dir.clone(),
                    new_dir_name: Some(OsString::from(trimmed)),
                };
                let running = UiPhase::FileOpRunning {
                    source_dir: job.source_dir.clone(),
                    dest_dir: job.dest_dir.clone(),
                    dialog: RunningDialog::Progress { kind: JobKind::Rename, progress: ProgressInfo::starting(0, 0) },
                };
                effects.push(Effect::RunJob(job));
                running
            }
            _ => UiPhase::FileOpSetup(FileOpSetup::RenameInput { source_dir, original_name, is_dir, input }),
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
/// entry under the cursor — descend into a directory, or open the
/// file-action menu for a file (file-action-menu "Enter on a file opens the
/// action menu"). Spawning an executable directly on Enter is gone; the
/// menu's Run entry is now the only way to reach the suspended-shell spawn
/// path (command-line: "Enter on an executable opens the menu instead of
/// spawning").
fn handle_enter(state: &mut State) -> Vec<Effect> {
    if !state.command_line.trim().is_empty() {
        return run_command_line(state);
    }
    let side = state.active;
    if state.panel(side).display_mode == DisplayMode::Tree {
        return handle_tree_enter(state, side);
    }
    let Some(entry) = state.panel(side).selected() else { return vec![] };
    match entry.kind {
        EntryKind::File => {
            let name = entry.name.clone();
            let executable = shell::is_executable_name(&name.to_string_lossy(), &state.shell.pathext);
            state.file_action_menu = Some(FileActionMenuState::new(name, executable));
            vec![]
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

// ---------------------------------------------------------------------
// Tree display mode (M5, design D7)
// ---------------------------------------------------------------------

/// Enter Tree mode on `side`: record the panel's current display mode so
/// Enter can restore it, root the tree at the panel's drive, and kick off
/// the drive root's immediate-children read — the only I/O Tree mode
/// performs up front (additional-panel-modes "No up-front full-drive
/// scan").
fn enter_tree_mode(state: &mut State, side: PanelSide) -> Vec<Effect> {
    let panel = state.panel(side);
    let prior_mode = panel.display_mode;
    let letter = drives::drive_letter_of(&panel.cwd).unwrap_or('C');
    let root = drives::drive_root(letter);
    let p = state.panel_mut(side);
    p.display_mode = DisplayMode::Tree;
    p.tree = Some(TreeState::new(root.clone(), prior_mode));
    // Same reasoning as `SetDisplayMode`: a quick filter narrowing the
    // prior list mode has no meaning in Tree mode and must not linger
    // invisibly (quick-filter "Substring narrowing as the pattern is
    // typed").
    p.clear_quick_filter();
    vec![Effect::ExpandTreeNode { panel: side, path: root }]
}

/// Move `side`'s tree cursor, then re-list the opposite panel at whatever
/// directory is now highlighted (additional-panel-modes "Tree mode drives
/// the opposite panel"), expanding the newly highlighted node if it hasn't
/// been already (additional-panel-modes "Children read on expand").
fn handle_tree_cursor_move(state: &mut State, side: PanelSide, m: CursorMove) -> Vec<Effect> {
    let (target, needs_expand) = {
        let Some(tree) = state.panel_mut(side).tree.as_mut() else { return vec![] };
        tree.move_cursor(m);
        let Some(node) = tree.selected() else { return vec![] };
        (node.path.clone(), !node.expanded)
    };
    let mut effects = begin_listing_inner(state, side.toggle(), target.clone());
    if needs_expand {
        effects.push(Effect::ExpandTreeNode { panel: side, path: target });
    }
    effects
}

/// Enter on a tree node: leave Tree mode, restoring the panel's prior
/// display mode, and navigate this panel (not the opposite one) to the
/// highlighted directory (additional-panel-modes "Enter returns to prior
/// list mode at chosen directory").
fn handle_tree_enter(state: &mut State, side: PanelSide) -> Vec<Effect> {
    let Some(tree) = &state.panel(side).tree else { return vec![] };
    let Some(node) = tree.selected() else { return vec![] };
    let target = node.path.clone();
    let prior_mode = tree.prior_mode;
    let p = state.panel_mut(side);
    p.tree = None;
    p.display_mode = prior_mode;
    begin_listing(state, side, target)
}

// ---------------------------------------------------------------------
// Ctrl+J fuzzy jump (M5)
// ---------------------------------------------------------------------

fn is_fuzzy_jump_command(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::FuzzyJumpChar(_) | Command::FuzzyJumpBackspace | Command::FuzzyJumpMove(_) | Command::FuzzyJumpConfirm | Command::FuzzyJumpCancel
    )
}

/// Drive the fuzzy-jump dialog (gated in [`update`] by `state.fuzzy_jump`).
/// The ranked/filtered list itself is never stored on the dialog — it is
/// recomputed from `state.dir_history` each time via
/// [`quicksearch::rank_directories`], which is why every arm re-derives it
/// rather than caching (fuzzy-jump "Fuzzy matching of visited directories",
/// "Frecency ranking").
fn handle_fuzzy_jump(state: &mut State, cmd: Command) -> Vec<Effect> {
    if state.fuzzy_jump.is_none() {
        return vec![];
    }
    match cmd {
        Command::FuzzyJumpChar(c) => state.fuzzy_jump.as_mut().unwrap().push(c),
        Command::FuzzyJumpBackspace => state.fuzzy_jump.as_mut().unwrap().backspace(),
        Command::FuzzyJumpMove(delta) => {
            let pattern = state.fuzzy_jump.as_ref().unwrap().pattern.clone();
            let len = quicksearch::rank_directories(&state.dir_history, &pattern, state.clock_ms).len();
            state.fuzzy_jump.as_mut().unwrap().move_cursor(delta, len);
        }
        Command::FuzzyJumpCancel => state.fuzzy_jump = None,
        Command::FuzzyJumpConfirm => {
            let fj = state.fuzzy_jump.take().unwrap();
            let ranked = quicksearch::rank_directories(&state.dir_history, &fj.pattern, state.clock_ms);
            if let Some(path) = ranked.get(fj.cursor).map(|e| e.path.clone()) {
                let side = state.active;
                return begin_listing(state, side, path);
            }
        }
        _ => {}
    }
    vec![]
}

// ---------------------------------------------------------------------
// Alt+F7 find file (M5)
// ---------------------------------------------------------------------

fn is_find_file_command(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::FindFileChar(_)
            | Command::FindFileBackspace
            | Command::FindFileSubmit
            | Command::FindFileMatch { .. }
            | Command::FindFileSearchDone { .. }
            | Command::FindFileMove(_)
            | Command::FindFileConfirm
            | Command::FindFileCancel
    )
}

/// Drive the find-file dialog (gated in [`update`] by `state.find_file`).
fn handle_find_file(state: &mut State, cmd: Command) -> Vec<Effect> {
    if state.find_file.is_none() {
        return vec![];
    }
    match cmd {
        Command::FindFileChar(c) => state.find_file.as_mut().unwrap().push(c),
        Command::FindFileBackspace => state.find_file.as_mut().unwrap().backspace(),
        Command::FindFileSubmit => {
            let request = state.next_request_id();
            let ff = state.find_file.as_mut().unwrap();
            ff.submit(request);
            let root = ff.root.clone();
            let pattern = ff.pattern.clone();
            return vec![Effect::FindInSubtree { root, pattern, request }];
        }
        Command::FindFileMatch { request, m } => state.find_file.as_mut().unwrap().push_match(request, m),
        Command::FindFileSearchDone { request } => state.find_file.as_mut().unwrap().mark_done(request),
        Command::FindFileMove(delta) => state.find_file.as_mut().unwrap().move_cursor(delta),
        // Dismissing abandons any in-progress search: the walk itself has
        // no cancel signal (like `git_info::query`, it simply finishes and
        // its late `FindFileMatch`/`FindFileSearchDone` replies find
        // `state.find_file` already `None` and are silently dropped by the
        // `state.find_file.is_some()` gate in `update`) (find-file "Esc
        // cancels").
        Command::FindFileCancel => state.find_file = None,
        Command::FindFileConfirm => {
            let ff = state.find_file.take().unwrap();
            let Some(m) = ff.selected().cloned() else { return vec![] };
            let target_dir = match m.relative_path.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => ff.root.join(parent),
                _ => ff.root.clone(),
            };
            let side = state.active;
            let effects = begin_listing(state, side, target_dir);
            state.panel_mut(side).pending_cursor_target = Some(m.entry.name);
            return effects;
        }
        _ => {}
    }
    vec![]
}

// ---------------------------------------------------------------------
// F2 user menu (M5)
// ---------------------------------------------------------------------

fn is_user_menu_command(cmd: &Command) -> bool {
    matches!(cmd, Command::UserMenuMove(_) | Command::UserMenuConfirm | Command::UserMenuCancel)
}

/// Drive the F2 user menu (gated in [`update`] by `state.user_menu`).
/// Entries themselves live in `state.user_menu_entries`, loaded once at
/// startup — the menu overlay only tracks the cursor (user-menu "Open the
/// F2 user menu", "Navigate and dismiss the user menu").
fn handle_user_menu(state: &mut State, cmd: Command) -> Vec<Effect> {
    if state.user_menu.is_none() {
        return vec![];
    }
    match cmd {
        Command::UserMenuMove(delta) => {
            let len = state.user_menu_entries.len();
            state.user_menu.as_mut().unwrap().move_cursor(delta, len);
        }
        Command::UserMenuCancel => state.user_menu = None,
        Command::UserMenuConfirm => {
            let menu = state.user_menu.take().unwrap();
            if let Some(entry) = state.user_menu_entries.get(menu.cursor).cloned() {
                let side = state.active;
                let cwd = state.panel(side).cwd.clone();
                return vec![Effect::RunShellCommand(shell::build_command(state.shell.shell.as_deref(), &entry.command, &cwd), side)];
            }
        }
        _ => {}
    }
    vec![]
}

// ---------------------------------------------------------------------
// Options -> Themes picker (visual-themes)
// ---------------------------------------------------------------------

fn is_theme_picker_command(cmd: &Command) -> bool {
    matches!(cmd, Command::ThemePickerMove(_) | Command::ThemePickerConfirm | Command::ThemePickerCancel)
}

/// Drive the Options → Themes picker (gated in [`update`] by
/// `state.theme_picker`). Enter applies the highlighted theme immediately —
/// swapping `state.theme` in this same reducer step, so the very next frame
/// renders with it — and returns `Effect::PersistTheme` so the TUI event
/// loop writes it to `config.toml`; Esc closes the dialog leaving both the
/// active theme and the config file untouched (theme-selection "Picker
/// navigation, apply, and cancel").
fn handle_theme_picker(state: &mut State, cmd: Command) -> Vec<Effect> {
    if state.theme_picker.is_none() {
        return vec![];
    }
    match cmd {
        Command::ThemePickerMove(delta) => {
            state.theme_picker.as_mut().unwrap().move_cursor(delta);
        }
        Command::ThemePickerCancel => state.theme_picker = None,
        Command::ThemePickerConfirm => {
            let picker = state.theme_picker.take().unwrap();
            if let Some(name) = crate::theme::BUILTIN_THEME_NAMES.get(picker.highlight) {
                if let Some(theme) = Theme::by_name(name) {
                    state.theme = theme;
                }
                return vec![Effect::PersistTheme((*name).to_string())];
            }
        }
        _ => {}
    }
    vec![]
}

// ---------------------------------------------------------------------
// F1 Help window + About dialog (M5)
// ---------------------------------------------------------------------

fn is_help_command(cmd: &Command) -> bool {
    matches!(cmd, Command::HelpMove(_) | Command::HelpActivate | Command::HelpCancel)
}

/// Drive the F1 Help window (gated in [`update`] by `state.help`).
fn handle_help(state: &mut State, cmd: Command) -> Vec<Effect> {
    if state.help.is_none() {
        return vec![];
    }
    match cmd {
        Command::HelpMove(delta) => {
            let visible = crate::dialogs::help_topic_visible_rows(crate::dialogs::help_window_height(state.term_size.1));
            state.help.as_mut().unwrap().move_cursor(delta, visible);
        }
        Command::HelpActivate => state.help.as_mut().unwrap().activate(),
        Command::HelpCancel => {
            let mut h = state.help.take().unwrap();
            // `back` returns `true` when it stepped down one level (About
            // -> list, or page -> list) rather than closing the window
            // outright; only then is the window kept open.
            if h.back() {
                state.help = Some(h);
            }
        }
        _ => {}
    }
    vec![]
}

/// The choke point every deliberate navigation of `side`'s directory flows
/// through — Enter into a directory, `..`, a typed `cd`, drive select,
/// Tree's own Enter-to-return, and the M5 fuzzy-jump/find-file dialogs — so
/// it is also where the Ctrl+J frecency history is recorded and persisted
/// (fuzzy-jump "Navigation records history"; design D6). Tree's cursor-move
/// preview of the *opposite* panel deliberately bypasses this (via
/// [`begin_listing_inner`] directly) since that is the tree being browsed,
/// not "the user navigating the active panel into a directory" the
/// fuzzy-jump requirement describes.
fn begin_listing(state: &mut State, side: PanelSide, path: PathBuf) -> Vec<Effect> {
    quicksearch::record_visit(&mut state.dir_history, &path, state.clock_ms);
    let mut effects = begin_listing_inner(state, side, path);
    effects.push(Effect::PersistHistory(config::HistoryFile { commands: state.history.clone(), directories: state.dir_history.clone() }));
    effects
}

fn begin_listing_inner(state: &mut State, side: PanelSide, path: PathBuf) -> Vec<Effect> {
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
        effects.push(Effect::QueryInfo { panel: side, path: path.clone(), request });
    }
    // Every navigation (and re-read) re-issues the git-info query for
    // wherever the panel just landed, regardless of display mode — the
    // branch suffix and marker column are independent of Info mode (git-info
    // "Query re-issued on navigation").
    effects.push(git_info_query_effect(state, side, path));
    effects
}

/// Mint a fresh generation id, record it as `side`'s outstanding git-info
/// request, and build the effect that runs the query on a worker thread
/// (git-info "Background repository detection"; design D3).
fn git_info_query_effect(state: &mut State, side: PanelSide, path: PathBuf) -> Effect {
    let request = state.next_request_id();
    state.panel_mut(side).git_request = Some(request);
    Effect::QueryGitInfo { panel: side, path, request }
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
            // A find-file navigation seeds this with the matched entry's
            // name (`update::handle_find_file`'s `FindFileConfirm`); once
            // this directory's listing has actually landed, settle the
            // cursor on it (find-file "Navigate to a chosen result" —
            // "cursor positioned on the matched entry").
            if let Some(name) = p.pending_cursor_target.take() {
                if let Some(idx) = p.entries.iter().position(|e| e.name == name) {
                    p.cursor = idx;
                    p.cursor_user_moved = true;
                }
            }
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
