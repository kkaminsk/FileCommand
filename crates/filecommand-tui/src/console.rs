//! Console control events during a suspended child run (application-shell
//! "Ctrl+C during a suspended child run stops only the child";
//! process-lifecycle-hardening design D1/D2).
//!
//! While the TUI is suspended for a Run-entry command, an F2 user-menu
//! command, or the F4 external editor, raw mode is off, so Ctrl+C is a
//! console control event again: Windows delivers it to every process on the
//! console and Unix sends SIGINT to the foreground process group. Without a
//! handler the default disposition ends FileCommand along with the child.
//!
//! The fix is gated on one process-wide flag, [`CHILD_RUNNING`], held by an
//! RAII [`ChildRunGuard`] for the whole suspended stretch:
//!
//! - **Windows**: a handler installed once at startup
//!   ([`install_ctrl_handler`]) answers `CTRL_C_EVENT`/`CTRL_BREAK_EVENT`
//!   with "handled" while the flag is set, and declines everything else —
//!   close, logoff, and shutdown still end the process, and Ctrl+C outside a
//!   child run takes the default path exactly as before. Handler routines
//!   are per-process and not inherited, so the child still receives the
//!   event and stops as it would from a shell.
//! - **Unix**: only *after* `spawn` has returned (the child has already
//!   `exec`ed with default dispositions) does the guard set `SIGINT` and
//!   `SIGQUIT` to `SIG_IGN`, saving the previous actions and restoring them
//!   when it drops. Ignoring them before the spawn would be inherited by
//!   the child, which is exactly what must not happen.

use std::io;
use std::process::{Child, Command, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};

/// Set while a suspended child run is in progress — from before
/// `guard.suspend()` until after `guard.resume()` has re-entered raw mode,
/// so the press-any-key prompt and the moments between steps are covered
/// too (design D1).
pub static CHILD_RUNNING: AtomicBool = AtomicBool::new(false);

/// Whether the control handler claims `event` given the current flag — the
/// pure decision [`ctrl_handler`] applies, kept separate so it can be
/// stated in one place: only Ctrl+C and Ctrl+Break, and only while a child
/// run is in progress. `CTRL_CLOSE_EVENT`, `CTRL_LOGOFF_EVENT`, and
/// `CTRL_SHUTDOWN_EVENT` are never claimed.
#[cfg(windows)]
fn claims_event(event: u32, child_running: bool) -> bool {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};
    child_running && (event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT)
}

/// The `HandlerRoutine` registered by [`install_ctrl_handler`]. Runs on a
/// system-created thread, so it touches nothing but the atomic flag.
/// Returns `TRUE` (handled — FileCommand keeps running) for Ctrl+C and
/// Ctrl+Break while [`CHILD_RUNNING`] is set, and `FALSE` otherwise so the
/// next handler in the chain (ultimately the default, which ends the
/// process) runs.
#[cfg(windows)]
pub extern "system" fn ctrl_handler(event: u32) -> windows_sys::Win32::Foundation::BOOL {
    use windows_sys::Win32::Foundation::{FALSE, TRUE};
    if claims_event(event, CHILD_RUNNING.load(Ordering::SeqCst)) {
        TRUE
    } else {
        FALSE
    }
}

/// Register [`ctrl_handler`] with the console. Called once by `app::run` at
/// startup, before the terminal is acquired. A registration failure is not
/// fatal — it only means the pre-existing behavior (Ctrl+C ends both
/// processes) remains — so the result is reported rather than unwrapped.
#[cfg(windows)]
pub fn install_ctrl_handler() -> std::io::Result<()> {
    use windows_sys::Win32::Foundation::TRUE;
    use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
    // SAFETY: `ctrl_handler` is a plain `extern "system"` function with the
    // `PHANDLER_ROUTINE` signature and `'static` lifetime; adding it is a
    // process-local registration with no pointer arguments beyond it.
    let ok = unsafe { SetConsoleCtrlHandler(Some(ctrl_handler), TRUE) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// RAII marker for one suspended child run: sets [`CHILD_RUNNING`] on
/// construction and clears it on drop — including during an unwind, so a
/// panic between suspend and resume can never leave Ctrl+C permanently
/// swallowed. On Unix it also owns the saved `SIGINT`/`SIGQUIT` actions
/// once [`child_started`](Self::child_started) has been called, restoring
/// them on drop.
///
/// Not `Clone`: exactly one guard exists per run, and nested runs do not
/// happen (the TUI is suspended for the whole stretch).
pub struct ChildRunGuard {
    #[cfg(unix)]
    saved: Option<unix::SavedSignals>,
    /// Keeps the struct non-constructible outside [`ChildRunGuard::new`].
    _private: (),
}

impl ChildRunGuard {
    /// Begin a child run: the flag goes up now, before the TUI suspends.
    pub fn new() -> ChildRunGuard {
        CHILD_RUNNING.store(true, Ordering::SeqCst);
        ChildRunGuard {
            #[cfg(unix)]
            saved: None,
            _private: (),
        }
    }

    /// Call once `std::process::Command::spawn` has returned `Ok`: the child
    /// is running with its own default signal dispositions, so the parent
    /// may now ignore `SIGINT`/`SIGQUIT` without the child inheriting that
    /// (design D2). A no-op on Windows, where the control handler is already
    /// gated by the flag.
    pub fn child_started(&mut self) {
        #[cfg(unix)]
        {
            if self.saved.is_none() {
                self.saved = Some(unix::ignore_interrupts());
            }
        }
    }
}

impl Default for ChildRunGuard {
    fn default() -> Self {
        ChildRunGuard::new()
    }
}

/// Spawn `command` and wait for it to exit inside `guard`: the Unix signal
/// ignore is armed only once `spawn` has returned, so the child never
/// inherits it (design D2). The spawn error is returned unchanged so the
/// callers' existing "Could not run `…`" messages stay as they are. On
/// Windows the arming step is a no-op; the flag the guard already holds is
/// what gates the control handler.
pub fn spawn_and_wait(command: &mut Command, guard: &mut ChildRunGuard) -> io::Result<ExitStatus> {
    let mut child: Child = command.spawn()?;
    guard.child_started();
    child.wait()
}

/// Serializes every test — in this module or elsewhere in the crate — that
/// reads or writes the process-wide [`CHILD_RUNNING`] flag.
#[cfg(test)]
pub(crate) static FLAG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl Drop for ChildRunGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(saved) = self.saved.take() {
            unix::restore_interrupts(saved);
        }
        CHILD_RUNNING.store(false, Ordering::SeqCst);
    }
}

#[cfg(unix)]
mod unix {
    //! The Unix half of design D2: `sigaction`-based save/ignore/restore of
    //! `SIGINT` and `SIGQUIT`.

    /// The dispositions in force before the guard ignored them.
    pub struct SavedSignals {
        int: libc::sigaction,
        quit: libc::sigaction,
    }

    fn ignore(sig: libc::c_int) -> libc::sigaction {
        // SAFETY: `sigaction` is plain data that is valid when zeroed; both
        // pointers are to live locals for the duration of the call.
        unsafe {
            let mut new: libc::sigaction = std::mem::zeroed();
            new.sa_sigaction = libc::SIG_IGN;
            libc::sigemptyset(&mut new.sa_mask);
            let mut old: libc::sigaction = std::mem::zeroed();
            libc::sigaction(sig, &new, &mut old);
            old
        }
    }

    fn restore(sig: libc::c_int, old: &libc::sigaction) {
        // SAFETY: `old` was filled in by a successful `sigaction` call on the
        // same signal; the output pointer may be null.
        unsafe {
            libc::sigaction(sig, old, std::ptr::null_mut());
        }
    }

    pub fn ignore_interrupts() -> SavedSignals {
        SavedSignals { int: ignore(libc::SIGINT), quit: ignore(libc::SIGQUIT) }
    }

    pub fn restore_interrupts(saved: SavedSignals) {
        restore(libc::SIGINT, &saved.int);
        restore(libc::SIGQUIT, &saved.quit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guard_sets_the_flag_while_alive_and_clears_it_on_drop() {
        let _serial = FLAG_LOCK.lock().unwrap();
        assert!(!CHILD_RUNNING.load(Ordering::SeqCst), "idle to begin with");
        {
            let _guard = ChildRunGuard::new();
            assert!(CHILD_RUNNING.load(Ordering::SeqCst));
        }
        assert!(!CHILD_RUNNING.load(Ordering::SeqCst), "cleared when the guard drops");
    }

    #[test]
    fn the_guard_clears_the_flag_during_an_unwind() {
        // A panic between suspend and resume must not leave Ctrl+C swallowed
        // for the rest of the session.
        let _serial = FLAG_LOCK.lock().unwrap();
        let result = std::panic::catch_unwind(|| {
            let _guard = ChildRunGuard::new();
            assert!(CHILD_RUNNING.load(Ordering::SeqCst));
            panic!("scripted panic inside a child run");
        });
        assert!(result.is_err());
        assert!(!CHILD_RUNNING.load(Ordering::SeqCst), "the unwind ran the guard's Drop");
    }

    // Calling the handler directly exercises its decision without sending
    // a real console control event, which would reach every process on
    // this console (task 1.5).
    #[cfg(windows)]
    mod windows_handler {
        use super::*;
        use windows_sys::Win32::Foundation::{FALSE, TRUE};
        use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT, CTRL_C_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT};

        #[test]
        fn ctrl_c_and_ctrl_break_are_claimed_only_while_a_child_run_is_in_progress() {
            let _serial = FLAG_LOCK.lock().unwrap();
            assert_eq!(ctrl_handler(CTRL_C_EVENT), FALSE, "outside a child run Ctrl+C takes the default path");
            assert_eq!(ctrl_handler(CTRL_BREAK_EVENT), FALSE);
            {
                let _guard = ChildRunGuard::new();
                assert_eq!(ctrl_handler(CTRL_C_EVENT), TRUE, "handled: FileCommand keeps running");
                assert_eq!(ctrl_handler(CTRL_BREAK_EVENT), TRUE);
            }
            assert_eq!(ctrl_handler(CTRL_C_EVENT), FALSE, "the guard is gone, so the default path is back");
        }

        #[test]
        fn close_logoff_and_shutdown_are_never_claimed() {
            // "any console close, logoff, or shutdown event ... still ends
            // FileCommand": declined with and without a child run.
            let _serial = FLAG_LOCK.lock().unwrap();
            for event in [CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT] {
                assert_eq!(ctrl_handler(event), FALSE, "event {event} outside a child run");
            }
            let _guard = ChildRunGuard::new();
            for event in [CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT] {
                assert_eq!(ctrl_handler(event), FALSE, "event {event} during a child run");
            }
        }

        #[test]
        fn claims_event_is_the_pure_form_of_the_handler() {
            assert!(claims_event(CTRL_C_EVENT, true));
            assert!(claims_event(CTRL_BREAK_EVENT, true));
            assert!(!claims_event(CTRL_C_EVENT, false));
            assert!(!claims_event(CTRL_CLOSE_EVENT, true));
        }
    }

    /// Task 1.6: with the guard armed *after* `sleep 5` has been spawned,
    /// SIGINT ends the child (it inherited the default disposition, not
    /// `SIG_IGN`) while this process — which also receives it — survives.
    /// The signal is sent to the child's pid and to this process explicitly
    /// rather than to the whole process group, so the test runner that
    /// spawned this binary (which shares the group) is not interrupted;
    /// the two deliveries together are what a group-wide Ctrl+C would do.
    #[cfg(unix)]
    #[test]
    fn sigint_after_spawn_ends_the_child_and_spares_this_process() {
        let _serial = FLAG_LOCK.lock().unwrap();
        let mut guard = ChildRunGuard::new();
        let mut child = Command::new("sleep").arg("5").stdin(std::process::Stdio::null()).spawn().expect("`sleep` exists on every Unix");
        guard.child_started();

        // SAFETY: plain signal sends to a child we own and to ourselves; the
        // guard has just made SIGINT ignored in this process.
        unsafe {
            libc::kill(child.id() as libc::pid_t, libc::SIGINT);
            libc::kill(libc::getpid(), libc::SIGINT);
        }

        let started = std::time::Instant::now();
        let status = child.wait().expect("wait on the signalled child");
        assert!(started.elapsed() < std::time::Duration::from_secs(4), "the child stopped on SIGINT rather than sleeping out its 5 s");
        assert!(!status.success(), "ended by a signal, not a normal exit: {status:?}");
        // Reaching this line is the "test process survives" half.
        drop(guard);
    }
}
