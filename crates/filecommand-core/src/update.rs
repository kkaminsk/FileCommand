//! The pure data-flow core: `State`, `Command`, `Effect`, and `update`.
//!
//! `update(state, command) -> (state, Vec<Effect>)` is the single path all
//! state mutations flow through — key-derived commands and worker-produced
//! events alike. It performs no I/O, spawns no threads, and reads no clock;
//! callers supply the current time via [`Command::Tick`].

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

use crate::config::{self, UserMenuEntry};
use crate::dialogs::{FileActionMenuEntry, FileActionMenuState, HelpState, HistoryDialogState, ThemePickerState, UserMenuState};
use crate::drives::{self, DriveSelect};
use crate::editor::{EditorMove, EditorState, NotEditableReason, ReplacePrompt};
use crate::external_editor::{self, EditorInvocation};
use crate::find_file::FindFileState;
use crate::fs_ops::dialog::{DropButtons, FileOpSetup, RunningDialog};
use crate::fs_ops::{ConflictChoice, ConflictInfo, ErrorChoice, ErrorInfo, Job, JobKind, JobOutcome, ProgressInfo, SkippedItem, SourceItem};
use crate::git_info::{self, GitInfo};
use crate::info::InfoValues;
use crate::line_edit::{LineEdit, TextEditOp};
use crate::listing::{format_count, Entry, EntryKind, FindMatch, SortMode};
use crate::menu::{MenuAction, MenuEntry, MenuId, MenuState};
use crate::panel::{ClipboardFeedback, CursorMove, DisplayMode, PanelState, TreeState};
use crate::panel_split;
use crate::quicksearch::{self, FrecencyEntry, FuzzyJumpState};
use crate::shell::{self, ShellConfig};
use crate::theme::Theme;
use crate::viewer::ViewerState;

mod command;
mod editor;
mod file_op;
mod listing;
mod overlays;
mod panels;
mod viewer;

pub use command::*;
pub use listing::panel_viewport_rows;
pub use overlays::{button_command, menu_action_command, Overlay};
pub use panels::{parse_cd, parse_del, parse_rmdir, resolve_cd_target};

use editor::*;
use file_op::*;
use listing::*;
use overlays::*;
use panels::*;
use viewer::*;

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

/// Top-level UI phase. Governs how commands are interpreted by `update`.
///
/// The modal overlays (the F9 menu, the drive-select dialog, and the rest of
/// [`Overlay`]) deliberately live *beside* the phase, in [`State::overlay`],
/// rather than inside it: they overlay the panels without replacing whatever
/// phase is underneath.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiPhase {
    Splash {
        started_at_ms: u64,
    },
    Panels,
    /// Gathering input before a Copy/Move/Mkdir/Delete job is dispatched.
    FileOpSetup(FileOpSetup),
    /// A job is running on the worker thread; `dialog` shows progress, or a
    /// conflict/error the worker is blocked waiting on.
    FileOpRunning {
        source_dir: PathBuf,
        dest_dir: PathBuf,
        dialog: RunningDialog,
    },
    /// End-of-job summary, shown when the job skipped 1+ items or recorded
    /// 1+ metadata-restore warnings — the two kinds share this vector and
    /// are told apart by `SkippedItem::kind` (operation-dialogs "End-of-job
    /// skipped-files summary", "Metadata-restore failures are reported in
    /// the end-of-job summary"; error-diagnostics design D5).
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
    /// The live command-line buffer — a shared text field per
    /// text-field-editing (the caret model and editing keys). Printable keys
    /// land here whenever a panel is focused and no dialog or quick-search
    /// owns them.
    pub command_line: LineEdit,
    /// Executed commands, oldest first.
    pub history: Vec<String>,
    /// Where Up/Down recall currently sits in `history`; `None` means "not
    /// recalling", which is the state every edit resets to.
    pub history_cursor: Option<usize>,
    /// The §4.7 type-ahead quick-search pattern. While this is `Some`, it —
    /// not the command line — consumes plain printable keys. Exactly one
    /// typing sink ever sees a given key.
    pub quick_search: Option<String>,
    /// The one modal overlay open above the phase, or `None` (reducer-
    /// overlay-structure design D1). Set through [`State::open_overlay`]
    /// (which debug-asserts the slot is empty) and
    /// [`State::set_startup_warning`], read through [`State::overlay`] and
    /// the typed accessors (`menu()`, `fuzzy_jump()`, …). It lives beside the
    /// phase: an overlay opens over the panels (or, for the startup warning,
    /// over whatever phase is showing) without replacing them.
    pub overlay: Option<Overlay>,
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
    /// `true` while `term_size` is below the 60x16 hard floor. A flag
    /// *beside* the phase rather than a phase of its own, so shrinking the
    /// terminal never discards the active screen: the phase, every overlay
    /// and every dialog stay exactly as they were, the renderer draws the
    /// "terminal too small" placeholder over them, and the TUI's input
    /// mappers let nothing but F10/Esc (the quit dialog) and the quit/
    /// startup-warning dialogs' own keys through until the terminal is
    /// enlarged again (application-shell "Resize handling with 60x16 hard
    /// floor and placeholder"; resize-preserves-session-state design D1).
    pub undersized: bool,
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
    /// fresher answer. See `State::next_request_id`.
    pub request_seq: u64,
    /// Directory frecency history backing the Ctrl+J fuzzy-jump dialog,
    /// persisted alongside command history in `history.json` (fuzzy-jump
    /// "Directory history persistence"; design D6).
    pub dir_history: Vec<FrecencyEntry>,
    /// The most recent monotonic clock reading `Command::Tick` delivered.
    /// Drives every animation and in-session timing; `update` never reads a
    /// clock directly — the TUI's injected `Clock` is still the only source
    /// of "now".
    pub clock_ms: u64,
    /// The most recent wall-clock reading (Unix-epoch milliseconds)
    /// `Command::Tick` delivered. Used only to stamp and rank frecency
    /// visits, so recency stays meaningful across restarts (fuzzy-jump
    /// "Frecency ranking"; history-persistence-hardening design D1).
    pub epoch_ms: u64,
    /// The F2 user menu's entries, snapshotted from `usermenu.toml` at
    /// startup (user-menu "Parse label and command entries from
    /// usermenu.toml").
    pub user_menu_entries: Vec<UserMenuEntry>,
    /// The `usermenu.toml` path the application resolved at startup (the
    /// profile directory's file — config-profile-directory), opened by
    /// Commands → Menu file edit / `Command::EditUserMenuFile` and reloaded
    /// via `Command::UserMenuReloaded` when the editor returns (user-menu
    /// "Edit the user menu file from the Commands menu"; design D4).
    pub user_menu_path: PathBuf,
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
    /// (mouse-panel-drag). Lives beside the phase like `overlay` — it
    /// overlays the live panels rather than replacing them. Cleared by a
    /// reducer post-condition (see `update`'s wrapper below) whenever a
    /// command leaves `UiPhase::Panels` or opens an overlay,
    /// so it can never survive into a context where a drop could complete
    /// (mouse-drag "Cancel and phase-change clear the drag"; design D5).
    pub drag: Option<DragState>,
    /// Whether the decorative UI animations (splash entrance, menu sweeps
    /// and unfolds, dialog iris, panel wipe) run at all. Copied from
    /// `config.toml`'s `animations` key / `--noanimations` by the TUI at
    /// startup; `false` in [`State::empty`] so states built without
    /// configuration — every existing reducer and snapshot test — render
    /// their final frame regardless of `clock_ms` (ui-animations
    /// "Animations are disabled by config or launch flag"; design D5).
    pub animations: bool,
    /// `clock_ms` when the currently visible modal dialog opened — or was
    /// replaced by a different one in the same slot (progress → conflict).
    /// Only one of these dialogs is ever visible at a time, so one stamp
    /// suffices; set by the reducer's animation post-condition in [`update`]
    /// and read only by the renderer's iris (design D4).
    pub dialog_opened_at_ms: u64,
    /// `clock_ms` when `quit_confirm` last became `true`. Separate from
    /// `dialog_opened_at_ms` because the quit dialog layers over whichever
    /// dialog is open without making that one re-animate.
    pub quit_opened_at_ms: u64,
    /// `clock_ms` when the splash gave way to the panels — `Some` only on
    /// that transition, so a skipped splash (`--nosplash`, below-floor
    /// placeholder) leaves frame 1 as the complete panel screen with no
    /// wipe.
    pub panels_revealed_at_ms: Option<u64>,
    /// Working-directory roots of repositories whose git status query timed
    /// out this session, spelled the way the queried panel path spelled them
    /// (git-info-timeout-memory design D2). While a root is remembered here,
    /// no git query is issued for any directory under it and the panel shows
    /// no git indicators there (git-info "Silent absence on timeout and
    /// stale-result discarding"). Session-only, never persisted.
    pub git_no_info_roots: Vec<PathBuf>,
    /// The command-line `cd`/`del`/`rmdir` target check in flight on a
    /// worker thread, or `None`. The reducer itself never touches the
    /// filesystem: a target that is not already listed in the panel is
    /// checked through `Effect::ProbePath`, and the command completes when
    /// the matching `Command::PathProbed` arrives (application-shell "The
    /// reducer performs no filesystem I/O"; offload-blocking-io design D1).
    pub command_probe: Option<PendingProbe>,
    /// The F3/F4 open in flight on a worker thread, or `None`. A reply is
    /// applied only when its id matches, the panels still own input, and no
    /// menu, dialog or overlay has opened since (application-shell "Slow
    /// storage never blocks the event loop"; design D4).
    pub pending_open: Option<PendingOpen>,
}

impl State {
    /// The clock reading animated surfaces should interpolate against:
    /// `Some(clock_ms)` when animations are on, `None` when every surface
    /// must render its final frame. The one place views learn whether to
    /// animate, so "disabled" is byte-identical to the pre-animation render.
    pub fn anim_now(&self) -> Option<u64> {
        self.animations.then_some(self.clock_ms)
    }

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
        if let Some(picker) = self.theme_picker() {
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
            command_line: LineEdit::default(),
            history: Vec::new(),
            history_cursor: None,
            quick_search: None,
            overlay: None,
            shell: ShellConfig::default(),
            editor: None,
            phase: UiPhase::Panels,
            theme,
            term_size: (MIN_COLS, MIN_ROWS),
            undersized: false,
            split_percent: panel_split::DEFAULT_SPLIT_PERCENT,
            request_seq: 0,
            dir_history: Vec::new(),
            clock_ms: 0,
            epoch_ms: 0,
            user_menu_entries: Vec::new(),
            user_menu_path: PathBuf::new(),
            quit_confirm: false,
            drag: None,
            animations: false,
            dialog_opened_at_ms: 0,
            quit_opened_at_ms: 0,
            panels_revealed_at_ms: None,
            git_no_info_roots: Vec::new(),
            command_probe: None,
            pending_open: None,
        }
    }

    fn too_small(size: (u16, u16)) -> bool {
        size.0 < MIN_COLS || size.1 < MIN_ROWS
    }

    /// The open modal overlay, if any (design D1).
    pub fn overlay(&self) -> Option<&Overlay> {
        self.overlay.as_ref()
    }

    /// Open `overlay` in the slot. Every synchronous opener runs with the
    /// slot empty — overlays open only from the `Panels` arm, which a command
    /// reaches only when no open overlay claimed it, and an overlay that
    /// leads to another closes itself first (`OverlayStep::Replace`/
    /// `CloseThen`) — so a non-empty slot here is a routing bug, caught in
    /// debug builds (design D5). A release build keeps the newer overlay.
    pub fn open_overlay(&mut self, overlay: Overlay) {
        debug_assert!(self.overlay.is_none(), "open_overlay over an open {:?}", self.overlay.as_ref().map(std::mem::discriminant));
        self.overlay = Some(overlay);
    }

    /// Close whatever overlay is open, returning it.
    pub fn close_overlay(&mut self) -> Option<Overlay> {
        self.overlay.take()
    }

    /// Raise (or replace) the startup warning: the one overlay whose openers
    /// are last-writer-wins — `app::run`'s startup assignments, the user-menu
    /// reload after an edit — so it installs over an empty slot and replaces
    /// the message of an existing warning, rather than asserting an empty
    /// slot like [`State::open_overlay`] (design D5). It debug-asserts that
    /// the slot holds nothing else: the warning is raised at session start
    /// and after the editor over `usermenu.toml` returns, both with no other
    /// overlay open.
    pub fn set_startup_warning(&mut self, message: String) {
        debug_assert!(
            matches!(self.overlay, None | Some(Overlay::StartupWarning(_))),
            "startup warning over an open {:?}",
            self.overlay.as_ref().map(std::mem::discriminant)
        );
        self.overlay = Some(Overlay::StartupWarning(message));
    }

    /// The startup warning's message while that overlay is open.
    pub fn startup_warning(&self) -> Option<&str> {
        match &self.overlay {
            Some(Overlay::StartupWarning(message)) => Some(message),
            _ => None,
        }
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
    /// (flag wins). `epoch_ms` is the wall-clock reading at startup, so
    /// visits recorded before the first `Tick` are stamped correctly
    /// (history-persistence-hardening design D1).
    pub fn initial(
        theme: Theme,
        term_size: (u16, u16),
        now_ms: u64,
        epoch_ms: u64,
        left_cwd: PathBuf,
        right_cwd: PathBuf,
        show_splash: bool,
    ) -> (State, Vec<Effect>) {
        // Below the floor the splash is skipped outright: the session starts
        // on the panels with the placeholder drawn over them, and enlarging
        // simply reveals the panels (startup-splash "Below the floor at
        // startup"; design D2).
        let undersized = Self::too_small(term_size);
        let phase = if show_splash && !undersized { UiPhase::Splash { started_at_ms: now_ms } } else { UiPhase::Panels };
        let mut state = State {
            left: PanelState::new(left_cwd.clone()),
            right: PanelState::new(right_cwd.clone()),
            phase,
            term_size,
            undersized,
            clock_ms: now_ms,
            epoch_ms,
            ..State::empty(theme)
        };
        // Each panel's first listing is tagged like every later one
        // (listing-request-guard design D1), so a reply from it can be told
        // apart from the listing a navigation issued before it finished.
        let left_request = state.next_request_id();
        state.left.listing_request = Some(left_request);
        let right_request = state.next_request_id();
        state.right.listing_request = Some(right_request);
        let mut effects = vec![
            Effect::StartListing { panel: PanelSide::Left, path: left_cwd.clone(), request: left_request },
            Effect::StartListing { panel: PanelSide::Right, path: right_cwd.clone(), request: right_request },
        ];
        // Both panels start their git-info query alongside their first
        // listing, exactly as a later navigation does (git-info "Background
        // repository detection").
        effects.extend(git_info_query_effect(&mut state, PanelSide::Left, left_cwd));
        effects.extend(git_info_query_effect(&mut state, PanelSide::Right, right_cwd));
        (state, effects)
    }
}

/// The typed read accessors over [`State::overlay`] (design D1): each
/// returns its overlay's state while that variant is open, and `None`
/// otherwise, so TUI readers and tests never match on the slot by hand.
macro_rules! overlay_accessors {
    ($($(#[$doc:meta])* $name:ident / $name_mut:ident => $variant:ident($ty:ty);)*) => {
        impl State {
            $(
                $(#[$doc])*
                pub fn $name(&self) -> Option<&$ty> {
                    match &self.overlay {
                        Some(Overlay::$variant(v)) => Some(v),
                        _ => None,
                    }
                }

                $(#[$doc])*
                pub fn $name_mut(&mut self) -> Option<&mut $ty> {
                    match &mut self.overlay {
                        Some(Overlay::$variant(v)) => Some(v),
                        _ => None,
                    }
                }
            )*
        }
    };
}

overlay_accessors! {
    /// The F9 menu while it is open.
    menu / menu_mut => Menu(MenuState);
    /// The drive-select dialog while it is open.
    drive_select / drive_select_mut => DriveSelect(DriveSelect);
    /// The Ctrl+J fuzzy-jump dialog while it is open.
    fuzzy_jump / fuzzy_jump_mut => FuzzyJump(FuzzyJumpState);
    /// The Alt+F7 find-file dialog while it is open.
    find_file / find_file_mut => FindFile(FindFileState);
    /// The F2 user menu while it is open.
    user_menu / user_menu_mut => UserMenu(UserMenuState);
    /// The Alt+F8 command-history dialog while it is open.
    history_dialog / history_dialog_mut => HistoryDialog(HistoryDialogState);
    /// The Options → Themes picker while it is open.
    theme_picker / theme_picker_mut => ThemePicker(ThemePickerState);
    /// The Enter-on-file action menu while it is open.
    file_action_menu / file_action_menu_mut => FileActionMenu(FileActionMenuState);
    /// The F1 Help window while it is open.
    help / help_mut => Help(HelpState);
}

/// Which layer owns input right now — the single precedence order the
/// reducer's routing, the TUI's key and mouse routing, and its render and
/// hit-map dispatch all follow (reducer-overlay-structure design D2;
/// application-shell "Modal overlays are mutually exclusive by
/// construction"). Highest first: the quit confirmation; the "terminal too
/// small" placeholder; a file-operation dialog; the viewer; the editor; the
/// open overlay; the panels or the startup splash. A layer handles only the
/// commands it claims and lets everything else pass to the layers below it,
/// so worker replies still reach a running job while the quit dialog is
/// open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputLayer {
    /// The quit-confirmation dialog, reachable from every context.
    QuitConfirm,
    /// The terminal is below the 60x16 floor: the placeholder hides the
    /// active screen, and only F10/Esc (a quit request) get through
    /// (application-shell "Resize handling with 60x16 hard floor and
    /// placeholder"). An open startup warning keeps its keys above it.
    Undersized,
    /// A file-operation dialog: setup, running job, or end-of-job summary.
    FileOp,
    /// The F3 viewer.
    Viewer,
    /// The F4 built-in editor.
    Editor,
    /// The open modal overlay in [`State::overlay`].
    Overlay,
    /// The panels and the command line.
    Panels,
    /// The startup splash.
    Splash,
}

/// The layer that owns input for `state` (design D2). See [`InputLayer`] for
/// the order.
pub fn input_layer(state: &State) -> InputLayer {
    if state.quit_confirm {
        return InputLayer::QuitConfirm;
    }
    // The startup warning is drawn over the placeholder and keeps its keys
    // there (resize-preserves-session-state design D3); every other screen
    // is hidden, so nothing may reach it.
    if state.undersized && !matches!(state.overlay, Some(Overlay::StartupWarning(_))) {
        return InputLayer::Undersized;
    }
    screen_layer(state)
}

/// The layer beneath the quit confirmation and the placeholder: what the
/// active screen itself is. This is the order the reducer routes a command
/// the quit layer did not claim through — the placeholder gates keys in the
/// TUI only, never the worker replies and timer ticks that must keep
/// reaching the hidden screen.
fn screen_layer(state: &State) -> InputLayer {
    match &state.phase {
        UiPhase::FileOpSetup(_) | UiPhase::FileOpRunning { .. } | UiPhase::FileOpSummary(_) => InputLayer::FileOp,
        UiPhase::Viewer(_) => InputLayer::Viewer,
        UiPhase::Editor(_) => InputLayer::Editor,
        UiPhase::Splash { .. } | UiPhase::Panels if state.overlay.is_some() => InputLayer::Overlay,
        UiPhase::Splash { .. } => InputLayer::Splash,
        UiPhase::Panels => InputLayer::Panels,
    }
}

/// The pure state transition. Equal `(state, command)` always yields equal
/// `(state, Vec<Effect>)`. A thin wrapper around `update_impl` that
/// enforces the one invariant that must hold no matter which internal path a
/// command took: `state.drag` is never `Some` outside `UiPhase::Panels` with
/// no overlay open (mouse-drag "Cancel and phase-change clear the drag";
/// design D5) — see `drag_allowed`. Applying it here, once, after every
/// call — rather than threading a clear into each of `update_impl`'s many
/// phase-transition arms — is what makes it a true postcondition: every
/// return path is covered by construction, including ones a future phase
/// addition might add without remembering this rule.
///
/// The same wrapper records the animation stamps (`crate::stamps`): when an
/// overlay opened or changed during the transition, it is stamped with
/// `state.clock_ms`. That is all the reducer does for animations — it never
/// delays a transition (ui-animations "Render-only motion model driven by the
/// reducer clock").
pub fn update(state: State, cmd: Command) -> (State, Vec<Effect>) {
    // Only a failure the panel actually accepts counts: a stale
    // `ListingFailed` from a directory the user already left is dropped by
    // `apply_listing_event` and must not cancel a drag in progress elsewhere
    // (panel-navigation "Superseded failure does not mark the new listing
    // failed"; listing-request-guard design D2).
    let listing_failed = matches!(cmd, Command::ListingFailed { .. }) && listing_reply_is_current(&state, &cmd);
    let marks = crate::stamps::Snapshot::capture(&state);
    let (mut state, effects) = update_impl(state, cmd);
    marks.apply(&mut state);
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
/// `UiPhase::Panels` with no modal overlay open — the same set
/// `input::mouse::context` (mouse-basics) already gates ordinary mouse input
/// behind, extended with `quit_confirm` since it uniquely overlays every
/// other context too (mouse-drag "Cancel and phase-change clear the drag":
/// job completion, listing failure, F9, quit request, resize below the
/// minimum — every one of those either leaves `UiPhase::Panels` or sets one
/// of these fields; the resize sets `undersized`, since the phase no longer
/// changes to signal it — design D6).
fn drag_allowed(state: &State) -> bool {
    matches!(state.phase, UiPhase::Panels) && !state.undersized && state.overlay.is_none() && !state.quit_confirm
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

    // Resize is handled uniformly regardless of phase. Dropping below the
    // 60x16 floor only raises `undersized`; the phase and every overlay are
    // left exactly as they are, so the screen the placeholder hides —
    // editor buffer, viewer position, running job and its dialog, open menus
    // — is still there when the terminal is enlarged (application-shell
    // "Resize handling with 60x16 hard floor and placeholder"; design D1).
    if let Command::Resize(w, h) = cmd {
        let was_undersized = state.undersized;
        state.term_size = (w, h);
        state.undersized = State::too_small((w, h));
        // The splash is the one exception: shrinking dismisses it for good,
        // and enlarging reveals the panels rather than the splash
        // (startup-splash "Shrinking below the floor mid-splash"; design D2).
        if state.undersized && matches!(state.phase, UiPhase::Splash { .. }) {
            state.phase = UiPhase::Panels;
        }
        // A shrinking terminal can leave either panel's cursor below its new,
        // shorter window; re-clamp both sides unconditionally (panel-
        // navigation "Terminal resize re-clamps").
        reconcile_panel_viewport(&mut state, PanelSide::Left);
        reconcile_panel_viewport(&mut state, PanelSide::Right);
        // `handle_editor` skips its caret re-scroll while undersized (design
        // D6), so the first resize back above the floor does it once here
        // against the restored viewport. The buffer and caret are untouched.
        if was_undersized && !state.undersized {
            if let UiPhase::Editor(editor) = &mut state.phase {
                let (rows, cols) = editor_viewport(state.term_size);
                editor.ensure_caret_visible(rows, cols);
            }
        }
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
    if let Command::Tick { now_ms, epoch_ms } = cmd {
        state.clock_ms = now_ms;
        state.epoch_ms = epoch_ms;
        for side in [PanelSide::Left, PanelSide::Right] {
            if matches!(&state.panel(side).clipboard_feedback, Some(f) if now_ms >= f.expires_at_ms) {
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
        // Applied only while the slot holds the drive-select dialog this
        // label was fetched for, so a late label can never touch another
        // overlay (reducer-overlay-structure design D5).
        Command::DriveLabelResolved { target, letter, label, generation } => {
            if let Some(dialog) = state.drive_select_mut() {
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
        Command::GitInfoResolved { panel, path, request, info, timed_out_root } => {
            // A timeout is evidence about the *repository*, not about the
            // panel's current directory, so the root is recorded even when
            // the reply itself is stale (git-info "Timeout recorded after
            // the panel moved on"). A root already covered by a remembered
            // one is not stored twice.
            if let Some(root) = timed_out_root {
                if !state.git_no_info_roots.iter().any(|existing| git_info::is_under_root(&root, existing)) {
                    state.git_no_info_roots.push(root);
                }
            }
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
        // The command-line target probe and the F3/F4 open replies are
        // matched against their pending request here, ahead of every
        // phase- and overlay-specific arm, so a reply that arrives after
        // the user opened a menu, a dialog, the viewer or the editor is
        // recognized as superseded and discarded rather than reaching the
        // Panels arm through an overlay that does not claim it
        // (offload-blocking-io design D1/D4).
        Command::PathProbed { request, kind } => {
            effects.extend(apply_path_probed(&mut state, request, kind));
            return (state, effects);
        }
        Command::ViewerOpened { request, path, file_len } => {
            if let Some(side) = take_current_open(&mut state, request) {
                // A prior failed F3/F4 attempt may have left `last_error`
                // set on this panel; a successful open has clearly moved
                // past it, so the mini-status line must not keep showing
                // the stale message (§7 error policy — errors are surfaced
                // until superseded by success, not left to linger).
                state.panel_mut(side).last_error = None;
                state.phase = UiPhase::Viewer(ViewerState::new(path, file_len));
            }
            return (state, effects);
        }
        Command::ViewerOpenFailed { request, message } => {
            if let Some(side) = take_current_open(&mut state, request) {
                state.panel_mut(side).last_error = Some(message);
            }
            return (state, effects);
        }
        Command::EditorOpened { request, editor } => {
            if let Some(side) = take_current_open(&mut state, request) {
                // Mirrors the `Command::ViewerOpened` clear (§7 error policy).
                state.panel_mut(side).last_error = None;
                state.phase = UiPhase::Editor(*editor);
            }
            return (state, effects);
        }
        Command::EditorTooLarge { request, path, size: _ } => {
            if let Some(side) = take_current_open(&mut state, request) {
                // builtin-editor "Large file redirects to the viewer": the
                // notice lands on the panel's mini-status (the same surface
                // `Command::ViewerOpenFailed` uses) since the viewer chrome
                // itself has no room reserved for a banner; it's visible the
                // moment the user returns from the viewer to the panels. The
                // viewer open is a fresh request for the same side (design
                // D4); its `Opening…` note is set first so it does not wipe
                // the notice.
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
                effects.push(request_open(&mut state, side, PendingOpenKind::Viewer, path));
                state.panel_mut(side).last_error = Some(format!("{name} is too large for the editor (10 MB limit) — opened in the viewer"));
            }
            return (state, effects);
        }
        Command::EditorNotEditable { request, path, reason } => {
            if let Some(side) = take_current_open(&mut state, request) {
                // Same mechanism and notice surface as the size redirect
                // (design D2); the wording names the reason so a UTF-16
                // file is not just "not UTF-8".
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
                effects.push(request_open(&mut state, side, PendingOpenKind::Viewer, path));
                state.panel_mut(side).last_error = Some(match reason {
                    NotEditableReason::Utf16 => format!("{name} is UTF-16 text — the editor only edits UTF-8; opened in the viewer"),
                    NotEditableReason::Binary => {
                        format!("{name} looks binary (contains NUL bytes) — the editor only edits UTF-8 text; opened in the viewer")
                    }
                    NotEditableReason::NotUtf8 => format!("{name} is not valid UTF-8 — the editor only edits UTF-8 text; opened in the viewer"),
                });
            }
            return (state, effects);
        }
        Command::EditorOpenFailed { request, message } => {
            if let Some(side) = take_current_open(&mut state, request) {
                state.panel_mut(side).last_error = Some(message);
            }
            return (state, effects);
        }
        _ => {}
    }

    // From here on the command is routed through the layers of the single
    // precedence order (`input_layer`; reducer-overlay-structure design D2):
    // each layer handles only the commands it claims, and everything else
    // falls through to the next layer down.
    //
    // The quit-confirmation dialog is the topmost layer, uniquely reachable
    // from *every* context — panels, the viewer, an open overlay, and every
    // dialog, including while a file operation is running — so it is handled
    // here, ahead of every layer below, rather than gated behind any one of
    // them (application-shell "Quit request keys and confirmation"; design
    // D5). It claims only `ConfirmQuit` and `CancelQuit`; worker replies and
    // everything else pass beneath it.
    if let Command::RequestQuit = cmd {
        state.quit_confirm = true;
        return (state, effects);
    }
    if input_layer(&state) == InputLayer::QuitConfirm {
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

    // The layers below the quit confirmation, in the same order
    // (`screen_layer`: the placeholder gates keys in the TUI only, so the
    // reducer looks straight through it).
    let layer = screen_layer(&state);

    // File-op setup/running/summary phases are handled uniformly here,
    // independent of the Splash/Panels phases below. This layer also claims
    // the job events that drive them from every phase, so a running job's
    // replies reach it beneath the quit dialog.
    if layer == InputLayer::FileOp
        || matches!(cmd, Command::JobProgress(_) | Command::JobConflict(_) | Command::JobError(_) | Command::JobDone { .. })
    {
        effects.extend(handle_file_op(&mut state, cmd));
        return (state, effects);
    }

    // The F3 viewer replaces the panels full-screen and owns input focus
    // while open (viewer: Frame-less full-screen chrome — "Viewer owns
    // focus while open"), so its commands are handled uniformly here.
    if layer == InputLayer::Viewer {
        effects.extend(handle_viewer(&mut state, cmd));
        return (state, effects);
    }

    // The F4 built-in editor, likewise, replaces the panels full-screen and
    // owns input focus while open (builtin-editor "Full-screen editor
    // chrome").
    if layer == InputLayer::Editor {
        effects.extend(handle_editor(&mut state, cmd));
        return (state, effects);
    }

    // The open modal overlay claims the commands it understands and nothing
    // else: take it out of the slot, hand its state to its handler, and
    // restore the slot from the step the handler returns (design D3). A
    // command it does not claim falls through to the base layer with the
    // overlay back in place, exactly as before.
    if let Some(mut overlay) = state.overlay.take() {
        if overlay.claims(&cmd) {
            return match overlay.handle(&mut state, cmd) {
                OverlayStep::Keep(more) => {
                    state.overlay = Some(overlay);
                    effects.extend(more);
                    (state, effects)
                }
                OverlayStep::Close(more) => {
                    effects.extend(more);
                    (state, effects)
                }
                OverlayStep::Replace(next, more) => {
                    state.overlay = Some(next);
                    effects.extend(more);
                    (state, effects)
                }
                // An activated menu item re-enters as the command it stands
                // for, so a menu action and its keyboard shortcut share one
                // implementation — and the item's dialog opens into the slot
                // the menu just left (application-shell "A menu item opens
                // its dialog in place of the menu").
                OverlayStep::CloseThen(next) => update_impl(state, next),
            };
        }
        state.overlay = Some(overlay);
    }

    match &state.phase {
        UiPhase::Splash { started_at_ms } => match cmd {
            Command::Tick { now_ms, .. } => {
                if now_ms.saturating_sub(*started_at_ms) >= SPLASH_MIN_HOLD_MS {
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
        UiPhase::Panels => return handle_panels(state, cmd, effects),
        UiPhase::FileOpSetup(_) | UiPhase::FileOpRunning { .. } | UiPhase::FileOpSummary(_) | UiPhase::Viewer(_) | UiPhase::Editor(_) => {
            unreachable!("handled above")
        }
    }

    (state, effects)
}

#[cfg(test)]
mod tests;
