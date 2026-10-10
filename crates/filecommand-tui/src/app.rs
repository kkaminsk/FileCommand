//! The event loop: drains crossterm input + worker events, converts them to
//! `Command`s, applies `core::update`, executes the returned effects, and
//! redraws. This is the only place effects are actually performed.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, SystemTime};

use crossterm::event::{self, Event, KeyEventKind, MouseEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

use filecommand_core::clock::{format_clock, Clock, WallClock};
use filecommand_core::editor::{EditorMove, EditorState, LoadResult as EditorLoadResult};
use filecommand_core::external_editor::EditorInvocation;
use filecommand_core::listing::DateTime;
use filecommand_core::shell::{Invocation, ShellConfig};
use filecommand_core::theme::{ColorDepth, Theme, BUILTIN_THEME_NAMES};
use filecommand_core::update::ClipboardPayloadKind;
use filecommand_core::viewer::hex::HEX_BYTES_PER_ROW;
use filecommand_core::viewer::{backward, forward, ByteSource, ViewMode, ViewerState};
use filecommand_core::{config, drives, identity, update, Command, Effect, PanelSide, State, UiPhase};

use crate::clipboard::{self, Clipboard, PlatformClipboard};
use crate::clock::{RealClock, RealWallClock};
use crate::hitmap::HitMap;
use crate::input::{self, MouseTracker, ViewerInput};
use crate::terminal::TerminalGuard;
use crate::views;
use crate::worker;

const POLL_INTERVAL: Duration = Duration::from_millis(33);
const CONFIG_FILE: &str = "config.toml";
const RESUME_PROMPT: &str = "Press any key to return to FileCommand . . .";

/// Everything the effect executor needs that isn't part of `core::State`:
/// channels, the terminal, and the paths persistence writes to.
struct Runtime {
    tx: Sender<Command>,
    active_job: Option<worker::JobHandle>,
    history_path: PathBuf,
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
    crate::terminal::install_panic_hook();
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
    let config = config::load(&config_path);
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

    let (mut state, effects) = State::initial(theme, term_size, clock.now_ms(), cwd.clone(), cwd, show_splash);
    apply_config(&mut state, &config);
    apply_launch_options(&mut state, &launch);
    // A `--config-dir` switch with no value and the last-resort cwd
    // fallback are session-only, in-memory warnings — same convention as
    // the theme override warning just below.
    if let Some(warning) = flag_warning {
        state.startup_warning = Some(warning);
    }
    if let Some(warning) = resolve_warning {
        state.startup_warning = Some(warning);
    }
    // Session-only: this only ever sets `state.startup_warning` in memory,
    // never touches `config.toml` (theme-selection "Override is
    // session-only"; design D4). `apply_user_menu` below may overwrite this
    // with its own warning if both fire at once — matching the pre-existing
    // single-`startup_warning`-field behavior, not a new decision here.
    if let Some(warning) = theme_warning {
        state.startup_warning = Some(warning);
    }
    let history_file = config::load_history_file(&history_path);
    state.history = history_file.commands;
    state.dir_history = history_file.directories;
    let user_menu = config::load_user_menu(&usermenu_path);
    apply_user_menu(&mut state, user_menu);
    // Commands → Menu file edit reads the path back out of `State`
    // (pulldown-menu-completion design D4).
    state.user_menu_path = usermenu_path.clone();
    // The legacy notice and the unwritable-profile warning write last, so
    // the persistence-breaking one (design D2) stays visible when several
    // warnings fire at once; still the same single-field, last-writer-wins
    // behavior (config-profile-directory design D3).
    if let Some(warning) = legacy_warning {
        state.startup_warning = Some(warning);
    }
    if let Some(warning) = create_warning {
        state.startup_warning = Some(warning);
    }
    stamp_startup_warning(&mut state);

    let (tx, rx) = mpsc::channel::<Command>();
    let mut rt = Runtime {
        tx,
        active_job: None,
        history_path,
        config_path,
        usermenu_path,
        persist_enabled,
        viewer_source: None,
        clipboard: Box::new(PlatformClipboard::default()),
        mouse_tracker: MouseTracker::new(),
    };

    if run_effects(effects, &mut guard, &mut rt)? {
        return Ok(());
    }
    let (mut state, _) = drain_events(state, &rx, &mut guard, &mut rt)?;
    let mut hitmap = draw(&mut guard, &state, &identity_lines, &wall_clock, rt.viewer_source.as_ref().map(|(_, s)| s))?;

    // A non-mouse event read while draining a burst of mouse events (design
    // D7) is stashed here rather than lost — crossterm has no way to push an
    // event back onto its queue, so the next loop iteration consumes this
    // instead of polling again.
    let mut pending_event: Option<Event> = None;

    loop {
        let (s, mut dirty) = drain_events(state, &rx, &mut guard, &mut rt)?;
        state = s;

        let next_event = match pending_event.take() {
            Some(e) => Some(e),
            None if event::poll(POLL_INTERVAL)? => Some(event::read()?),
            None => None,
        };

        match next_event {
            Some(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                // The quit-confirmation dialog outranks the viewer/editor's
                // own direct-dispatch key routing: once it is up, every key
                // goes through `map_key` (which checks `quit_confirm`
                // first), regardless of `state.phase` — the same way it
                // outranks every other overlay inside `map_key` itself
                // (application-shell "Quit request keys and
                // confirmation"; design D5).
                let cmd = if state.quit_confirm {
                    let page_size = active_panel_page_size(&state);
                    input::map_key(key, &state, page_size, &keys)
                } else {
                    match &state.phase {
                        UiPhase::Viewer(viewer) => {
                            let rows_visible = views::viewer::body_rows(state.term_size.1);
                            let source = rt.viewer_source.as_ref().map(|(_, s)| s);
                            input::map_viewer_key(key, viewer, rows_visible)
                                .and_then(|action| resolve_viewer_navigation(viewer, source, action, rows_visible))
                        }
                        UiPhase::Editor(editor) => {
                            let rows_visible = views::editor::body_rows(state.term_size.1);
                            input::map_editor_key(key, editor, rows_visible)
                        }
                        _ => {
                            let page_size = active_panel_page_size(&state);
                            input::map_key(key, &state, page_size, &keys)
                        }
                    }
                };
                match cmd {
                    Some(cmd) => {
                        let (s, quit) = apply(state, cmd, &mut guard, &mut rt)?;
                        if quit {
                            return Ok(());
                        }
                        state = s;
                        dirty = true;
                    }
                    None => continue,
                }
            }
            Some(Event::Resize(w, h)) => {
                let (s, quit) = apply(state, Command::Resize(w, h), &mut guard, &mut rt)?;
                if quit {
                    return Ok(());
                }
                state = s;
                dirty = true;
            }
            Some(Event::Mouse(first)) => {
                // Drain every mouse event already queued this frame after
                // the first, so a burst of wheel/drag events causes at most
                // one redraw (mouse-input "Mouse events are coalesced";
                // design D7). A non-mouse event read mid-drain is stashed in
                // `pending_event` rather than lost.
                let mut batch = vec![first];
                while event::poll(Duration::ZERO)? {
                    match event::read()? {
                        Event::Mouse(m) => batch.push(m),
                        other => {
                            pending_event = Some(other);
                            break;
                        }
                    }
                }
                let (s, quit, batch_dirty) = apply_mouse_batch(state, batch, &hitmap, &mut guard, &mut rt)?;
                if quit {
                    return Ok(());
                }
                state = s;
                dirty = dirty || batch_dirty;
            }
            Some(_) => {}
            None => {
                let (s, quit) = apply(state, Command::Tick(clock.now_ms()), &mut guard, &mut rt)?;
                if quit {
                    return Ok(());
                }
                state = s;
                dirty = true;
            }
        }

        if dirty {
            // Effects that feed a command straight back (drive enumeration)
            // must land before the frame, so the dialog paints complete on
            // its very first appearance rather than a frame later.
            let (s, _) = drain_events(state, &rx, &mut guard, &mut rt)?;
            state = s;
            hitmap = draw(&mut guard, &state, &identity_lines, &wall_clock, rt.viewer_source.as_ref().map(|(_, s)| s))?;
        }
    }
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
        match &state.phase {
            UiPhase::Viewer(_) => match m.kind {
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
            UiPhase::Editor(_) => match m.kind {
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
        state.startup_warning = Some(format!("{} is malformed; F2 uses the default user menu", config::USERMENU_FILE));
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
        rt.viewer_source = None;
    }
    // Commands → Menu file edit, built-in-editor leg: remember whether the
    // editor currently has the user-menu file open, so the transition out of
    // `UiPhase::Editor` (any of the quit paths, save-on-exit included)
    // triggers the reload below (user-menu "Edit the user menu file from the
    // Commands menu"; design D4).
    let editing_user_menu = matches!(&state.phase, UiPhase::Editor(editor) if editor.path == rt.usermenu_path);
    let (state, effects) = update(state, cmd);
    let quit = run_effects(effects, guard, rt)?;
    if editing_user_menu && !matches!(state.phase, UiPhase::Editor(_)) {
        reload_user_menu(rt);
    }
    Ok((state, quit))
}

/// Re-read `usermenu.toml` and feed the parse result back as
/// `Command::UserMenuReloaded` (design D4) — drained before the next frame,
/// exactly like the `RereadPanel` an external editor's return sends.
fn reload_user_menu(rt: &Runtime) {
    let _ = rt.tx.send(Command::UserMenuReloaded(config::load_user_menu(&rt.usermenu_path)));
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
            Effect::StartListing { panel, path } => worker::spawn_listing(panel, path, rt.tx.clone()),
            Effect::Quit => quit = true,
            Effect::RunJob(job) => rt.active_job = Some(worker::spawn_job(job, rt.tx.clone())),
            Effect::CancelJob => {
                if let Some(handle) = &rt.active_job {
                    handle.cancel.cancel();
                }
            }
            Effect::SendConflictReply(choice) => {
                if let Some(handle) = &rt.active_job {
                    let _ = handle.reply_tx.send(worker::JobReply::Conflict(choice));
                }
            }
            Effect::SendErrorReply(choice) => {
                if let Some(handle) = &rt.active_job {
                    let _ = handle.reply_tx.send(worker::JobReply::Error(choice));
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
                let _ = rt.tx.send(Command::RereadPanel(side));
            }
            Effect::OpenWithAssociation { path, cwd } => {
                // Fire-and-forget (open-with-associated-app design D1/D3):
                // no suspend, no wait, no mouse-tracker reset, no panel
                // re-read. Only a launch failure reports back, as the
                // command that lands the message on the panel's status line.
                if let Err(message) = open_with_association(&path, &cwd) {
                    let _ = rt.tx.send(Command::OpenWithAssociationFailed { message });
                }
            }
            Effect::ShowScrollback => {
                show_scrollback(guard)?;
                rt.mouse_tracker.reset();
            }
            Effect::PersistHistory(file) => {
                // A history write failing (read-only directory, full disk)
                // must never take the session down with it. Skipped
                // entirely when the profile directory is unwritable this
                // session (config-profile-directory; design D2).
                if rt.persist_enabled {
                    let _ = config::save_history_file_atomic(&rt.history_path, &file);
                }
            }
            Effect::PersistTheme(name) => {
                // Same tolerance as history: a failed config write must
                // never take the session down (the theme is already active
                // in memory either way); likewise skipped when persistence
                // is off for the session (design D2).
                if rt.persist_enabled {
                    let _ = config::save_theme_atomic(&rt.config_path, &name);
                }
            }
            Effect::PersistPanelSplit(percent) => {
                // Same tolerance as the theme write above (panel-split
                // "Split persistence to configuration"; design D2).
                if rt.persist_enabled {
                    let _ = config::save_panel_split_atomic(&rt.config_path, percent);
                }
            }
            Effect::EnumerateDrives(target) => {
                // Cheap enough for the input path: a bitmask read, no media
                // or network probing. Fed straight back so the dialog's
                // first painted frame already lists every letter.
                let _ = rt.tx.send(Command::DriveListReady { target, drives: drives::enumerate_drives() });
            }
            Effect::FetchDriveLabel { target, letter, generation } => worker::spawn_drive_label(target, letter, generation, rt.tx.clone()),
            Effect::QueryInfo { panel, path, request } => worker::spawn_info_query(panel, path, request, rt.tx.clone()),
            Effect::QueryGitInfo { panel, path, request } => worker::spawn_git_info_query(panel, path, request, rt.tx.clone()),
            Effect::ExpandTreeNode { panel, path } => worker::spawn_tree_expand(panel, path, rt.tx.clone()),
            Effect::OpenViewer { path } => match ByteSource::open(&path) {
                Ok(source) => {
                    // O(1): opening/mapping touches only the handle and its
                    // length, never the content (viewer: Instant open), so
                    // this runs synchronously rather than on a worker
                    // thread, exactly like `EnumerateDrives`.
                    let file_len = source.len();
                    rt.viewer_source = Some((path.clone(), source));
                    let _ = rt.tx.send(Command::ViewerOpened { path, file_len });
                }
                Err(e) => {
                    let _ = rt.tx.send(Command::ViewerOpenFailed { message: e.to_string() });
                }
            },
            Effect::RunViewerSearch { path, start_offset, pattern, request } => {
                worker::spawn_viewer_search(path, start_offset, pattern, request, rt.tx.clone());
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
                        let _ = rt.tx.send(Command::RereadPanel(side));
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
                        let _ = rt.tx.send(Command::ExternalEditorSpawnFailed { message });
                    }
                }
            }
            Effect::OpenEditor { path } => match EditorState::open(&path) {
                // Synchronous, like `Effect::OpenViewer`: the whole read is
                // bounded by the 10 MB cap, so this is cheap enough for the
                // input path rather than warranting a worker thread (design
                // D1).
                Ok(EditorLoadResult::Loaded(editor)) => {
                    let _ = rt.tx.send(Command::EditorOpened(Box::new(editor)));
                }
                Ok(EditorLoadResult::TooLarge { size }) => {
                    let _ = rt.tx.send(Command::EditorTooLarge { path, size });
                }
                Err(e) => {
                    let _ = rt.tx.send(Command::EditorOpenFailed { message: e.to_string() });
                }
            },
            Effect::FindInSubtree { root, pattern, request } => {
                worker::spawn_find_subtree(root, pattern, request, rt.tx.clone());
            }
            Effect::SaveEditor { editor, then_quit } => {
                let mut editor = *editor;
                match editor.save() {
                    Ok(()) => {
                        let _ = rt.tx.send(Command::EditorSaved { editor: Box::new(editor), then_quit });
                    }
                    Err(e) => {
                        let _ = rt.tx.send(Command::EditorSaveFailed { message: e.to_string() });
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
                let _ = rt.tx.send(Command::ClipboardResult { payload: normalized_payload, fell_back_to_paths, result });
            }
            Effect::ReadClipboardText => {
                // The read half of paste (text-field-editing "Paste into a
                // text field"; design D4): clipboard text comes back as
                // `TextEdit(Paste(..))` for whichever field owns the
                // keyboard when it arrives. Line breaks flatten to a single
                // space here (`LineEdit::insert_str` does the same again,
                // so the core behavior never depends on this path), and a
                // clipboard with no text feeds nothing back — the key is
                // then a no-op ("Empty clipboard is a no-op").
                if let Some(text) = rt.clipboard.get_text() {
                    let text = text.replace("\r\n", " ").replace('\r', " ").replace('\n', " ");
                    if !text.is_empty() {
                        let _ = rt.tx.send(Command::TextEdit(filecommand_core::line_edit::TextEditOp::Paste(text)));
                    }
                }
            }
        }
    }
    Ok(quit)
}

/// Suspend the TUI, run the command on the real terminal, and come back.
///
/// Restore runs on every path — a spawn failure, a non-zero exit, or a
/// child that scribbled over the screen — because `resume` is idempotent
/// and the panic hook covers the rest.
fn run_shell_command(guard: &mut TerminalGuard, invocation: &Invocation) -> io::Result<()> {
    guard.suspend()?;
    let status = std::process::Command::new(&invocation.program)
        .args(&invocation.args)
        .current_dir(&invocation.cwd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();
    if let Err(e) = &status {
        // The message lands in the scrollback the user is about to see.
        let _ = writeln!(io::stdout(), "\r\nCould not run `{}`: {e}", invocation.program);
    }
    wait_for_key()?;
    guard.resume()
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
    guard.suspend()?;
    let result = spawn_editor_and_wait(invocation);
    if let Err(message) = &result {
        let _ = writeln!(io::stdout(), "\r\n{message}");
    }
    wait_for_key()?;
    guard.resume()?;
    Ok(result)
}

/// The process-spawn half of [`run_external_editor`], factored out so it is
/// testable without a real console (`guard.suspend`/`resume` need one; this
/// does not). `Ok(())` on any exit (including non-zero — matching
/// `run_shell_command`'s precedent of not treating the child's exit code as
/// FileCommand's own error); `Err` only when the process could not be
/// spawned at all (external-editor: Editor spawn errors do not crash the
/// app).
fn spawn_editor_and_wait(invocation: &EditorInvocation) -> Result<(), String> {
    std::process::Command::new(&invocation.program)
        .args(&invocation.leading_args)
        .arg(&invocation.file_arg)
        .current_dir(&invocation.cwd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
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

    /// Regression test for the F4 external editor being silently dead: a
    /// config-driven startup (via `config::parse`, the real parsing path,
    /// not a hand-built `Config`) must populate `state.editor`, mirroring
    /// the already-wired `state.shell`. Before this fix, `run()` never
    /// assigned `state.editor` at all, so `editor =` in `config.toml` was
    /// parsed correctly but never reached `State`.
    #[test]
    fn apply_config_wires_the_configured_editor_and_shell_into_state() {
        let config = config::parse("editor = \"code --wait\"\nshell = \"pwsh -NoLogo -Command\"\n");
        let mut state = State::empty(Theme::classic());
        assert_eq!(state.editor, None, "sanity: editor starts unset before wiring");
        apply_config(&mut state, &config);
        assert_eq!(state.editor.as_deref(), Some("code --wait"));
        assert_eq!(state.shell.shell.as_deref(), Some("pwsh -NoLogo -Command"));
    }

    #[test]
    fn apply_config_wires_the_configured_panel_split_into_state() {
        let config = config::parse("panel_split = 66\n");
        let mut state = State::empty(Theme::classic());
        assert_eq!(state.split_percent, 50, "sanity: split starts at the default before wiring");
        apply_config(&mut state, &config);
        assert_eq!(state.split_percent, 66);
    }

    #[test]
    fn apply_config_leaves_editor_unset_when_config_omits_it() {
        let config = config::parse("splash = false\n");
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
        apply_config(&mut state, &config::parse(""));
        assert!(state.animations, "default is animated");
        apply_config(&mut state, &config::parse("animations = false\n"));
        assert!(!state.animations, "animations = false disables them");
    }

    /// ui-animations "Flag overrides config": `--noanimations` wins over
    /// `animations = true`, and without the flag the config value stands.
    #[test]
    fn noanimations_flag_overrides_the_config_value() {
        let on = config::parse("animations = true\n");
        let mut state = State::empty(Theme::classic());
        apply_config(&mut state, &on);
        apply_launch_options(&mut state, &parse_launch_args(["--noanimations".to_string()]));
        assert!(!state.animations, "the flag wins over animations = true");

        let mut state = State::empty(Theme::classic());
        apply_config(&mut state, &on);
        apply_launch_options(&mut state, &parse_launch_args(Vec::<String>::new()));
        assert!(state.animations, "no flag leaves the config value alone");

        let mut state = State::empty(Theme::classic());
        apply_config(&mut state, &config::parse("animations = false\n"));
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
        let config = config::parse("theme = \"no-such-theme\"\n");
        let (theme, warning) = resolve_startup_theme(&config, None);
        assert_eq!(theme.name, Theme::classic().name);
        assert_eq!(warning, None, "no --theme switch was passed, so no warning is raised");
    }

    /// Same scenario, unset case: an omitted `theme` key already defaults to
    /// `nc-classic` in `Config::default`, so this exercises the full
    /// unset -> load -> resolve path end to end.
    #[test]
    fn resolve_startup_theme_falls_back_to_nc_classic_when_theme_is_unset() {
        let config = config::parse("splash = false\n");
        let (theme, warning) = resolve_startup_theme(&config, None);
        assert_eq!(theme.name, Theme::classic().name);
        assert_eq!(warning, None);
    }

    /// Sanity check that a known configured theme round-trips through
    /// `resolve_startup_theme` unchanged rather than always collapsing to
    /// the fallback.
    #[test]
    fn resolve_startup_theme_loads_a_known_configured_theme() {
        let config = config::parse("theme = \"terminal-green\"\n");
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
        let config = config::parse("theme = \"nc-classic\"\n");
        let (theme, warning) = resolve_startup_theme(&config, Some("yellow-storm"));
        assert_eq!(theme.name, Theme::yellow_storm().name);
        assert_eq!(warning, None, "a valid override produces no warning");
    }

    /// theme-selection "Unknown name warns and falls back": an unrecognized
    /// override falls through to the configured theme and produces a
    /// warning naming the rejected value and listing the valid names.
    #[test]
    fn resolve_startup_theme_unknown_override_falls_back_with_a_warning() {
        let config = config::parse("theme = \"nc-mono\"\n");
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
        let config = config::parse("theme = \"nc-mono\"\n");
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
        let config = config::parse("theme = \"nc-classic\"\n");
        let (theme, warning) = resolve_startup_theme(&config, Some("terminal-green"));
        assert_eq!(theme.name, Theme::terminal_green().name);
        assert_eq!(warning, None);

        let (state, _) = State::initial(theme, (80, 24), 0, PathBuf::from("."), PathBuf::from("."), false);
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
        let ok = config::UserMenuLoadResult { entries: entries.clone(), warnings: Vec::new(), created_default: false, malformed: false };
        apply_user_menu(&mut state, ok);
        assert_eq!(state.user_menu_entries, entries);
        assert_eq!(state.startup_warning, None);
    }

    #[test]
    fn spawn_editor_and_wait_succeeds_for_a_real_program() {
        assert_eq!(spawn_editor_and_wait(&stub_invocation("0")), Ok(()));
    }

    #[test]
    fn spawn_editor_and_wait_reports_an_unspawnable_program_as_an_error_without_panicking() {
        let inv = EditorInvocation {
            program: "filecommand-definitely-not-a-real-editor-binary".to_string(),
            leading_args: vec![],
            file_arg: OsString::from("report.txt"),
            cwd: std::env::temp_dir(),
        };
        let result = spawn_editor_and_wait(&inv);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("filecommand-definitely-not-a-real-editor-binary"));
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
}
