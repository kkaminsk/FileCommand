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

use filecommand_core::clock::Clock;
use filecommand_core::listing::DateTime;
use filecommand_core::shell::{Invocation, ShellConfig};
use filecommand_core::theme::{ColorDepth, Theme};
use filecommand_core::{config, drives, identity, update, Command, Effect, State};

use crate::clock::RealClock;
use crate::input;
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
}

pub fn run(no_splash_flag: bool) -> io::Result<()> {
    crate::terminal::install_panic_hook();
    let mut guard = TerminalGuard::new()?;

    let clock = RealClock::new();
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
    // `PATHEXT` and the configured shell are read once here so `update`
    // stays a pure function of `State` rather than of the environment.
    state.shell = ShellConfig::from_env(config.shell.clone());
    state.history = config::load_history(Path::new(config::HISTORY_FILE));

    let (tx, rx) = mpsc::channel::<Command>();
    let mut rt = Runtime { tx, active_job: None, history_path: PathBuf::from(config::HISTORY_FILE) };

    if run_effects(effects, &mut guard, &mut rt)? {
        return Ok(());
    }
    let (mut state, _) = drain_events(state, &rx, &mut guard, &mut rt)?;
    draw(&mut guard, &state, &identity_lines)?;

    loop {
        let (s, mut dirty) = drain_events(state, &rx, &mut guard, &mut rt)?;
        state = s;

        if event::poll(POLL_INTERVAL)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    let page_size = layout::compute(state.term_size).entries_visible;
                    match input::map_key(key, &state, page_size, &keys) {
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
            draw(&mut guard, &state, &identity_lines)?;
        }
    }
}

/// Apply one command and execute its effects. The `bool` is `true` when the
/// event loop should exit.
fn apply(state: State, cmd: Command, guard: &mut TerminalGuard, rt: &mut Runtime) -> io::Result<(State, bool)> {
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
            Effect::PersistHistory(entries) => {
                // A history write failing (read-only directory, full disk)
                // must never take the session down with it.
                let _ = config::save_history_atomic(&rt.history_path, &entries);
            }
            Effect::EnumerateDrives(target) => {
                // Cheap enough for the input path: a bitmask read, no media
                // or network probing. Fed straight back so the dialog's
                // first painted frame already lists every letter.
                let _ = rt.tx.send(Command::DriveListReady { target, drives: drives::enumerate_drives() });
            }
            Effect::FetchDriveLabel { target, letter } => worker::spawn_drive_label(target, letter, rt.tx.clone()),
            Effect::QueryInfo { panel, path } => worker::spawn_info_query(panel, path, rt.tx.clone()),
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

fn draw(guard: &mut TerminalGuard, state: &State, identity_lines: &[String; 4]) -> io::Result<()> {
    guard.terminal.draw(|frame| {
        let area = frame.area();
        let depth = ColorDepth::Ansi16;
        views::render(frame.buffer_mut(), area, state, depth, identity_lines);
    })?;
    Ok(())
}
