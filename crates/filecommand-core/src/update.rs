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
use crate::fs_ops::dialog::{DropButtons, FileOpSetup, RunningDialog};
use crate::fs_ops::{ConflictChoice, ConflictInfo, ErrorChoice, ErrorInfo, Job, JobKind, JobOutcome, ProgressInfo, SkippedItem, SourceItem};
use crate::git_info::GitInfo;
use crate::info::InfoValues;
use crate::listing::{Entry, EntryKind, FindMatch, SortMode};
use crate::menu::{MenuAction, MenuEntry, MenuId, MenuState};
use crate::panel::{ClipboardFeedback, CursorMove, DisplayMode, PanelState, TreeState};
use crate::panel_split;
use crate::quicksearch::{self, FrecencyEntry, FuzzyJumpState};
use crate::shell::{self, ShellConfig};
use crate::theme::Theme;
use crate::viewer::ViewerState;

pub const MIN_COLS: u16 = 60;
pub const MIN_ROWS: u16 = 16;
pub const SPLASH_MIN_HOLD_MS: u64 = 800;
/// How long a clipboard action's mini-status feedback stays up before a
/// `Tick` expires it, absent an intervening key press (clipboard-export
/// "Clipboard feedback").
pub const CLIPBOARD_FEEDBACK_MS: u64 = 3000;

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

/// A mouse click's modifier state, as `input::map_mouse` resolves it from
/// the raw `crossterm::event::KeyModifiers` on a `MouseEvent` — never
/// `crossterm::event::KeyModifiers` itself, which must never cross into this
/// crate (mouse-input "Hit-testing stays in the TUI"; design D2). `Shift` is
/// carried for completeness even though Shift+click range selection is
/// deliberately unspecified (design D3) — most terminal emulators intercept
/// it for native text selection before it ever reaches crossterm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickMods {
    Plain,
    Ctrl,
    Shift,
}

/// Where a drag-and-drop is currently hovering, already validated against
/// mouse-drag's "Valid drop targets" (design D4) — never a raw, unvalidated
/// coordinate hit. `PanelDir`'s `side` stands for that whole panel as a
/// target — its title, blank body area, or any non-directory row all
/// resolve to this same variant, since they all mean "drop into this
/// panel's current directory" (the TUI's hit-testing doesn't need to
/// distinguish them). `SubDir`'s `name` covers the `..` row exactly like an
/// ordinary subdirectory row. `TreeNode` is a Tree-mode panel's node.
/// `Tab`'s `index` is a position in that panel's full ordered tab list
/// (`PanelState::tab_dirs`), standing for that tab's directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropTarget {
    PanelDir(PanelSide),
    SubDir { side: PanelSide, name: OsString },
    TreeNode { side: PanelSide, path: PathBuf },
    Tab { side: PanelSide, index: usize },
}

/// An in-progress mouse drag's frozen identity plus its live target and
/// proposed verb (mouse-drag "Drag lifecycle"; design D4). `source`,
/// `source_dir`, and `items` are captured once at `DragBegin` and never
/// change for the life of the drag — a streamed listing chunk, a re-sort, or
/// even the source panel navigating away changes nothing here (mouse-drag
/// "Robust against listing changes"); `op` and `target` are instead updated
/// by every later `DragOver`/`DragDrop`, since the proposed verb is
/// recomputed from the modifiers of each of those events (design D2) and the
/// hovered target can change on every pointer move. Lives beside `UiPhase`,
/// like `State::menu`/`drive_select`/etc., rather than inside it — a drag
/// overlays the live panels, it is not itself a phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DragState {
    pub source: PanelSide,
    pub source_dir: PathBuf,
    pub items: Vec<SourceItem>,
    pub op: JobKind,
    pub target: Option<DropTarget>,
}

/// Which dialog button a click landed on, resolved by the TUI's hit map
/// (design D2: "Dialog buttons" includes the hotkey text spans of dialogs
/// with no framed buttons, e.g. the conflict dialog's `(O)verwrite  (S)kip
/// …` row). `Command::DialogButtonClick` maps one of these straight onto the
/// exact command the equivalent keypress already issues, via
/// [`button_command`] — a dialog button is never a new way to do something,
/// only a new way to reach an existing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonId {
    DeleteConfirmYes,
    DeleteConfirmNo,
    ConflictOverwrite,
    ConflictOverwriteAll,
    ConflictSkip,
    ConflictSkipAll,
    ConflictRename,
    ErrorRetry,
    ErrorSkip,
    ErrorSkipAll,
    ErrorAbort,
    ProgressCancel,
    /// The skipped-items summary's "Press any key to continue" footer —
    /// there is no literal button glyph, but design D5 gates this dialog as
    /// buttons-only, so the whole footer row is recorded as one clickable
    /// span (mirroring how `Command::FileOpConfirm` is what any key already
    /// does here).
    SummaryContinue,
    QuitYes,
    QuitNo,
    /// The drop-initiated destination dialog's `[ Copy ]` button
    /// (operation-dialogs "Drop-initiated destination dialog").
    DropDialogCopy,
    /// Its `[ Move ]` button.
    DropDialogMove,
    /// Its `[ Cancel ]` button — routes to exactly the same command as the
    /// keyboard dialog's Esc.
    DropDialogCancel,
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
    /// Terminal is below the 60x16 hard floor. Growing back always resolves
    /// to `Panels`, never back to `Splash`.
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
    /// The vertical panel split, stored as a left-panel percentage
    /// (default 50); `filecommand-tui::layout::compute` derives the
    /// effective column split from this via
    /// `panel_split::effective_left_width` (panel-split "Split ratio
    /// semantics and panel minimum").
    pub split_percent: u16,
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
    /// A mouse drag-and-drop in progress, or `None` otherwise
    /// (mouse-panel-drag). Lives beside the phase like `menu`/`drive_select`
    /// — it overlays the live panels rather than replacing them. Cleared by
    /// a reducer post-condition (see `update`'s wrapper below) whenever a
    /// command leaves `UiPhase::Panels` or opens any of the overlays above,
    /// so it can never survive into a context where a drop could complete
    /// (mouse-drag "Cancel and phase-change clear the drag"; design D5).
    pub drag: Option<DragState>,
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

    /// The theme every renderer should draw with this frame: while the
    /// theme picker is open, the currently *highlighted* built-in theme
    /// (falling back to the active theme on a lookup miss, which never
    /// happens in practice since the highlight is always clamped within
    /// `BUILTIN_THEME_NAMES`); otherwise the applied `state.theme`. This is
    /// a pure, render-only derivation — it never mutates `state.theme` or
    /// touches persistence (theme-selection: "Live theme preview while the
    /// picker is open").
    pub fn render_theme(&self) -> Theme {
        if let Some(picker) = &self.theme_picker {
            if let Some(name) = crate::theme::BUILTIN_THEME_NAMES.get(picker.highlight) {
                if let Some(theme) = Theme::by_name(name) {
                    return theme;
                }
            }
        }
        self.theme.clone()
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
            split_percent: panel_split::DEFAULT_SPLIT_PERCENT,
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
            drag: None,
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

    // Adjustable panel split (Ctrl+Left/Ctrl+Right/Ctrl+=; panel-split
    // "Adjust and reset the panel split").
    /// Ctrl+Right: move the divider `panel_split::SPLIT_STEP` columns
    /// right (grows the left panel). A no-op at the right panel's minimum.
    SplitGrow,
    /// Ctrl+Left: move the divider `panel_split::SPLIT_STEP` columns left
    /// (shrinks the left panel). A no-op at the left panel's minimum.
    SplitShrink,
    /// Ctrl+=: reset the split to 50/50, unconditionally.
    SplitReset,
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
    /// The drop-initiated destination dialog's `[ Copy ]`/`[ Move ]` buttons:
    /// confirms exactly as `Command::FileOpConfirm` does, but commits the
    /// given verb regardless of whichever `kind` the dialog opened with —
    /// the only way to switch the verb without reopening the dialog
    /// (operation-dialogs "Switching the verb in the dialog"). A no-op
    /// outside `FileOpSetup::DestinationInput` (`DeleteConfirm`/
    /// `RenameInput` never carry a button row that could produce this).
    FileOpConfirmAs(JobKind),

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
    /// Enter: on a user entry, run its command via the shell passthrough in
    /// the active panel's directory (user-menu "Run the selected entry's
    /// command via the shell in the active panel directory"); on the
    /// compiled-in built-in Themes slot (cursor at `entries.len()`), open
    /// the theme picker instead — no shell effect (user-menu "Built-in
    /// Themes entry opens the theme selector"). Either way the user menu
    /// closes first.
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

    // Clipboard export (clipboard-export). Reachable from Ctrl+C/Ctrl+Ins
    // and Ctrl+Shift+Ins over the panels, the Files pull-down's three-item
    // group, and the file-action menu's `Send to clipboard` (which always
    // requests `Files`, scoped to its single target entry rather than
    // `active_selection_sources` — see `activate_file_action`).
    /// Places the F5-scope entries (`active_selection_sources`) on the OS
    /// clipboard per `kind` (clipboard-export "Clipboard payloads and
    /// scope"). A no-op, reported in the mini-status, when the scope is
    /// empty (nothing selected and the cursor is on `..` or off the list).
    CopyToClipboard(ClipboardPayloadKind),
    /// Reply to `Effect::SetClipboard` once the TUI's `Clipboard` trait has
    /// run. `payload` echoes back the request (the resolved paths are
    /// needed to render the singular "Path copied: <path>" message without
    /// re-deriving scope); `fell_back_to_paths` is set only when a `Files`
    /// request was carried out as a plain-text `Paths` write because file
    /// objects aren't supported on this platform (clipboard-export
    /// "Non-Windows fallback"). `Ok(())` shows the per-kind success
    /// message; `Err(message)` shows `message` verbatim in the error role
    /// (clipboard-export "Clipboard busy retry" — the TUI retries before
    /// giving up, so by the time this arrives the failure is final).
    ClipboardResult { payload: ClipboardPayload, fell_back_to_paths: bool, result: Result<(), String> },

    // Mouse (mouse-basics). `input::map_mouse` is the only source of these —
    // raw coordinates and `crossterm::event::KeyModifiers` never reach this
    // crate (mouse-input "Hit-testing stays in the TUI"; design D2).
    /// A left-click on an entry row: focuses `side` and moves its cursor to
    /// the named entry, without changing the selection set (`Plain`), or
    /// toggles that entry's selection in place (`Ctrl`) — never both
    /// (mouse-input "Click focuses and places the cursor", "Ctrl+click
    /// toggles selection"). `input::map_mouse` also turns a same-row
    /// double-click into a plain `Command::Enter` instead of this (mouse-
    /// input "Double-click acts as Enter").
    ClickEntry { side: PanelSide, name: OsString, mods: ClickMods },
    /// A left-click on a panel's title or blank body area: focuses `side`
    /// without moving its cursor (mouse-input "Click focuses and places the
    /// cursor").
    FocusPanel(PanelSide),
    /// A wheel notch (or several, coalesced — design D7) over `side`'s
    /// panel: move its cursor by `delta` rows (positive down, negative up),
    /// independent of which panel is active (mouse-input "Wheel moves the
    /// cursor of the panel under the pointer"). The viewer/editor have their
    /// own wheel handling that never reaches this command (design D6).
    ScrollPanel { side: PanelSide, delta: isize },
    /// A left-click on a function-key-bar slot (1..=10, `10` for F10):
    /// dispatches exactly what that F-key would (mouse-input "Key bar, menu
    /// bar, pull-down items, and dialog buttons are clickable").
    KeybarPress(u8),
    /// A left-click on a menu-bar title: opens that pull-down (or switches
    /// to it, if a different one is already open).
    MenuTitleClick(MenuId),
    /// A left-click on an open pull-down's `index`'th row: activates it
    /// exactly as `Command::MenuActivate` would once the highlight is there.
    MenuItemClick(usize),
    /// A left-click on a dialog button (or a buttonless dialog's hotkey text
    /// span — design D2): activates it exactly as the equivalent key would,
    /// via [`button_command`].
    DialogButtonClick(ButtonId),
    /// A right-click on an entry row: moves the cursor to the named entry
    /// and opens the file-action menu for it (mouse-input "Right-click opens
    /// the action menu"). `input::map_mouse` reaches this only for files
    /// until the directory-target support of `file-action-menu` "Directory
    /// targets and selection-scoped invocation" lands (mouse-basics section
    /// 3).
    OpenActionMenuAt { side: PanelSide, name: OsString },

    // Mouse drag-and-drop (mouse-panel-drag; design D2/D4). `op` on each of
    // these is the verb the drag's button/modifiers propose right now,
    // recomputed by the TUI on every drag/release event and carried fresh
    // rather than derived once and cached (design D2: "recomputed ... on
    // every drag event"). `target`/`candidate` are the TUI's raw geometric
    // hit-test result — a `DropTarget` or `None` for "off any potential
    // target region" — which `update` is responsible for validating (mouse-
    // drag "Valid drop targets"); core never receives raw coordinates.
    /// A press on an entry row moved at least one cell (mouse-basics'
    /// `press.moved`, mouse-panel-drag's territory): freezes the drag's
    /// items — `name`'s panel's selection set if `name` is a member of it,
    /// else `name` alone, never `..` — and its initial proposed verb
    /// (mouse-drag "Drag lifecycle"). A no-op if `name` no longer names a
    /// selectable entry.
    DragBegin { side: PanelSide, name: OsString, op: JobKind },
    /// The drag's pointer resolved to a new de-duplicated position (the
    /// TUI's `MouseTracker` owns de-duplication, so core only sees actual
    /// changes — design D4). A no-op if no drag is in progress.
    DragOver { op: JobKind, target: Option<DropTarget> },
    /// The button released. Opens the drop-initiated destination dialog if
    /// the drag's target is (still) valid and the source panel still shows
    /// the captured directory; otherwise ends the drag with no effect
    /// (mouse-drag "Release on an invalid spot", "Robust against listing
    /// changes"). A no-op if no drag is in progress.
    DragDrop { op: JobKind },
    /// Esc mid-drag: ends the drag with no effect (mouse-drag "Esc
    /// cancels").
    DragCancel,
}

/// Which of the three clipboard actions a `Command::CopyToClipboard` /
/// `Effect::SetClipboard` targets (clipboard-export "Clipboard payloads and
/// scope"). `Names` is menu-only — reachable only through the Files
/// pull-down, never a key binding (design D2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardPayloadKind {
    /// `CF_HDROP` file objects on Windows; falls back to `Paths`-as-text
    /// elsewhere (clipboard-export "Windows file-object payload",
    /// "Non-Windows fallback").
    Files,
    /// One absolute path per line, plain text.
    Paths,
    /// One file name per line, plain text.
    Names,
}

/// The resolved payload for an `Effect::SetClipboard`: the action kind and
/// the absolute, `..`-excluded paths of the F5-scope entries
/// (`active_selection_sources`/`named_entry_source`) it was built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardPayload {
    pub kind: ClipboardPayloadKind,
    pub items: Vec<PathBuf>,
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
    /// Rewrite `config.toml`'s `panel_split =` key atomically after a
    /// successful split adjustment or reset (panel-split "Split
    /// persistence to configuration").
    PersistPanelSplit(u16),
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
    /// Write `payload` to the OS clipboard via the TUI's `Clipboard` trait
    /// and report the outcome back via `Command::ClipboardResult`. Executed
    /// synchronously, like `EnumerateDrives` — `OpenClipboard` binds to the
    /// calling thread and set-and-close is sub-millisecond, so no worker
    /// thread is needed (design D3; clipboard-export "Clipboard payloads
    /// and scope").
    SetClipboard(ClipboardPayload),
}

/// The pure state transition. Equal `(state, command)` always yields equal
/// `(state, Vec<Effect>)`. A thin wrapper around [`update_impl`] that
/// enforces the one invariant that must hold no matter which internal path a
/// command took: `state.drag` is never `Some` outside `UiPhase::Panels` with
/// no overlay open (mouse-drag "Cancel and phase-change clear the drag";
/// design D5) — see [`drag_allowed`]. Applying it here, once, after every
/// call — rather than threading a clear into each of `update_impl`'s many
/// phase-transition arms — is what makes it a true postcondition: every
/// return path is covered by construction, including ones a future phase
/// addition might add without remembering this rule.
pub fn update(state: State, cmd: Command) -> (State, Vec<Effect>) {
    let listing_failed = matches!(cmd, Command::ListingFailed { .. });
    let (mut state, effects) = update_impl(state, cmd);
    // A listing failure doesn't change `state.phase` or open any overlay —
    // `drag_allowed` alone wouldn't catch it — but mouse-drag's "Cancel and
    // phase-change clear the drag" explicitly lists listing failure as a
    // trigger: the failed panel's contents are no longer trustworthy enough
    // to resolve a drop against.
    if listing_failed || !drag_allowed(&state) {
        state.drag = None;
    }
    (state, effects)
}

/// Whether `state.drag` is allowed to remain `Some`: only in
/// `UiPhase::Panels` with none of the modal overlays below open — the same
/// set `input::mouse::context` (mouse-basics) already gates ordinary mouse
/// input behind, extended with `quit_confirm` since it uniquely overlays
/// every other context too (mouse-drag "Cancel and phase-change clear the
/// drag": job completion, listing failure, F9, quit request, resize below
/// the minimum — every one of those either leaves `UiPhase::Panels` or sets
/// one of these fields).
fn drag_allowed(state: &State) -> bool {
    matches!(state.phase, UiPhase::Panels)
        && state.menu.is_none()
        && state.drive_select.is_none()
        && state.fuzzy_jump.is_none()
        && state.find_file.is_none()
        && state.user_menu.is_none()
        && state.theme_picker.is_none()
        && state.file_action_menu.is_none()
        && state.help.is_none()
        && state.startup_warning.is_none()
        && !state.quit_confirm
}

/// The actual state-transition logic. Never call this directly outside
/// [`update`]'s own recursive re-entry (a dialog-button click, an activated
/// menu item, a key-bar slot) — every one of those re-enters here rather
/// than through the public `update`, so the drag-clearing postcondition runs
/// exactly once per external call instead of once per internal recursion.
fn update_impl(mut state: State, cmd: Command) -> (State, Vec<Effect>) {
    // A dialog button click is never a new way to do something — only a new
    // way to reach an existing keyboard command — so it translates via
    // `button_command` and re-enters here with that command, independent of
    // phase: the TUI's mode-gating table (design D5) only ever dispatches
    // this while the matching dialog is actually on screen (e.g. `QuitYes`/
    // `QuitNo` only while `state.quit_confirm` is set), so no extra gating is
    // needed here.
    if let Command::DialogButtonClick(id) = cmd {
        return match button_command(id) {
            Some(next) => update_impl(state, next),
            None => (state, Vec::new()),
        };
    }

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
        // A shrinking terminal can leave either panel's cursor below its new,
        // shorter window; re-clamp both sides unconditionally (panel-
        // navigation "Terminal resize re-clamps").
        reconcile_panel_viewport(&mut state, PanelSide::Left);
        reconcile_panel_viewport(&mut state, PanelSide::Right);
        return (state, effects);
    }

    // Every `Tick` updates the clock reading `begin_listing` timestamps
    // frecency visits with, regardless of phase — mirrored generically here
    // rather than duplicated in every phase arm that might navigate. A tick
    // also expires any clipboard feedback whose ~3s has elapsed
    // (clipboard-export "Clipboard feedback"); every other command counts
    // as "the next key press" and clears it immediately instead of waiting
    // out the timer — except the two commands that manage the feedback
    // themselves, whose own handling below sets it fresh.
    if let Command::Tick(now) = cmd {
        state.clock_ms = now;
        for side in [PanelSide::Left, PanelSide::Right] {
            if matches!(&state.panel(side).clipboard_feedback, Some(f) if now >= f.expires_at_ms) {
                state.panel_mut(side).clipboard_feedback = None;
            }
        }
    } else if !matches!(cmd, Command::CopyToClipboard(_) | Command::ClipboardResult { .. }) {
        state.left.clipboard_feedback = None;
        state.right.clipboard_feedback = None;
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
            // Expansion can grow the flattened node list arbitrarily; the
            // cursor itself doesn't move, but a growing list can overflow
            // the window it previously fit in, so re-clamp (additional-
            // panel-modes "Expanding a directory can overflow and shows the
            // scrollbar").
            reconcile_panel_viewport(&mut state, panel);
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
            // An activated item re-enters as the command it stands for, so a
            // menu action and its keyboard shortcut share one implementation.
            let (state, more) = update_impl(state, next);
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
                    reconcile_panel_viewport(&mut state, side);
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
            Command::ToggleSelectAtCursor => {
                let side = state.active;
                state.panel_mut(side).toggle_selection_and_advance();
                reconcile_panel_viewport(&mut state, side);
            }
            Command::GroupSelectAll => state.panel_mut(state.active).select_matching("*"),
            Command::GroupDeselectAll => state.panel_mut(state.active).deselect_matching("*"),
            Command::InvertSelection => state.panel_mut(state.active).invert_selection(),
            Command::RequestCopy => enter_file_op_setup(&mut state, JobKind::Copy),
            Command::RequestMove => enter_file_op_setup(&mut state, JobKind::Move),
            Command::RequestMkdir => enter_file_op_setup(&mut state, JobKind::Mkdir),
            Command::RequestDelete => enter_delete_confirm(&mut state),
            Command::CopyToClipboard(kind) => effects.extend(handle_copy_to_clipboard(&mut state, kind)),
            Command::ClipboardResult { payload, fell_back_to_paths, result } => {
                apply_clipboard_result(&mut state, payload, fell_back_to_paths, result)
            }

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

            Command::SetSortMode { side, mode } => {
                state.panel_mut(side).set_sort_mode(mode);
                reconcile_panel_viewport(&mut state, side);
            }

            Command::QuickFilterStart => state.panel_mut(state.active).activate_quick_filter(),
            Command::QuickFilterChar(c) => {
                let side = state.active;
                state.panel_mut(side).quick_filter_push(c);
                reconcile_panel_viewport(&mut state, side);
            }
            Command::QuickFilterBackspace => {
                let side = state.active;
                state.panel_mut(side).quick_filter_backspace();
                reconcile_panel_viewport(&mut state, side);
            }
            Command::QuickFilterEnd => {
                let side = state.active;
                state.panel_mut(side).clear_quick_filter();
                reconcile_panel_viewport(&mut state, side);
            }

            Command::OpenTab => {
                let side = state.active;
                // The new tab inherits the active tab's own (never-stale)
                // state, so it can never itself come up stale — no
                // fresh-read check needed here (panel-tabs "Stale
                // background tab refresh on activation").
                state.panel_mut(side).open_tab();
                reconcile_panel_viewport(&mut state, side);
            }
            Command::CloseTab => {
                let side = state.active;
                if state.panel_mut(side).close_tab() {
                    let path = state.panel(side).cwd.clone();
                    effects.extend(begin_listing(&mut state, side, path));
                }
                reconcile_panel_viewport(&mut state, side);
            }
            Command::SwitchTab(n) => {
                let side = state.active;
                if state.panel_mut(side).switch_tab(n) {
                    let path = state.panel(side).cwd.clone();
                    effects.extend(begin_listing(&mut state, side, path));
                }
                reconcile_panel_viewport(&mut state, side);
            }

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

            Command::SplitGrow => effects.extend(adjust_split(&mut state, panel_split::SPLIT_STEP as i32)),
            Command::SplitShrink => effects.extend(adjust_split(&mut state, -(panel_split::SPLIT_STEP as i32))),
            Command::SplitReset => {
                if state.split_percent != panel_split::DEFAULT_SPLIT_PERCENT {
                    state.split_percent = panel_split::DEFAULT_SPLIT_PERCENT;
                    effects.push(Effect::PersistPanelSplit(state.split_percent));
                }
            }

            // Mouse (mouse-basics; design D2).
            Command::FocusPanel(side) => state.active = side,
            Command::ClickEntry { side, name, mods } => effects.extend(handle_click_entry(&mut state, side, name, mods)),
            Command::ScrollPanel { side, delta } => effects.extend(handle_scroll_panel(&mut state, side, delta)),
            Command::KeybarPress(slot) => {
                if let Some(next) = keybar_command(slot) {
                    return update_impl(state, next);
                }
            }
            Command::MenuTitleClick(id) => state.menu = Some(MenuState::for_menu(id)),
            Command::MenuItemClick(index) => {
                if let Some(next) = handle_menu_item_click(&mut state, index) {
                    return update_impl(state, next);
                }
            }
            Command::OpenActionMenuAt { side, name } => effects.extend(handle_open_action_menu_at(&mut state, side, name)),

            // Mouse drag-and-drop (mouse-panel-drag; design D2/D4).
            Command::DragBegin { side, name, op } => {
                let items = drag_selection_sources(&state, side, &name);
                if !items.is_empty() {
                    let source_dir = state.panel(side).cwd.clone();
                    state.drag = Some(DragState { source: side, source_dir, items, op, target: None });
                }
            }
            Command::DragOver { op, target } => {
                // Both borrows below are immutable — computed fully before
                // the mutable one that follows — so there's no conflict with
                // `state.drag.as_mut()` afterwards.
                let valid_target =
                    state.drag.as_ref().and_then(|drag| target.as_ref().filter(|t| valid_drop_target(&state, drag, t)).cloned());
                if let Some(drag) = state.drag.as_mut() {
                    drag.op = op;
                    drag.target = valid_target;
                }
            }
            Command::DragDrop { op } => {
                if let Some(mut drag) = state.drag.take() {
                    drag.op = op;
                    // "Robust against listing changes": the source panel
                    // must still show the directory the items were captured
                    // from, and the target must still resolve to a live
                    // directory (mouse-drag "Robust against listing
                    // changes").
                    let source_ok = state.panel(drag.source).cwd == drag.source_dir;
                    let resolved = if source_ok {
                        drag.target.as_ref().filter(|t| valid_drop_target(&state, &drag, t)).and_then(|t| drop_target_path(&state, t))
                    } else {
                        None
                    };
                    if let Some(target_path) = resolved {
                        let prefill = target_path.display().to_string();
                        enter_file_op_setup_for_sources(
                            &mut state,
                            drag.op,
                            drag.items,
                            drag.source,
                            prefill,
                            Some(DropButtons { focused: drag.op }),
                        );
                    }
                    // Otherwise: invalid/cancelled — the drag is already
                    // taken (cleared), and nothing else happens (mouse-drag
                    // "Release on an invalid spot").
                }
            }
            Command::DragCancel => state.drag = None,

            Command::Tick(_) => {}
            Command::ConfirmQuit | Command::CancelQuit | Command::Resize(..) | Command::DialogButtonClick(_) => unreachable!("handled above"),
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

/// Run the typed line: only the three built-in verbs (`cd`, `del`,
/// `rmdir`) are recognized (command-line "Command-line builtin
/// whitelist") — anything else is rejected without spawning anything. The
/// file-action menu's Run entry and the F2 user menu build their own
/// `Effect::RunShellCommand` independently and are unaffected by this
/// dispatch (design.md "`Effect::RunShellCommand` and `shell::
/// build_command` are untouched").
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
        return dispatch_cd(state, &text, &target);
    }
    if let Some(target) = parse_del(&text) {
        return dispatch_delete_builtin(state, &target, false);
    }
    if let Some(target) = parse_rmdir(&text) {
        return dispatch_delete_builtin(state, &target, true);
    }

    // Unrecognized: reject outright — no process spawns, and (matching
    // `cd`'s own rejection below) nothing is added to command history
    // (command-line "An unrecognized command is rejected without spawning
    // anything").
    let side = state.active;
    let word = text.split_whitespace().next().unwrap_or(text.as_str());
    state.panel_mut(side).last_error = Some(format!("'{word}' is not a recognized command"));
    vec![]
}

/// `cd <path>`: navigate the active panel to `target` resolved against its
/// cwd, but only after confirming the resolved path exists and is a
/// directory — a nonexistent or non-directory target is rejected outright,
/// leaving `cwd` untouched and starting no listing (command-line "cd
/// navigates the active panel or rejects a nonexistent target"; this is
/// also what fixes the panel switching into a nonexistent directory).
fn dispatch_cd(state: &mut State, text: &str, target: &str) -> Vec<Effect> {
    let side = state.active;
    let Some(path) = resolve_cd_target(&state.panel(side).cwd, target) else {
        // ".." at a drive root, the only case `resolve_cd_target` itself
        // rejects — there is nowhere to navigate to.
        state.panel_mut(side).last_error = Some(format!("{target} not found"));
        return vec![];
    };
    match probe_command_line_target(state, side, &path) {
        Some((true, _)) => {
            config::push_history(&mut state.history, text);
            let mut effects =
                vec![Effect::PersistHistory(config::HistoryFile { commands: state.history.clone(), directories: state.dir_history.clone() })];
            effects.extend(begin_listing(state, side, path));
            effects
        }
        Some((false, _)) => {
            state.panel_mut(side).last_error = Some(format!("{} is not a directory", path.display()));
            vec![]
        }
        None => {
            state.panel_mut(side).last_error = Some(format!("{} not found", path.display()));
            vec![]
        }
    }
}

/// `del <target>` (`want_dir == false`) / `rmdir <target>`
/// (`want_dir == true`): resolve `target` against the active panel's cwd
/// and, for an existing target of the matching type, open the same F8
/// delete-confirmation dialog `enter_delete_confirm` uses — never deleting
/// directly. A missing target or a type mismatch (`del` on a directory,
/// `rmdir` on a file) is rejected: no dialog opens (command-line "del and
/// rmdir route into the existing delete-confirmation flow").
fn dispatch_delete_builtin(state: &mut State, target: &str, want_dir: bool) -> Vec<Effect> {
    let side = state.active;
    let Some(path) = resolve_cd_target(&state.panel(side).cwd, target) else {
        state.panel_mut(side).last_error = Some(format!("{target} not found"));
        return vec![];
    };
    match probe_command_line_target(state, side, &path) {
        Some((is_dir, original_name)) if is_dir == want_dir => {
            enter_delete_confirm_for_sources(state, vec![SourceItem { original_name, path, is_dir }]);
            vec![]
        }
        Some((true, _)) => {
            state.panel_mut(side).last_error = Some(format!("{} is a directory", path.display()));
            vec![]
        }
        Some((false, _)) => {
            state.panel_mut(side).last_error = Some(format!("{} is not a directory", path.display()));
            vec![]
        }
        None => {
            state.panel_mut(side).last_error = Some(format!("{} not found", path.display()));
            vec![]
        }
    }
}

/// Whether `path` exists and, if so, whether it's a directory, plus its
/// on-disk name — preferring the active panel's already-listed entries (no
/// I/O, and the exact on-disk-cased name) when `path` names a currently
/// visible direct child of the panel's directory, and otherwise falling
/// back to a single synchronous filesystem check. `None` means the target
/// doesn't exist. Shared by `cd`'s existence check and `del`/`rmdir`'s
/// target resolution (design.md decisions on both).
fn probe_command_line_target(state: &State, side: PanelSide, path: &Path) -> Option<(bool, OsString)> {
    let panel = state.panel(side);
    if let (Some(name), Some(parent)) = (path.file_name(), path.parent()) {
        if parent == panel.cwd.as_path() {
            if let Some(entry) = panel.entries.iter().find(|e| e.name == name) {
                return Some((entry.is_dir_like(), entry.name.clone()));
            }
        }
    }
    let metadata = std::fs::metadata(path).ok()?;
    let name = path.file_name().map(OsString::from).unwrap_or_else(|| OsString::from(path.display().to_string()));
    Some((metadata.is_dir(), name))
}

/// The target of a `cd <path>` line, or `None` if this isn't a `cd`.
pub fn parse_cd(text: &str) -> Option<String> {
    parse_builtin_arg(text, "cd")
}

/// The target of a `del <target>` line, or `None` if this isn't a `del`.
pub fn parse_del(text: &str) -> Option<String> {
    parse_builtin_arg(text, "del")
}

/// The target of a `rmdir <target>` line, or `None` if this isn't a
/// `rmdir`.
pub fn parse_rmdir(text: &str) -> Option<String> {
    parse_builtin_arg(text, "rmdir")
}

/// Strip a case-insensitive `"<verb> "` prefix (`cd`/`CD`/`Cd`/...,
/// matching classic NC/cmd usage — command-line "Builtin verbs are
/// case-insensitive") and return the trimmed, unquoted argument, or `None`
/// if `text` isn't `verb` or carries no argument.
fn parse_builtin_arg(text: &str, verb: &str) -> Option<String> {
    let trimmed = text.trim();
    // `get` (rather than `split_at`) never panics on a non-ASCII `trimmed`
    // whose byte offset `verb.len()` doesn't land on a char boundary — it
    // just reports no match, which is the correct outcome either way.
    let head = trimmed.get(..verb.len())?;
    if !head.eq_ignore_ascii_case(verb) {
        return None;
    }
    let rest = &trimmed[verb.len()..];
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
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
    reconcile_panel_viewport(state, side);
}

// ---------------------------------------------------------------------
// Adjustable panel split (panel-split)
// ---------------------------------------------------------------------

/// Ctrl+Left/Ctrl+Right: move the divider `delta_cols` columns (negative =
/// left, positive = right) via `panel_split::adjust_percent`, updating and
/// persisting `state.split_percent` only when the adjustment doesn't
/// violate either panel's minimum width — an adjustment at the limit is a
/// no-op, per panel-split "Adjustment at the limit is a no-op".
fn adjust_split(state: &mut State, delta_cols: i32) -> Vec<Effect> {
    match panel_split::adjust_percent(state.split_percent, delta_cols, state.term_size.0) {
        Some(new_percent) => {
            state.split_percent = new_percent;
            vec![Effect::PersistPanelSplit(new_percent)]
        }
        None => vec![],
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

/// A panel's body entry-row count for a given terminal height, display mode,
/// and tab count — the same shape as [`editor_viewport`], kept as a
/// duplicate of `filecommand-tui`'s `layout::compute` +
/// `views::panel::render` geometry rather than a cross-crate dependency, so
/// `core::update` can call [`PanelState::ensure_cursor_visible`] without
/// depending on `filecommand-tui` (design D2). Only the terminal height
/// matters here — the command-line and F-key bar rows are shared by both
/// panels regardless of the panel split, and `split_percent` only ever
/// changes each panel's *width*, which entry-row counting doesn't depend on
/// (Brief mode's column count, a width-derived quantity, is the
/// `additional-panel-modes` follow-up group's concern, not this one's).
///
/// Mirrors, in order: `layout::compute`'s `panels_h` (terminal height minus
/// the one-row command line and one-row F-key bar), `views::panel::render`'s
/// `has_strip`/`reserved` (a tab strip, shown at 2+ tabs, costs a row beyond
/// the top+bottom border), and its `rows_h` per display mode — Full and Tree
/// both reserve a header row on top of the body, Brief does not.
///
/// `pub` so `filecommand-tui` can also derive the PgUp/PgDn paging step from
/// it directly, in place of the layout-level `entries_visible` (which
/// over-counts by one when the tab strip is visible and mismatches Brief) —
/// one source of truth for both the viewport clamp and the paging step
/// (design D2's risk note).
pub fn panel_viewport_rows(term_size: (u16, u16), display_mode: DisplayMode, tab_count: usize) -> usize {
    let panels_h = term_size.1.saturating_sub(2);
    let has_strip = panels_h >= 4 && tab_count >= 2;
    let reserved: u16 = if has_strip { 3 } else { 2 };
    let body_h = panels_h.saturating_sub(reserved);
    let rows = match display_mode {
        DisplayMode::Full | DisplayMode::Tree => body_h.saturating_sub(1),
        _ => body_h,
    };
    rows.max(1) as usize
}

/// `side`'s panel interior width (the panel's rendered width, border
/// columns subtracted) for the current terminal width and split — mirrors
/// `layout::compute`'s `left_w`/`right_w` (via
/// `panel_split::effective_left_width`) and `views::panel::render`'s
/// `w.saturating_sub(2)`, kept as a duplicate here for the same reason
/// [`panel_viewport_rows`] is: `core::update` needs it (for Brief mode's
/// column count) without a cross-crate dependency on `filecommand-tui`.
fn panel_interior_width(term_size: (u16, u16), split_percent: u16, side: PanelSide) -> u16 {
    let left_w = panel_split::effective_left_width(split_percent, term_size.0);
    let panel_w = match side {
        PanelSide::Left => left_w,
        PanelSide::Right => term_size.0.saturating_sub(left_w),
    };
    panel_w.saturating_sub(2)
}

/// Brief mode's column count for a given interior width: `max(1,
/// floor(interior_w / 12))`, byte-identical to `filecommand-tui`'s
/// `brief_column_widths`/`render_brief_body` formula (design D4;
/// additional-panel-modes "Brief mode column scrolling").
fn brief_column_count(interior_w: u16) -> usize {
    ((interior_w / 12).max(1)) as usize
}

/// Re-clamp `side`'s scroll offset against its current viewport — the one
/// call site every cursor-moving or list-mutating path funnels through
/// (design D6), so the renderer never has to reason about scrolling itself.
/// Dispatches to the mode-appropriate clamp: Brief's column-window
/// reconciliation, Tree's over its own `TreeState::scroll_offset` (a no-op
/// if Tree mode was already left before this ran), or Full/Info/QuickView's
/// line-wise `PanelState::ensure_cursor_visible`.
fn reconcile_panel_viewport(state: &mut State, side: PanelSide) {
    let panel = state.panel(side);
    let display_mode = panel.display_mode;
    let rows = panel_viewport_rows(state.term_size, display_mode, panel.tab_count());
    match display_mode {
        DisplayMode::Brief => {
            let interior_w = panel_interior_width(state.term_size, state.split_percent, side);
            let cols = brief_column_count(interior_w);
            state.panel_mut(side).ensure_cursor_visible_brief(rows, cols);
        }
        DisplayMode::Tree => {
            if let Some(tree) = state.panel_mut(side).tree.as_mut() {
                tree.ensure_cursor_visible(rows);
            }
        }
        _ => {
            state.panel_mut(side).ensure_cursor_visible(rows);
        }
    }
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
        MenuAction::ClipboardFiles => Some(Command::CopyToClipboard(ClipboardPayloadKind::Files)),
        MenuAction::ClipboardPaths => Some(Command::CopyToClipboard(ClipboardPayloadKind::Paths)),
        MenuAction::ClipboardNames => Some(Command::CopyToClipboard(ClipboardPayloadKind::Names)),
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
// Clipboard export (clipboard-export)
// ---------------------------------------------------------------------

/// Ctrl+C/Ctrl+Ins, Ctrl+Shift+Ins, and the Files pull-down's clipboard
/// group: resolve the F5 scope and either request the write (leaving
/// feedback to `apply_clipboard_result` once `Effect::SetClipboard`
/// replies) or, when the scope is empty, report that directly — the parent
/// pseudo-entry is never included, since `active_selection_sources` already
/// excludes it (clipboard-export "Clipboard payloads and scope": "Parent
/// entry is never copied").
fn handle_copy_to_clipboard(state: &mut State, kind: ClipboardPayloadKind) -> Vec<Effect> {
    let sources = active_selection_sources(state);
    if sources.is_empty() {
        set_clipboard_feedback(state, "Nothing to copy".to_string(), false);
        return vec![];
    }
    let items = sources.into_iter().map(|s| s.path).collect();
    vec![Effect::SetClipboard(ClipboardPayload { kind, items })]
}

/// The file-action menu's `Send to clipboard`: always `Files`, scoped to
/// whatever `selection_or_single_source` resolved — the menu's single
/// target entry, or the whole selection set for a selection-scoped
/// invocation (design D3 of `file-action-menu`, D4 of `mouse-basics`,
/// mirrored here from `activate_file_action`'s existing Copy/Move/Delete
/// handling). A no-op if the target has already vanished from the panel.
fn handle_send_target_to_clipboard(sources: Vec<SourceItem>) -> Vec<Effect> {
    if sources.is_empty() {
        return vec![];
    }
    let items = sources.into_iter().map(|s| s.path).collect();
    vec![Effect::SetClipboard(ClipboardPayload { kind: ClipboardPayloadKind::Files, items })]
}

/// Reply to `Effect::SetClipboard`: render the outcome into the active
/// panel's mini-status feedback (clipboard-export "Clipboard feedback",
/// "Non-Windows fallback").
fn apply_clipboard_result(state: &mut State, payload: ClipboardPayload, fell_back_to_paths: bool, result: Result<(), String>) {
    let (message, is_error) = match result {
        Err(message) => (message, true),
        Ok(()) => (clipboard_success_message(&payload, fell_back_to_paths), false),
    };
    set_clipboard_feedback(state, message, is_error);
}

/// The success feedback template for a completed `ClipboardPayload`
/// (clipboard-export "Clipboard feedback"). `Paths` gets a singular
/// variant naming the one path copied; `Files`/`Names` always use the
/// plural template, matching the literal message set the requirement
/// enumerates. A `Files` request downgraded to a `Paths` text write on a
/// non-Windows host reports that explicitly instead (clipboard-export
/// "Non-Windows fallback").
fn clipboard_success_message(payload: &ClipboardPayload, fell_back_to_paths: bool) -> String {
    if fell_back_to_paths {
        return "Paths copied (file objects unsupported here)".to_string();
    }
    let n = payload.items.len();
    match payload.kind {
        ClipboardPayloadKind::Files => format!("{n} files copied to clipboard"),
        ClipboardPayloadKind::Paths if n == 1 => format!("Path copied: {}", payload.items[0].display()),
        ClipboardPayloadKind::Paths => format!("{n} paths copied"),
        ClipboardPayloadKind::Names => format!("{n} names copied"),
    }
}

/// Show `message` in the active panel's mini-status until the next key
/// press or `CLIPBOARD_FEEDBACK_MS` elapses, whichever comes first
/// (clipboard-export "Clipboard feedback").
fn set_clipboard_feedback(state: &mut State, message: String, is_error: bool) {
    let side = state.active;
    let expires_at_ms = state.clock_ms.saturating_add(CLIPBOARD_FEEDBACK_MS);
    state.panel_mut(side).clipboard_feedback = Some(ClipboardFeedback { message, is_error, expires_at_ms });
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
        state.phase =
            UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources: vec![], source_dir, input: String::new(), buttons: None });
        return;
    }
    let sources = active_selection_sources(state);
    if sources.is_empty() {
        return;
    }
    let side = state.active;
    let prefill = state.panel(side.toggle()).cwd.display().to_string();
    enter_file_op_setup_for_sources(state, kind, sources, side, prefill, None);
}

/// Shared by `enter_file_op_setup` (F5/F6, selection- or cursor-scoped), the
/// file-action menu's Copy/Move (cursor-entry-only, D3), and `Command::
/// DragDrop` (mouse-panel-drag) — the setup dialog itself doesn't care which
/// chose its `sources`. `source_side`/`prefill` are explicit rather than
/// derived from `state.active`/its opposite panel internally, so a caller
/// acting on a side other than the active panel (a drag begun on the
/// inactive panel, which a press never focuses before the drag completes)
/// gets the right source directory and the right prefill without the dialog
/// silently assuming the active panel (operation-dialogs design D3).
/// `buttons` is `None` for every keyboard-reached caller (byte-identical to
/// the pre-`mouse-panel-drag` dialog) and `Some` only from `DragDrop`, which
/// also passes the exact drop path as `prefill` rather than the opposite
/// panel's cwd (operation-dialogs "Drop-initiated destination dialog").
fn enter_file_op_setup_for_sources(
    state: &mut State,
    kind: JobKind,
    sources: Vec<SourceItem>,
    source_side: PanelSide,
    prefill: String,
    buttons: Option<DropButtons>,
) {
    let source_dir = state.panel(source_side).cwd.clone();
    state.phase = UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, source_dir, input: prefill, buttons });
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
            activate_file_action(state, menu.selected(), target_name, menu.selection_scoped)
        }
        Command::FileActionMenuHotkey(c) => {
            let menu = state.file_action_menu.as_ref().unwrap();
            let action = menu.hotkey_action(c);
            match action {
                Some(action) => {
                    let target_name = menu.target_name.clone();
                    let selection_scoped = menu.selection_scoped;
                    state.file_action_menu = None;
                    activate_file_action(state, action, target_name, selection_scoped)
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
/// flows"). `selection_scoped` — also captured at open time — widens
/// Copy/Move/Delete/Send to clipboard to the whole selection set when the
/// menu was opened on an already-selected entry (mouse-basics design D4;
/// file-action-menu "Directory targets and selection-scoped invocation").
fn activate_file_action(state: &mut State, action: FileActionMenuEntry, target_name: OsString, selection_scoped: bool) -> Vec<Effect> {
    let side = state.active;
    match action {
        // Run: the same suspended-shell spawn path a typed command line
        // uses, and the only way to reach it now that Enter no longer
        // spawns an executable directly (command-line: "Enter on an
        // executable opens the menu instead of spawning"). Always
        // single-target — Run has no selection-scoped meaning.
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
        // the whole selection when `selection_scoped`, else to the menu's
        // target entry only (design D3/D4) — `enter_file_op_setup_for_sources`/
        // `enter_delete_confirm_for_sources` already title their dialog with
        // `sources.len()`, so a selection-scoped invocation gets the count
        // for free.
        FileActionMenuEntry::Copy => {
            let sources = selection_or_single_source(state, side, &target_name, selection_scoped);
            if !sources.is_empty() {
                let prefill = state.panel(side.toggle()).cwd.display().to_string();
                enter_file_op_setup_for_sources(state, JobKind::Copy, sources, side, prefill, None);
            }
            vec![]
        }
        FileActionMenuEntry::Move => {
            let sources = selection_or_single_source(state, side, &target_name, selection_scoped);
            if !sources.is_empty() {
                let prefill = state.panel(side.toggle()).cwd.display().to_string();
                enter_file_op_setup_for_sources(state, JobKind::Move, sources, side, prefill, None);
            }
            vec![]
        }
        FileActionMenuEntry::Delete => {
            let sources = selection_or_single_source(state, side, &target_name, selection_scoped);
            if !sources.is_empty() {
                enter_delete_confirm_for_sources(state, sources);
            }
            vec![]
        }
        // Rename: new in-place variant (file-action-menu "In-place Rename").
        // Always single-target — an in-place rename of several entries at
        // once has no meaning, and the requirement only lists
        // Copy/Move/Delete/Send to clipboard as selection-scoped.
        FileActionMenuEntry::Rename => {
            if let Some(source) = named_entry_source(state, side, &target_name) {
                enter_rename_input(state, source.original_name, source.is_dir);
            }
            vec![]
        }
        // Send to clipboard: the `clipboard-export` Files action, scoped the
        // same way Copy/Move/Delete are above — never mutates the filesystem
        // (file-action-menu "Menu actions route to existing flows", "No
        // mutation without an intervening dialog").
        FileActionMenuEntry::SendToClipboard => {
            let sources = selection_or_single_source(state, side, &target_name, selection_scoped);
            handle_send_target_to_clipboard(sources)
        }
    }
}

/// Copy/Move/Delete/Send to clipboard's shared scope resolution: the whole
/// selection set via the same `active_selection_sources` F5/F6/Ctrl+C
/// already use when `selection_scoped`, else `target_name` alone — falling
/// back to the single target if the selection somehow emptied out while the
/// menu was open (mouse-basics design D4; file-action-menu "Directory
/// targets and selection-scoped invocation").
fn selection_or_single_source(state: &State, side: PanelSide, target_name: &OsStr, selection_scoped: bool) -> Vec<SourceItem> {
    if selection_scoped {
        let sources = active_selection_sources(state);
        if !sources.is_empty() {
            return sources;
        }
    }
    named_entry_source(state, side, target_name).into_iter().collect()
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

// ---------------------------------------------------------------------
// Mouse drag-and-drop (mouse-panel-drag)
// ---------------------------------------------------------------------

/// The items a drag beginning on `side`'s entry named `name` freezes at
/// `DragBegin` (mouse-drag "Drag lifecycle"; design D4): the panel's
/// selection set if `name` is a member of it, else `name` alone. Empty (and
/// so a no-op for the caller) if `name` no longer names a selectable entry —
/// `named_entry_source` already excludes the `..` pseudo-entry, matching
/// "the parent-directory pseudo-entry SHALL never be dragged". Unlike
/// `selection_or_single_source` (used by the file-action menu, always
/// scoped to `state.active`), this is keyed by the drag's own source `side`:
/// a press-drag never changes `state.active` before the drag completes, so
/// the source panel need not be the active one at all.
fn drag_selection_sources(state: &State, side: PanelSide, name: &OsStr) -> Vec<SourceItem> {
    let panel = state.panel(side);
    if panel.selected.contains(name) {
        panel
            .entries
            .iter()
            .filter(|e| panel.selected.contains(&e.name))
            .map(|e| SourceItem { original_name: e.name.clone(), path: panel.cwd.join(&e.name), is_dir: e.is_dir_like() })
            .collect()
    } else {
        named_entry_source(state, side, name).into_iter().collect()
    }
}

/// The directory `target` currently names, re-resolved live against
/// whichever panel it points into — `None` if it no longer resolves to a
/// directory (mouse-drag "Robust against listing changes": "the target row
/// no longer resolves to a directory"). Never touches the filesystem: a
/// renamed-away or deleted row is indistinguishable here from one that never
/// existed, and both are simply not found.
fn drop_target_path(state: &State, target: &DropTarget) -> Option<PathBuf> {
    match target {
        DropTarget::PanelDir(side) => Some(state.panel(*side).cwd.clone()),
        DropTarget::SubDir { side, name } => {
            let panel = state.panel(*side);
            let entry = panel.entries.iter().find(|e| &e.name == name)?;
            match entry.kind {
                // The `..` row: the panel's own parent, exactly like
                // `Command::ParentDir` resolves it.
                EntryKind::ParentDir => crate::panel::parent_path(&panel.cwd),
                EntryKind::Directory => Some(panel.cwd.join(name)),
                EntryKind::File => None,
            }
        }
        DropTarget::TreeNode { path, .. } => Some(path.clone()),
        DropTarget::Tab { side, index } => state.panel(*side).tab_dirs().get(*index).cloned(),
    }
}

/// Whether `candidate` is a valid drop target for `drag`'s frozen items
/// right now (mouse-drag "Valid drop targets"): the region it names must
/// still make sense for the panel's current display mode (Info/Quick View
/// panels, and a Tree node/subdirectory row that no longer matches the
/// panel's actual mode, are never targets), the target must still resolve
/// to a live directory ([`drop_target_path`]), and it must be neither the
/// items' own source directory nor equal to/inside a directory being
/// dragged. Shared by `DragOver` (validating the TUI's raw geometric hit
/// before it is allowed onto `state.drag.target`) and `DragDrop`
/// (re-validating at release, since a listing can change between the last
/// `DragOver` and the button-up — mouse-drag "Robust against listing
/// changes").
fn valid_drop_target(state: &State, drag: &DragState, candidate: &DropTarget) -> bool {
    let region_ok = match candidate {
        DropTarget::PanelDir(side) => !matches!(state.panel(*side).display_mode, DisplayMode::Info | DisplayMode::QuickView),
        DropTarget::SubDir { side, .. } => matches!(state.panel(*side).display_mode, DisplayMode::Full | DisplayMode::Brief),
        DropTarget::TreeNode { side, .. } => state.panel(*side).display_mode == DisplayMode::Tree,
        // A tab always stands for its own directory, independent of
        // whichever mode its panel's *active* tab currently renders in
        // (design D7: "a tab in the strip stands for its directory").
        DropTarget::Tab { .. } => true,
    };
    if !region_ok {
        return false;
    }
    let Some(target_path) = drop_target_path(state, candidate) else { return false };
    // "A target equal to the items' own directory ... SHALL be invalid"
    // (mouse-drag "Valid drop targets").
    if target_path == drag.source_dir {
        return false;
    }
    // "... or equal to or inside a dragged directory, SHALL be invalid."
    // Only dragged directories can have descendants to protect; a dragged
    // file has none.
    drag.items.iter().filter(|item| item.is_dir).all(|item| target_path != item.path && !target_path.starts_with(&item.path))
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

/// Shared by `Command::FileOpConfirm` (Enter — confirms with whatever `kind`
/// the dialog opened with) and `Command::FileOpConfirmAs` (the drop
/// dialog's `[ Copy ]`/`[ Move ]` buttons — confirms with an explicit `kind`
/// override instead): an empty/whitespace destination re-shows the same
/// dialog unchanged, exactly as before this was split out; otherwise builds
/// and dispatches the job. `buttons` only ever rides along unchanged — it
/// has no bearing on which job runs.
fn confirm_destination_input(
    kind: JobKind,
    sources: Vec<SourceItem>,
    source_dir: PathBuf,
    input: String,
    buttons: Option<DropButtons>,
    effects: &mut Vec<Effect>,
) -> UiPhase {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, source_dir, input, buttons });
    }
    let job = match kind {
        JobKind::Mkdir => {
            Job { kind, sources: vec![], source_dir: source_dir.clone(), dest_dir: source_dir.clone(), new_dir_name: Some(OsString::from(trimmed)) }
        }
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

fn handle_file_op_setup(setup: FileOpSetup, cmd: Command, effects: &mut Vec<Effect>) -> UiPhase {
    match setup {
        FileOpSetup::DestinationInput { kind, sources, source_dir, mut input, buttons } => match cmd {
            Command::FileOpInputChar(c) => {
                input.push(c);
                UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, source_dir, input, buttons })
            }
            Command::FileOpInputBackspace => {
                input.pop();
                UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, source_dir, input, buttons })
            }
            Command::FileOpCancel => UiPhase::Panels,
            // Enter: confirm with whatever `kind` the dialog opened with —
            // the drop dialog's initially proposed verb, or the plain
            // keyboard dialog's fixed Copy/Move/Mkdir.
            Command::FileOpConfirm => confirm_destination_input(kind, sources, source_dir, input, buttons, effects),
            // The drop dialog's `[ Copy ]`/`[ Move ]` buttons: confirm with
            // an explicit verb instead, overriding `kind` (operation-dialogs
            // "Switching the verb in the dialog").
            Command::FileOpConfirmAs(explicit_kind) => confirm_destination_input(explicit_kind, sources, source_dir, input, buttons, effects),
            _ => UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, source_dir, input, buttons }),
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
            // Background tabs (not currently displayed on either side)
            // don't get the immediate re-read above — their cached listing
            // would otherwise go silently stale. Mark any whose directory
            // was touched by this job so activating them later (Alt+`n`,
            // or the neighbor a Ctrl+W close falls back to) triggers a
            // fresh read instead (file-operations "Automatic panel re-read
            // on completion"; panel-tabs "Stale background tab refresh on
            // activation"). `dest_dir == source_dir` for Delete/Rename, so
            // this is a harmless duplicate match in that case, not a bug.
            for side in [PanelSide::Left, PanelSide::Right] {
                let panel = state.panel_mut(side);
                panel.mark_background_tabs_stale(&source_dir);
                panel.mark_background_tabs_stale(&dest_dir);
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
    // A brand-new tree is a single root node, so this is a no-op today, but
    // it keeps the freshly entered tree's viewport state consistent with
    // every other tree-mutating path rather than relying on it happening to
    // already be zeroed.
    reconcile_panel_viewport(state, side);
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
    // Keep the tree cursor inside its own scrolled window (additional-
    // panel-modes "Tree mode scrolling") before the opposite panel's
    // re-listing kicks off below.
    reconcile_panel_viewport(state, side);
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
/// F2 user menu", "Navigate and dismiss the user menu"). The cursor domain
/// is `0..=state.user_menu_entries.len()`: the last index is the
/// compiled-in "Themes" slot, not a config entry (design D3).
fn handle_user_menu(state: &mut State, cmd: Command) -> Vec<Effect> {
    if state.user_menu.is_none() {
        return vec![];
    }
    match cmd {
        Command::UserMenuMove(delta) => {
            // Cursor domain is the user entries plus one compiled-in
            // built-in slot at index `entries.len()` for "Themes" (design
            // D3) — not an entry appended to `state.user_menu_entries`.
            let len = state.user_menu_entries.len() + 1;
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
            // Cursor is at the built-in Themes slot (`entries.len()`): the
            // user menu is already closed above (design D4); open the
            // theme picker via the exact same path as
            // `MenuAction::OpenThemes`, pre-highlighting the active theme.
            // No shell effect.
            state.theme_picker = Some(ThemePickerState::open(&state.theme.name));
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
            let visible = crate::dialogs::help_topic_visible_rows(crate::dialogs::help_window_height(state.term_size));
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
            // Keeps the pinned-to-top offset (0) while the user hasn't moved
            // the cursor, exactly as `insert_streamed` already pins the
            // cursor itself; re-clamps if the user had already moved it
            // (panel-navigation "Streamed listing keeps the top pinned until
            // the user moves").
            reconcile_panel_viewport(state, panel);
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
            reconcile_panel_viewport(state, panel);
        }
        Command::ListingFailed { panel, message } => {
            let p = state.panel_mut(panel);
            p.progress = crate::panel::ListingProgress::Complete { count: p.entries.len() };
            p.last_error = Some(message);
        }
        _ => unreachable!("apply_listing_event only called for listing events"),
    }
}

// ---------------------------------------------------------------------
// Mouse (mouse-basics, design D2)
// ---------------------------------------------------------------------

/// The command a dialog button click stands for — the exact command the
/// equivalent keypress already issues, so a click is never a new way to do
/// something, only a new way to reach an existing one. `None` for a button
/// that has no reachable meaning outside its own gated context (there is
/// none today — kept `Option` for symmetry with [`menu_action_command`] and
/// so a future button can be added without an infallible-match refactor).
pub fn button_command(id: ButtonId) -> Option<Command> {
    match id {
        ButtonId::DeleteConfirmYes => Some(Command::FileOpConfirm),
        ButtonId::DeleteConfirmNo => Some(Command::FileOpCancel),
        ButtonId::ConflictOverwrite => Some(Command::FileOpConflictChoice(ConflictChoice::Overwrite)),
        ButtonId::ConflictOverwriteAll => Some(Command::FileOpConflictChoice(ConflictChoice::OverwriteAll)),
        ButtonId::ConflictSkip => Some(Command::FileOpConflictChoice(ConflictChoice::Skip)),
        ButtonId::ConflictSkipAll => Some(Command::FileOpConflictChoice(ConflictChoice::SkipAll)),
        ButtonId::ConflictRename => Some(Command::FileOpBeginRename),
        ButtonId::ErrorRetry => Some(Command::FileOpErrorChoice(ErrorChoice::Retry)),
        ButtonId::ErrorSkip => Some(Command::FileOpErrorChoice(ErrorChoice::Skip)),
        ButtonId::ErrorSkipAll => Some(Command::FileOpErrorChoice(ErrorChoice::SkipAll)),
        ButtonId::ErrorAbort => Some(Command::FileOpErrorChoice(ErrorChoice::Abort)),
        ButtonId::ProgressCancel => Some(Command::FileOpCancelJob),
        ButtonId::SummaryContinue => Some(Command::FileOpConfirm),
        ButtonId::QuitYes => Some(Command::ConfirmQuit),
        ButtonId::QuitNo => Some(Command::CancelQuit),
        ButtonId::DropDialogCopy => Some(Command::FileOpConfirmAs(JobKind::Copy)),
        ButtonId::DropDialogMove => Some(Command::FileOpConfirmAs(JobKind::Move)),
        ButtonId::DropDialogCancel => Some(Command::FileOpCancel),
    }
}

/// `Command::ClickEntry`: focus `side`, and either move the cursor to the
/// named entry (`Plain`/`Shift`) or toggle its selection in place without
/// advancing the cursor from wherever it lands (`Ctrl`) — mirroring `Ins`'s
/// `toggle_selection_and_advance` except the cursor's destination is the
/// clicked entry, not "one past wherever it already was" (mouse-input
/// "Click focuses and places the cursor", "Ctrl+click toggles selection"). A
/// name the panel no longer lists (e.g. a listing that changed between the
/// hit map being built and the click landing) is a silent no-op rather than
/// a panic. The parent `..` pseudo-entry is never selectable, matching
/// `toggle_selection_and_advance`'s own guard (mouse-input "Parent entry
/// ignored").
fn handle_click_entry(state: &mut State, side: PanelSide, name: OsString, mods: ClickMods) -> Vec<Effect> {
    state.active = side;
    let panel = state.panel_mut(side);
    let Some(idx) = panel.entries.iter().position(|e| e.name == name) else { return vec![] };
    panel.cursor = idx;
    panel.cursor_user_moved = true;
    if mods == ClickMods::Ctrl && panel.entries[idx].kind != EntryKind::ParentDir {
        if !panel.selected.remove(&name) {
            panel.selected.insert(name);
        }
    }
    reconcile_panel_viewport(state, side);
    vec![]
}

/// `Command::ScrollPanel`: move `side`'s cursor by `delta` rows and let the
/// existing scroll-offset clamp bring the viewport along, so the
/// cursor-in-window invariant holds exactly as it does for a keyboard
/// Up/Down (mouse-input "Wheel moves the cursor of the panel under the
/// pointer"; design D6). Tree mode has no flat `entries` list to move a
/// bare `CursorMove` over, so it reuses the same tree-cursor path the
/// keyboard's Up/Down already takes there.
fn handle_scroll_panel(state: &mut State, side: PanelSide, delta: isize) -> Vec<Effect> {
    let m = if delta < 0 { CursorMove::Up(delta.unsigned_abs()) } else { CursorMove::Down(delta as usize) };
    if state.panel(side).display_mode == DisplayMode::Tree {
        return handle_tree_cursor_move(state, side, m);
    }
    state.panel_mut(side).move_cursor(m);
    reconcile_panel_viewport(state, side);
    vec![]
}

/// `Command::KeybarPress`: the command each function-key-bar slot stands
/// for, exactly mirroring `input::map_panel_key`'s own F-key arms (mouse-
/// input "Key bar, menu bar, pull-down items, and dialog buttons are
/// clickable"). `None` for a slot number the bar never actually draws.
fn keybar_command(slot: u8) -> Option<Command> {
    match slot {
        1 => Some(Command::HelpOpen),
        2 => Some(Command::UserMenuOpen),
        3 => Some(Command::RequestViewer),
        4 => Some(Command::RequestEditor),
        5 => Some(Command::RequestCopy),
        6 => Some(Command::RequestMove),
        7 => Some(Command::RequestMkdir),
        8 => Some(Command::RequestDelete),
        9 => Some(Command::MenuOpen),
        10 => Some(Command::RequestQuit),
        _ => None,
    }
}

/// `Command::MenuItemClick`: highlight `index` (only if it names an enabled
/// item) and return the command activating it stands for, exactly like
/// `handle_menu`'s own `Command::MenuActivate` arm — the caller re-enters
/// `update` with it. `None` (no-op) for an index that is out of range, a
/// separator, or disabled, and whenever no menu is open at all.
fn handle_menu_item_click(state: &mut State, index: usize) -> Option<Command> {
    let active_side = state.active;
    let menu = state.menu.as_mut()?;
    match menu.items().get(index) {
        Some(MenuEntry::Item(item)) if item.is_enabled() => {
            menu.selected = index;
            let side = menu.active.target_side(active_side);
            let action = item.action;
            state.menu = None;
            menu_action_command(action, side)
        }
        _ => None,
    }
}

/// `Command::OpenActionMenuAt`: move `side`'s cursor to the named entry and
/// open the file-action menu for it — for a file, the same shape
/// `handle_enter`'s `EntryKind::File` arm builds; for a directory, the
/// `View`/`Edit`/`Run`-less menu `file-action-menu`'s "Directory targets and
/// selection-scoped invocation" requirement adds (mouse-input "Right-click
/// opens the action menu"; design D4). The menu is also opened
/// selection-scoped whenever `name` is already a member of the panel's
/// selection set, so `activate_file_action` later acts on the whole
/// selection rather than `name` alone. A `..` target, or a name the panel no
/// longer lists, is a no-op — `..` is never a valid action-menu target,
/// mirroring Ctrl+click's own "Parent entry ignored" guard.
fn handle_open_action_menu_at(state: &mut State, side: PanelSide, name: OsString) -> Vec<Effect> {
    state.active = side;
    let panel = state.panel_mut(side);
    let Some(idx) = panel.entries.iter().position(|e| e.name == name) else { return vec![] };
    panel.cursor = idx;
    panel.cursor_user_moved = true;
    reconcile_panel_viewport(state, side);
    let panel = state.panel(side);
    let kind = panel.entries[idx].kind;
    if kind == EntryKind::ParentDir {
        return vec![];
    }
    let is_dir = kind == EntryKind::Directory;
    let executable = !is_dir && shell::is_executable_name(&name.to_string_lossy(), &state.shell.pathext);
    let selection_scoped = panel.selected.contains(&name);
    state.file_action_menu = Some(FileActionMenuState::open(name, is_dir, executable, selection_scoped));
    vec![]
}

#[cfg(test)]
mod tests;
