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

/// A source of local wall-clock time for the on-screen clock widget
/// (§4.1 — "A clock (`h:mm a` style, black on cyan)"). Separate from
/// [`Clock`]: that trait is monotonic-only (relative differences, arbitrary
/// reference point) and cannot be formatted as a time-of-day. The real
/// implementation lives in `filecommand-tui` since it reads the OS local
/// clock; core only depends on the trait plus a pinnable test double, so
/// rendering stays deterministic in snapshot tests.
pub trait WallClock {
    /// Local wall-clock time as `(hour 0-23, minute 0-59)`.
    fn now_local(&self) -> (u8, u8);
}

/// A `WallClock` pinned to a fixed `(hour, minute)`. Never advances on its
/// own.
#[derive(Debug, Clone, Copy)]
pub struct TestWallClock {
    hour: u8,
    minute: u8,
}

impl TestWallClock {
    pub fn new(hour: u8, minute: u8) -> Self {
        Self { hour: hour % 24, minute: minute % 60 }
    }
}

impl WallClock for TestWallClock {
    fn now_local(&self) -> (u8, u8) {
        (self.hour, self.minute)
    }
}

/// Format `(hour, minute)` in the spec's `h:mm a` style: 12-hour, no
/// leading zero on the hour, zero-padded minute, uppercase AM/PM — e.g.
/// `3:04 PM`, `12:00 AM`.
pub fn format_clock(hour: u8, minute: u8) -> String {
    let hour = hour % 24;
    let minute = minute % 60;
    let period = if hour < 12 { "AM" } else { "PM" };
    let h12 = match hour % 12 {
        0 => 12,
        h => h,
    };
    format!("{h12}:{minute:02} {period}")
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

    #[test]
    fn test_wall_clock_is_pinned() {
        let clock = TestWallClock::new(15, 42);
        assert_eq!(clock.now_local(), (15, 42));
        assert_eq!(clock.now_local(), (15, 42));
    }

    #[test]
    fn format_clock_uses_twelve_hour_with_am_pm() {
        assert_eq!(format_clock(0, 0), "12:00 AM");
        assert_eq!(format_clock(11, 59), "11:59 AM");
        assert_eq!(format_clock(12, 0), "12:00 PM");
        assert_eq!(format_clock(23, 59), "11:59 PM");
        assert_eq!(format_clock(15, 4), "3:04 PM");
        assert_eq!(format_clock(1, 5), "1:05 AM");
    }

    #[test]
    fn format_clock_never_shows_a_leading_zero_on_the_hour() {
        for h in 0..24u8 {
            let text = format_clock(h, 0);
            let hour_part = text.split(':').next().unwrap();
            assert!(!hour_part.starts_with('0'), "`{text}` has a leading zero");
        }
    }
}
