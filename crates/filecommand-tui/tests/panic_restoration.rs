//! Verifies that a panic occurring while the terminal guard is "active"
//! triggers terminal restoration *before* the previously installed panic
//! hook runs, and that the previous hook is still invoked (chaining).
//!
//! Real raw-mode/alternate-screen syscalls aren't observable portably
//! inside a captured test harness (no guaranteed real console), so this
//! test injects a restore callback via
//! [`filecommand_tui::terminal::install_panic_hook_with`] and asserts
//! ordering with atomics instead.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;

use filecommand_tui::terminal::install_panic_hook_with;

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
