//! RAII terminal ownership: enters raw mode + the alternate screen on
//! construction, and guarantees release on every exit path — normal return,
//! early error, or panic.

use std::io::{self, Stdout};

use crossterm::cursor::{Hide, Show};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

/// Tracks whether the TUI currently owns the terminal, so suspend/resume
/// are idempotent: calling either twice performs the transition once.
///
/// Split out from [`TerminalGuard`] because the guard cannot be constructed
/// in a headless test runner (there is no console to put into raw mode),
/// while this — the part that actually decides whether a transition
/// happens — can be exercised directly.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SuspendState {
    suspended: bool,
}

impl SuspendState {
    pub fn is_suspended(&self) -> bool {
        self.suspended
    }

    /// `true` when the caller should actually leave raw mode and the
    /// alternate screen; `false` when it already has.
    pub fn begin_suspend(&mut self) -> bool {
        if self.suspended {
            return false;
        }
        self.suspended = true;
        true
    }

    /// `true` when the caller should actually re-enter raw mode and the
    /// alternate screen.
    pub fn begin_resume(&mut self) -> bool {
        if !self.suspended {
            return false;
        }
        self.suspended = false;
        true
    }
}

pub struct TerminalGuard {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
    state: SuspendState,
    /// Snapshot of `config.toml`'s `[mouse] enabled` key (and `--nomouse`)
    /// at startup, per [`new`](Self::new). Fixed for the guard's lifetime —
    /// there is no runtime toggle (design D1) — so `suspend`/`resume` know
    /// whether to touch mouse capture at all without a config lookup of
    /// their own (mouse-input "Mouse capture configuration").
    mouse_enabled: bool,
}

impl TerminalGuard {
    /// `mouse_enabled`: the resolved `config.toml` `[mouse] enabled` value
    /// ANDed with `!--nomouse`, computed by the caller before any terminal
    /// state is touched (application-shell "Terminal acquired on startup":
    /// "enables mouse capture when configured").
    pub fn new(mouse_enabled: bool) -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(e) = enter_commands(&mut stdout, mouse_enabled) {
            let _ = disable_raw_mode();
            return Err(e);
        }
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal, state: SuspendState::default(), mouse_enabled })
    }

    /// Hand the real terminal back: leave raw mode and the alternate
    /// screen, exposing the host's scrollback. Idempotent.
    ///
    /// The suspended flag is set *before* the transition is attempted, so a
    /// partial failure still leaves [`resume`](Self::resume) able to put
    /// things back rather than believing it is already restored.
    ///
    /// When mouse capture is configured on, it is released before the
    /// alternate screen is left — otherwise a child shell run from the
    /// command line would inherit mouse tracking, and on Windows capture
    /// leaves conhost's QuickEdit disabled for as long as it stays on
    /// (design D1; application-shell "Suspended shell run gets a normal
    /// terminal").
    pub fn suspend(&mut self) -> io::Result<()> {
        if !self.state.begin_suspend() {
            return Ok(());
        }
        suspend_with(
            self.mouse_enabled,
            disable_raw_mode,
            || execute!(io::stdout(), DisableMouseCapture).map(|_| ()),
            || execute!(io::stdout(), LeaveAlternateScreen, Show).map(|_| ()),
        )
    }

    /// Retake the terminal and force a full repaint. Idempotent.
    ///
    /// Mouse capture (when configured on) is re-enabled as part of
    /// re-entering the alternate screen, symmetric with `suspend` releasing
    /// it before leaving. Resetting the TUI-side press/double-click state
    /// once that lands (`MouseTracker`, mouse-basics task 2.2) is the
    /// caller's job at each `resume()` call site, same as it is here for
    /// `self.terminal.clear()` below (design D1).
    pub fn resume(&mut self) -> io::Result<()> {
        if !self.state.begin_resume() {
            return Ok(());
        }
        resume_with(
            self.mouse_enabled,
            enable_raw_mode,
            || execute!(io::stdout(), EnterAlternateScreen, Hide).map(|_| ()),
            || execute!(io::stdout(), EnableMouseCapture).map(|_| ()),
        )?;
        // The alternate screen we come back to is blank, and ratatui's
        // diffing would otherwise assume the previous frame is still there.
        self.terminal.clear()?;
        Ok(())
    }

    pub fn is_suspended(&self) -> bool {
        self.state.is_suspended()
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

/// Write the "enter" side of the screen/capture transition, used only by
/// `new` (which cannot be exercised in a headless test runner — see
/// [`SuspendState`]'s doc comment — so it has no need of the closure-injected
/// shape [`suspend_with`]/[`resume_with`] give `suspend`/`resume`): the
/// alternate screen, a hidden cursor, and — when `mouse_enabled` — mouse
/// capture, in that order. Mouse capture last so it only ever becomes active
/// once the alternate screen is already in place.
fn enter_commands(w: &mut impl io::Write, mouse_enabled: bool) -> io::Result<()> {
    if mouse_enabled {
        execute!(w, EnterAlternateScreen, Hide, EnableMouseCapture)
    } else {
        execute!(w, EnterAlternateScreen, Hide)
    }
}

/// The ordered steps [`TerminalGuard::suspend`] performs, with the actual
/// raw-mode, mouse-capture, and alternate-screen operations injected as
/// separate closures so the *order between them* — raw mode off, then (when
/// `mouse_enabled`) mouse capture off, then the alternate screen left — is
/// unit-testable without a real console, the same constraint
/// [`install_panic_hook_with`] works around for the panic hook (design D1:
/// "otherwise a child shell inherits mouse tracking"; application-shell
/// "Suspended shell run gets a normal terminal"). Production code and the
/// test in `tests/panic_restoration.rs` both call this one function, so
/// there is no separate "description" of the order to drift from what
/// actually runs.
pub fn suspend_with(
    mouse_enabled: bool,
    set_raw_mode_off: impl FnOnce() -> io::Result<()>,
    disable_mouse_capture: impl FnOnce() -> io::Result<()>,
    leave_alternate_screen: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    set_raw_mode_off()?;
    if mouse_enabled {
        disable_mouse_capture()?;
    }
    leave_alternate_screen()
}

/// The [`TerminalGuard::resume`] counterpart to [`suspend_with`]: raw mode
/// on, then the alternate screen re-entered, then (when `mouse_enabled`)
/// mouse capture re-enabled — the reverse of `suspend_with`'s order, so
/// capture is never active outside the alternate screen either way.
pub fn resume_with(
    mouse_enabled: bool,
    set_raw_mode_on: impl FnOnce() -> io::Result<()>,
    enter_alternate_screen: impl FnOnce() -> io::Result<()>,
    enable_mouse_capture: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    set_raw_mode_on()?;
    enter_alternate_screen()?;
    if mouse_enabled {
        enable_mouse_capture()?;
    }
    Ok(())
}

/// Leave raw mode and the alternate screen, and unconditionally release
/// mouse capture — harmless when it was never enabled (the panic hook and
/// `Drop` have no access to the `[mouse] enabled` flag, so there is nothing
/// to gate on; application-shell "Panic hook restores the terminal before
/// reporting"; design D1). Idempotent and infallible from the caller's
/// perspective (errors are swallowed) — safe to call from `Drop` or from a
/// panic hook where nothing useful can be done with a failure anyway.
pub fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen, Show);
}

/// Install a panic hook that restores the terminal *before* the default (or
/// previously installed) hook prints its report, then chains to that hook.
pub fn install_panic_hook() {
    install_panic_hook_with(restore_terminal);
}

/// Same as [`install_panic_hook`], but with the restore step injected —
/// exists so tests can verify hook ordering/chaining without depending on a
/// real console (raw-mode/alternate-screen calls are not portable inside a
/// captured test harness).
pub fn install_panic_hook_with(restore: fn()) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A panic inside a guarded scope must leave raw mode and the alternate
    /// screen *before* the panic report prints — otherwise the report gets
    /// smeared across a raw-mode alternate-screen buffer the user can't see
    /// or scroll back to. Headless test runners can't allocate a real
    /// console, so this exercises the actual ordering contract
    /// `install_panic_hook` relies on (`install_panic_hook_with`: restore
    /// runs, then the chained previous hook runs) via a real panic through
    /// `catch_unwind`, restoring whatever hook was installed immediately
    /// after so it can't leak into other tests.
    #[test]
    fn panic_inside_guarded_scope_restores_terminal_before_report_prints() {
        // A plain `fn()` (required by `install_panic_hook_with`'s
        // signature) can't capture a local `Arc`, so both sides of the
        // ordering check record into one process-wide static log instead —
        // that's what actually lets this test prove *order*, not just that
        // both steps ran.
        static ORDER: Mutex<Vec<&str>> = Mutex::new(Vec::new());
        ORDER.lock().unwrap().clear();

        fn record_restore() {
            ORDER.lock().unwrap().push("restore");
        }

        let default_hook = std::panic::take_hook();
        // Stands in for "the previously installed hook" that
        // `install_panic_hook_with` chains to after restoring.
        std::panic::set_hook(Box::new(|_info: &std::panic::PanicHookInfo<'_>| {
            ORDER.lock().unwrap().push("report");
        }));

        install_panic_hook_with(record_restore);
        let result = std::panic::catch_unwind(|| panic!("boom"));
        std::panic::set_hook(default_hook);

        assert!(result.is_err());
        assert_eq!(*ORDER.lock().unwrap(), vec!["restore", "report"]);
    }

    #[test]
    fn restore_terminal_is_idempotent_and_does_not_panic() {
        restore_terminal();
        restore_terminal();
    }

    #[test]
    fn suspend_and_resume_transition_exactly_once_each() {
        let mut state = SuspendState::default();
        assert!(!state.is_suspended());

        assert!(state.begin_suspend(), "the first suspend performs the transition");
        assert!(!state.begin_suspend(), "a second suspend is a no-op");
        assert!(state.is_suspended());

        assert!(state.begin_resume(), "the first resume performs the transition");
        assert!(!state.begin_resume(), "a second resume is a no-op");
        assert!(!state.is_suspended());
    }

    #[test]
    fn resume_without_a_preceding_suspend_does_nothing() {
        let mut state = SuspendState::default();
        assert!(!state.begin_resume());
        assert!(!state.is_suspended());
    }

    #[test]
    fn suspend_resume_cycles_repeatedly() {
        // A failing child, then Ctrl+O, then another command must each get a
        // clean transition rather than sticking in either state.
        let mut state = SuspendState::default();
        for _ in 0..3 {
            assert!(state.begin_suspend());
            assert!(state.begin_resume());
        }
        assert!(!state.is_suspended());
    }
}
