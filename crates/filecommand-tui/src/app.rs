//! The event loop: drains crossterm input + worker events, converts them to
//! `Command`s, applies `core::update`, executes the returned effects, and
//! redraws. This is the only place effects are actually performed.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime};

use crossterm::event::{self, Event, KeyEvent, KeyEventKind, MouseEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

use filecommand_core::clock::{format_clock, Clock, WallClock};
use filecommand_core::editor::{EditorMove, EditorState, LoadResult as EditorLoadResult};
use filecommand_core::external_editor::EditorInvocation;
use filecommand_core::fs_ops::CancelFlag;
use filecommand_core::line_edit::flatten_line_breaks;
use filecommand_core::listing::DateTime;
use filecommand_core::shell::{Invocation, ShellConfig};
use filecommand_core::theme::{ColorDepth, Theme, BUILTIN_THEME_NAMES};
use filecommand_core::update::ClipboardPayloadKind;
use filecommand_core::viewer::hex::HEX_BYTES_PER_ROW;
use filecommand_core::viewer::{backward, forward, ByteSource, ViewMode, ViewerState};
use filecommand_core::{anim, config, drives, identity, update, Command, Effect, PanelSide, State, UiPhase};

use crate::clipboard::{self, Clipboard, PlatformClipboard};
use crate::clock::{RealClock, RealWallClock};
use crate::console;
use crate::diag;
use crate::history_writer::HistoryWriter;
use crate::hitmap::HitMap;
use crate::input::{self, MouseTracker, ViewerInput};
use crate::terminal::TerminalGuard;
use crate::views;
use crate::worker;

const POLL_INTERVAL: Duration = Duration::from_millis(33);
/// A burst of already-queued input applies at most this many events before
/// a draw, so the screen never falls far behind the keyboard
/// (idle-aware-event-loop "Bursts of key input render once"; design D5).
const BURST_MAX_EVENTS: usize = 64;
/// The burst's other bound: about this much processing time, then a draw.
const BURST_MAX_DURATION: Duration = Duration::from_millis(8);
/// How long a confirmed quit waits for a running job to finish its
/// cancellation cleanup — removing a partially written destination file —
/// before exiting anyway (application-shell "Quit waits for job cancellation
/// cleanup"; process-lifecycle-hardening design D3). Bounded so a write
/// blocked on an unresponsive share cannot hang quit.
const QUIT_JOB_WAIT: Duration = Duration::from_secs(2);
/// How often [`shutdown_job`] re-checks whether the job thread has ended.
const QUIT_JOB_POLL: Duration = Duration::from_millis(10);
const CONFIG_FILE: &str = "config.toml";
const RESUME_PROMPT: &str = "Press any key to return to FileCommand . . .";

/// Everything the effect executor needs that isn't part of `core::State`:
/// channels, the terminal, and the paths persistence writes to.
struct Runtime {
    tx: Sender<Command>,
    active_job: Option<worker::JobHandle>,
    /// The cancel flags of the cancellable background requests still running
    /// — directory listings, find-file walks, viewer searches — keyed by the
    /// request id the reducer minted for them, so `Effect::CancelRequest`
    /// can stop one (listing-request-guard design D3). Owned here, like
    /// `active_job`, because flags are TUI plumbing, not pure state.
    cancels: CancelRegistry,
    /// The background `history.json` writer `Effect::PersistHistory` hands
    /// its snapshot to; it owns the history path, and is the disabled no-op
    /// when persistence is off for the session. Owned here so every exit
    /// path from `run` flushes it on drop (fuzzy-jump "Quitting flushes
    /// unwritten history"; history-persistence-hardening design D4).
    history_writer: HistoryWriter,
    /// Where `Effect::PersistTheme` writes the `theme =` key back
    /// (theme-selection "Applied theme persists to configuration").
    config_path: PathBuf,
    /// Where the F2 user menu lives; `Command::EditUserMenuFile` opens it via
    /// `State::user_menu_path`, and when the editor that had it open returns
    /// (built-in close or external exit) the file is reloaded and fed back as
    /// `Command::UserMenuReloaded` (user-menu "Edit the user menu file from
    /// the Commands menu"; design D4).
    usermenu_path: PathBuf,
    /// `false` when the profile directory could not be created at startup:
    /// the three persist effects are then skipped for the whole session
    /// (config-profile-directory "Unwritable profile falls back without
    /// losing the session"; design D2).
    persist_enabled: bool,
    /// The F3 viewer's open byte window, kept here rather than in
    /// `core::State` because `ByteSource` is an I/O handle, not application
    /// state (design D1). Set on `Effect::OpenViewer`, cleared on
    /// `Command::ViewerClose` — the only path out of `UiPhase::Viewer`.
    viewer_source: Option<(PathBuf, ByteSource)>,
    /// The OS clipboard behind `Effect::SetClipboard` — `Box<dyn Clipboard>`
    /// rather than a concrete type so tests can swap in
    /// `clipboard::RecordingClipboard` (clipboard-export; design D3).
    clipboard: Box<dyn Clipboard>,
    /// Press/double-click bookkeeping for `input::map_mouse`, kept here
    /// (rather than a local in `run`'s loop) so it's reachable from
    /// `run_effects`, which resets it at every `guard.resume()` call site —
    /// crossterm may otherwise believe a button is still held after a
    /// suspended shell/editor/scrollback run (design D1 risk).
    mouse_tracker: MouseTracker,
    /// Per-panel git-query slots (left, right) bounding the app to one
    /// in-flight git status query per panel: a navigation while one is
    /// running queues only the newest request, and a queued request inside a
    /// repository that just timed out is never started (git-info "Rapid
    /// navigation does not pile up queries"; git-info-timeout-memory design
    /// D3).
    git_slots: [worker::GitQuerySlot; 2],
    /// How many worker threads that can still post to `tx` are alive
    /// (idle-aware-event-loop design D3): the loop polls at the usual ~33 ms
    /// cadence only while this is nonzero, and sleeps until the next
    /// time-driven deadline otherwise.
    workers: worker::WorkerGauge,
}

/// The cancel flags of the background requests the reducer can supersede
/// (listing-request-guard design D3): one `fs_ops::job::CancelFlag` — the
/// same flag that backs job cancellation — per running `StartListing`,
/// `FindInSubtree`, or `RunViewerSearch`, keyed by request id. The reducer
/// decides supersession (it mints and compares the ids) and emits
/// `Effect::CancelRequest(id)`; this registry owns the flags and flips the
/// right one. Ids are globally unique (`State::next_request_id`), so one map
/// serves all three request kinds.
#[derive(Default)]
struct CancelRegistry {
    flags: HashMap<u64, CancelFlag>,
}

impl CancelRegistry {
    /// A fresh, unset flag for the request `id`, remembered until it is
    /// cancelled or retired. The clone handed back travels to the worker.
    fn register(&mut self, id: u64) -> CancelFlag {
        let flag = CancelFlag::new();
        self.flags.insert(id, flag.clone());
        flag
    }

    /// `Effect::CancelRequest(id)`: set the flag and forget the entry. An
    /// unknown id — a request that already finished, or one that was never
    /// cancellable — is a silent no-op, so the reducer never needs to know
    /// whether a worker is still alive.
    fn cancel(&mut self, id: u64) {
        if let Some(flag) = self.flags.remove(&id) {
            flag.cancel();
        }
    }

    /// Forget the entry without setting its flag: the request's final reply
    /// has passed through, so there is nothing left to stop.
    fn retire(&mut self, id: u64) {
        self.flags.remove(&id);
    }
}

impl Runtime {
    /// `Effect::OpenViewer`'s effect body. O(1): the open touches only the
    /// handle and its length, never the content (viewer: Instant open —
    /// "First frame reads only the visible window"), so this runs
    /// synchronously rather than on a worker thread, exactly like
    /// `EnumerateDrives`.
    fn open_viewer_source(&mut self, path: PathBuf) {
        match ByteSource::open(&path) {
            Ok(source) => {
                let file_len = source.len();
                self.viewer_source = Some((path.clone(), source));
                worker::deliver(&self.tx, "viewer opened", Command::ViewerOpened { path, file_len });
            }
            Err(e) => {
                worker::deliver(&self.tx, "viewer open failure", Command::ViewerOpenFailed { message: cannot_message("open", &path, &e) });
            }
        }
    }

    /// `Command::ViewerClose`'s release of the viewer's byte source, so the
    /// file handle is closed with the viewer (viewer: Instant open —
    /// "Closing the viewer releases the file").
    fn release_viewer_source(&mut self) {
        self.viewer_source = None;
    }

    /// `apply`'s post-`update` check (resize-preserves-session-state design
    /// D7): the source goes when the viewer phase has just ended by any
    /// route, or when a `ViewerOpened` was applied but the phase did not
    /// become `Viewer` (the reply was discarded). It is keyed to that
    /// *transition*, never to "the phase is not `Viewer`" — `open_viewer_source`
    /// installs the source while the phase is still `Panels`, and a command
    /// applied before the queued `ViewerOpened` must not drop it.
    fn release_viewer_source_if_viewer_ended(&mut self, was_viewer: bool, viewer_opened: bool, after: &State) {
        if (was_viewer || viewer_opened) && !matches!(after.phase, UiPhase::Viewer(_)) {
            self.release_viewer_source();
        }
    }
}

/// Launch-time command-line switches, parsed once by [`parse_launch_args`]
/// and threaded through to [`run`]. Kept as a small struct rather than a
/// growing list of `bool`/`Option` parameters on `run` itself (design D2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchOptions {
    /// `--nosplash`: skip the startup splash screen regardless of
    /// `config.toml`'s `splash` key (startup-splash).
    pub no_splash: bool,
    /// `--nomouse`: disable mouse capture regardless of `config.toml`'s
    /// `[mouse] enabled` key, mirroring `--nosplash` (mouse-input "Mouse
    /// capture configuration").
    pub no_mouse: bool,
    /// `--noanimations`: run with every decorative animation off regardless
    /// of `config.toml`'s `animations` key, mirroring `--nosplash`
    /// (ui-animations "Animations are disabled by config or launch flag" —
    /// the flag wins when the two disagree).
    pub no_animations: bool,
    /// `--theme <name>` or `--theme=<name>`: `None` when the switch was not
    /// passed at all; `Some(String::new())` when it was passed with no
    /// following value (e.g. as the final argument); `Some(name)` otherwise,
    /// where `name` may or may not resolve to a built-in theme — that
    /// resolution happens in `resolve_startup_theme`, not here
    /// (theme-selection "Launch-time theme override via --theme").
    pub theme: Option<String>,
    /// `--config-dir <path>` or `--config-dir=<path>`: `None` when the
    /// switch was not passed at all; `Some(String::new())` when it was
    /// passed with no following value (a startup warning downstream, never
    /// a crash); `Some(path)` otherwise, where `path` wins outright over
    /// portable mode and the platform default — resolution happens in
    /// `resolve_profile_dir`, not here (config-profile-directory
    /// "Configuration profile directory": "Explicit --config-dir
    /// override"). Same absent/empty/value convention as `theme` above.
    pub config_dir: Option<String>,
    /// `--log <path>` or `--log=<path>`: enable the diagnostic log at
    /// `path` for this session, winning over the `FILECOMMAND_LOG`
    /// environment variable. Same absent/empty/value convention as `theme`
    /// and `config_dir` above; resolution happens in `run` via
    /// [`resolve_log_path`] (error-diagnostics; application-shell "Optional
    /// diagnostic log").
    pub log: Option<String>,
}

/// Parse launch switches from an argument iterator (excluding the program
/// name). Pure and independent of `std::env` so it is testable without a
/// real process; `main.rs` calls this with `std::env::args().skip(1)` and
/// forwards the result to [`run`] (design D2: "`main.rs` stays a thin
/// parse-and-forward layer; parsing moves to a small pure helper").
///
/// Both `--theme <name>` and `--theme=<name>` are accepted (design D1).
/// Switches may appear in either order and alongside `--nosplash`.
pub fn parse_launch_args<I: IntoIterator<Item = String>>(args: I) -> LaunchOptions {
    let mut opts = LaunchOptions::default();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        if arg == "--nosplash" {
            opts.no_splash = true;
        } else if arg == "--nomouse" {
            opts.no_mouse = true;
        } else if arg == "--noanimations" {
            opts.no_animations = true;
        } else if arg == "--theme" {
            // A missing value (switch is the final argument) is recorded as
            // an empty string rather than `None`, so `resolve_startup_theme`
            // can tell "switch not passed" (no warning) apart from "switch
            // passed with nothing after it" (warns: missing value).
            opts.theme = Some(iter.next().unwrap_or_default());
        } else if let Some(value) = arg.strip_prefix("--theme=") {
            opts.theme = Some(value.to_string());
        } else if arg == "--config-dir" {
            // Same missing-value convention as --theme above: recorded as
            // an empty string so startup warns and falls through to normal
            // profile resolution rather than crashing (task 1.1).
            opts.config_dir = Some(iter.next().unwrap_or_default());
        } else if let Some(value) = arg.strip_prefix("--config-dir=") {
            opts.config_dir = Some(value.to_string());
        } else if arg == "--log" {
            // Same missing-value convention again (error-diagnostics design
            // D2): an empty string so startup warns "--log needs a path"
            // instead of silently disabling the log.
            opts.log = Some(iter.next().unwrap_or_default());
        } else if let Some(value) = arg.strip_prefix("--log=") {
            opts.log = Some(value.to_string());
        }
    }
    opts
}

/// The platform default profile directory: `%APPDATA%\FileCommand` on
/// Windows, `$XDG_CONFIG_HOME/filecommand` (else `~/.config/filecommand`)
/// elsewhere; `None` when no config-home environment variable is set
/// (config-profile-directory "Configuration profile directory"; design D1).
/// `pub` so `main.rs`'s usage text can name it (design D4) — nothing is
/// created here.
pub fn platform_default_profile_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(|appdata| PathBuf::from(appdata).join("FileCommand"))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(|xdg| PathBuf::from(xdg).join("filecommand"))
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config").join("filecommand")))
    }
}

/// Why the last-resort cwd fallback fired, named in its startup warning —
/// the environment variable(s) that would normally resolve the platform
/// default (design D1: "never silent").
#[cfg(windows)]
const CONFIG_HOME_UNSET_HINT: &str = "APPDATA is not set";
#[cfg(not(windows))]
const CONFIG_HOME_UNSET_HINT: &str = "neither XDG_CONFIG_HOME nor HOME is set";

/// Resolve the one profile directory every configuration file lives in
/// (config-profile-directory "Configuration profile directory"). Order
/// (design D1): an explicit `--config-dir` flag wins outright — a relative
/// value is resolved against the working directory at startup; else, when
/// a file named `portable` exists beside the executable, the executable's
/// own directory (portable mode); else the platform default. When no
/// config-home variable resolves either, the working directory is the
/// last-resort fallback and the returned warning says so — resolution is
/// never silent.
fn resolve_profile_dir(flag: Option<PathBuf>, exe_dir: Option<&Path>) -> (PathBuf, Option<String>) {
    if let Some(path) = flag {
        let path = if path.is_absolute() { path } else { std::env::current_dir().map(|cwd| cwd.join(&path)).unwrap_or(path) };
        return (path, None);
    }
    if let Some(dir) = exe_dir {
        if dir.join("portable").exists() {
            return (dir.to_path_buf(), None);
        }
    }
    if let Some(default) = platform_default_profile_dir() {
        return (default, None);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let warning = format!("{CONFIG_HOME_UNSET_HINT}; using the current directory {} for configuration files", cwd.display());
    (cwd, Some(warning))
}

/// Interpret the raw `--config-dir` flag value the way
/// `resolve_startup_theme` interprets `--theme`: `None` (switch absent) is
/// no override; `Some(path)` wins outright; `Some("")` — the switch passed
/// with no value — produces a startup warning, not a crash, and falls
/// through to normal resolution (application-shell "Configuration profile
/// directory": "Explicit --config-dir override"; task 1.1).
fn config_dir_override(raw: Option<&str>) -> (Option<PathBuf>, Option<String>) {
    match raw {
        Some("") => (None, Some("--config-dir requires a value".to_string())),
        Some(path) => (Some(PathBuf::from(path)), None),
        None => (None, None),
    }
}

/// Resolve the diagnostic-log path chosen at launch (error-diagnostics
/// design D2; application-shell "Optional diagnostic log"): the `--log`
/// flag wins over `FILECOMMAND_LOG` outright, an empty flag value is a
/// warning rather than a path, and an empty or unset variable counts as
/// unset. `Err` is the startup-warning text; `Ok(None)` leaves logging off
/// and creates nothing ("No log unless asked for").
fn resolve_log_path(flag: Option<&str>, env: Option<&str>) -> Result<Option<String>, String> {
    match flag {
        Some("") => Err("--log needs a path".to_string()),
        Some(path) => Ok(Some(path.to_string())),
        None => match env {
            Some(value) if !value.is_empty() => Ok(Some(value.to_string())),
            _ => Ok(None),
        },
    }
}

/// Open the log [`resolve_log_path`] chose. Runs before profile resolution
/// so every startup warning that follows can be recorded (design D2/D3). An
/// unopenable path downgrades to a startup warning naming the path and the
/// reason — the session continues without logging ("Unwritable log path
/// warns and continues").
fn init_diagnostic_log(path: Option<String>) -> Option<String> {
    let path = path?;
    match diag::init(Path::new(&path)) {
        Ok(()) => None,
        Err(err) => Some(format!("Diagnostic log disabled: cannot open {path} ({err})")),
    }
}

/// Create the profile directory on first run. Failure is soft — the
/// session runs on in-memory defaults with persistence skipped — and the
/// returned warning names the path and the error (design D2; scenario
/// "Unwritable profile falls back without losing the session").
fn ensure_profile_dir(profile: &Path) -> Option<String> {
    std::fs::create_dir_all(profile)
        .err()
        .map(|err| format!("Could not create {}: {err} — settings will not be saved this session", profile.display()))
}

/// The legacy-files startup notice (design D3; scenario "Legacy files in
/// the working directory produce a notice, not a migration"): when the
/// profile directory has no `config.toml` and the launch directory still
/// holds pre-profile `config.toml`/`history.json`/`usermenu.toml` files,
/// say where FileCommand now reads from and that the old files can be
/// moved — nothing is copied or deleted. `None` when no notice applies.
fn legacy_notice(cwd: &Path, profile: &Path) -> Option<String> {
    let names = config::legacy_files_present(cwd, profile);
    if names.is_empty() {
        return None;
    }
    Some(format!(
        "Found {} in {}; FileCommand now reads {}. Move them there to keep your settings.",
        names.join(", "),
        cwd.display(),
        profile.display()
    ))
}

pub fn run(launch: LaunchOptions) -> io::Result<()> {
    // Installed from this thread, which thereby becomes the UI thread the
    // hook restores the terminal for; worker panics leave it alone
    // (process-lifecycle-hardening design D4).
    crate::terminal::install_panic_hook();
    // The console control handler that keeps a Ctrl+C typed during a
    // suspended child run from ending FileCommand (application-shell
    // "Ctrl+C during a suspended child run stops only the child"; design
    // D1). Registered once; a failure only leaves the old behavior in place,
    // so it is logged rather than fatal.
    #[cfg(windows)]
    if let Err(e) = console::install_ctrl_handler() {
        diag::log("startup", format_args!("console control handler not installed: {e}"));
    }
    // The diagnostic log is resolved and opened before anything below can
    // warn, so every startup warning — including ones a later warning
    // replaces in the single-slot dialog — is recorded (error-diagnostics
    // design D2/D3; application-shell "Optional diagnostic log"). The flag
    // wins over FILECOMMAND_LOG; a missing value or an unopenable file
    // degrades to a startup warning rather than aborting the session.
    let log_warning = match resolve_log_path(launch.log.as_deref(), std::env::var("FILECOMMAND_LOG").ok().as_deref()) {
        Ok(path) => init_diagnostic_log(path),
        Err(warning) => Some(warning),
    };
    // The profile directory is resolved before anything loads: every
    // configuration file (config.toml, history.json, usermenu.toml) lives
    // there now, never relative to the launch directory
    // (config-profile-directory "Configuration profile directory").
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let exe_dir = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf));
    let (flag, flag_warning) = config_dir_override(launch.config_dir.as_deref());
    let (profile_dir, resolve_warning) = resolve_profile_dir(flag, exe_dir.as_deref());
    let create_warning = ensure_profile_dir(&profile_dir);
    let persist_enabled = create_warning.is_none();
    let config_path = profile_dir.join(CONFIG_FILE);
    let history_path = profile_dir.join(config::HISTORY_FILE);
    let usermenu_path = profile_dir.join(config::USERMENU_FILE);
    // Probed before the loads below (none of which create a config.toml):
    // legacy files left over from the per-launch-directory layout (design
    // D3).
    let legacy_warning = legacy_notice(&cwd, &profile_dir);
    // Loaded before the guard so the guard can enable mouse capture (or not)
    // on its very first frame rather than flipping it on a moment later
    // (application-shell "Terminal acquired on startup"; mouse-input "Mouse
    // capture configuration") — `config::load` is pure file I/O with no
    // terminal dependency, so this reorder ahead of `TerminalGuard::new` is
    // safe.
    let config::ParsedConfig { config, warnings: parsed_warnings } = config::load(&config_path);
    // config.toml warnings first, usermenu.toml warnings appended below, in
    // file-then-line order (configuration-files "Several warnings are
    // summarized"; design D4).
    let mut config_warnings: Vec<String> = parsed_warnings.iter().map(ToString::to_string).collect();
    let mouse_enabled = config.mouse_enabled && !launch.no_mouse;
    let mut guard = TerminalGuard::new(mouse_enabled)?;

    let clock = RealClock::new();
    let wall_clock = RealWallClock;
    let (theme, theme_warning) = resolve_startup_theme(&config, launch.theme.as_deref());
    let show_splash = config.splash && !launch.no_splash;
    let keys = config.keys.clone();

    let size = guard.terminal.size()?;
    let term_size = (size.width, size.height);
    let year = DateTime::from_system_time(SystemTime::now()).year;
    let identity_lines = identity::identity_lines(year);

    let (mut state, effects) = State::initial(theme, term_size, clock.now_ms(), wall_clock.epoch_ms(), cwd.clone(), cwd, show_splash);
    apply_config(&mut state, &config);
    apply_launch_options(&mut state, &launch);
    // A `--log` switch with no value, an unopenable log file, a
    // `--config-dir` switch with no value, and the last-resort cwd fallback
    // are session-only, in-memory warnings — same convention as the theme
    // override warning just below. Each goes through `set_startup_warning`
    // so it lands in the diagnostic log even when a later warning replaces
    // it in the dialog (error-diagnostics design D3).
    if let Some(warning) = log_warning {
        set_startup_warning(&mut state, warning);
    }
    if let Some(warning) = flag_warning {
        set_startup_warning(&mut state, warning);
    }
    if let Some(warning) = resolve_warning {
        set_startup_warning(&mut state, warning);
    }
    // Session-only: this only ever sets `state.startup_warning` in memory,
    // never touches `config.toml` (theme-selection "Override is
    // session-only"; design D4). `apply_user_menu` below may overwrite this
    // with its own warning if both fire at once — matching the pre-existing
    // single-`startup_warning`-field behavior, with the superseded warning
    // kept in the diagnostic log.
    if let Some(warning) = theme_warning {
        set_startup_warning(&mut state, warning);
    }
    // A file from an earlier build (no format marker) carries session-
    // relative visit times; they are re-stamped to the startup epoch, future
    // times are clamped, and the index is pruned to its cap (fuzzy-jump
    // "Legacy history file is migrated at load"; design D2).
    let mut loaded = config::load_history_document(&history_path);
    config::migrate_loaded_history(&mut loaded.file, loaded.legacy, state.epoch_ms);
    state.history = loaded.file.commands;
    state.dir_history = loaded.file.directories;
    let user_menu = config::load_user_menu(&usermenu_path);
    config_warnings.extend(user_menu.warnings.iter().map(ToString::to_string));
    apply_user_menu(&mut state, user_menu);
    // Commands → Menu file edit reads the path back out of `State`
    // (pulldown-menu-completion design D4).
    state.user_menu_path = usermenu_path.clone();
    // The legacy notice and the unwritable-profile warning write last, so
    // the persistence-breaking one (design D2) stays visible when several
    // warnings fire at once; still the same single-field, last-writer-wins
    // behavior (config-profile-directory design D3), with every superseded
    // warning kept in the diagnostic log.
    if let Some(warning) = legacy_warning {
        set_startup_warning(&mut state, warning);
    }
    if let Some(warning) = create_warning {
        set_startup_warning(&mut state, warning);
    }
    // Lowest precedence of all: the configuration-content summary is shown
    // only when no other startup warning fired (design D4).
    apply_configuration_warnings(&mut state, &config_warnings);
    stamp_startup_warning(&mut state);

    let (tx, rx) = mpsc::channel::<Command>();
    let history_writer = if persist_enabled {
        HistoryWriter::spawn(history_path, crate::history_writer::DEBOUNCE, crate::history_writer::MAX_WAIT)
    } else {
        HistoryWriter::disabled()
    };
    let mut rt = Runtime {
        tx,
        active_job: None,
        cancels: CancelRegistry::default(),
        history_writer,
        config_path,
        usermenu_path,
        persist_enabled,
        viewer_source: None,
        clipboard: Box::new(PlatformClipboard::default()),
        mouse_tracker: MouseTracker::new(),
        git_slots: [worker::GitQuerySlot::new(), worker::GitQuerySlot::new()],
        workers: worker::WorkerGauge::new(),
    };

    if run_effects(effects, &mut guard, &mut rt)? {
        return shutdown(&mut rt);
    }
    let (mut state, _) = drain_events(state, &rx, &mut guard, &mut rt)?;
    let mut hitmap = draw(&mut guard, &state, &identity_lines, &wall_clock, rt.viewer_source.as_ref().map(|(_, s)| s))?;

    // A non-mouse event read while draining a burst of mouse events (design
    // D7), or a mouse event that ended a key burst (idle-aware-event-loop
    // design D5), is stashed here rather than lost — crossterm has no way to
    // push an event back onto its queue, so the next loop iteration consumes
    // this instead of polling again.
    let mut pending_event: Option<Event> = None;

    // Idle-aware-event-loop design D1: `needs_draw` persists across
    // iterations and is cleared only by an actual draw, so a worker result
    // drained in an iteration whose key then maps to no command still
    // repaints (application-shell "Redraw survives an unmapped key") — the
    // old per-iteration `dirty` flag lost that redraw.
    let mut needs_draw = false;
    // The wall-clock reading the last drawn frame's header clock showed; a
    // changed reading at a poll timeout means a minute boundary passed and
    // the header must be repainted (application-shell "Header clock still
    // advances while idle").
    let mut drawn_clock = wall_clock.now_local();
    let mut events = CrosstermEvents;

    loop {
        let (s, applied) = drain_events(state, &rx, &mut guard, &mut rt)?;
        state = s;
        needs_draw |= applied;

        // Design D3's race: a worker can post its last message and drop its
        // gauge token between the drain above and this gauge read. The send
        // happens-before the token's release-decrement, so with a zero
        // reading one more drain is guaranteed to see that message — without
        // it the loop could sleep a whole deadline with it unapplied.
        let workers_alive = rt.workers.live() > 0;
        if !workers_alive {
            let (s, applied) = drain_events(state, &rx, &mut guard, &mut rt)?;
            state = s;
            needs_draw |= applied;
        }

        // While a draw is already pending there is no sleeping: poll with a
        // zero timeout so input that is already queued applies before the
        // frame (one draw for the whole batch), then fall through to it.
        let timeout = if needs_draw {
            Duration::ZERO
        } else {
            let now = clock.now_ms();
            next_timeout(
                now,
                anim::next_wake_ms(&state),
                Duration::from_millis(crate::clock::ms_until_next_minute(crate::clock::now_unix_ms())),
                workers_alive,
                // `streaming-listing-performance`'s drain backlog forces a
                // zero timeout here once it lands; until then there is none.
                false,
            )
        };

        let next_event = match pending_event.take() {
            Some(e) => Some(e),
            None if events.poll(timeout)? => Some(events.read()?),
            None => None,
        };

        match next_event {
            Some(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                let (s, quit) = tick_before_input(state, &clock, &wall_clock, &mut guard, &mut rt)?;
                if quit {
                    return shutdown(&mut rt);
                }
                state = s;
                // Design D5: with the first key handled, keep applying
                // already-queued input — each key mapped against the state
                // the previous one produced — then draw once.
                let mut handler = LoopHandler { guard: &mut guard, rt: &mut rt, keys: &keys };
                let (s, quit, burst_applied) = drain_key_burst(key, state, &mut events, &mut pending_event, &mut handler)?;
                state = s;
                needs_draw |= burst_applied;
                if quit {
                    return shutdown(&mut rt);
                }
            }
            Some(Event::Resize(w, h)) => {
                let (s, quit) = tick_before_input(state, &clock, &wall_clock, &mut guard, &mut rt)?;
                if quit {
                    return shutdown(&mut rt);
                }
                let (s, quit) = apply(s, Command::Resize(w, h), &mut guard, &mut rt)?;
                if quit {
                    return shutdown(&mut rt);
                }
                state = s;
                needs_draw = true;
            }
            Some(Event::Mouse(first)) => {
                let (s, quit) = tick_before_input(state, &clock, &wall_clock, &mut guard, &mut rt)?;
                if quit {
                    return shutdown(&mut rt);
                }
                state = s;
                // Drain every mouse event already queued this frame after
                // the first, so a burst of wheel/drag events causes at most
                // one redraw (mouse-input "Mouse events are coalesced";
                // design D7). A non-mouse event read mid-drain is stashed in
                // `pending_event` rather than lost.
                let mut batch = vec![first];
                while events.poll(Duration::ZERO)? {
                    match events.read()? {
                        Event::Mouse(m) => batch.push(m),
                        other => {
                            pending_event = Some(other);
                            break;
                        }
                    }
                }
                let (s, quit, batch_dirty) = apply_mouse_batch(state, batch, &hitmap, &mut guard, &mut rt)?;
                if quit {
                    return shutdown(&mut rt);
                }
                state = s;
                needs_draw |= batch_dirty;
            }
            // A bracketed paste (never produced on Windows — design D6) is
            // one batch of commands into the field Ctrl+V would paste into,
            // drawn once.
            Some(Event::Paste(text)) => {
                let (s, quit) = tick_before_input(state, &clock, &wall_clock, &mut guard, &mut rt)?;
                if quit {
                    return shutdown(&mut rt);
                }
                state = s;
                for cmd in input::map_paste(&text, &state) {
                    let (s, quit) = apply(state, cmd, &mut guard, &mut rt)?;
                    state = s;
                    needs_draw = true;
                    if quit {
                        return shutdown(&mut rt);
                    }
                }
            }
            // Key releases, focus events: nothing to apply.
            Some(_) => {}
            None => {
                // A poll timeout applies nothing and draws nothing unless a
                // deadline actually came due (design D1/D2) — the
                // worker-cadence wakeups this arm also gets are what used to
                // cost a full frame every 33 ms forever (audit P-05).
                let now = clock.now_ms();
                let core_due = anim::next_wake_ms(&state).is_some_and(|wake| now >= wake);
                let minute_due = wall_clock.now_local() != drawn_clock;
                if core_due || minute_due {
                    let (s, quit) = apply(state, Command::Tick { now_ms: now, epoch_ms: wall_clock.epoch_ms() }, &mut guard, &mut rt)?;
                    if quit {
                        return shutdown(&mut rt);
                    }
                    state = s;
                    needs_draw = true;
                }
            }
        }

        if needs_draw {
            // Effects that feed a command straight back (drive enumeration)
            // must land before the frame, so the dialog paints complete on
            // its very first appearance rather than a frame later.
            let (s, _) = drain_events(state, &rx, &mut guard, &mut rt)?;
            state = s;
            hitmap = draw(&mut guard, &state, &identity_lines, &wall_clock, rt.viewer_source.as_ref().map(|(_, s)| s))?;
            needs_draw = false;
            drawn_clock = wall_clock.now_local();
        }
    }
}

/// The one exit path for every quit return in [`run`] (application-shell
/// "Quit waits for job cancellation cleanup"; process-lifecycle-hardening
/// design D3). Acts on `rt.active_job` directly — whatever phase core is
/// in, so a quit confirmed from a screen that no longer shows
/// `FileOpRunning` still waits for the job — and returns `Ok(())` so the
/// call sites read `return shutdown(&mut rt)`. With no job there is nothing
/// to wait for and quit exits at once.
fn shutdown(rt: &mut Runtime) -> io::Result<()> {
    if let Some(job) = rt.active_job.take() {
        if !shutdown_job(job, QUIT_JOB_WAIT) {
            // The same partial-file risk as before this change, now confined
            // to a job whose write has not returned in two seconds
            // (error-diagnostics coordination: the timeout is logged).
            diag::log("job", format_args!("quit: job thread still running after {} ms; exiting without its cleanup", QUIT_JOB_WAIT.as_millis()));
        }
    }
    Ok(())
}

/// Request `job`'s cancellation and wait up to `wait` for its thread to
/// end, polling `is_finished()` every [`QUIT_JOB_POLL`]. Returns `true`
/// when the thread ended (and was joined), `false` when it was abandoned at
/// the bound. No dialog answer is sent: the sink's reply loop turns the
/// cancel flag into an abort for whichever prompt is pending (design D5),
/// so a conflict and an error reply can never be mismatched. The screen is
/// not redrawn meanwhile — at most `wait` passes with the last frame still
/// showing.
fn shutdown_job(job: worker::JobHandle, wait: Duration) -> bool {
    job.cancel.cancel();
    let deadline = Instant::now() + wait;
    while !job.thread.is_finished() {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(QUIT_JOB_POLL);
    }
    // Finished: the join cannot block, and a contained panic has already
    // been turned into the job's `JobDone` reply by `spawn_job`.
    let _ = job.thread.join();
    true
}

/// A wheel/scroll command not yet applied, accumulated across consecutive
/// same-target events in the current drained batch (design D7: "SHALL sum
/// consecutive wheel notches"). Kept as running totals rather than replaying
/// each notch through `apply` individually: repeated single-notch
/// `move_cursor` calls happen to land on the same final position for a
/// simple clamped cursor, but a batch that reverses direction near a list's
/// end would not — `clamp(clamp(x + d1) + d2) != clamp(x + d1 + d2)` in
/// general — so only a true running sum matches "sum" literally. It also
/// spares the viewer a backward/forward scan per notch: one scan for the
/// whole batch's net delta instead of N.
enum PendingScroll {
    None,
    /// Net notches (positive down) while `state.phase` has stayed
    /// `UiPhase::Viewer` for the whole streak.
    Viewer(i64),
    /// Net notches while `state.phase` has stayed `UiPhase::Editor`.
    Editor(i64),
    /// Net `ScrollPanel` delta for one panel side.
    Panel(PanelSide, isize),
}

/// Apply whatever `pending` has accumulated so far, exactly once, via the
/// normal `apply` path (so its effects run and history/frecency/etc. see
/// one command, not N). A net delta of zero — equal ups and downs canceling
/// out — applies nothing at all, matching "no redraw storm" (design D7).
fn flush_pending_scroll(pending: PendingScroll, mut state: State, guard: &mut TerminalGuard, rt: &mut Runtime) -> io::Result<(State, bool, bool)> {
    let mut dirty = false;
    match pending {
        PendingScroll::None => {}
        PendingScroll::Viewer(net) if net != 0 => {
            if let UiPhase::Viewer(viewer) = &state.phase {
                let rows_visible = views::viewer::body_rows(state.term_size.1);
                let source = rt.viewer_source.as_ref().map(|(_, s)| s);
                if let Some(cmd) = resolve_viewer_navigation(viewer, source, ViewerInput::ScrollLines(net * 3), rows_visible) {
                    let (s, quit) = apply(state, cmd, guard, rt)?;
                    if quit {
                        return Ok((s, true, true));
                    }
                    state = s;
                    dirty = true;
                }
            }
        }
        PendingScroll::Editor(net) if net != 0 => {
            // The editor has no caret-independent viewport scroll, so the
            // net delta is still applied as individual `EditorMove` steps
            // (three per notch) rather than a single jump command
            // (builtin-editor).
            let step = if net > 0 { EditorMove::Down } else { EditorMove::Up };
            for _ in 0..(net.unsigned_abs() * 3) {
                let (s, quit) = apply(state, Command::EditorMove(step), guard, rt)?;
                if quit {
                    return Ok((s, true, dirty));
                }
                state = s;
                dirty = true;
            }
        }
        PendingScroll::Panel(side, delta) if delta != 0 => {
            let (s, quit) = apply(state, Command::ScrollPanel { side, delta }, guard, rt)?;
            if quit {
                return Ok((s, true, true));
            }
            state = s;
            dirty = true;
        }
        PendingScroll::Viewer(_) | PendingScroll::Editor(_) | PendingScroll::Panel(..) => {} // net == 0
    }
    Ok((state, false, dirty))
}

/// Apply a frame's worth of already-drained mouse events (design D7):
/// `Moved` is discarded (no hover feature), the viewer/editor accept wheel
/// only — dispatched directly from `state.phase`, mirroring how the key path
/// calls `map_viewer_key`/`map_editor_key` directly rather than through
/// `map_key` — and every other phase goes through `input::map_mouse`, gated
/// by its own mode-gating table (design D5). Consecutive wheel notches
/// (ScrollUp/ScrollDown) targeting the same viewer/editor/panel accumulate
/// in `PendingScroll` rather than being applied one at a time; a click or a
/// switch of target flushes whatever was pending first, so ordering is
/// preserved.
fn apply_mouse_batch(
    mut state: State,
    batch: Vec<crossterm::event::MouseEvent>,
    hitmap: &HitMap,
    guard: &mut TerminalGuard,
    rt: &mut Runtime,
) -> io::Result<(State, bool, bool)> {
    let mut dirty = false;
    let mut pending = PendingScroll::None;
    macro_rules! flush {
        () => {{
            let (s, quit, d) = flush_pending_scroll(std::mem::replace(&mut pending, PendingScroll::None), state, guard, rt)?;
            state = s;
            dirty |= d;
            if quit {
                return Ok((state, true, dirty));
            }
        }};
    }

    for m in batch {
        if matches!(m.kind, MouseEventKind::Moved) {
            continue;
        }
        // While the placeholder hides the screen, the viewer/editor wheel
        // arms are skipped and every event goes through `input::map_mouse`,
        // whose `context` ignores all of it except the quit dialog's buttons
        // (resize-preserves-session-state design D3).
        match &state.phase {
            UiPhase::Viewer(_) if !state.undersized => match m.kind {
                MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                    let notch = if m.kind == MouseEventKind::ScrollDown { 1 } else { -1 };
                    pending = match pending {
                        PendingScroll::Viewer(n) => PendingScroll::Viewer(n + notch),
                        _ => {
                            flush!();
                            PendingScroll::Viewer(notch)
                        }
                    };
                }
                _ => flush!(),
            },
            UiPhase::Editor(_) if !state.undersized => match m.kind {
                MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                    let notch = if m.kind == MouseEventKind::ScrollDown { 1 } else { -1 };
                    pending = match pending {
                        PendingScroll::Editor(n) => PendingScroll::Editor(n + notch),
                        _ => {
                            flush!();
                            PendingScroll::Editor(notch)
                        }
                    };
                }
                _ => flush!(),
            },
            _ => {
                if let Some(cmd) = input::map_mouse(m, hitmap, &mut rt.mouse_tracker, &state) {
                    if let Command::ScrollPanel { side, delta } = cmd {
                        pending = match pending {
                            PendingScroll::Panel(s, d) if s == side => PendingScroll::Panel(s, d + delta),
                            _ => {
                                flush!();
                                PendingScroll::Panel(side, delta)
                            }
                        };
                    } else {
                        flush!();
                        let (s, quit) = apply(state, cmd, guard, rt)?;
                        if quit {
                            return Ok((s, true, true));
                        }
                        state = s;
                        dirty = true;
                    }
                }
            }
        }
    }
    flush!();
    Ok((state, false, dirty))
}

/// The loop's terminal-event source, factored behind a trait so the burst
/// drain can be driven by a scripted queue in tests (idle-aware-event-loop
/// task 2.7). Production reads crossterm.
trait EventSource {
    fn poll(&mut self, timeout: Duration) -> io::Result<bool>;
    fn read(&mut self) -> io::Result<Event>;
}

struct CrosstermEvents;

impl EventSource for CrosstermEvents {
    fn poll(&mut self, timeout: Duration) -> io::Result<bool> {
        event::poll(timeout)
    }
    fn read(&mut self) -> io::Result<Event> {
        event::read()
    }
}

/// The poll timeout for one loop iteration (idle-aware-event-loop design
/// D2/D3), factored pure for tests. Zero while a drained worker backlog is
/// still pending (`streaming-listing-performance` supplies that signal once
/// it lands — always `false` until then); at most `POLL_INTERVAL` while
/// worker threads that can still post results are alive, so results keep
/// today's latency; otherwise the earliest of the reducer's next wake
/// (`core_wake`, on the same monotonic-millisecond axis as `now`) and the
/// header clock's next minute flip (`minute_wake`).
fn next_timeout(now: u64, core_wake: Option<u64>, minute_wake: Duration, workers_alive: bool, backlog: bool) -> Duration {
    if backlog {
        return Duration::ZERO;
    }
    let mut timeout = minute_wake;
    if let Some(wake) = core_wake {
        timeout = timeout.min(Duration::from_millis(wake.saturating_sub(now)));
    }
    if workers_alive {
        timeout = timeout.min(POLL_INTERVAL);
    }
    timeout
}

/// Design D4: bring the reducer clock up to date before the first event of
/// an input batch is applied, when the last delivered tick is older than
/// `POLL_INTERVAL` — so a surface opened after an idle stretch is stamped
/// with the real time and animates from its opening frame, and two clicks
/// separated by idle time are not mistaken for a double-click
/// (application-shell "Clicks separated by idle time are not a
/// double-click"). A no-op while the clock is already fresh; applies no
/// transition early (a key during the splash hold still dismisses the
/// splash, exactly as before).
fn tick_before_input(state: State, clock: &RealClock, wall_clock: &RealWallClock, guard: &mut TerminalGuard, rt: &mut Runtime) -> io::Result<(State, bool)> {
    let now = clock.now_ms();
    if now.saturating_sub(state.clock_ms) > POLL_INTERVAL.as_millis() as u64 {
        // Both clocks travel in the tick, so a visit recorded right after an
        // idle stretch is stamped with fresh wall-clock time too
        // (history-persistence-hardening design D1).
        apply(state, Command::Tick { now_ms: now, epoch_ms: wall_clock.epoch_ms() }, guard, rt)
    } else {
        Ok((state, false))
    }
}

/// What the key-burst drain needs from the loop around it: the per-key
/// mapping (factored out of the loop body, design D5) and the command
/// application path. Production wires [`map_pressed_key`] + [`apply`] through
/// [`LoopHandler`]; tests drive the same drain with a recording handler and
/// no terminal.
trait BurstHandler {
    fn map_key(&mut self, key: KeyEvent, state: &State) -> Option<Command>;
    fn apply(&mut self, state: State, cmd: Command) -> io::Result<(State, bool)>;
}

struct LoopHandler<'a> {
    guard: &'a mut TerminalGuard,
    rt: &'a mut Runtime,
    keys: &'a config::Keys,
}

impl BurstHandler for LoopHandler<'_> {
    fn map_key(&mut self, key: KeyEvent, state: &State) -> Option<Command> {
        map_pressed_key(key, state, self.keys, self.rt.viewer_source.as_ref().map(|(_, s)| s))
    }
    fn apply(&mut self, state: State, cmd: Command) -> io::Result<(State, bool)> {
        apply(state, cmd, self.guard, self.rt)
    }
}

/// The loop's per-key mapping (idle-aware-event-loop design D5), factored
/// out of the loop body so the first key of an iteration and every drained
/// burst key go through the same routing: the quit-confirmation dialog
/// outranks the viewer/editor's own direct dispatch (application-shell
/// "Quit request keys and confirmation"; design D5), then the viewer and
/// editor map through their own functions, then everything else goes
/// through `input::map_key`. The "terminal too small" placeholder takes the
/// same precedence as the quit dialog: while `state.undersized` is set the
/// viewer's and editor's direct dispatch is bypassed so `map_key`'s
/// undersized arm can gate every key (resize-preserves-session-state design
/// D3).
fn map_pressed_key(key: KeyEvent, state: &State, keys: &config::Keys, viewer_source: Option<&ByteSource>) -> Option<Command> {
    if state.quit_confirm || state.undersized {
        let page_size = active_panel_page_size(state);
        return input::map_key(key, state, page_size, keys);
    }
    match &state.phase {
        UiPhase::Viewer(viewer) => {
            let rows_visible = views::viewer::body_rows(state.term_size.1);
            input::map_viewer_key(key, viewer, rows_visible).and_then(|action| resolve_viewer_navigation(viewer, viewer_source, action, rows_visible))
        }
        UiPhase::Editor(editor) => {
            let rows_visible = views::editor::body_rows(state.term_size.1);
            input::map_editor_key(key, editor, rows_visible)
        }
        _ => {
            let page_size = active_panel_page_size(state);
            input::map_key(key, state, page_size, keys)
        }
    }
}

/// One iteration's application of an input burst (idle-aware-event-loop
/// "Bursts of key input render once"; design D5): apply `first` (a pressed
/// key), then keep reading with a zero poll while the burst budget
/// ([`BURST_MAX_EVENTS`] events or [`BURST_MAX_DURATION`]) lasts — each
/// pressed key mapped against the state the previous one produced, so
/// state-dependent routing (Enter opening a dialog, the next characters
/// landing in its field) is exactly as if the keys had arrived one loop
/// iteration at a time. Key releases and focus events are skipped; a resize
/// is applied like any command; a paste goes through `input::map_paste` as
/// one batch; a mouse event ends the burst and is stashed in
/// `pending_event` for the next iteration's mouse-batch path. Returns
/// `(state, quit, applied)`, `applied` feeding the loop's `needs_draw`.
fn drain_key_burst(
    first: KeyEvent,
    mut state: State,
    events: &mut dyn EventSource,
    pending_event: &mut Option<Event>,
    handler: &mut dyn BurstHandler,
) -> io::Result<(State, bool, bool)> {
    let started = Instant::now();
    let mut applied = false;
    let mut handled = 0usize;
    let mut next = Event::Key(first);
    loop {
        match next {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if let Some(cmd) = handler.map_key(key, &state) {
                    let (s, quit) = handler.apply(state, cmd)?;
                    state = s;
                    applied = true;
                    if quit {
                        return Ok((state, true, applied));
                    }
                }
            }
            Event::Resize(w, h) => {
                let (s, quit) = handler.apply(state, Command::Resize(w, h))?;
                state = s;
                applied = true;
                if quit {
                    return Ok((state, true, applied));
                }
            }
            Event::Paste(text) => {
                for cmd in input::map_paste(&text, &state) {
                    let (s, quit) = handler.apply(state, cmd)?;
                    state = s;
                    applied = true;
                    if quit {
                        return Ok((state, true, applied));
                    }
                }
            }
            Event::Mouse(mouse) => {
                *pending_event = Some(Event::Mouse(mouse));
                return Ok((state, false, applied));
            }
            _ => {}
        }
        handled += 1;
        if handled >= BURST_MAX_EVENTS || started.elapsed() >= BURST_MAX_DURATION {
            return Ok((state, false, applied));
        }
        if !events.poll(Duration::ZERO)? {
            return Ok((state, false, applied));
        }
        next = events.read()?;
    }
}

/// Resolve the startup theme from `config.theme` and an optional `--theme`
/// launch override, returning the theme to start with plus an optional
/// warning message for `state.startup_warning`.
///
/// `override_name`: `None` when `--theme` was not passed (no warning is
/// ever produced in that case); `Some("")` when `--theme` was passed with no
/// value; `Some(name)` otherwise. A valid `name` wins over `config.theme`
/// with no warning (theme-selection "Valid override wins over
/// configuration"). An empty or unrecognized `name` produces a warning
/// naming the problem and listing `BUILTIN_THEME_NAMES`, and falls through
/// to the existing config-then-default resolution unchanged (theme-selection
/// "Unknown name warns and falls back" / "Missing value warns and falls
/// back").
///
/// An unset `config.theme` key already defaults to `nc-classic` in
/// `config::Config::default`, and any configured name that isn't one of the
/// six built-ins (renamed/typo'd/hand-edited) falls back to `nc-classic`
/// here (theme-selection "Unknown configured theme falls back to default").
/// Factored out of `run` so this fallback composition — previously a bare
/// inline expression with no direct regression test — is testable without a
/// real terminal, matching `apply_config`'s rationale.
fn resolve_startup_theme(config: &config::Config, override_name: Option<&str>) -> (Theme, Option<String>) {
    let configured = || Theme::by_name(&config.theme).unwrap_or_else(Theme::classic);
    match override_name {
        None => (configured(), None),
        Some("") => {
            let warning = format!("--theme requires a value; valid themes: {}", BUILTIN_THEME_NAMES.join(", "));
            (configured(), Some(warning))
        }
        Some(name) => match Theme::by_name(name) {
            Some(theme) => (theme, None),
            None => {
                let warning = format!("--theme {name} is not a recognized theme; valid themes: {}", BUILTIN_THEME_NAMES.join(", "));
                (configured(), Some(warning))
            }
        },
    }
}

/// Snapshot config-driven values into `State` at startup: the configured
/// shell/`PATHEXT` and the F4 external-editor command, so `update` stays a
/// pure function of `State` rather than reading the environment or config
/// itself. Factored out of `run` so the config-driven startup wiring is
/// testable without a real terminal (regression coverage for the F4
/// external-editor command silently never reaching `State` — see
/// `apply_config_wires_the_configured_editor_and_shell_into_state` below).
fn apply_config(state: &mut State, config: &config::Config) {
    state.shell = ShellConfig::from_env(config.shell.clone());
    state.editor = config.editor.clone();
    state.split_percent = config.panel_split;
    state.animations = config.animations;
}

/// Apply launch switches that override a config-driven `State` value:
/// `--noanimations` forces animations off even when `animations = true`
/// (ui-animations "Flag overrides config"). Run after [`apply_config`], which
/// copies the config's value, so the flag always has the last word.
fn apply_launch_options(state: &mut State, launch: &LaunchOptions) {
    if launch.no_animations {
        state.animations = false;
    }
}

/// The one writer of `state.startup_warning` (error-diagnostics design
/// D3): the dialog slot stays last-writer-wins, but every warning —
/// including ones a later assignment replaces — is recorded in the
/// diagnostic log (application-shell "Optional diagnostic log": "Superseded
/// startup warnings are still recorded"). With the log disabled this is
/// exactly the old bare assignment.
fn set_startup_warning(state: &mut State, warning: String) {
    diag::log("startup", &warning);
    state.startup_warning = Some(warning);
}

/// Stamp the startup-warning dialog's open time so it irises in like every
/// other modal. It is raised here, in `run`, rather than by a reducer
/// command, so the reducer's own stamping post-condition never sees it open.
fn stamp_startup_warning(state: &mut State) {
    if state.startup_warning.is_some() {
        state.dialog_opened_at_ms = state.clock_ms;
    }
}

/// Snapshot the F2 user menu into `State` at startup, factored out of `run`
/// for the same reason as `apply_config` — testable without a real
/// terminal. On a malformed `usermenu.toml`, `config::load_user_menu` has
/// already fallen back to default entries in memory without touching the
/// file; this raises the spec-required dismissable startup-warning dialog
/// (`state.startup_warning`) rather than a panel mini-status, since the
/// warning concerns the whole session, not either panel specifically
/// (user-menu "Malformed file warns and falls back without overwriting").
fn apply_user_menu(state: &mut State, user_menu: config::UserMenuLoadResult) {
    state.user_menu_entries = user_menu.entries;
    if user_menu.malformed {
        let file = config::USERMENU_FILE;
        let warning = match user_menu.problem {
            // A not-UTF-8 file gets its own message naming the way out,
            // because the built-in editor will not open it
            // (configuration-files "Non-UTF-8 usermenu.toml is kept, not
            // overwritten"; design D3).
            Some(config::UserMenuProblem::NotUtf8) => format!("{file} is not UTF-8 text; using the default menu — re-save it as UTF-8 or delete it to recreate the defaults"),
            Some(config::UserMenuProblem::Unreadable(error)) => format!("{file} could not be read ({error}); using the default menu"),
            Some(config::UserMenuProblem::InvalidToml) | None => format!("{file} is malformed; F2 uses the default user menu"),
        };
        set_startup_warning(state, warning);
    }
}

/// Show the `config.toml`/`usermenu.toml` content warnings as one summary
/// line in the startup-warning dialog, but only when no other startup
/// warning is already up: the summary has the lowest precedence, so every
/// warning an existing requirement mandates (profile directory, `--theme`,
/// malformed user menu, legacy notice, unwritable profile) still shows
/// (configuration-files "Profile warning keeps precedence", "Theme override
/// warning keeps precedence"; design D4). Every individual warning goes to
/// the diagnostic log regardless, since the dialog shows only the first.
fn apply_configuration_warnings(state: &mut State, warnings: &[String]) {
    for warning in warnings {
        diag::log("config", warning);
    }
    if state.startup_warning.is_none() {
        if let Some(summary) = config::summarize_warnings(warnings) {
            set_startup_warning(state, summary);
        }
    }
}

/// The PgUp/PgDn paging step for `map_key`'s active-panel commands: the
/// active panel's own body entry-row count, from the same per-mode
/// derivation `core::update` uses to reconcile the scroll viewport
/// (`filecommand_core::update::panel_viewport_rows`) rather than the
/// layout-level `Layout::entries_visible`, which over-counts by one with a
/// visible tab strip and mismatches Brief mode's row count (panel-scrolling
/// design D2; panel-navigation "Scroll offset is core panel state").
fn active_panel_page_size(state: &State) -> usize {
    let panel = state.active_panel();
    filecommand_core::update::panel_viewport_rows(state.term_size, panel.display_mode, panel.tab_count())
}

/// Resolve a viewer key's meaning into a core `Command`. Simple toggles pass
/// straight through; scrolling needs the open `ByteSource` to compute the
/// new offset (backward/forward line-start scans, or hex row math) — the
/// I/O `core::update` itself must stay free of (design D1). `None` when
/// there is no byte source yet (the brief window before the first frame) or
/// the key has no effect (e.g. an arrow key with nothing open).
fn resolve_viewer_navigation(viewer: &ViewerState, source: Option<&ByteSource>, action: ViewerInput, rows_visible: usize) -> Option<Command> {
    match action {
        ViewerInput::Cmd(cmd) => Some(cmd),
        ViewerInput::ScrollLines(delta) => Some(Command::ViewerSetTop(scroll_lines(viewer, source?, delta))),
        ViewerInput::ScrollCols(delta) => Some(Command::ViewerSetHScroll((viewer.h_scroll as i64 + delta).max(0) as usize)),
        ViewerInput::Home => Some(Command::ViewerSetTop(0)),
        ViewerInput::End => Some(Command::ViewerSetTop(scroll_to_end(viewer, source?, rows_visible))),
    }
}

/// Move `viewer.top_offset` by `delta` lines (text mode) or rows (hex mode).
/// Text mode walks one line at a time via the bounded backward/forward
/// scans (design D3), stopping early if a scan makes no further progress
/// (start or end of file) rather than looping past it.
fn scroll_lines(viewer: &ViewerState, source: &ByteSource, delta: i64) -> u64 {
    match viewer.mode {
        ViewMode::Hex => {
            let step = delta.saturating_mul(HEX_BYTES_PER_ROW as i64);
            (viewer.top_offset as i64 + step).max(0) as u64
        }
        ViewMode::Text => {
            let cap = backward::DEFAULT_MAX_LINE_LEN;
            let mut offset = viewer.top_offset;
            if delta < 0 {
                for _ in 0..delta.unsigned_abs() {
                    if offset == 0 {
                        break;
                    }
                    let prev = backward::previous_line_start(source, offset, cap);
                    if prev == offset {
                        break;
                    }
                    offset = prev;
                }
            } else {
                for _ in 0..delta {
                    let next = forward::next_line_start(source, offset, cap);
                    if next <= offset {
                        break;
                    }
                    offset = next;
                }
            }
            offset
        }
    }
}

/// The top offset that shows the file's last page: for hex, the last
/// `rows_visible` full rows; for text, `rows_visible` lines walked backward
/// from the end via the same bounded scan `scroll_lines` uses.
fn scroll_to_end(viewer: &ViewerState, source: &ByteSource, rows_visible: usize) -> u64 {
    let rows = rows_visible.max(1) as u64;
    match viewer.mode {
        ViewMode::Hex => {
            let total_rows = source.len().div_ceil(HEX_BYTES_PER_ROW as u64);
            total_rows.saturating_sub(rows) * HEX_BYTES_PER_ROW as u64
        }
        ViewMode::Text => {
            let cap = backward::DEFAULT_MAX_LINE_LEN;
            let mut offset = source.len();
            for _ in 0..rows {
                if offset == 0 {
                    break;
                }
                let prev = backward::previous_line_start(source, offset, cap);
                if prev == offset {
                    break;
                }
                offset = prev;
            }
            offset
        }
    }
}

/// Apply one command and execute its effects. The `bool` is `true` when the
/// event loop should exit.
fn apply(state: State, cmd: Command, guard: &mut TerminalGuard, rt: &mut Runtime) -> io::Result<(State, bool)> {
    // `ViewerClose` is the only path out of `UiPhase::Viewer` (`update`
    // never re-enters it once left), so it is also the one place the
    // cached byte source needs to be dropped — checked before `update`
    // consumes `cmd`.
    if matches!(cmd, Command::ViewerClose) {
        rt.release_viewer_source();
    }
    // A cancellable request's final reply retires its cancel flag — there is
    // nothing left to stop (listing-request-guard design D3). Same pre-
    // `update` interception point as the viewer source above.
    match &cmd {
        Command::ListingComplete { request, .. }
        | Command::ListingFailed { request, .. }
        | Command::FindFileSearchDone { request }
        | Command::ViewerSearchResult { request, .. } => rt.cancels.retire(*request),
        _ => {}
    }
    // The source must also go whenever the viewer phase *ends* by any other
    // route, and when a `ViewerOpened` is discarded, so the file is held
    // exactly as long as the viewer exists — see
    // `Runtime::release_viewer_source_if_viewer_ended` (resize-preserves-
    // session-state design D7; viewer "Closing the viewer releases the file").
    let was_viewer = matches!(state.phase, UiPhase::Viewer(_));
    let viewer_opened = matches!(cmd, Command::ViewerOpened { .. });
    // Commands → Menu file edit, built-in-editor leg: remember whether the
    // editor currently has the user-menu file open, so the transition out of
    // `UiPhase::Editor` (any of the quit paths, save-on-exit included)
    // triggers the reload below (user-menu "Edit the user menu file from the
    // Commands menu"; design D4).
    let editing_user_menu = matches!(&state.phase, UiPhase::Editor(editor) if editor.path == rt.usermenu_path);
    let (state, effects) = update(state, cmd);
    let quit = run_effects(effects, guard, rt)?;
    rt.release_viewer_source_if_viewer_ended(was_viewer, viewer_opened, &state);
    if editing_user_menu && !matches!(state.phase, UiPhase::Editor(_)) {
        reload_user_menu(rt);
    }
    Ok((state, quit))
}

/// Re-read `usermenu.toml` and feed the parse result back as
/// `Command::UserMenuReloaded` (design D4) — drained before the next frame,
/// exactly like the `RereadPanel` an external editor's return sends.
fn reload_user_menu(rt: &Runtime) {
    worker::deliver(&rt.tx, "user menu reload", Command::UserMenuReloaded(config::load_user_menu(&rt.usermenu_path)));
}

/// Drain every queued worker event, including any command an effect fed
/// straight back. The `bool` is `true` if anything was applied, so the
/// caller knows to redraw.
fn drain_events(mut state: State, rx: &Receiver<Command>, guard: &mut TerminalGuard, rt: &mut Runtime) -> io::Result<(State, bool)> {
    let mut applied = false;
    while let Ok(cmd) = rx.try_recv() {
        applied = true;
        let (s, quit) = apply(state, cmd, guard, rt)?;
        state = s;
        if quit {
            break;
        }
    }
    Ok((state, applied))
}

/// Execute effects returned by `update`. Returns `true` if the caller should
/// exit the event loop (a `Quit` effect was requested).
fn run_effects(effects: Vec<Effect>, guard: &mut TerminalGuard, rt: &mut Runtime) -> io::Result<bool> {
    let mut quit = false;
    for effect in effects {
        match effect {
            Effect::StartListing { panel, path, request } => {
                let cancel = rt.cancels.register(request);
                worker::spawn_listing(panel, path, request, cancel, rt.tx.clone(), rt.workers.token());
            }
            Effect::CancelRequest(request) => rt.cancels.cancel(request),
            Effect::Quit => quit = true,
            Effect::RunJob(job) => rt.active_job = Some(worker::spawn_job(job, rt.tx.clone(), rt.workers.token())),
            Effect::CancelJob => {
                if let Some(handle) = &rt.active_job {
                    handle.cancel.cancel();
                }
            }
            Effect::SendConflictReply(choice) => {
                if let Some(handle) = &rt.active_job {
                    // A failed send means the job thread has already ended;
                    // the answer is lost, so it is logged rather than
                    // dropped silently (error-diagnostics design D3).
                    if handle.reply_tx.send(worker::JobReply::Conflict(choice)).is_err() {
                        diag::log("job", "conflict answer not delivered: job ended");
                    }
                }
            }
            Effect::SendErrorReply(choice) => {
                if let Some(handle) = &rt.active_job {
                    if handle.reply_tx.send(worker::JobReply::Error(choice)).is_err() {
                        diag::log("job", "error answer not delivered: job ended");
                    }
                }
            }
            Effect::RunShellCommand(invocation, side) => {
                run_shell_command(guard, &invocation)?;
                // Mirrors `TerminalGuard::resume`'s own reasoning (design
                // D1): a button held when the TUI suspended may never
                // deliver its `Up` back through crossterm once resumed, so
                // any in-progress press/double-click bookkeeping is
                // discarded rather than trusted.
                rt.mouse_tracker.reset();
                // Fed straight back, same as drive enumeration: the panel
                // that owned the command must show whatever the command did
                // to its directory the moment the TUI repaints.
                worker::deliver(&rt.tx, "panel re-read", Command::RereadPanel(side));
            }
            Effect::OpenWithAssociation { path, cwd } => {
                // Fire-and-forget (open-with-associated-app design D1/D3):
                // no suspend, no wait, no mouse-tracker reset, no panel
                // re-read. Only a launch failure reports back, as the
                // command that lands the message on the panel's status line.
                if let Err(message) = open_with_association(&path, &cwd) {
                    worker::deliver(&rt.tx, "open-with failure", Command::OpenWithAssociationFailed { message });
                }
            }
            Effect::ShowScrollback => {
                show_scrollback(guard)?;
                rt.mouse_tracker.reset();
            }
            Effect::PersistHistory(file) => {
                // Handed to the background writer, which coalesces bursts,
                // merges with the on-disk file, and writes durably off this
                // thread (fuzzy-jump "Navigation does not wait for the
                // history write"; history-persistence-hardening design D4).
                // The writer is the disabled no-op when the profile
                // directory is unwritable this session
                // (config-profile-directory; design D2), and a failed write
                // is logged there rather than taking the session down.
                rt.history_writer.submit(file);
            }
            Effect::PersistTheme(name) => {
                // Same tolerance as history: a failed config write must
                // never take the session down (the theme is already active
                // in memory either way); likewise skipped when persistence
                // is off for the session (design D2).
                if rt.persist_enabled {
                    diag::log_err("persist", format_args!("config write {}", rt.config_path.display()), config::save_theme_atomic(&rt.config_path, &name));
                }
            }
            Effect::PersistPanelSplit(percent) => {
                // Same tolerance as the theme write above (panel-split
                // "Split persistence to configuration"; design D2).
                if rt.persist_enabled {
                    diag::log_err("persist", format_args!("config write {}", rt.config_path.display()), config::save_panel_split_atomic(&rt.config_path, percent));
                }
            }
            Effect::EnumerateDrives(target) => {
                // Cheap enough for the input path: a bitmask read, no media
                // or network probing. Fed straight back so the dialog's
                // first painted frame already lists every letter.
                worker::deliver(&rt.tx, "drive list", Command::DriveListReady { target, drives: drives::enumerate_drives() });
            }
            Effect::FetchDriveLabel { target, letter, generation } => worker::spawn_drive_label(target, letter, generation, rt.tx.clone(), rt.workers.token()),
            Effect::QueryInfo { panel, path, request } => worker::spawn_info_query(panel, path, request, rt.tx.clone(), rt.workers.token()),
            Effect::QueryGitInfo { panel, path, request } => {
                let slot = match panel {
                    PanelSide::Left => &rt.git_slots[0],
                    PanelSide::Right => &rt.git_slots[1],
                };
                slot.request(panel, path, request, rt.tx.clone(), rt.workers.token());
            }
            Effect::ExpandTreeNode { panel, path } => worker::spawn_tree_expand(panel, path, rt.tx.clone(), rt.workers.token()),
            Effect::OpenViewer { path } => rt.open_viewer_source(path),
            Effect::RunViewerSearch { path, start_offset, pattern, request } => {
                let cancel = rt.cancels.register(request);
                worker::spawn_viewer_search(path, start_offset, pattern, request, cancel, rt.tx.clone(), rt.workers.token());
            }
            Effect::RunExternalEditor(invocation, side) => {
                let result = run_external_editor(guard, &invocation)?;
                rt.mouse_tracker.reset();
                match result {
                    Ok(()) => {
                        // The editor may have written to the file; re-read the
                        // panel that owned the cursor entry, mirroring
                        // `RunShellCommand` (external-editor: Synchronous wait
                        // and panel re-read — "Panel refreshed after edit").
                        worker::deliver(&rt.tx, "panel re-read", Command::RereadPanel(side));
                        // Commands → Menu file edit returns through here too:
                        // reload the user menu when the file the editor had
                        // open was `usermenu.toml` (design D4). A panel F4
                        // edit's `file_arg` is a bare file name, never the
                        // absolute menu path, so this never false-fires.
                        if Path::new(&invocation.file_arg) == rt.usermenu_path.as_path() {
                            reload_user_menu(rt);
                        }
                    }
                    Err(message) => {
                        worker::deliver(&rt.tx, "external editor failure", Command::ExternalEditorSpawnFailed { message });
                    }
                }
            }
            Effect::OpenEditor { path } => match EditorState::open(&path) {
                // Synchronous, like `Effect::OpenViewer`: the whole read is
                // bounded by the 10 MB cap, so this is cheap enough for the
                // input path rather than warranting a worker thread (design
                // D1).
                Ok(EditorLoadResult::Loaded(editor)) => {
                    worker::deliver(&rt.tx, "editor opened", Command::EditorOpened(Box::new(editor)));
                }
                Ok(EditorLoadResult::TooLarge { size }) => {
                    worker::deliver(&rt.tx, "editor too large", Command::EditorTooLarge { path, size });
                }
                Ok(EditorLoadResult::NotEditable { reason }) => {
                    worker::deliver(&rt.tx, "editor not editable", Command::EditorNotEditable { path, reason });
                }
                Err(e) => {
                    worker::deliver(&rt.tx, "editor open failure", Command::EditorOpenFailed { message: cannot_message("open", &path, &e) });
                }
            },
            Effect::FindInSubtree { root, pattern, request } => {
                let cancel = rt.cancels.register(request);
                worker::spawn_find_subtree(root, pattern, request, cancel, rt.tx.clone(), rt.workers.token());
            }
            Effect::SaveEditor { editor, then_quit } => {
                let mut editor = *editor;
                match editor.save() {
                    Ok(()) => {
                        worker::deliver(&rt.tx, "editor saved", Command::EditorSaved { editor: Box::new(editor), then_quit });
                    }
                    Err(e) => {
                        worker::deliver(&rt.tx, "editor save failure", Command::EditorSaveFailed { message: cannot_message("save", &editor.path, &e) });
                    }
                }
            }
            Effect::SetClipboard(payload) => {
                // Path normalisation (clipboard-export "Windows file-object
                // payload"; design D4) happens once here, up front, so the
                // same normalized items back the actual clipboard write and
                // the `ClipboardResult` reply core renders into feedback
                // (e.g. the singular "Path copied: <path>" message) —
                // neither ever shows the internal `\\?\` form.
                let items: Vec<PathBuf> = payload.items.iter().map(|p| clipboard::normalize_path(p)).collect();
                let (result, fell_back_to_paths) = match payload.kind {
                    ClipboardPayloadKind::Files => match rt.clipboard.set_files(&items) {
                        Ok(fell_back) => (Ok(()), fell_back),
                        Err(message) => (Err(message), false),
                    },
                    ClipboardPayloadKind::Paths => (rt.clipboard.set_text(&clipboard::paths_text(&items)), false),
                    ClipboardPayloadKind::Names => (rt.clipboard.set_text(&clipboard::names_text(&items)), false),
                };
                let normalized_payload = update::ClipboardPayload { kind: payload.kind, items };
                worker::deliver(&rt.tx, "clipboard result", Command::ClipboardResult { payload: normalized_payload, fell_back_to_paths, result });
            }
            Effect::ReadClipboardText => {
                // The read half of paste (text-field-editing "Paste into a
                // text field"; design D4): clipboard text comes back as
                // `TextEdit(Paste(..))` for whichever field owns the
                // keyboard when it arrives. Line breaks flatten to a single
                // space here (`LineEdit::insert_str` does the same again,
                // so the core behavior never depends on this path — and the
                // terminal bracketed-paste route flattens through the same
                // helper; idle-aware-event-loop design D6), and a clipboard
                // with no text feeds nothing back — the key is then a no-op
                // ("Empty clipboard is a no-op").
                if let Some(text) = rt.clipboard.get_text() {
                    let text = flatten_line_breaks(&text);
                    if !text.is_empty() {
                        worker::deliver(&rt.tx, "paste", Command::TextEdit(filecommand_core::line_edit::TextEditOp::Paste(text)));
                    }
                }
            }
        }
    }
    Ok(quit)
}

/// The status-line error forms of filesystem-error-handling "Error messages
/// name the affected path" (error-diagnostics design D4): `Cannot
/// <verb> <name>: <reason>`, where the name is the file's own name — the
/// panel title directly above already shows the directory, and a full path
/// would push the reason off the truncated mini-status — or the whole path
/// when there is no file name (a drive or share root).
fn cannot_message(verb: &str, path: &Path, reason: impl std::fmt::Display) -> String {
    format!("Cannot {verb} {}: {reason}", worker::name_or_path(path))
}

/// Suspend the TUI, run the command on the real terminal, and come back.
///
/// Restore runs on every path — a spawn failure, a non-zero exit, or a
/// child that scribbled over the screen — because `resume` is idempotent
/// and the panic hook covers the rest.
///
/// The whole stretch, from before the suspend until after the resume has
/// re-entered raw mode, runs inside a [`console::ChildRunGuard`], so a
/// Ctrl+C or Ctrl+Break typed meanwhile reaches only the child
/// (application-shell "Ctrl+C during a suspended child run stops only the
/// child"; process-lifecycle-hardening design D1/D2).
fn run_shell_command(guard: &mut TerminalGuard, invocation: &Invocation) -> io::Result<()> {
    let mut child_run = console::ChildRunGuard::new();
    guard.suspend()?;
    let status = console::spawn_and_wait(
        std::process::Command::new(&invocation.program)
            .args(&invocation.args)
            .current_dir(&invocation.cwd)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit()),
        &mut child_run,
    );
    if let Err(e) = &status {
        // The message lands in the scrollback the user is about to see.
        let _ = writeln!(io::stdout(), "\r\nCould not run `{}`: {e}", invocation.program);
    }
    wait_for_key()?;
    guard.resume()?;
    drop(child_run);
    Ok(())
}

/// Open `path` with the application the platform associates it with
/// (open-with-associated-app; design D1): `ShellExecuteW` with the `open`
/// verb on Windows, a detached `xdg-open`/`open` elsewhere. Never suspends
/// the TUI and never waits on the launched program; `Err(message)` is the
/// user-facing failure text for the active panel's status line.
#[cfg(windows)]
fn open_with_association(path: &Path, cwd: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    // ShellExecuteW predates long-path support, so the `\\?\` verbatim
    // prefix must come off first — the same normalization the clipboard-
    // export payload applies (its "Long-path prefix is stripped" scenario;
    // file-action-menu "Open with the associated application": "Long path is
    // passed without the verbatim prefix").
    let path = clipboard::normalize_path(path);
    let cwd = clipboard::normalize_path(cwd);
    let verb = wide(std::ffi::OsStr::new("open"));
    let file = wide(path.as_os_str());
    let directory = wide(cwd.as_os_str());

    // SAFETY: every pointer is to a null-terminated UTF-16 buffer that
    // outlives the call; a null hwnd simply means no owner window.
    let result = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), directory.as_ptr(), SW_SHOWNORMAL) };
    // Per MSDN a return value greater than 32 is success; anything else is
    // an error code (a SE_ERR_* / ERROR_* value).
    let code = result as usize;
    if code > 32 {
        return Ok(());
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
    Err(open_with_association_error(&name, code as u32))
}

/// The user-facing message for a failed `ShellExecuteW` return code,
/// factored out pure so it is testable without touching the shell (design
/// D1): `SE_ERR_NOASSOC` names the missing association,
/// `ERROR_FILE_NOT_FOUND` the missing target, and anything else reports the
/// raw code (file-action-menu "Open with the associated application").
#[cfg(windows)]
fn open_with_association_error(name: &str, code: u32) -> String {
    use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows_sys::Win32::UI::Shell::SE_ERR_NOASSOC;

    match code {
        SE_ERR_NOASSOC => format!("No application is associated with {name}"),
        ERROR_FILE_NOT_FOUND => format!("{name} not found"),
        _ => format!("Could not open {name} (error {code})"),
    }
}

/// The non-Windows half of the Open action (design D1): `xdg-open`
/// (Linux/BSD) or `open` (macOS), spawned detached — all stdio to null, no
/// wait — so the TUI stays live and the launched program's output can never
/// interleave with it. A spawn error maps to the same failure message shape
/// the Windows path produces.
#[cfg(not(windows))]
fn open_with_association(path: &Path, cwd: &Path) -> Result<(), String> {
    let program = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    std::process::Command::new(program)
        .arg(path)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
            format!("Could not open {name} ({e})")
        })
}

/// Suspend the TUI, run the F4 external editor on the real terminal, and
/// come back — the same suspend/restore seam `run_shell_command` uses
/// (design D7), reused rather than duplicated (external-editor: TUI suspend
/// and restore). Unlike a shell command, a spawn failure here must reach
/// `core::update` as an inline panel error rather than only being printed
/// to the scrollback, so the `Result` is returned instead of swallowed.
///
/// Restore runs on every path — spawn failure, a crash, or a non-zero exit
/// — so a misbehaving editor can never leave the terminal corrupted
/// (external-editor: Terminal restored when the editor crashes).
fn run_external_editor(guard: &mut TerminalGuard, invocation: &EditorInvocation) -> io::Result<Result<(), String>> {
    // Same child-run guard as `run_shell_command`: Ctrl+C inside the editor
    // never ends FileCommand (application-shell "Ctrl+C inside the external
    // editor does not end FileCommand"; design D1/D2).
    let mut child_run = console::ChildRunGuard::new();
    guard.suspend()?;
    let result = spawn_editor_and_wait(invocation, &mut child_run);
    if let Err(message) = &result {
        let _ = writeln!(io::stdout(), "\r\n{message}");
    }
    wait_for_key()?;
    guard.resume()?;
    drop(child_run);
    Ok(result)
}

/// The process-spawn half of [`run_external_editor`], factored out so it is
/// testable without a real console (`guard.suspend`/`resume` need one; this
/// does not). `Ok(())` on any exit (including non-zero — matching
/// `run_shell_command`'s precedent of not treating the child's exit code as
/// FileCommand's own error); `Err` only when the process could not be
/// spawned at all (external-editor: Editor spawn errors do not crash the
/// app). `child_run` is armed once the editor has started (design D2).
fn spawn_editor_and_wait(invocation: &EditorInvocation, child_run: &mut console::ChildRunGuard) -> Result<(), String> {
    console::spawn_and_wait(
        std::process::Command::new(&invocation.program)
            .args(&invocation.leading_args)
            .arg(&invocation.file_arg)
            .current_dir(&invocation.cwd)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit()),
        child_run,
    )
    .map(|_| ())
    .map_err(|e| format!("Could not run `{}`: {e}", invocation.program))
}

/// Ctrl+O: hand the screen back so the host terminal's scrollback (which is
/// where prior command output lives — FileCommand keeps no buffer of its
/// own) is visible until any key is pressed.
fn show_scrollback(guard: &mut TerminalGuard) -> io::Result<()> {
    guard.suspend()?;
    wait_for_key()?;
    guard.resume()
}

/// Prompt, then block until one key press. Raw mode is re-enabled just for
/// the read so a single keystroke suffices instead of a whole line.
fn wait_for_key() -> io::Result<()> {
    let mut stdout = io::stdout();
    let _ = write!(stdout, "\r\n{RESUME_PROMPT}");
    let _ = stdout.flush();
    enable_raw_mode()?;
    loop {
        match event::read() {
            Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => break,
            Ok(_) => continue,
            Err(e) => {
                let _ = disable_raw_mode();
                return Err(e);
            }
        }
    }
    disable_raw_mode()?;
    let _ = writeln!(stdout);
    Ok(())
}

/// Draws one frame and returns the [`HitMap`] it recorded, which the caller
/// keeps around to feed `input::map_mouse` for whatever mouse events arrive
/// before the next draw (design D2).
fn draw(
    guard: &mut TerminalGuard,
    state: &State,
    identity_lines: &[String; 4],
    wall_clock: &dyn WallClock,
    viewer_source: Option<&ByteSource>,
) -> io::Result<HitMap> {
    let (hour, minute) = wall_clock.now_local();
    let clock_text = format_clock(hour, minute);
    let mut hitmap = HitMap::default();
    guard.terminal.draw(|frame| {
        let area = frame.area();
        let depth = ColorDepth::Ansi16;
        // `views::render` takes `frame.buffer_mut()`, a mutable borrow that
        // ends when the call returns — only then can `frame.set_cursor_position`
        // borrow `frame` again, which is why the cursor position travels
        // back as a return value instead of being set from inside `render`
        // itself (builtin-editor "Full-screen editor chrome" — caret as the
        // terminal cursor).
        let output = views::render(frame.buffer_mut(), area, state, depth, identity_lines, &clock_text, viewer_source);
        if let Some((x, y)) = output.cursor {
            frame.set_cursor_position((x, y));
        }
        hitmap = output.hitmap;
    })?;
    Ok(hitmap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};
    use std::ffi::OsString;

    /// A harmless, always-present "editor" invocation per platform — exits
    /// immediately without touching the file system, so the test exercises
    /// a real spawn + wait without depending on any real editor being
    /// installed (external-editor: F4 launches the editor / round-trip with
    /// a stub editor command).
    fn stub_invocation(file_arg: &str) -> EditorInvocation {
        let (program, leading_args) = if cfg!(windows) {
            ("cmd.exe".to_string(), vec!["/C".to_string(), "exit".to_string()])
        } else {
            ("/bin/sh".to_string(), vec!["-c".to_string(), "exit 0".to_string()])
        };
        EditorInvocation { program, leading_args, file_arg: OsString::from(file_arg), cwd: std::env::temp_dir() }
    }

    /// listing-request-guard design D3 (task 5.14): `cancel` sets the
    /// registered flag and forgets it, an unknown id is a no-op, and
    /// `retire` forgets without setting.
    #[test]
    fn cancel_registry_cancels_retires_and_ignores_unknown_ids() {
        let mut registry = CancelRegistry::default();
        let listing = registry.register(1);
        let walk = registry.register(2);
        assert!(!listing.is_cancelled() && !walk.is_cancelled(), "fresh flags are unset");

        registry.cancel(1);
        assert!(listing.is_cancelled(), "cancel sets the worker's clone of the flag");
        assert!(!registry.flags.contains_key(&1), "and removes the entry");
        assert!(!walk.is_cancelled(), "other requests are untouched");

        registry.cancel(1);
        registry.cancel(999);
        assert_eq!(registry.flags.len(), 1, "an already-finished or unknown id is a silent no-op");

        registry.retire(2);
        assert!(!walk.is_cancelled(), "retire never sets the flag");
        assert!(registry.flags.is_empty());
    }

    /// viewer: Instant open — "Closing the viewer releases the file": after
    /// `Effect::OpenViewer` installs the source and `Command::ViewerClose`
    /// clears it, the file and its directory can both be removed — an open
    /// handle blocks directory removal on Windows, so this fails if the
    /// source leaks. `apply`/`run_effects` need a `TerminalGuard`, which a
    /// headless test cannot construct, so this drives the same `Runtime`
    /// methods those two call.
    #[test]
    fn closing_the_viewer_releases_the_file_and_its_directory() {
        let dir = std::env::temp_dir().join(format!("filecommand-tui-app-viewer-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("file.bin");
        std::fs::write(&path, b"hello world").unwrap();

        let (tx, rx) = mpsc::channel();
        let mut rt = Runtime {
            tx,
            active_job: None,
            cancels: CancelRegistry::default(),
            history_writer: HistoryWriter::disabled(),
            config_path: dir.join(CONFIG_FILE),
            usermenu_path: dir.join("usermenu.toml"),
            persist_enabled: false,
            viewer_source: None,
            clipboard: Box::new(clipboard::RecordingClipboard::default()),
            mouse_tracker: MouseTracker::new(),
            git_slots: [worker::GitQuerySlot::new(), worker::GitQuerySlot::new()],
            workers: worker::WorkerGauge::new(),
        };

        rt.open_viewer_source(path.clone());
        assert!(rt.viewer_source.is_some());
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Command::ViewerOpened { path: opened, file_len: 11 }) => assert_eq!(opened, path),
            other => panic!("expected ViewerOpened, got {other:?}"),
        }

        rt.release_viewer_source();
        assert!(rt.viewer_source.is_none());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    /// A `Runtime` with no terminal and no worker threads, for the viewer-
    /// source tests below (and the one above, which predates it).
    fn headless_runtime(dir: &std::path::Path) -> (Runtime, Receiver<Command>) {
        let (tx, rx) = mpsc::channel();
        let rt = Runtime {
            tx,
            active_job: None,
            cancels: CancelRegistry::default(),
            history_writer: HistoryWriter::disabled(),
            config_path: dir.join(CONFIG_FILE),
            usermenu_path: dir.join("usermenu.toml"),
            persist_enabled: false,
            viewer_source: None,
            clipboard: Box::new(clipboard::RecordingClipboard::default()),
            mouse_tracker: MouseTracker::new(),
            git_slots: [worker::GitQuerySlot::new(), worker::GitQuerySlot::new()],
            workers: worker::WorkerGauge::new(),
        };
        (rt, rx)
    }

    /// Mirrors `apply` minus `run_effects` (which needs a `TerminalGuard`):
    /// the pre-`update` `ViewerClose` release and the post-`update`
    /// transition check, in the same order.
    fn apply_headless(state: State, cmd: Command, rt: &mut Runtime) -> State {
        if matches!(cmd, Command::ViewerClose) {
            rt.release_viewer_source();
        }
        let was_viewer = matches!(state.phase, UiPhase::Viewer(_));
        let viewer_opened = matches!(cmd, Command::ViewerOpened { .. });
        let (state, _effects) = update(state, cmd);
        rt.release_viewer_source_if_viewer_ended(was_viewer, viewer_opened, &state);
        state
    }

    /// resize-preserves-session-state design D7 (task 3.4): the byte source
    /// follows the viewer *phase*. It survives an unrelated command applied
    /// between `Effect::OpenViewer` and the queued `ViewerOpened` (the phase
    /// is still `Panels` then), survives a shrink below the floor and the
    /// enlarge back, and is released exactly when the viewer closes.
    #[test]
    fn viewer_source_follows_the_viewer_phase_across_a_resize() {
        let dir = std::env::temp_dir().join(format!("filecommand-tui-app-viewer-resize-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("file.bin");
        std::fs::write(&path, b"hello world").unwrap();
        let (mut rt, rx) = headless_runtime(&dir);

        let mut state = State::empty(Theme::classic());
        state.term_size = (80, 24);
        rt.open_viewer_source(path.clone());
        assert!(rt.viewer_source.is_some());
        let opened = match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(cmd @ Command::ViewerOpened { .. }) => cmd,
            other => panic!("expected ViewerOpened, got {other:?}"),
        };

        // A listing chunk already queued ahead of the reply is applied while
        // the phase is still `Panels`: the fresh source must survive it.
        let request = state.left.listing_request.unwrap_or(0);
        let state = apply_headless(state, Command::ListingChunk { panel: PanelSide::Left, entries: vec![], request }, &mut rt);
        assert!(rt.viewer_source.is_some(), "an unrelated command before ViewerOpened must not drop the source");
        let state = apply_headless(state, opened, &mut rt);
        assert!(matches!(state.phase, UiPhase::Viewer(_)));
        assert!(rt.viewer_source.is_some());

        // Shrink below the floor and enlarge: the viewer phase persists, and
        // so does its source.
        let state = apply_headless(state, Command::Resize(50, 12), &mut rt);
        assert!(state.undersized && matches!(state.phase, UiPhase::Viewer(_)));
        assert!(rt.viewer_source.is_some(), "the hidden viewer keeps its file open");
        let state = apply_headless(state, Command::Resize(80, 24), &mut rt);
        assert!(!state.undersized && matches!(state.phase, UiPhase::Viewer(_)));
        assert!(rt.viewer_source.is_some(), "the source is still there when the viewer reappears");

        // Closing releases it — and only then.
        let state = apply_headless(state, Command::ViewerClose, &mut rt);
        assert_eq!(state.phase, UiPhase::Panels);
        assert!(rt.viewer_source.is_none());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    /// Design D7's second clause: a `ViewerOpened` the reducer discards
    /// (here it arrives while the end-of-job summary owns the phase) must not
    /// leave the source `Effect::OpenViewer` installed behind.
    #[test]
    fn a_discarded_viewer_opened_releases_the_source() {
        let dir = std::env::temp_dir().join(format!("filecommand-tui-app-viewer-discard-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("file.bin");
        std::fs::write(&path, b"hello world").unwrap();
        let (mut rt, rx) = headless_runtime(&dir);

        rt.open_viewer_source(path.clone());
        let opened = rx.recv_timeout(Duration::from_secs(5)).expect("ViewerOpened");
        let mut state = State::empty(Theme::classic());
        state.phase = UiPhase::FileOpSummary(vec![]);
        let state = apply_headless(state, opened, &mut rt);
        assert!(!matches!(state.phase, UiPhase::Viewer(_)), "the summary dialog swallowed the reply");
        assert!(rt.viewer_source.is_none());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    /// Regression test for the F4 external editor being silently dead: a
    /// config-driven startup (via `config::parse`, the real parsing path,
    /// not a hand-built `Config`) must populate `state.editor`, mirroring
    /// the already-wired `state.shell`. Before this fix, `run()` never
    /// assigned `state.editor` at all, so `editor =` in `config.toml` was
    /// parsed correctly but never reached `State`.
    #[test]
    fn apply_config_wires_the_configured_editor_and_shell_into_state() {
        let config = config::parse("editor = \"code --wait\"\nshell = \"pwsh -NoLogo -Command\"\n").config;
        let mut state = State::empty(Theme::classic());
        assert_eq!(state.editor, None, "sanity: editor starts unset before wiring");
        apply_config(&mut state, &config);
        assert_eq!(state.editor.as_deref(), Some("code --wait"));
        assert_eq!(state.shell.shell.as_deref(), Some("pwsh -NoLogo -Command"));
    }

    #[test]
    fn apply_config_wires_the_configured_panel_split_into_state() {
        let config = config::parse("panel_split = 66\n").config;
        let mut state = State::empty(Theme::classic());
        assert_eq!(state.split_percent, 50, "sanity: split starts at the default before wiring");
        apply_config(&mut state, &config);
        assert_eq!(state.split_percent, 66);
    }

    #[test]
    fn apply_config_leaves_editor_unset_when_config_omits_it() {
        let config = config::parse("splash = false\n").config;
        let mut state = State::empty(Theme::classic());
        apply_config(&mut state, &config);
        assert_eq!(state.editor, None);
    }

    /// ui-animations "Animations are disabled by config or launch flag":
    /// `State::empty` starts with animations off (so existing tests are
    /// unaffected); a real launch turns them on from the default config.
    #[test]
    fn apply_config_turns_animations_on_by_default_and_off_when_configured() {
        let mut state = State::empty(Theme::classic());
        assert!(!state.animations, "sanity: a state built without config does not animate");
        apply_config(&mut state, &config::parse("").config);
        assert!(state.animations, "default is animated");
        apply_config(&mut state, &config::parse("animations = false\n").config);
        assert!(!state.animations, "animations = false disables them");
    }

    /// ui-animations "Flag overrides config": `--noanimations` wins over
    /// `animations = true`, and without the flag the config value stands.
    #[test]
    fn noanimations_flag_overrides_the_config_value() {
        let on = config::parse("animations = true\n").config;
        let mut state = State::empty(Theme::classic());
        apply_config(&mut state, &on);
        apply_launch_options(&mut state, &parse_launch_args(["--noanimations".to_string()]));
        assert!(!state.animations, "the flag wins over animations = true");

        let mut state = State::empty(Theme::classic());
        apply_config(&mut state, &on);
        apply_launch_options(&mut state, &parse_launch_args(Vec::<String>::new()));
        assert!(state.animations, "no flag leaves the config value alone");

        let mut state = State::empty(Theme::classic());
        apply_config(&mut state, &config::parse("animations = false\n").config);
        apply_launch_options(&mut state, &parse_launch_args(Vec::<String>::new()));
        assert!(!state.animations, "the flag never turns animations on");
    }

    #[test]
    fn parse_launch_args_reads_noanimations_alongside_other_switches() {
        let opts = parse_launch_args(["--noanimations".to_string()]);
        assert!(opts.no_animations);
        assert!(!opts.no_splash && !opts.no_mouse);
        assert!(!parse_launch_args(["--nosplash".to_string()]).no_animations, "absent switch leaves it off");

        let mixed = parse_launch_args(["--theme=terminal-green".to_string(), "--noanimations".to_string(), "--nosplash".to_string()]);
        assert!(mixed.no_animations && mixed.no_splash);
        assert_eq!(mixed.theme.as_deref(), Some("terminal-green"));
    }

    /// The startup warning is raised outside the reducer, so `run` stamps its
    /// open time explicitly for the iris; with no warning nothing is stamped.
    #[test]
    fn stamp_startup_warning_records_the_clock_only_when_a_warning_is_up() {
        let mut state = State::empty(Theme::classic());
        state.clock_ms = 1234;
        stamp_startup_warning(&mut state);
        assert_eq!(state.dialog_opened_at_ms, 0, "no warning: nothing to stamp");
        state.startup_warning = Some("usermenu.toml is malformed".to_string());
        stamp_startup_warning(&mut state);
        assert_eq!(state.dialog_opened_at_ms, 1234);
    }

    /// theme-selection "Applied theme persists to configuration" scenario
    /// "Unknown configured theme falls back to default": a `config.toml`
    /// naming a theme that isn't one of the six built-ins (typo, renamed,
    /// hand-edited) must resolve to `nc-classic`, not panic or silently pick
    /// something else.
    #[test]
    fn resolve_startup_theme_falls_back_to_nc_classic_for_an_unknown_name() {
        let config = config::parse("theme = \"no-such-theme\"\n").config;
        let (theme, warning) = resolve_startup_theme(&config, None);
        assert_eq!(theme.name, Theme::classic().name);
        assert_eq!(warning, None, "no --theme switch was passed, so no warning is raised");
    }

    /// Same scenario, unset case: an omitted `theme` key already defaults to
    /// `nc-classic` in `Config::default`, so this exercises the full
    /// unset -> load -> resolve path end to end.
    #[test]
    fn resolve_startup_theme_falls_back_to_nc_classic_when_theme_is_unset() {
        let config = config::parse("splash = false\n").config;
        let (theme, warning) = resolve_startup_theme(&config, None);
        assert_eq!(theme.name, Theme::classic().name);
        assert_eq!(warning, None);
    }

    /// Sanity check that a known configured theme round-trips through
    /// `resolve_startup_theme` unchanged rather than always collapsing to
    /// the fallback.
    #[test]
    fn resolve_startup_theme_loads_a_known_configured_theme() {
        let config = config::parse("theme = \"terminal-green\"\n").config;
        let (theme, warning) = resolve_startup_theme(&config, None);
        assert_eq!(theme.name, Theme::terminal_green().name);
        assert_eq!(warning, None);
    }

    // -- theme-selection "Launch-time theme override via --theme" --------

    /// Parser: `--theme <name>` (space-separated spelling).
    #[test]
    fn parse_launch_args_reads_theme_with_a_space_separated_value() {
        let opts = parse_launch_args(["--theme".to_string(), "yellow-storm".to_string()]);
        assert_eq!(opts.theme.as_deref(), Some("yellow-storm"));
        assert!(!opts.no_splash);
    }

    /// Parser: `--theme=<name>` (equals-separated spelling).
    #[test]
    fn parse_launch_args_reads_theme_with_an_equals_separated_value() {
        let opts = parse_launch_args(["--theme=purple-lights".to_string()]);
        assert_eq!(opts.theme.as_deref(), Some("purple-lights"));
    }

    /// Parser: `--theme` as the final argument has no value to consume, so
    /// it is recorded as an empty string rather than panicking or being
    /// dropped (spec scenario "Missing value warns and falls back").
    #[test]
    fn parse_launch_args_records_an_empty_value_when_theme_has_no_following_argument() {
        let opts = parse_launch_args(["--theme".to_string()]);
        assert_eq!(opts.theme.as_deref(), Some(""));
    }

    /// Parser: no `--theme` switch at all leaves the field unset, distinct
    /// from the missing-value case above.
    #[test]
    fn parse_launch_args_leaves_theme_unset_when_the_switch_is_absent() {
        let opts = parse_launch_args(["--nosplash".to_string()]);
        assert_eq!(opts.theme, None);
        assert!(opts.no_splash);
    }

    /// Parser: `--nosplash` is unaffected by `--theme` parsing, in either
    /// order, when both switches are present.
    #[test]
    fn parse_launch_args_reads_both_switches_regardless_of_order() {
        let nosplash_first = parse_launch_args(["--nosplash".to_string(), "--theme".to_string(), "terminal-green".to_string()]);
        assert!(nosplash_first.no_splash);
        assert_eq!(nosplash_first.theme.as_deref(), Some("terminal-green"));

        let theme_first = parse_launch_args(["--theme=terminal-green".to_string(), "--nosplash".to_string()]);
        assert!(theme_first.no_splash);
        assert_eq!(theme_first.theme.as_deref(), Some("terminal-green"));
    }

    // -- mouse-input "Mouse capture configuration" ------------------------

    /// Parser: `--nomouse` sets `no_mouse`, mirroring `--nosplash` exactly,
    /// and leaves the other switches unaffected.
    #[test]
    fn parse_launch_args_reads_nomouse() {
        let opts = parse_launch_args(["--nomouse".to_string()]);
        assert!(opts.no_mouse);
        assert!(!opts.no_splash);
        assert_eq!(opts.theme, None);
    }

    /// Parser: no `--nomouse` switch at all leaves it unset (mouse capture
    /// stays governed by `config.toml` alone).
    #[test]
    fn parse_launch_args_leaves_nomouse_unset_when_the_switch_is_absent() {
        let opts = parse_launch_args(["--nosplash".to_string()]);
        assert!(!opts.no_mouse);
    }

    /// Parser: `--nomouse` composes with `--nosplash` and `--theme`
    /// regardless of order, just like any two independent switches.
    #[test]
    fn parse_launch_args_reads_nomouse_alongside_other_switches_regardless_of_order() {
        let nomouse_first = parse_launch_args(["--nomouse".to_string(), "--nosplash".to_string(), "--theme".to_string(), "terminal-green".to_string()]);
        assert!(nomouse_first.no_mouse);
        assert!(nomouse_first.no_splash);
        assert_eq!(nomouse_first.theme.as_deref(), Some("terminal-green"));

        let nomouse_last = parse_launch_args(["--theme=terminal-green".to_string(), "--nosplash".to_string(), "--nomouse".to_string()]);
        assert!(nomouse_last.no_mouse);
        assert!(nomouse_last.no_splash);
        assert_eq!(nomouse_last.theme.as_deref(), Some("terminal-green"));
    }

    // -- config-profile-directory "Configuration profile directory" -------

    /// Parser: `--config-dir <path>` (space-separated spelling).
    #[test]
    fn parse_launch_args_reads_config_dir_with_a_space_separated_value() {
        let opts = parse_launch_args(["--config-dir".to_string(), "D:\\fc-test".to_string()]);
        assert_eq!(opts.config_dir.as_deref(), Some("D:\\fc-test"));
        assert!(!opts.no_splash && !opts.no_mouse && !opts.no_animations);
        assert_eq!(opts.theme, None);
    }

    /// Parser: `--config-dir=<path>` (equals-separated spelling).
    #[test]
    fn parse_launch_args_reads_config_dir_with_an_equals_separated_value() {
        let opts = parse_launch_args(["--config-dir=relative/dir".to_string()]);
        assert_eq!(opts.config_dir.as_deref(), Some("relative/dir"));
    }

    /// Parser: `--config-dir` as the final argument has no value to
    /// consume, recorded as an empty string (a startup warning downstream,
    /// never a crash) — the same convention as `--theme` (task 1.1).
    #[test]
    fn parse_launch_args_records_an_empty_value_when_config_dir_has_no_following_argument() {
        let opts = parse_launch_args(["--config-dir".to_string()]);
        assert_eq!(opts.config_dir.as_deref(), Some(""));
    }

    /// Parser: no `--config-dir` switch at all leaves the field unset,
    /// distinct from the missing-value case above.
    #[test]
    fn parse_launch_args_leaves_config_dir_unset_when_the_switch_is_absent() {
        assert_eq!(parse_launch_args(["--nosplash".to_string()]).config_dir, None);
    }

    // -- error-diagnostics "Optional diagnostic log" ----------------------

    /// Parser: `--log <path>` (space-separated spelling).
    #[test]
    fn parse_launch_args_reads_log_with_a_space_separated_value() {
        let opts = parse_launch_args(["--log".to_string(), "D:\\logs\\fc.log".to_string()]);
        assert_eq!(opts.log.as_deref(), Some("D:\\logs\\fc.log"));
        assert!(!opts.no_splash && !opts.no_mouse && !opts.no_animations);
        assert_eq!(opts.theme, None);
        assert_eq!(opts.config_dir, None);
    }

    /// Parser: `--log=<path>` (equals-separated spelling).
    #[test]
    fn parse_launch_args_reads_log_with_an_equals_separated_value() {
        let opts = parse_launch_args(["--log=fc.log".to_string()]);
        assert_eq!(opts.log.as_deref(), Some("fc.log"));
    }

    /// Parser: `--log` as the final argument has no value to consume,
    /// recorded as an empty string (the "--log needs a path" startup warning
    /// downstream, never a crash) — the same convention as `--theme`
    /// (design D2).
    #[test]
    fn parse_launch_args_records_an_empty_value_when_log_has_no_following_argument() {
        let opts = parse_launch_args(["--log".to_string()]);
        assert_eq!(opts.log.as_deref(), Some(""));
    }

    /// Parser: no `--log` switch at all leaves the field unset, distinct
    /// from the missing-value case above.
    #[test]
    fn parse_launch_args_leaves_log_unset_when_the_switch_is_absent() {
        assert_eq!(parse_launch_args(["--nosplash".to_string()]).log, None);
    }

    /// Design D2 / "The flag wins over the environment variable": a present
    /// `--log` beats `FILECOMMAND_LOG`; with no flag the variable applies,
    /// unless it is empty or unset.
    #[test]
    fn resolve_log_path_prefers_the_flag_over_the_environment() {
        assert_eq!(resolve_log_path(Some("flag.log"), Some("env.log")), Ok(Some("flag.log".to_string())));
        assert_eq!(resolve_log_path(None, Some("env.log")), Ok(Some("env.log".to_string())), "the variable enables logging without the flag");
        assert_eq!(resolve_log_path(None, None), Ok(None), "neither set: logging stays off");
        assert_eq!(resolve_log_path(None, Some("")), Ok(None), "an empty variable counts as unset");
        assert_eq!(resolve_log_path(Some("flag.log"), None), Ok(Some("flag.log".to_string())));
    }

    /// Design D2 / "the flag is given without a path": an empty `--log`
    /// value is a startup warning naming the switch, and it still wins over
    /// the variable (an empty flag must not silently fall back).
    #[test]
    fn resolve_log_path_warns_when_the_flag_has_no_path() {
        let warning = resolve_log_path(Some(""), Some("env.log")).expect_err("an empty flag value warns");
        assert!(warning.contains("--log"), "the warning names the switch: {warning}");
        assert!(warning.contains("path"), "the warning says what is missing: {warning}");
    }

    /// Design D2 / "Unwritable log path warns and continues": an
    /// unopenable log path becomes a startup warning naming the path and
    /// the reason it could not be opened; `None` (no log requested) stays
    /// silent. The failing call never enables the global log, so this test
    /// is safe for the other diag tests in the binary.
    #[test]
    fn init_diagnostic_log_warns_naming_path_and_reason_when_open_fails() {
        let missing_dir = std::env::temp_dir().join(format!("filecommand-tui-app-log-init-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&missing_dir);
        let path = missing_dir.join("fc.log");
        let warning = init_diagnostic_log(Some(path.display().to_string())).expect("an unopenable log path warns");
        assert!(warning.contains("Diagnostic log disabled"), "{warning}");
        assert!(warning.contains(&path.display().to_string()), "the warning names the path: {warning}");
        assert_eq!(init_diagnostic_log(None), None, "no log requested, no warning");
    }

    /// Task 1.1: a missing value becomes a warning and normal resolution
    /// continues; a real value passes through untouched.
    #[test]
    fn config_dir_override_warns_on_a_missing_value_and_passes_a_real_one_through() {
        let (flag, warning) = config_dir_override(Some(""));
        assert_eq!(flag, None, "normal resolution continues after the warning");
        assert!(warning.as_deref().is_some_and(|w| w.contains("--config-dir")), "the warning names the switch: {warning:?}");

        let (flag, warning) = config_dir_override(Some("D:\\fc-test"));
        assert_eq!(flag.as_deref(), Some(Path::new("D:\\fc-test")));
        assert_eq!(warning, None);

        assert_eq!(config_dir_override(None), (None, None), "no switch is neither an override nor a warning");
    }

    /// Design D1: an explicit flag wins outright — the portable marker and
    /// the platform default are not even consulted — and a relative value
    /// resolves against the working directory at startup.
    #[test]
    fn resolve_profile_dir_flag_wins_over_everything() {
        let exe_dir = std::env::temp_dir().join(format!("filecommand-profile-flag-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&exe_dir);
        std::fs::create_dir_all(&exe_dir).unwrap();
        std::fs::write(exe_dir.join("portable"), "").unwrap(); // would win, if consulted

        let absolute = PathBuf::from(if cfg!(windows) { "D:\\fc-test" } else { "/tmp/fc-test" });
        let (dir, warning) = resolve_profile_dir(Some(absolute.clone()), Some(&exe_dir));
        assert_eq!(dir, absolute);
        assert_eq!(warning, None);

        let cwd = std::env::current_dir().unwrap();
        let (dir, warning) = resolve_profile_dir(Some(PathBuf::from("fc-rel")), Some(&exe_dir));
        assert_eq!(dir, cwd.join("fc-rel"), "a relative flag value resolves against the cwd");
        assert_eq!(warning, None);
        let _ = std::fs::remove_dir_all(&exe_dir);
    }

    /// Design D1 / scenario "Portable mode via marker file": with no flag,
    /// a `portable` marker beside the executable makes the executable's
    /// directory the profile, beating the platform default.
    #[test]
    fn resolve_profile_dir_portable_marker_wins_over_the_platform_default() {
        let exe_dir = std::env::temp_dir().join(format!("filecommand-profile-portable-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&exe_dir);
        std::fs::create_dir_all(&exe_dir).unwrap();
        std::fs::write(exe_dir.join("portable"), "").unwrap();

        let (dir, warning) = resolve_profile_dir(None, Some(&exe_dir));
        assert_eq!(dir, exe_dir);
        assert_eq!(warning, None);
        let _ = std::fs::remove_dir_all(&exe_dir);
    }

    /// Design D1 / scenario "Platform default is used when nothing
    /// overrides it": no flag and no marker file means the platform
    /// default applies (asserted per-platform just below).
    #[test]
    fn resolve_profile_dir_without_a_marker_uses_the_platform_default() {
        let exe_dir = std::env::temp_dir().join(format!("filecommand-profile-nomarker-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&exe_dir);
        std::fs::create_dir_all(&exe_dir).unwrap();

        let (dir, warning) = resolve_profile_dir(None, Some(&exe_dir));
        assert_eq!(dir, platform_default_profile_dir().expect("a config home is set on this test host"));
        assert_eq!(warning, None);
        let _ = std::fs::remove_dir_all(&exe_dir);
    }

    /// Design D1: the platform default is `%APPDATA%\FileCommand`.
    #[cfg(windows)]
    #[test]
    fn platform_default_profile_dir_is_appdata_filecommand() {
        let appdata = std::env::var_os("APPDATA").expect("APPDATA is set on Windows hosts");
        assert_eq!(platform_default_profile_dir(), Some(PathBuf::from(appdata).join("FileCommand")));
    }

    /// Design D1: the platform default is `$XDG_CONFIG_HOME/filecommand`,
    /// else `~/.config/filecommand`.
    #[cfg(not(windows))]
    #[test]
    fn platform_default_profile_dir_is_xdg_or_home_filecommand() {
        let expected = std::env::var_os("XDG_CONFIG_HOME")
            .map(|xdg| PathBuf::from(xdg).join("filecommand"))
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config").join("filecommand")))
            .expect("XDG_CONFIG_HOME or HOME is set on Unix hosts");
        assert_eq!(platform_default_profile_dir(), Some(expected));
    }

    /// Design D2 / scenario "Unwritable profile falls back without losing
    /// the session": an uncreatable profile yields the fail-soft warning
    /// naming the path and the error; a creatable one yields none.
    #[test]
    fn ensure_profile_dir_warns_naming_path_and_error_when_creation_fails() {
        let dir = std::env::temp_dir().join(format!("filecommand-profile-ensure-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, "a file, not a directory").unwrap();

        let target = blocker.join("sub");
        let warning = ensure_profile_dir(&target).expect("creating below a plain file must fail");
        assert!(warning.contains(&target.display().to_string()), "the warning names the path: {warning}");
        assert!(warning.contains("settings will not be saved this session"), "the warning says persistence is off: {warning}");

        assert_eq!(ensure_profile_dir(&dir.join("fresh")), None, "a creatable directory is no warning");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Design D3 / scenario "Legacy files in the working directory produce
    /// a notice, not a migration": the notice names the found files and
    /// both directories, says they can be moved, and leaves them exactly
    /// as they were.
    #[test]
    fn legacy_notice_names_the_files_and_both_directories() {
        let root = std::env::temp_dir().join(format!("filecommand-profile-legacy-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let cwd = root.join("cwd");
        let profile = root.join("profile");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(cwd.join("config.toml"), "theme = \"nc-mono\"\n").unwrap();
        std::fs::write(cwd.join("history.json"), "{}\n").unwrap();

        let notice = legacy_notice(&cwd, &profile).expect("legacy files raise the notice");
        assert!(notice.contains("config.toml, history.json"), "names the found files: {notice}");
        assert!(notice.contains(&cwd.display().to_string()), "names the working directory: {notice}");
        assert!(notice.contains(&profile.display().to_string()), "names the profile directory: {notice}");
        assert!(notice.contains("Move them there"), "says the files can be moved: {notice}");

        // Nothing was copied or moved: the old files are untouched and the
        // profile stays empty.
        assert!(cwd.join("config.toml").exists() && cwd.join("history.json").exists());
        assert!(!profile.join("config.toml").exists());

        std::fs::write(profile.join("config.toml"), "").unwrap();
        assert_eq!(legacy_notice(&cwd, &profile), None, "a populated profile silences the notice");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// theme-selection "Valid override wins over configuration": a valid
    /// `--theme` value beats a different `config.theme`, with no warning.
    #[test]
    fn resolve_startup_theme_override_wins_over_configuration() {
        let config = config::parse("theme = \"nc-classic\"\n").config;
        let (theme, warning) = resolve_startup_theme(&config, Some("yellow-storm"));
        assert_eq!(theme.name, Theme::yellow_storm().name);
        assert_eq!(warning, None, "a valid override produces no warning");
    }

    /// theme-selection "Unknown name warns and falls back": an unrecognized
    /// override falls through to the configured theme and produces a
    /// warning naming the rejected value and listing the valid names.
    #[test]
    fn resolve_startup_theme_unknown_override_falls_back_with_a_warning() {
        let config = config::parse("theme = \"nc-mono\"\n").config;
        let (theme, warning) = resolve_startup_theme(&config, Some("no-such-theme"));
        assert_eq!(theme.name, Theme::mono().name);
        let warning = warning.expect("an unknown override raises a warning");
        assert!(warning.contains("no-such-theme"), "warning should name the rejected value: {warning}");
        for name in BUILTIN_THEME_NAMES {
            assert!(warning.contains(name), "warning should list every built-in theme name: {warning}");
        }
    }

    /// theme-selection "Missing value warns and falls back": `--theme` with
    /// no value falls through to the configured theme and warns that the
    /// value was missing (distinct wording from the unknown-name case).
    #[test]
    fn resolve_startup_theme_missing_value_falls_back_with_a_warning() {
        let config = config::parse("theme = \"nc-mono\"\n").config;
        let (theme, warning) = resolve_startup_theme(&config, Some(""));
        assert_eq!(theme.name, Theme::mono().name);
        let warning = warning.expect("a missing value raises a warning");
        assert!(warning.to_lowercase().contains("value"), "warning should describe a missing value: {warning}");
    }

    /// theme-selection "Override is session-only": a picker apply during a
    /// session that started via `--theme` persists exactly as it would
    /// without the switch — the override only changes the initial `Theme`
    /// passed into `State::initial`, so `update`'s own persistence logic is
    /// untouched (design D4).
    #[test]
    fn picker_apply_persists_normally_during_an_overridden_session() {
        let config = config::parse("theme = \"nc-classic\"\n").config;
        let (theme, warning) = resolve_startup_theme(&config, Some("terminal-green"));
        assert_eq!(theme.name, Theme::terminal_green().name);
        assert_eq!(warning, None);

        let (state, _) = State::initial(theme, (80, 24), 0, 0, PathBuf::from("."), PathBuf::from("."), false);
        let (state, _) = update(state, Command::ThemePickerOpen);
        let target = BUILTIN_THEME_NAMES.iter().position(|n| *n == "purple-lights").unwrap();
        let current = state.theme_picker.as_ref().unwrap().highlight;
        let (state, _) = update(state, Command::ThemePickerMove(target as isize - current as isize));
        let (state, effects) = update(state, Command::ThemePickerConfirm);
        assert_eq!(state.theme.name, "purple-lights", "the picker's choice wins, not the launch override");
        assert_eq!(effects, vec![Effect::PersistTheme("purple-lights".to_string())], "persistence is unaffected by how the session started");
    }

    /// Regression test for the M5 review finding: a malformed
    /// `usermenu.toml` used to only set the left panel's inline
    /// `last_error` (with a comment noting there was no startup-warning
    /// dialog); the spec requires a dismissable modal instead. Verifies
    /// `apply_user_menu` raises `state.startup_warning`, falls back to
    /// default entries, and never touches either panel's `last_error`.
    #[test]
    fn apply_user_menu_raises_the_startup_warning_dialog_on_malformed_content() {
        let mut state = State::empty(Theme::classic());
        let default_entries = config::default_user_menu_entries();
        let malformed = config::UserMenuLoadResult {
            entries: default_entries.clone(),
            warnings: Vec::new(),
            created_default: false,
            malformed: true,
            problem: Some(config::UserMenuProblem::InvalidToml),
        };
        apply_user_menu(&mut state, malformed);
        assert_eq!(state.user_menu_entries, default_entries, "falls back to default entries");
        assert!(
            state.startup_warning.as_deref().is_some_and(|m| m.contains(config::USERMENU_FILE)),
            "expected a startup warning naming the malformed file: {:?}",
            state.startup_warning
        );
        assert_eq!(state.left.last_error, None, "the warning is a dedicated modal, not a panel mini-status");
        assert_eq!(state.right.last_error, None);
    }

    #[test]
    fn apply_user_menu_does_not_warn_on_well_formed_content() {
        let mut state = State::empty(Theme::classic());
        let entries = config::default_user_menu_entries();
        let ok = config::UserMenuLoadResult { entries: entries.clone(), warnings: Vec::new(), created_default: false, malformed: false, problem: None };
        apply_user_menu(&mut state, ok);
        assert_eq!(state.user_menu_entries, entries);
        assert_eq!(state.startup_warning, None);
    }

    /// configuration-files "Non-UTF-8 usermenu.toml is kept, not
    /// overwritten": the not-UTF-8 outcome gets its own message that says
    /// how to recover, instead of the generic malformed one (design D3).
    #[test]
    fn apply_user_menu_names_the_not_utf8_recovery_for_a_non_utf8_file() {
        let mut state = State::empty(Theme::classic());
        let result = config::UserMenuLoadResult {
            entries: config::default_user_menu_entries(),
            warnings: Vec::new(),
            created_default: false,
            malformed: true,
            problem: Some(config::UserMenuProblem::NotUtf8),
        };
        apply_user_menu(&mut state, result);
        let warning = state.startup_warning.expect("a not-UTF-8 user menu raises the dialog");
        assert!(warning.starts_with("usermenu.toml is not UTF-8 text"), "{warning}");
        assert!(warning.contains("re-save it as UTF-8"), "{warning}");
    }

    /// history-persistence-hardening design D1: the renderer ranks the
    /// Ctrl+J list with `state.epoch_ms`, exactly as the reducer's Enter
    /// does, so the first drawn row is the row Enter navigates to even when
    /// the monotonic and wall clocks differ by decades.
    #[test]
    fn fuzzy_jump_renderer_and_reducer_rank_with_the_same_epoch_clock() {
        use filecommand_core::quicksearch::FrecencyEntry;
        use ratatui::buffer::Buffer;
        use ratatui::layout::Rect;
        let e = 1_787_000_000_000u64;
        let day = 24 * 60 * 60 * 1000;
        let mut state = State::empty(Theme::classic());
        state.clock_ms = 500;
        state.epoch_ms = e + 3 * day + 60_000;
        state.dir_history = vec![
            FrecencyEntry { path: PathBuf::from(r"C:\old"), visit_count: 3, last_visited_ms: e },
            FrecencyEntry { path: PathBuf::from(r"C:\new"), visit_count: 3, last_visited_ms: e + 3 * day },
        ];
        let (state, _) = update(state, Command::FuzzyJumpOpen);

        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let mut buf = Buffer::empty(area);
        views::fuzzy_jump::render_fuzzy_jump(&mut buf, area, &state.theme, ColorDepth::Ansi16, state.fuzzy_jump.as_ref().unwrap(), &state.dir_history, state.epoch_ms);
        let first_row = (0..area.height)
            .map(|y| (0..area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>())
            .find(|row| row.contains("C:\\"))
            .expect("the list draws at least one directory row");
        assert!(first_row.contains(r"C:\new"), "the renderer draws the more recently visited directory first: {first_row:?}");

        let (state, effects) = update(state, Command::FuzzyJumpConfirm);
        assert_eq!(state.left.cwd, PathBuf::from(r"C:\new"), "Enter lands on the row that was drawn first");
        assert!(effects.iter().any(|e| matches!(e, Effect::StartListing { path, .. } if path == Path::new(r"C:\new"))));
    }

    // -- configuration-files: the warning summary and its precedence ------

    /// The config.toml warnings come first, then usermenu.toml's, and the
    /// dialog shows the first plus a `(+N more)` count across both files
    /// (configuration-files "Several warnings are summarized"; design D4).
    #[test]
    fn configuration_warnings_summarize_across_both_files() {
        let mut state = State::empty(Theme::classic());
        let parsed = config::parse("colour = \"red\"\nsplash false\n");
        let menu = config::parse_usermenu("[[entry]]\nlabel = \"A\"\ncommand = \"a\"\nicon = \"x\"\n");
        let mut warnings: Vec<String> = parsed.warnings.iter().map(ToString::to_string).collect();
        warnings.extend(menu.warnings.iter().map(ToString::to_string));
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        apply_configuration_warnings(&mut state, &warnings);
        let shown = state.startup_warning.expect("the summary is shown");
        assert!(shown.starts_with("config.toml line 1: unknown setting \"colour\""), "{shown}");
        assert!(shown.ends_with("(+2 more)"), "{shown}");
    }

    /// configuration-files "Profile warning keeps precedence": an existing
    /// startup warning (here the unwritable-profile one) is never replaced
    /// by the configuration summary.
    #[test]
    fn configuration_warnings_never_displace_an_existing_startup_warning() {
        let mut state = State::empty(Theme::classic());
        let profile_warning = "Could not create the profile directory C:\\nope: access denied; settings will not be saved".to_string();
        set_startup_warning(&mut state, profile_warning.clone());
        let warnings = vec!["config.toml line 2: unknown setting \"colour\"; ignored".to_string()];
        apply_configuration_warnings(&mut state, &warnings);
        assert_eq!(state.startup_warning.as_deref(), Some(profile_warning.as_str()));
    }

    /// configuration-files "Theme override warning keeps precedence": the
    /// `--theme` warning wins over an unknown-setting summary; so does a
    /// malformed `usermenu.toml`.
    #[test]
    fn theme_override_and_malformed_user_menu_warnings_outrank_the_summary() {
        let config = config::parse("colour = \"red\"\n");
        let summary: Vec<String> = config.warnings.iter().map(ToString::to_string).collect();

        let mut state = State::empty(Theme::classic());
        let (_, theme_warning) = resolve_startup_theme(&config.config, Some("no-such-theme"));
        set_startup_warning(&mut state, theme_warning.expect("unknown --theme warns"));
        apply_configuration_warnings(&mut state, &summary);
        let shown = state.startup_warning.clone().unwrap();
        assert!(shown.contains("--theme no-such-theme"), "{shown}");

        let mut state = State::empty(Theme::classic());
        let malformed = config::UserMenuLoadResult {
            entries: config::default_user_menu_entries(),
            warnings: Vec::new(),
            created_default: false,
            malformed: true,
            problem: Some(config::UserMenuProblem::InvalidToml),
        };
        apply_user_menu(&mut state, malformed);
        apply_configuration_warnings(&mut state, &summary);
        let shown = state.startup_warning.unwrap();
        assert!(shown.contains("usermenu.toml is malformed"), "{shown}");
    }

    /// configuration-files "Clean configuration produces no warning".
    #[test]
    fn no_configuration_warnings_leave_the_startup_warning_alone() {
        let mut state = State::empty(Theme::classic());
        apply_configuration_warnings(&mut state, &[]);
        assert_eq!(state.startup_warning, None);
        let parsed = config::parse("splash = false # c\ntheme = \"nc-mono\" # c\n[mouse] # c\nenabled = true\n");
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    }

    #[test]
    fn spawn_editor_and_wait_succeeds_for_a_real_program() {
        let _serial = console::FLAG_LOCK.lock().unwrap();
        let mut child_run = console::ChildRunGuard::new();
        assert_eq!(spawn_editor_and_wait(&stub_invocation("0"), &mut child_run), Ok(()));
    }

    #[test]
    fn spawn_editor_and_wait_reports_an_unspawnable_program_as_an_error_without_panicking() {
        let inv = EditorInvocation {
            program: "filecommand-definitely-not-a-real-editor-binary".to_string(),
            leading_args: vec![],
            file_arg: OsString::from("report.txt"),
            cwd: std::env::temp_dir(),
        };
        let _serial = console::FLAG_LOCK.lock().unwrap();
        let mut child_run = console::ChildRunGuard::new();
        let result = spawn_editor_and_wait(&inv, &mut child_run);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("filecommand-definitely-not-a-real-editor-binary"));
    }

    // -- process-lifecycle-hardening: quit waits for job cleanup (task 2.3,
    // 2.4; application-shell "Quit waits for job cancellation cleanup";
    // design D3) ---------------------------------------------------------

    /// A `Runtime` with no terminal behind it, enough for `shutdown` and the
    /// effect bodies the tests here drive.
    fn test_runtime(dir: &Path) -> (Runtime, Receiver<Command>) {
        let (tx, rx) = mpsc::channel();
        let rt = Runtime {
            tx,
            active_job: None,
            cancels: CancelRegistry::default(),
            history_writer: HistoryWriter::disabled(),
            config_path: dir.join(CONFIG_FILE),
            usermenu_path: dir.join("usermenu.toml"),
            persist_enabled: false,
            viewer_source: None,
            clipboard: Box::new(clipboard::RecordingClipboard::default()),
            mouse_tracker: MouseTracker::new(),
            git_slots: [worker::GitQuerySlot::new(), worker::GitQuerySlot::new()],
            workers: worker::WorkerGauge::new(),
        };
        (rt, rx)
    }

    /// A stand-in job thread: `body` runs with the handle's cancel flag.
    fn fake_job(body: impl FnOnce(filecommand_core::fs_ops::CancelFlag) + Send + 'static) -> worker::JobHandle {
        let cancel = filecommand_core::fs_ops::CancelFlag::new();
        let (reply_tx, _reply_rx) = mpsc::channel();
        let thread_cancel = cancel.clone();
        let thread = std::thread::spawn(move || body(thread_cancel));
        worker::JobHandle { cancel, reply_tx, thread }
    }

    #[test]
    fn shutdown_waits_for_a_job_that_finishes_its_cleanup_after_the_cancel() {
        // The thread notices the flag, then takes 300 ms to clean up (the
        // partial-file removal) before ending: quit returns only after that.
        let job = fake_job(|cancel| {
            while !cancel.is_cancelled() {
                std::thread::sleep(Duration::from_millis(5));
            }
            std::thread::sleep(Duration::from_millis(300));
        });
        let started = Instant::now();
        assert!(shutdown_job(job, QUIT_JOB_WAIT), "the thread ended within the bound");
        let elapsed = started.elapsed();
        assert!(elapsed >= Duration::from_millis(300), "returned only after the cleanup finished, took {elapsed:?}");
        assert!(elapsed < QUIT_JOB_WAIT, "and well before the bound, took {elapsed:?}");
    }

    #[test]
    fn shutdown_gives_up_on_a_job_that_never_finishes_after_about_two_seconds() {
        // Scenario "The wait is bounded": a write blocked on an unresponsive
        // share never returns; quit exits anyway after `QUIT_JOB_WAIT`.
        let job = fake_job(|_cancel| std::thread::sleep(Duration::from_secs(20)));
        let started = Instant::now();
        assert!(!shutdown_job(job, QUIT_JOB_WAIT), "abandoned at the bound");
        let elapsed = started.elapsed();
        assert!(elapsed >= QUIT_JOB_WAIT, "took {elapsed:?}");
        assert!(elapsed < QUIT_JOB_WAIT + Duration::from_secs(1), "took {elapsed:?}");
    }

    #[test]
    fn shutdown_with_no_job_returns_immediately() {
        // Scenario "No job means no wait".
        let (mut rt, _rx) = test_runtime(&std::env::temp_dir());
        let started = Instant::now();
        shutdown(&mut rt).unwrap();
        assert!(started.elapsed() < Duration::from_millis(100), "took {:?}", started.elapsed());
    }

    #[test]
    fn shutdown_joins_a_job_that_has_already_finished_without_waiting() {
        let job = fake_job(|_cancel| {});
        // Let the thread end first.
        while !job.thread.is_finished() {
            std::thread::yield_now();
        }
        let started = Instant::now();
        assert!(shutdown_job(job, QUIT_JOB_WAIT));
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    /// Scenario "Quit during a large copy leaves no partial file" (task
    /// 2.4), end to end through the real `spawn_job` and `RealFs`: quit
    /// confirmed mid-copy sets the flag, the engine's cancel path removes
    /// the partially written destination, and `shutdown` returns only once
    /// it has. The copy source is a sparse 256 MB file so the test is quick
    /// to set up while the copy still takes long enough to be interrupted.
    #[cfg(windows)]
    #[test]
    fn shutdown_mid_copy_leaves_no_partial_destination_file() {
        use filecommand_core::fs_ops::{Job, JobKind, JobOutcome, SourceItem};

        let dir = std::env::temp_dir().join(format!("filecommand-tui-app-quit-copy-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let src_dir = dir.join("src");
        let dst_dir = dir.join("dst");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&dst_dir).unwrap();
        let src = src_dir.join("big.bin");
        std::fs::File::create(&src).unwrap().set_len(256 * 1024 * 1024).unwrap();
        let dst = dst_dir.join("big.bin");

        let (mut rt, rx) = test_runtime(&dir);
        let job = Job {
            kind: JobKind::Copy,
            sources: vec![SourceItem { original_name: OsString::from("big.bin"), path: src.clone(), is_dir: false }],
            source_dir: src_dir.clone(),
            dest_dir: dst_dir.clone(),
            new_dir_name: None,
        };
        rt.active_job = Some(worker::spawn_job(job, rt.tx.clone(), rt.workers.token()));

        // Mid-copy: since durable-file-copy the engine writes a sibling
        // `<name>.~fc-partial` file and renames it over the target only once
        // the copy is complete, so the partial file is what shows the copy
        // is in progress. If the final target appears first the copy already
        // finished and this run cannot exercise the scenario.
        let partial = dst_dir.join(format!("big.bin{}", filecommand_core::fs_ops::PARTIAL_SUFFIX));
        let deadline = Instant::now() + Duration::from_secs(30);
        while !partial.exists() {
            assert!(!dst.exists(), "the copy finished before it could be interrupted; enlarge the source");
            assert!(Instant::now() < deadline, "the copy never started");
            std::thread::sleep(Duration::from_millis(1));
        }

        let started = Instant::now();
        shutdown(&mut rt).unwrap();
        let elapsed = started.elapsed();

        assert!(!partial.exists(), "the partially written destination was removed before quit returned");
        assert!(!dst.exists(), "an interrupted copy never lands on the final name");
        assert!(elapsed < QUIT_JOB_WAIT, "the job ended within the bound rather than being abandoned, took {elapsed:?}");
        let outcomes: Vec<Command> = rx.try_iter().filter(|c| matches!(c, Command::JobDone { .. })).collect();
        assert!(
            matches!(outcomes.as_slice(), [Command::JobDone { outcome: JobOutcome::Cancelled { .. }, .. }]),
            "the job reported itself cancelled: {outcomes:?}"
        );
        assert!(std::fs::metadata(&src).is_ok(), "the source is untouched");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- error-diagnostics: status-line errors name the item (task 4.3) ---

    /// filesystem-error-handling "Error messages name the affected path":
    /// the viewer and editor open failures read `Cannot open <file>: <OS
    /// message>` (scenario "Viewer open failure names the file").
    #[test]
    fn cannot_message_formats_the_viewer_and_editor_open_failure() {
        let err = io::Error::new(io::ErrorKind::PermissionDenied, "Access is denied. (os error 5)");
        let path = Path::new("dir").join("locked.db");
        assert_eq!(cannot_message("open", &path, &err), "Cannot open locked.db: Access is denied. (os error 5)");
    }

    /// Scenario "Editor save failure names the file": `Cannot save <file>:
    /// <reason>`, where the reason may be the editor's own refusal text as
    /// well as an OS message.
    #[test]
    fn cannot_message_formats_the_editor_save_failure() {
        let err = io::Error::new(io::ErrorKind::StorageFull, "There is not enough space on the disk. (os error 112)");
        let path = Path::new("dir").join("notes.txt");
        assert_eq!(cannot_message("save", &path, &err), "Cannot save notes.txt: There is not enough space on the disk. (os error 112)");
        // A non-`io::Error` reason (the editor's own refusal) formats the
        // same way around it.
        assert_eq!(cannot_message("save", Path::new("notes.txt"), "notes.txt is read-only — not saved"), "Cannot save notes.txt: notes.txt is read-only — not saved");
    }

    /// open-with-associated-app design D1 / file-action-menu "Open with the
    /// associated application": the `ShellExecuteW` error-code mapping —
    /// 31 (`SE_ERR_NOASSOC`) names the missing association, 2
    /// (`ERROR_FILE_NOT_FOUND`) the missing target, anything else reports
    /// the raw code. Pure-value test; the constants' values are pinned by
    /// the Win32 API.
    #[cfg(windows)]
    #[test]
    fn open_with_association_error_maps_the_known_shell_execute_codes() {
        assert_eq!(open_with_association_error("report.xyz", 31), "No application is associated with report.xyz");
        assert_eq!(open_with_association_error("report.xyz", 2), "report.xyz not found");
        assert_eq!(open_with_association_error("report.xyz", 5), "Could not open report.xyz (error 5)");
    }

    #[test]
    fn scroll_lines_in_hex_mode_moves_by_whole_rows_and_never_underflows() {
        let viewer = {
            let mut v = ViewerState::new(PathBuf::from("f.bin"), 1000);
            v.mode = ViewMode::Hex;
            v.top_offset = 32;
            v
        };
        let src = temp_source("hex-mode", b"unused for hex-mode math, which touches no bytes");
        assert_eq!(scroll_lines(&viewer, &src, 1), 48);
        assert_eq!(scroll_lines(&viewer, &src, -2), 0);
    }

    #[test]
    fn scroll_lines_in_text_mode_walks_line_starts_forward_and_backward() {
        let src = temp_source("text-mode", b"line1\nline2\nline3\n");
        let mut viewer = ViewerState::new(PathBuf::from("f.txt"), src.len());
        viewer.top_offset = 0;
        assert_eq!(scroll_lines(&viewer, &src, 1), 6);
        assert_eq!(scroll_lines(&viewer, &src, 2), 12);
        viewer.top_offset = 12;
        assert_eq!(scroll_lines(&viewer, &src, -1), 6);
    }

    #[test]
    fn scroll_to_end_lands_on_the_files_last_page() {
        let src = temp_source("scroll-to-end", b"a\nb\nc\nd\ne\n");
        let viewer = ViewerState::new(PathBuf::from("f.txt"), src.len());
        // 5 two-byte lines; asking for the last 2 rows lands on "d\n".
        assert_eq!(scroll_to_end(&viewer, &src, 2), 6);
    }

    fn temp_source(name: &str, contents: &[u8]) -> ByteSource {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("filecommand-tui-app-viewer-nav-test-{}-{}", std::process::id(), name));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("file.bin");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(contents).unwrap();
        f.flush().unwrap();
        ByteSource::open(&path).unwrap()
    }

    // -----------------------------------------------------------------
    // idle-aware-event-loop: the poll-timeout choice (design D2/D3; task
    // 2.3) and the key-burst drain (design D5; task 2.7)
    // -----------------------------------------------------------------

    #[test]
    fn next_timeout_is_zero_with_a_drain_backlog() {
        assert_eq!(next_timeout(1_000, Some(1_033), Duration::from_millis(60_000), true, true), Duration::ZERO);
        assert_eq!(next_timeout(1_000, None, Duration::from_millis(60_000), false, true), Duration::ZERO);
    }

    #[test]
    fn next_timeout_is_capped_at_the_poll_interval_while_workers_are_alive() {
        // No deadlines at all: the worker cadence governs (design D3).
        assert_eq!(next_timeout(1_000, None, Duration::from_millis(60_000), true, false), POLL_INTERVAL);
        // A nearer deadline wins over the cap; a farther one does not raise it.
        assert_eq!(next_timeout(1_000, Some(1_010), Duration::from_millis(60_000), true, false), Duration::from_millis(10));
        assert_eq!(next_timeout(1_000, Some(5_000), Duration::from_millis(60_000), true, false), POLL_INTERVAL);
    }

    #[test]
    fn next_timeout_is_the_earliest_deadline_with_no_workers() {
        // The minute boundary alone bounds the sleep (application-shell
        // "Header clock still advances while idle").
        assert_eq!(next_timeout(1_000, None, Duration::from_millis(42_000), false, false), Duration::from_millis(42_000));
        // A core deadline ahead of it wins.
        assert_eq!(next_timeout(1_000, Some(1_033), Duration::from_millis(42_000), false, false), Duration::from_millis(33));
        // A deadline already in the past is due now.
        assert_eq!(next_timeout(1_000, Some(900), Duration::from_millis(42_000), false, false), Duration::ZERO);
    }

    /// A scripted `EventSource`: `poll` reports "ready" while the queue is
    /// non-empty, `read` pops it.
    struct ScriptedEvents {
        queue: std::collections::VecDeque<Event>,
    }

    impl ScriptedEvents {
        fn of(events: impl IntoIterator<Item = Event>) -> ScriptedEvents {
            ScriptedEvents { queue: events.into_iter().collect() }
        }

        fn remaining(&self) -> usize {
            self.queue.len()
        }
    }

    impl EventSource for ScriptedEvents {
        fn poll(&mut self, _timeout: Duration) -> io::Result<bool> {
            Ok(!self.queue.is_empty())
        }
        fn read(&mut self) -> io::Result<Event> {
            Ok(self.queue.pop_front().expect("poll reported an event ready"))
        }
    }

    /// A `BurstHandler` that records the mapped commands and applies them
    /// through the real reducer — no terminal, and effects reduced to the
    /// quit flag (a headless test cannot run a `TerminalGuard`).
    struct RecordingHandler {
        keys: config::Keys,
        applied: Vec<Command>,
    }

    impl BurstHandler for RecordingHandler {
        fn map_key(&mut self, key: KeyEvent, state: &State) -> Option<Command> {
            map_pressed_key(key, state, &self.keys, None)
        }
        fn apply(&mut self, state: State, cmd: Command) -> io::Result<(State, bool)> {
            self.applied.push(cmd.clone());
            let (state, effects) = update(state, cmd);
            Ok((state, effects.contains(&Effect::Quit)))
        }
    }

    fn press(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    /// The pre-burst-loop behavior, as the reference the burst must match:
    /// map each key against the state the previous one produced, apply,
    /// repeat (application-shell "Bursts of key input render once":
    /// "Applying a burst SHALL produce the same final state as applying the
    /// same keys one at a time").
    fn apply_one_at_a_time(mut state: State, codes: &[KeyCode]) -> (State, Vec<Command>) {
        let mut applied = Vec::new();
        for &code in codes {
            if let Some(cmd) = map_pressed_key(KeyEvent::new(code, KeyModifiers::NONE), &state, &config::Keys::default(), None) {
                applied.push(cmd.clone());
                let (s, _) = update(state, cmd);
                state = s;
            }
        }
        (state, applied)
    }

    fn burst(initial: State, first: KeyCode, rest: Vec<Event>) -> (State, bool, bool, Vec<Command>, ScriptedEvents, Option<Event>) {
        let mut events = ScriptedEvents::of(rest);
        let mut pending_event = None;
        let mut handler = RecordingHandler { keys: config::Keys::default(), applied: Vec::new() };
        let (state, quit, applied) =
            drain_key_burst(KeyEvent::new(first, KeyModifiers::NONE), initial, &mut events, &mut pending_event, &mut handler).unwrap();
        (state, quit, applied, handler.applied, events, pending_event)
    }

    #[test]
    fn a_held_page_down_burst_applies_every_key_and_draws_once() {
        // application-shell "Holding PgDn renders one frame per burst": five
        // queued PgDn presses apply five page movements, the queue empties
        // in one drain (one loop iteration, hence one draw), and the final
        // state matches one-at-a-time application.
        let mut state = State::empty(Theme::classic());
        state.term_size = (80, 24);
        let entries: Vec<_> = (0..500).map(|i| filecommand_core::listing::Entry {
            name: OsString::from(format!("f{i:03}.txt")),
            kind: filecommand_core::listing::EntryKind::File,
            size: 1,
            modified: None,
            hidden: false,
            link: false,
        }).collect();
        state.left.listing_request = Some(1);
        let (s, _) = update(state, Command::ListingChunk { panel: PanelSide::Left, entries, request: 1 });
        state = s;

        let keys = [KeyCode::PageDown; 5];
        let (burst_state, quit, applied, commands, events, pending) = burst(state.clone(), keys[0], keys[1..].iter().map(|&c| press(c)).collect());
        assert!(!quit);
        assert!(applied, "the burst applied commands, so the loop draws once after it");
        assert_eq!(events.remaining(), 0, "the whole burst drained in one call");
        assert!(pending.is_none());
        assert_eq!(commands.len(), 5, "every queued key was applied");

        let (reference_state, reference_commands) = apply_one_at_a_time(state, &keys);
        assert_eq!(commands, reference_commands);
        assert_eq!(burst_state, reference_state, "burst and one-at-a-time produce the same state");
        assert!(burst_state.left.cursor > 0, "the cursor actually paged down");
    }

    #[test]
    fn burst_routing_follows_the_state_each_key_produces() {
        // application-shell "Routing follows the state each key produces":
        // F7 opens the Make-directory dialog, `new` types into its field,
        // Enter confirms it — exactly as if the keys had been pressed
        // slowly. Mapping the burst against the *original* state would
        // route the characters to the command line and Enter to
        // `Command::Enter` instead of `FileOpConfirm`.
        let state = State::empty(Theme::classic());
        let rest = vec![press(KeyCode::Char('n')), press(KeyCode::Char('e')), press(KeyCode::Char('w')), press(KeyCode::Enter)];
        let (burst_state, quit, applied, commands, events, pending) = burst(state.clone(), KeyCode::F(7), rest);
        assert!(!quit && applied && pending.is_none() && events.remaining() == 0);
        assert_eq!(
            commands,
            vec![
                Command::RequestMkdir,
                Command::TextEdit(filecommand_core::line_edit::TextEditOp::Insert('n')),
                Command::TextEdit(filecommand_core::line_edit::TextEditOp::Insert('e')),
                Command::TextEdit(filecommand_core::line_edit::TextEditOp::Insert('w')),
                Command::FileOpConfirm,
            ],
            "each key mapped against the state the previous one produced"
        );
        let (reference_state, _) = apply_one_at_a_time(state, &[KeyCode::F(7), KeyCode::Char('n'), KeyCode::Char('e'), KeyCode::Char('w'), KeyCode::Enter]);
        assert_eq!(burst_state, reference_state);
        assert!(
            matches!(burst_state.phase, UiPhase::FileOpSetup(filecommand_core::fs_ops::dialog::FileOpSetup::DestinationInput { .. }) | UiPhase::FileOpRunning { .. }),
            "the dialog opened and was confirmed: {:?}",
            burst_state.phase
        );
    }

    #[test]
    fn a_burst_past_the_event_cap_stops_and_draws_before_continuing() {
        // application-shell "A paste delivered as key events renders once
        // per burst": 70 queued characters apply 64, then the drain returns
        // so the loop draws before the remaining 6 are read next iteration.
        let state = State::empty(Theme::classic());
        let rest: Vec<Event> = (0..69).map(|_| press(KeyCode::Char('x'))).collect();
        let (state, quit, applied, commands, events, _) = burst(state, KeyCode::Char('x'), rest);
        assert!(!quit && applied);
        assert_eq!(commands.len(), BURST_MAX_EVENTS, "the burst stops at the cap");
        assert_eq!(events.remaining(), 70 - BURST_MAX_EVENTS);
        assert_eq!(state.command_line.text().len(), BURST_MAX_EVENTS, "all 64 characters landed in the command line");
    }

    #[test]
    fn a_mouse_event_ends_the_burst_and_is_stashed_for_the_next_iteration() {
        // design D5: the mouse-batch path takes the stashed event next
        // iteration.
        let state = State::empty(Theme::classic());
        let mouse = Event::Mouse(crossterm::event::MouseEvent { kind: MouseEventKind::ScrollDown, column: 5, row: 5, modifiers: KeyModifiers::NONE });
        let (_state, quit, applied, commands, events, pending) = burst(state, KeyCode::PageDown, vec![mouse]);
        assert!(!quit && applied);
        assert_eq!(commands.len(), 1, "only the first key applied before the mouse event");
        assert_eq!(events.remaining(), 0);
        assert!(matches!(pending, Some(Event::Mouse(_))), "the mouse event is stashed, not lost or applied mid-burst");
    }
}
