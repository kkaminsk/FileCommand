//! RAII terminal ownership: enters raw mode + the alternate screen on
//! construction, and guarantees release on every exit path — normal return,
//! early error, or panic.

use std::io::{self, Stdout};

use crossterm::cursor::{Hide, Show};
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
}

impl TerminalGuard {
    pub fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(e) = execute!(stdout, EnterAlternateScreen, Hide) {
            let _ = disable_raw_mode();
            return Err(e);
        }
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal, state: SuspendState::default() })
    }

    /// Hand the real terminal back: leave raw mode and the alternate
    /// screen, exposing the host's scrollback. Idempotent.
    ///
    /// The suspended flag is set *before* the transition is attempted, so a
    /// partial failure still leaves [`resume`](Self::resume) able to put
    /// things back rather than believing it is already restored.
    pub fn suspend(&mut self) -> io::Result<()> {
        if !self.state.begin_suspend() {
            return Ok(());
        }
        disable_raw_mode()?;
        execute!(io::stdout(), LeaveAlternateScreen, Show)?;
        Ok(())
    }

    /// Retake the terminal and force a full repaint. Idempotent.
    pub fn resume(&mut self) -> io::Result<()> {
        if !self.state.begin_resume() {
            return Ok(());
        }
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, Hide)?;
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

/// Leave raw mode and the alternate screen. Idempotent and infallible from
/// the caller's perspective (errors are swallowed) — safe to call from
/// `Drop` or from a panic hook where nothing useful can be done with a
/// failure anyway.
pub fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
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
