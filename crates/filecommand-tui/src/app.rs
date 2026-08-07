//! The event loop: drains crossterm input + worker events, converts them to
//! `Command`s, applies `core::update`, executes the returned effects, and
//! redraws. This is the only place effects are actually performed.

use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, SystemTime};

use crossterm::event::{self, Event, KeyEventKind};

use filecommand_core::clock::Clock;
use filecommand_core::listing::DateTime;
use filecommand_core::theme::{ColorDepth, Theme};
use filecommand_core::{identity, update, Command, Effect, State};

use crate::clock::RealClock;
use crate::input;
use crate::layout;
use crate::terminal::TerminalGuard;
use crate::views;
use crate::worker;

const POLL_INTERVAL: Duration = Duration::from_millis(33);

pub fn run(no_splash_flag: bool) -> io::Result<()> {
    crate::terminal::install_panic_hook();
    let mut guard = TerminalGuard::new()?;

    let clock = RealClock::new();
    let config = filecommand_core::config::load(std::path::Path::new("config.toml"));
    let theme = Theme::by_name(&config.theme).unwrap_or_else(Theme::classic);
    let show_splash = config.splash && !no_splash_flag;

    let size = guard.terminal.size()?;
    let term_size = (size.width, size.height);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let year = DateTime::from_system_time(SystemTime::now()).year;
    let identity_lines = identity::identity_lines(year);

    let (mut state, effects) = State::initial(theme, term_size, clock.now_ms(), cwd.clone(), cwd, show_splash);
    let (tx, rx) = mpsc::channel::<Command>();
    if run_effects(effects, &tx) {
        return Ok(());
    }

    draw(&mut guard, &state, &identity_lines)?;

    loop {
        let mut dirty = false;

        while let Ok(cmd) = rx.try_recv() {
            let (new_state, effects) = update(state, cmd);
            state = new_state;
            dirty = true;
            if run_effects(effects, &tx) {
                return Ok(());
            }
        }

        if event::poll(POLL_INTERVAL)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    let page_size = layout::compute(state.term_size).entries_visible;
                    if let Some(cmd) = input::map_key(key, &state.phase, page_size) {
                        let (new_state, effects) = update(state, cmd);
                        state = new_state;
                        dirty = true;
                        if run_effects(effects, &tx) {
                            return Ok(());
                        }
                    }
                }
                Event::Resize(w, h) => {
                    let (new_state, effects) = update(state, Command::Resize(w, h));
                    state = new_state;
                    dirty = true;
                    if run_effects(effects, &tx) {
                        return Ok(());
                    }
                }
                _ => {}
            }
        } else {
            let (new_state, effects) = update(state, Command::Tick(clock.now_ms()));
            state = new_state;
            dirty = true;
            if run_effects(effects, &tx) {
                return Ok(());
            }
        }

        if dirty {
            draw(&mut guard, &state, &identity_lines)?;
        }
    }
}

/// Execute effects returned by `update`. Returns `true` if the caller should
/// exit the event loop (a `Quit` effect was requested).
fn run_effects(effects: Vec<Effect>, tx: &Sender<Command>) -> bool {
    let mut quit = false;
    for effect in effects {
        match effect {
            Effect::StartListing { panel, path } => worker::spawn_listing(panel, path, tx.clone()),
            Effect::Quit => quit = true,
        }
    }
    quit
}

fn draw(guard: &mut TerminalGuard, state: &State, identity_lines: &[String; 4]) -> io::Result<()> {
    guard.terminal.draw(|frame| {
        let area = frame.area();
        let depth = ColorDepth::Ansi16;
        views::render(frame.buffer_mut(), area, state, depth, identity_lines);
    })?;
    Ok(())
}
