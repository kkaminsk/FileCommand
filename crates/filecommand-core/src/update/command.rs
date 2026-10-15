//! The reducer's vocabulary: [`Command`], [`Effect`], and the value types they
//! carry (`ButtonId`, `ClickMods`, `DropTarget`, `DragState`, the pending-
//! request records). Re-exported from `update` so every existing
//! `filecommand_core::update::…` path still resolves.

use super::*;

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
/// like the overlay slot `State::overlay`, rather than inside it — a drag
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
    /// The overwrite-conflict dialog's `(C)ancel` control — the same job
    /// abort Esc and `C` issue there (operation-dialogs "Overwrite-conflict
    /// dialog"; process-lifecycle-hardening design D7).
    ConflictCancel,
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

/// What `Effect::ProbePath` found at the probed path (offload-blocking-io
/// design D2): a directory (links followed, so a junction to a directory is
/// `Dir`), some other kind of entry, or nothing — any error at all maps to
/// `Missing`, exactly as the old synchronous `metadata(..).ok()?` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeKind {
    Dir,
    File,
    Missing,
}

/// Which command-line builtin an outstanding `Effect::ProbePath` completes
/// (design D1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeIntent {
    /// `cd`: on `Dir`, push `text` to command history and navigate.
    Cd { text: String },
    /// `del` (`want_dir: false`) / `rmdir` (`want_dir: true`): on a type
    /// match, open the delete-confirmation dialog.
    Delete { want_dir: bool },
}

/// The one command-line target check that may be outstanding (design D1).
/// `cwd` is the panel's directory when the command was entered: a reply is
/// applied only while the panel still shows it, so navigating away
/// invalidates the probe without any explicit cancel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingProbe {
    pub request: u64,
    pub side: PanelSide,
    pub cwd: PathBuf,
    pub path: PathBuf,
    pub intent: ProbeIntent,
}

/// Which kind of open `Effect::OpenViewer`/`OpenEditor` was requested for
/// (design D4). Recorded for diagnostics and symmetry with the effect pair;
/// the reply gate itself keys on the request id and the panel phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingOpenKind {
    Viewer,
    Editor,
}

/// The one F3/F4 open that may be outstanding (design D4). A newer request
/// replaces it, which is what makes the older reply stale. `side` is the
/// panel the open was requested from, so a late failure lands there and not
/// on whichever panel is active when the reply arrives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingOpen {
    pub request: u64,
    pub side: PanelSide,
    pub kind: PendingOpenKind,
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
    /// The current time, supplied by the TUI's injected clocks: `now_ms`
    /// from the monotonic `Clock` (animations, every in-session timing) and
    /// `epoch_ms` from the `WallClock` (Unix-epoch milliseconds; frecency
    /// stamps only — history-persistence-hardening design D1).
    Tick {
        now_ms: u64,
        epoch_ms: u64,
    },

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

    // Shared single-line text-field editing (text-field-editing; design D2).
    /// One editing operation routed to whichever field owns the keyboard:
    /// the destination/name or Rename input in `FileOpSetup`, the conflict
    /// dialog's rename input in `FileOpRunning`, the find-file pattern, the
    /// fuzzy-jump pattern, else the command line. Replaces the six private
    /// per-field char/backspace command pairs those fields used to have.
    TextEdit(TextEditOp),
    /// Shift+Ins/Ctrl+V in a dialog field, Ctrl+V over the panels: read the
    /// system clipboard's text (via `Effect::ReadClipboardText`) and feed it
    /// back as `TextEdit(TextEditOp::Paste)` (design D4).
    RequestPaste,

    // Type-ahead quick search (§4.7), which competes with the command line
    // for printable keys.
    QuickSearchStart(char),
    QuickSearchChar(char),
    QuickSearchBackspace,
    QuickSearchEnd,

    // Sort modes (Ctrl+F3..Ctrl+F7).
    SetSortMode {
        side: PanelSide,
        mode: SortMode,
    },

    // Ctrl+P inline quick filter (§4.7). `QuickFilterStart` carries the
    // panel side so the Left/Right pull-down's Filter item can start the
    // filter on its own panel (design D1 of pulldown-menu-completion); the
    // keymap passes the active side. Starting the filter also makes `side`
    // the active panel, so typed characters reach it.
    QuickFilterStart(PanelSide),
    QuickFilterChar(char),
    QuickFilterBackspace,
    QuickFilterEnd,

    // Panel tabs (§4.1/§4.7). `OpenTab`/`CloseTab` carry the panel side for
    // the same Left/Right-menu reason as `QuickFilterStart` (but never move
    // focus — design D1); the keymap passes the active side.
    /// Ctrl+T.
    OpenTab(PanelSide),
    /// Ctrl+W.
    CloseTab(PanelSide),
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
    TreeNodeExpanded {
        panel: PanelSide,
        path: PathBuf,
        children: Vec<Entry>,
    },

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
    DriveListReady {
        target: PanelSide,
        drives: Vec<char>,
    },
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
    /// (builtin-editor "Small file opens in the editor"). Every
    /// `Effect::OpenEditor` reply echoes the effect's `request`, which is
    /// matched against `State::pending_open` so a reply the user has since
    /// superseded — a newer F3/F4, a menu or dialog opened meanwhile — is
    /// discarded instead of pulling them out of what they are doing
    /// (offload-blocking-io design D4).
    EditorOpened {
        request: u64,
        editor: Box<EditorState>,
    },
    /// Reply to `Effect::OpenEditor`: the file is `size` bytes, at or above
    /// the cap — redirected to the F3 viewer with a notice (builtin-editor
    /// "Large file redirects to the viewer").
    EditorTooLarge {
        request: u64,
        path: PathBuf,
        size: u64,
    },
    /// Reply to `Effect::OpenEditor`: the file is under the cap but is not
    /// UTF-8 text the editor can write back byte-exactly (UTF-16, NUL
    /// bytes, or invalid UTF-8) — redirected to the F3 viewer with a
    /// reason-specific notice, exactly like `EditorTooLarge` (builtin-editor
    /// "Non-UTF-8 file redirects to the viewer"; editor-encoding-safety
    /// design D2).
    EditorNotEditable {
        request: u64,
        path: PathBuf,
        reason: NotEditableReason,
    },
    /// Reply to `Effect::OpenEditor` when the file could not be opened.
    EditorOpenFailed {
        request: u64,
        message: String,
    },

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
    EditorSaved {
        editor: Box<EditorState>,
        then_quit: bool,
    },
    /// Reply to `Effect::SaveEditor`: the write failed — aborts a save-on-
    /// exit quit attempt and surfaces the message inline rather than losing
    /// the buffer (builtin-editor "Modified indicator and save-on-exit
    /// prompt").
    EditorSaveFailed {
        message: String,
    },
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
    /// Replies to `Effect::StartListing`. `request` echoes the id that
    /// effect was issued with and is matched against the panel's
    /// `PanelState::listing_request`; a reply from a listing the panel has
    /// since superseded (a navigation, re-read, drive change, or tab switch
    /// landed first) is dropped before it touches any panel state, so a
    /// slow directory's entries never pour into whatever the panel shows
    /// now (panel-navigation "Superseded directory listings are discarded
    /// and cancelled"; listing-request-guard design D1/D2).
    ListingChunk {
        panel: PanelSide,
        entries: Vec<Entry>,
        request: u64,
    },
    /// `unreadable` is how many entries the reader skipped because their own
    /// metadata could not be read; when non-zero the panel's mini-status
    /// reports the count on completion (panel-navigation "One unreadable
    /// entry does not fail a listing"; link-aware-listing design D6/D7).
    ListingComplete {
        panel: PanelSide,
        total: usize,
        unreadable: usize,
        request: u64,
    },
    ListingFailed {
        panel: PanelSide,
        message: String,
        request: u64,
    },
    JobProgress(ProgressInfo),
    JobConflict(ConflictInfo),
    JobError(ErrorInfo),
    JobDone {
        outcome: JobOutcome,
        source_dir: PathBuf,
        dest_dir: PathBuf,
    },
    DriveLabelResolved {
        target: PanelSide,
        letter: char,
        label: Option<String>,
        generation: u64,
    },
    InfoResolved {
        panel: PanelSide,
        path: PathBuf,
        request: u64,
        values: InfoValues,
    },
    /// Reply to `Effect::QueryGitInfo`: either a resolved repository's
    /// branch and per-entry statuses, or `GitInfo::none()` for "outside any
    /// repository" and for a timed-out query (git-info "Background
    /// repository detection", "Silent absence on timeout and stale-result
    /// discarding"). `request` is matched against
    /// `PanelState::git_request` the same way `InfoResolved`'s `request` is
    /// matched against `info_request`, so a reply for a directory/generation
    /// the panel has since moved past is dropped. `timed_out_root` is `Some`
    /// only when the query hit its timeout: the repository's
    /// working-directory root, or the queried directory itself when the
    /// repository could not even be located in time. It is recorded in
    /// `State::git_no_info_roots` even when the reply is otherwise stale —
    /// a timeout is evidence about the repository, not about the panel's
    /// current directory (git-info "Timeout recorded after the panel moved
    /// on"; git-info-timeout-memory design D1).
    GitInfoResolved {
        panel: PanelSide,
        path: PathBuf,
        request: u64,
        info: GitInfo,
        timed_out_root: Option<PathBuf>,
    },
    /// Reply to `Effect::OpenViewer`: the file opened at `file_len` bytes
    /// (viewer: Instant open). `request` echoes the effect's id and is
    /// matched against `State::pending_open` exactly as the editor replies
    /// are (offload-blocking-io design D4).
    ViewerOpened {
        request: u64,
        path: PathBuf,
        file_len: u64,
    },
    /// Reply to `Effect::OpenViewer`: the file could not be opened or
    /// mapped (§7 error policy — surfaced as an inline error, never a
    /// panic).
    ViewerOpenFailed {
        request: u64,
        message: String,
    },
    /// Reply to `Effect::ProbePath`: what the worker found at the probed
    /// command-line target. Matched against `State::command_probe` by
    /// `request`; a stale or superseded reply changes nothing
    /// (application-shell "The reducer performs no filesystem I/O";
    /// offload-blocking-io design D1).
    PathProbed {
        request: u64,
        kind: ProbeKind,
    },
    /// Reply to `Effect::RunViewerSearch` (viewer: F7 streaming search).
    /// `request` echoes the id `Effect::RunViewerSearch` was issued with, so
    /// a reply from a search superseded by a since-closed/reopened viewer
    /// session (`ViewerState::search_request`) is recognized as stale and
    /// dropped rather than applied to whatever viewer session happens to be
    /// open when it arrives.
    ViewerSearchResult {
        offset: Option<u64>,
        match_range: Option<(u64, u64)>,
        request: u64,
    },
    /// Reply to `Effect::RunExternalEditor` when the editor could not be
    /// spawned (external-editor: Editor spawn errors do not crash the app).
    ExternalEditorSpawnFailed {
        message: String,
    },

    // Ctrl+J fuzzy jump (M5).
    /// Open the dialog (default binding, overridable via `config.toml`).
    FuzzyJumpOpen,
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
    /// Enter on the pattern input: kick off `Effect::FindInSubtree`.
    FindFileSubmit,
    /// Reply to `Effect::FindInSubtree`: one matched entry, streamed as the
    /// walk finds it. `request` is matched against `FindFileState::request`
    /// so a reply from an abandoned/superseded search is dropped (find-file
    /// "Non-blocking search with static progress").
    FindFileMatch {
        request: u64,
        m: FindMatch,
    },
    /// Reply to `Effect::FindInSubtree`: the walk has finished.
    FindFileSearchDone {
        request: u64,
    },
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

    // Alt+F8 command-history dialog (pulldown-menu-completion; command-line
    // "Command history dialog (Alt+F8)"). Opened by Alt+F8 or Commands →
    // History; a list overlay over `State::history`, newest first.
    HistoryDialogOpen,
    /// Up/Down/Home/End/PgUp/PgDn (the last four arrive as huge deltas, the
    /// same convention as `DriveSelectMove`).
    HistoryDialogMove(isize),
    /// Enter: replace the command-line buffer with the highlighted entry
    /// (caret at the end, no history recall in progress) and close — nothing
    /// runs until the user presses Enter again. A no-op on an empty history,
    /// leaving the dialog open.
    HistoryDialogConfirm,
    /// Esc: close, leaving the command line unchanged.
    HistoryDialogCancel,

    /// Ctrl+U / Commands → Swap panels: exchange the two panels' entire
    /// state, keeping the active side and split unchanged (panel-navigation
    /// "Swap panels (Ctrl+U)"; design D3).
    SwapPanels,

    // Commands → Menu file edit (user-menu "Edit the user menu file from the
    // Commands menu"; design D4).
    /// Open `State::user_menu_path` via the F4 editor path: the configured
    /// external editor when `editor =` is set, else the built-in editor.
    EditUserMenuFile,
    /// Sent by the TUI once the editor that had the user-menu file open has
    /// closed/returned, carrying the freshly re-loaded parse result. Applies
    /// the entries, or — on malformed content — raises the warning dialog
    /// and keeps the previous entries (never overwriting the file).
    UserMenuReloaded(config::UserMenuLoadResult),

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
    SetDisplayMode {
        side: PanelSide,
        mode: DisplayMode,
    },

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
    /// A left-click on the open file-action menu's `index`'th row: activates
    /// it exactly as `Command::FileActionMenuConfirm` would once the
    /// highlight is there (mouse-input "File-action menu entries are
    /// clickable"). `input::map_mouse` is the only source of this.
    FileActionMenuItemClick(usize),
    /// Reply to `Effect::OpenWithAssociation` when the launch failed: the
    /// message lands on the status line (`last_error`) of `side`, the panel
    /// the Open action was invoked on — not whichever panel is active when
    /// the late reply arrives — exactly as `Command::EditorOpenFailed`
    /// reports a failed editor open (file-action-menu "Open with the
    /// associated application": "No association is reported inline";
    /// offload-blocking-io design D7).
    OpenWithAssociationFailed {
        side: PanelSide,
        message: String,
    },

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
    /// needed to render the singular "Path copied: `<path>`" message without
    /// re-deriving scope); `fell_back_to_paths` is set only when a `Files`
    /// request was carried out as a plain-text `Paths` write because file
    /// objects aren't supported on this platform (clipboard-export
    /// "Non-Windows fallback"). `Ok(())` shows the per-kind success
    /// message; `Err(message)` shows `message` verbatim in the error role
    /// (clipboard-export "Clipboard busy retry" — the TUI retries before
    /// giving up, so by the time this arrives the failure is final).
    ClipboardResult {
        payload: ClipboardPayload,
        fell_back_to_paths: bool,
        result: Result<(), String>,
    },

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
    ClickEntry {
        side: PanelSide,
        name: OsString,
        mods: ClickMods,
    },
    /// A left-click on a panel's title or blank body area: focuses `side`
    /// without moving its cursor (mouse-input "Click focuses and places the
    /// cursor").
    FocusPanel(PanelSide),
    /// A wheel notch (or several, coalesced — design D7) over `side`'s
    /// panel: move its cursor by `delta` rows (positive down, negative up),
    /// independent of which panel is active (mouse-input "Wheel moves the
    /// cursor of the panel under the pointer"). The viewer/editor have their
    /// own wheel handling that never reaches this command (design D6).
    ScrollPanel {
        side: PanelSide,
        delta: isize,
    },
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
    OpenActionMenuAt {
        side: PanelSide,
        name: OsString,
    },

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
    DragBegin {
        side: PanelSide,
        name: OsString,
        op: JobKind,
    },
    /// The drag's pointer resolved to a new de-duplicated position (the
    /// TUI's `MouseTracker` owns de-duplication, so core only sees actual
    /// changes — design D4). A no-op if no drag is in progress.
    DragOver {
        op: JobKind,
        target: Option<DropTarget>,
    },
    /// The button released. Opens the drop-initiated destination dialog if
    /// the drag's target is (still) valid and the source panel still shows
    /// the captured directory; otherwise ends the drag with no effect
    /// (mouse-drag "Release on an invalid spot", "Robust against listing
    /// changes"). A no-op if no drag is in progress.
    DragDrop {
        op: JobKind,
    },
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
    /// Read `path` on a worker thread and stream it back as
    /// `Command::ListingChunk`/`ListingComplete`/`ListingFailed`, each
    /// echoing `request` — the id minted for this listing and recorded as
    /// the panel's `PanelState::listing_request` (listing-request-guard
    /// design D1). The TUI registers a cancel flag under `request` so a
    /// later `CancelRequest(request)` can stop the read.
    StartListing {
        panel: PanelSide,
        path: PathBuf,
        request: u64,
    },
    /// Stop the background work issued under this request id, if it is
    /// still running: a directory listing (`StartListing`), a find-file
    /// subtree walk (`FindInSubtree`), or a viewer F7 search
    /// (`RunViewerSearch`). The reducer emits it whenever it supersedes or
    /// abandons one of those requests; the TUI flips the matching cancel
    /// flag and the worker goes silent — it sends nothing further, not even
    /// a terminal reply. An id with no running worker (already finished,
    /// or never cancellable) is a silent no-op, so the reducer never needs
    /// to know whether the worker is still alive (listing-request-guard
    /// design D3).
    CancelRequest(u64),
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
    /// Launch `path` through the platform's file association —
    /// `ShellExecuteW` with the `open` verb on Windows, detached
    /// `xdg-open`/`open` elsewhere — with `cwd` (the menu's panel directory)
    /// as the working directory. Fire-and-forget: the TUI is never
    /// suspended, the launched program is never waited on, and no panel
    /// re-read is scheduled (open-with-associated-app design D1/D3;
    /// file-action-menu "Open with the associated application"). Runs on a
    /// worker thread — with COM initialized apartment-threaded on Windows —
    /// so a slow handler never freezes the panels; a launch failure comes
    /// back as `Command::OpenWithAssociationFailed` for `side`, the panel
    /// the menu acted on (offload-blocking-io design D7).
    OpenWithAssociation {
        side: PanelSide,
        path: PathBuf,
        cwd: PathBuf,
    },
    /// Check what exists at `path` — the resolved target of a command-line
    /// `cd`/`del`/`rmdir` that is not already listed in the panel — on a
    /// worker thread, through the `\\?\` long-path abstraction, and reply
    /// with `Command::PathProbed { request, .. }`. The reducer never queries
    /// the filesystem itself, so an unreachable share blocks only the worker
    /// (application-shell "The reducer performs no filesystem I/O";
    /// offload-blocking-io design D1/D2).
    ProbePath {
        request: u64,
        path: PathBuf,
    },
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
    FetchDriveLabel {
        target: PanelSide,
        letter: char,
        generation: u64,
    },
    /// Gather the Info panel's async values on a worker thread. `request`
    /// travels with the result so a reply from a since-superseded query can
    /// be told apart from the current one.
    QueryInfo {
        panel: PanelSide,
        path: PathBuf,
        request: u64,
    },
    /// Run `git_info::query` for `path` on a dedicated worker thread —
    /// never the shared Info worker, since a slow status call on one panel
    /// must not head-of-line-block the other (design D3) — and report the
    /// result back via `Command::GitInfoResolved`. `request` travels with
    /// the result so a stale or timed-out reply can be told apart from the
    /// query that's still current (git-info "Background repository
    /// detection", "Pathspec-scoped status queries").
    QueryGitInfo {
        panel: PanelSide,
        path: PathBuf,
        request: u64,
    },
    /// Read `path`'s immediate child directories for Tree-mode expansion
    /// (`listing::list_child_dirs`) on a worker thread, reporting them back
    /// via `Command::TreeNodeExpanded` — one directory's worth of I/O per
    /// call, never a recursive scan (additional-panel-modes "Children read
    /// on expand"; design D7).
    ExpandTreeNode {
        panel: PanelSide,
        path: PathBuf,
    },
    /// Open `path` for the F3 viewer on a worker thread and report back its
    /// length via `Command::ViewerOpened`, or the failure via
    /// `Command::ViewerOpenFailed`, each echoing `request`. The open is O(1)
    /// in work but not in latency — a dropped SMB session blocks the open
    /// for the redirector timeout — so it runs off the event loop and the
    /// panel shows `Opening <name>…` meanwhile (viewer: Instant open;
    /// application-shell "Slow storage never blocks the event loop";
    /// offload-blocking-io design D4/D5).
    OpenViewer {
        request: u64,
        path: PathBuf,
    },
    /// Run the F7 streaming search from `start_offset` for `pattern`
    /// against `path`, reporting the match back via
    /// `Command::ViewerSearchResult` (viewer: F7 streaming search).
    /// `request` must be echoed back unchanged in the `ViewerSearchResult`
    /// reply so a stale reply can be told apart from the current search.
    RunViewerSearch {
        path: PathBuf,
        start_offset: u64,
        pattern: Vec<u8>,
        request: u64,
    },
    /// Suspend the TUI, launch the external editor per `EditorInvocation`,
    /// wait for it to exit, then restore and re-read the panel side that
    /// owned the cursor entry — the same suspend/restore seam
    /// `RunShellCommand` uses (design D7; external-editor: TUI suspend and
    /// restore, Synchronous wait and panel re-read).
    RunExternalEditor(EditorInvocation, PanelSide),
    /// Open `path` for the F4 built-in editor on a worker thread: stat it
    /// and, under the 10 MB cap, read and decode it in full, reporting back
    /// via `Command::EditorOpened`/`EditorTooLarge`/`EditorNotEditable`/
    /// `EditorOpenFailed`, each echoing `request`. The panel shows
    /// `Opening <name>…` while the read is outstanding (application-shell
    /// "Slow storage never blocks the event loop"; offload-blocking-io
    /// design D4).
    OpenEditor {
        request: u64,
        path: PathBuf,
    },
    /// Write `editor`'s buffer to disk via `EditorState::save` on a worker
    /// thread, reporting the post-save state back via `Command::EditorSaved`
    /// (or the failure via `Command::EditorSaveFailed`). `then_quit` travels
    /// through unchanged so the reply knows whether to close the editor once
    /// the write lands (builtin-editor "Save in place …", design D9). Only
    /// one save is ever in flight — `EditorState::save_pending` gates the
    /// editor until the reply — so no request id is needed (offload-
    /// blocking-io design D6).
    SaveEditor {
        editor: Box<EditorState>,
        then_quit: bool,
    },
    /// Walk `root`'s subtree for entries whose name contains `pattern` on a
    /// worker thread, streaming each match back via `Command::FindFileMatch`
    /// as it's found and finishing with `Command::FindFileSearchDone` —
    /// never blocking the UI event loop (find-file "Non-blocking search
    /// with static progress"). `request` travels with every reply so a
    /// stale/abandoned search's results are dropped once the dialog has
    /// moved on.
    FindInSubtree {
        root: PathBuf,
        pattern: String,
        request: u64,
    },
    /// Write `payload` to the OS clipboard via the TUI's `Clipboard` trait
    /// and report the outcome back via `Command::ClipboardResult`. Executed
    /// synchronously, like `EnumerateDrives` — `OpenClipboard` binds to the
    /// calling thread and set-and-close is sub-millisecond, so no worker
    /// thread is needed (design D3; clipboard-export "Clipboard payloads
    /// and scope").
    SetClipboard(ClipboardPayload),
    /// Read the OS clipboard's text via the TUI's `Clipboard` trait and feed
    /// it back as `Command::TextEdit(TextEditOp::Paste)` for whichever field
    /// owns the keyboard when it arrives (text-field-editing "Paste into a
    /// text field"; design D4). A clipboard holding no text feeds nothing
    /// back — the key is then a no-op. Synchronous like `SetClipboard`.
    ReadClipboardText,
}
