//! Verifies that a panic occurring while the terminal guard is "active"
//! triggers terminal restoration *before* the previously installed panic
//! hook runs, and that the previous hook is still invoked (chaining) —
//! plus (mouse-basics task 1.3) that `TerminalGuard::suspend`/`resume`
//! order mouse capture correctly around the alternate screen: capture
//! disabled before `LeaveAlternateScreen` on suspend, re-enabled on resume.
//!
//! Real raw-mode/alternate-screen/mouse-capture syscalls aren't observable
//! portably inside a captured test harness (no guaranteed real console), so
//! these tests inject stand-ins — a restore callback via
//! [`filecommand_tui::terminal::install_panic_hook_with`] for the panic-hook
//! ordering, and step closures via
//! [`filecommand_tui::terminal::suspend_with`]/[`resume_with`] for the
//! suspend/resume ordering — and assert ordering with atomics/a `Vec`
//! instead. Production code calls these same functions, so there is no
//! separate "description" of the order for the real behavior to drift from.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;

use filecommand_tui::terminal::{install_panic_hook_with, resume_with, suspend_with};

// The global panic hook is process-wide; serialize any test that touches it.
static HOOK_TEST_LOCK: Mutex<()> = Mutex::new(());

static RESTORE_CALLED: AtomicBool = AtomicBool::new(false);
static SEQUENCE: AtomicU8 = AtomicU8::new(0);
static RESTORE_SEQ: AtomicU8 = AtomicU8::new(0);
static PREVIOUS_HOOK_SEQ: AtomicU8 = AtomicU8::new(0);

fn record_restore() {
    RESTORE_CALLED.store(true, Ordering::SeqCst);
    RESTORE_SEQ.store(SEQUENCE.fetch_add(1, Ordering::SeqCst), Ordering::SeqCst);
}

#[test]
fn panic_restores_terminal_before_previous_hook_runs() {
    let _guard = HOOK_TEST_LOCK.lock().unwrap();

    RESTORE_CALLED.store(false, Ordering::SeqCst);
    SEQUENCE.store(0, Ordering::SeqCst);
    RESTORE_SEQ.store(u8::MAX, Ordering::SeqCst);
    PREVIOUS_HOOK_SEQ.store(u8::MAX, Ordering::SeqCst);

    // A silent "previously installed" hook that just records when it ran.
    std::panic::set_hook(Box::new(|_info| {
        PREVIOUS_HOOK_SEQ.store(SEQUENCE.fetch_add(1, Ordering::SeqCst), Ordering::SeqCst);
    }));

    install_panic_hook_with(record_restore);

    let result = std::panic::catch_unwind(|| {
        panic!("synthetic panic for terminal-restoration test");
    });
    assert!(result.is_err());

    // Restore the default hook so we don't leak our test hook into other tests.
    let _ = std::panic::take_hook();

    assert!(RESTORE_CALLED.load(Ordering::SeqCst), "restore callback was never invoked");
    let restore_seq = RESTORE_SEQ.load(Ordering::SeqCst);
    let previous_seq = PREVIOUS_HOOK_SEQ.load(Ordering::SeqCst);
    assert_ne!(previous_seq, u8::MAX, "previously installed hook was never chained to");
    assert!(restore_seq < previous_seq, "restore must run before the previous (report-printing) hook");
}

// ---------------------------------------------------------------------
// Suspend/resume mouse-capture ordering (mouse-basics task 1.3; design D1;
// application-shell "Suspended shell run gets a normal terminal").
// ---------------------------------------------------------------------

/// Mouse capture must be released before the alternate screen is left —
/// otherwise a child shell run from the command line (or an external
/// editor, or Ctrl+O's scrollback view) would inherit mouse tracking, and
/// on Windows capture leaves conhost's QuickEdit disabled the whole time.
#[test]
fn suspend_disables_mouse_capture_before_leaving_the_alternate_screen() {
    // A `Mutex` (rather than a bare `&mut Vec`) so all three step closures
    // can share one recorder: each is a separate `FnOnce` argument, and the
    // borrow checker won't allow more than one of them to hold `&mut order`
    // at the call site even though only one ever runs at a time.
    let order: Mutex<Vec<&str>> = Mutex::new(Vec::new());
    suspend_with(
        true,
        || {
            order.lock().unwrap().push("raw_mode_off");
            Ok(())
        },
        || {
            order.lock().unwrap().push("mouse_capture_off");
            Ok(())
        },
        || {
            order.lock().unwrap().push("leave_alternate_screen");
            Ok(())
        },
    )
    .expect("suspend_with must succeed when every step succeeds");
    assert_eq!(*order.lock().unwrap(), vec!["raw_mode_off", "mouse_capture_off", "leave_alternate_screen"]);
}

/// When mouse capture was never configured on, `suspend` must not touch it
/// at all — `[mouse] enabled = false` / `--nomouse` means "behave exactly
/// as before this capability existed" (mouse-input "Mouse capture
/// configuration").
#[test]
fn suspend_skips_the_mouse_capture_step_when_mouse_is_disabled() {
    // A `Mutex` (rather than a bare `&mut Vec`) so all three step closures
    // can share one recorder: each is a separate `FnOnce` argument, and the
    // borrow checker won't allow more than one of them to hold `&mut order`
    // at the call site even though only one ever runs at a time.
    let order: Mutex<Vec<&str>> = Mutex::new(Vec::new());
    suspend_with(
        false,
        || {
            order.lock().unwrap().push("raw_mode_off");
            Ok(())
        },
        || {
            order.lock().unwrap().push("mouse_capture_off");
            Ok(())
        },
        || {
            order.lock().unwrap().push("leave_alternate_screen");
            Ok(())
        },
    )
    .expect("suspend_with must succeed when every step succeeds");
    assert_eq!(*order.lock().unwrap(), vec!["raw_mode_off", "leave_alternate_screen"], "mouse capture was never enabled, so nothing disables it");
}

/// `resume` re-enables mouse capture once the alternate screen has been
/// re-entered — the mirror image of `suspend` releasing it beforehand, so
/// capture is active only while the TUI actually owns the screen.
#[test]
fn resume_re_enables_mouse_capture_after_entering_the_alternate_screen() {
    // A `Mutex` (rather than a bare `&mut Vec`) so all three step closures
    // can share one recorder: each is a separate `FnOnce` argument, and the
    // borrow checker won't allow more than one of them to hold `&mut order`
    // at the call site even though only one ever runs at a time.
    let order: Mutex<Vec<&str>> = Mutex::new(Vec::new());
    resume_with(
        true,
        || {
            order.lock().unwrap().push("raw_mode_on");
            Ok(())
        },
        || {
            order.lock().unwrap().push("enter_alternate_screen");
            Ok(())
        },
        || {
            order.lock().unwrap().push("mouse_capture_on");
            Ok(())
        },
    )
    .expect("resume_with must succeed when every step succeeds");
    assert_eq!(*order.lock().unwrap(), vec!["raw_mode_on", "enter_alternate_screen", "mouse_capture_on"]);
}

/// The disabled-mouse counterpart of the resume test above: no capture step
/// runs at all.
#[test]
fn resume_skips_the_mouse_capture_step_when_mouse_is_disabled() {
    // A `Mutex` (rather than a bare `&mut Vec`) so all three step closures
    // can share one recorder: each is a separate `FnOnce` argument, and the
    // borrow checker won't allow more than one of them to hold `&mut order`
    // at the call site even though only one ever runs at a time.
    let order: Mutex<Vec<&str>> = Mutex::new(Vec::new());
    resume_with(
        false,
        || {
            order.lock().unwrap().push("raw_mode_on");
            Ok(())
        },
        || {
            order.lock().unwrap().push("enter_alternate_screen");
            Ok(())
        },
        || {
            order.lock().unwrap().push("mouse_capture_on");
            Ok(())
        },
    )
    .expect("resume_with must succeed when every step succeeds");
    assert_eq!(*order.lock().unwrap(), vec!["raw_mode_on", "enter_alternate_screen"]);
}
