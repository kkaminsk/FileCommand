//! The event loop: drains crossterm input + worker events, converts them to
//! `Command`s, applies `core::update`, executes the returned effects, and
//! redraws. This is the only place effects are actually performed.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, SystemTime};

use crossterm::event::{self, Event, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

use filecommand_core::clock::{format_clock, Clock, WallClock};
use filecommand_core::editor::{EditorState, LoadResult as EditorLoadResult};
use filecommand_core::external_editor::EditorInvocation;
use filecommand_core::listing::DateTime;
use filecommand_core::shell::{Invocation, ShellConfig};
use filecommand_core::theme::{ColorDepth, Theme};
use filecommand_core::viewer::hex::HEX_BYTES_PER_ROW;
use filecommand_core::viewer::{backward, forward, ByteSource, ViewMode, ViewerState};
use filecommand_core::{config, drives, identity, update, Command, Effect, State, UiPhase};

use crate::clock::{RealClock, RealWallClock};
use crate::input::{self, ViewerInput};
use crate::layout;
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
    /// The F3 viewer's open byte window, kept here rather than in
    /// `core::State` because `ByteSource` is an I/O handle, not application
    /// state (design D1). Set on `Effect::OpenViewer`, cleared on
    /// `Command::ViewerClose` — the only path out of `UiPhase::Viewer`.
    viewer_source: Option<(PathBuf, ByteSource)>,
}

pub fn run(no_splash_flag: bool) -> io::Result<()> {
    crate::terminal::install_panic_hook();
    let mut guard = TerminalGuard::new()?;

    let clock = RealClock::new();
    let wall_clock = RealWallClock;
    let config = config::load(Path::new(CONFIG_FILE));
    let theme = Theme::by_name(&config.theme).unwrap_or_else(Theme::classic);
    let show_splash = config.splash && !no_splash_flag;
    let keys = config.keys.clone();

    let size = guard.terminal.size()?;
    let term_size = (size.width, size.height);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let year = DateTime::from_system_time(SystemTime::now()).year;
    let identity_lines = identity::identity_lines(year);

    let (mut state, effects) = State::initial(theme, term_size, clock.now_ms(), cwd.clone(), cwd, show_splash);
    apply_config(&mut state, &config);
    let history_file = config::load_history_file(Path::new(config::HISTORY_FILE));
    state.history = history_file.commands;
    state.dir_history = history_file.directories;
    let user_menu = config::load_user_menu(Path::new(config::USERMENU_FILE));
    apply_user_menu(&mut state, user_menu);

    let (tx, rx) = mpsc::channel::<Command>();
    let mut rt = Runtime { tx, active_job: None, history_path: PathBuf::from(config::HISTORY_FILE), viewer_source: None };

    if run_effects(effects, &mut guard, &mut rt)? {
        return Ok(());
    }
    let (mut state, _) = drain_events(state, &rx, &mut guard, &mut rt)?;
    draw(&mut guard, &state, &identity_lines, &wall_clock, rt.viewer_source.as_ref().map(|(_, s)| s))?;

    loop {
        let (s, mut dirty) = drain_events(state, &rx, &mut guard, &mut rt)?;
        state = s;

        if event::poll(POLL_INTERVAL)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    let cmd = match &state.phase {
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
                            let page_size = layout::compute(state.term_size).entries_visible;
                            input::map_key(key, &state, page_size, &keys)
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
                Event::Resize(w, h) => {
                    let (s, quit) = apply(state, Command::Resize(w, h), &mut guard, &mut rt)?;
                    if quit {
                        return Ok(());
                    }
                    state = s;
                    dirty = true;
                }
                _ => {}
            }
        } else {
            let (s, quit) = apply(state, Command::Tick(clock.now_ms()), &mut guard, &mut rt)?;
            if quit {
                return Ok(());
            }
            state = s;
            dirty = true;
        }

        if dirty {
            // Effects that feed a command straight back (drive enumeration)
            // must land before the frame, so the dialog paints complete on
            // its very first appearance rather than a frame later.
            let (s, _) = drain_events(state, &rx, &mut guard, &mut rt)?;
            state = s;
            draw(&mut guard, &state, &identity_lines, &wall_clock, rt.viewer_source.as_ref().map(|(_, s)| s))?;
        }
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
    let (state, effects) = update(state, cmd);
    let quit = run_effects(effects, guard, rt)?;
    Ok((state, quit))
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
                // Fed straight back, same as drive enumeration: the panel
                // that owned the command must show whatever the command did
                // to its directory the moment the TUI repaints.
                let _ = rt.tx.send(Command::RereadPanel(side));
            }
            Effect::ShowScrollback => show_scrollback(guard)?,
            Effect::PersistHistory(file) => {
                // A history write failing (read-only directory, full disk)
                // must never take the session down with it.
                let _ = config::save_history_file_atomic(&rt.history_path, &file);
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
            Effect::RunExternalEditor(invocation, side) => match run_external_editor(guard, &invocation)? {
                Ok(()) => {
                    // The editor may have written to the file; re-read the
                    // panel that owned the cursor entry, mirroring
                    // `RunShellCommand` (external-editor: Synchronous wait
                    // and panel re-read — "Panel refreshed after edit").
                    let _ = rt.tx.send(Command::RereadPanel(side));
                }
                Err(message) => {
                    let _ = rt.tx.send(Command::ExternalEditorSpawnFailed { message });
                }
            },
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

fn draw(
    guard: &mut TerminalGuard,
    state: &State,
    identity_lines: &[String; 4],
    wall_clock: &dyn WallClock,
    viewer_source: Option<&ByteSource>,
) -> io::Result<()> {
    let (hour, minute) = wall_clock.now_local();
    let clock_text = format_clock(hour, minute);
    guard.terminal.draw(|frame| {
        let area = frame.area();
        let depth = ColorDepth::Ansi16;
        // `views::render` takes `frame.buffer_mut()`, a mutable borrow that
        // ends when the call returns — only then can `frame.set_cursor_position`
        // borrow `frame` again, which is why the cursor position travels
        // back as a return value instead of being set from inside `render`
        // itself (builtin-editor "Full-screen editor chrome" — caret as the
        // terminal cursor).
        let cursor = views::render(frame.buffer_mut(), area, state, depth, identity_lines, &clock_text, viewer_source);
        if let Some((x, y)) = cursor {
            frame.set_cursor_position((x, y));
        }
    })?;
    Ok(())
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
    fn apply_config_leaves_editor_unset_when_config_omits_it() {
        let config = config::parse("splash = false\n");
        let mut state = State::empty(Theme::classic());
        apply_config(&mut state, &config);
        assert_eq!(state.editor, None);
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
