//! Injected clock seam so timing (e.g. the splash-screen minimum hold) is
//! deterministic in tests. The real monotonic implementation lives in
//! `filecommand-tui` since it wraps `std::time::Instant`; core only depends
//! on the trait plus a simple pinnable test double.

/// A source of monotonic milliseconds since some fixed (implementation
/// defined) reference point. Only relative differences are meaningful.
pub trait Clock {
    fn now_ms(&self) -> u64;
}

/// A `Clock` whose value is set explicitly by tests. Never advances on its
/// own; call [`TestClock::set`] or [`TestClock::advance`] to move it.
#[derive(Debug, Default)]
pub struct TestClock {
    now_ms: std::cell::Cell<u64>,
}

impl TestClock {
    pub fn new(start_ms: u64) -> Self {
        Self { now_ms: std::cell::Cell::new(start_ms) }
    }

    pub fn set(&self, now_ms: u64) {
        self.now_ms.set(now_ms);
    }

    pub fn advance(&self, delta_ms: u64) {
        self.now_ms.set(self.now_ms.get() + delta_ms);
    }
}

impl Clock for TestClock {
    fn now_ms(&self) -> u64 {
        self.now_ms.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clock_is_pinned_until_advanced() {
        let clock = TestClock::new(1_000);
        assert_eq!(clock.now_ms(), 1_000);
        assert_eq!(clock.now_ms(), 1_000);
        clock.advance(250);
        assert_eq!(clock.now_ms(), 1_250);
        clock.set(9_999);
        assert_eq!(clock.now_ms(), 9_999);
    }
}
