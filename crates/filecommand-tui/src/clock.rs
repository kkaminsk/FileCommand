//! The real monotonic `Clock` implementation, backed by `std::time::Instant`,
//! plus the real `WallClock` implementation for the on-screen `h:mm a` clock
//! widget.

use filecommand_core::clock::{Clock, WallClock};
use std::time::Instant;

pub struct RealClock {
    start: Instant,
}

impl RealClock {
    pub fn new() -> Self {
        Self { start: Instant::now() }
    }
}

impl Default for RealClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for RealClock {
    fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}

/// The real local-time reader for the panel-top-border clock widget
/// (design doc §4.1). Windows-first: reads the OS local time directly via
/// `GetLocalTime` (a thin kernel32 call, same FFI-over-bindings-crate
/// tradeoff `filecommand_core::drives` makes). Non-Windows hosts are
/// best-effort per the project's cross-platform stance and fall back to
/// UTC, since the standard library alone cannot resolve the local
/// timezone.
pub struct RealWallClock;

impl WallClock for RealWallClock {
    fn now_local(&self) -> (u8, u8) {
        platform::now_local()
    }

    /// `SystemTime` since the Unix epoch, saturating to 0 should the clock
    /// read before it (history-persistence-hardening design D1).
    fn epoch_ms(&self) -> u64 {
        now_unix_ms()
    }
}

#[cfg(windows)]
mod platform {
    /// Mirrors the Win32 `SYSTEMTIME` layout; only the fields
    /// `GetLocalTime` requires a home for are read.
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetLocalTime(system_time: *mut SystemTime);
    }

    pub fn now_local() -> (u8, u8) {
        let mut st = SystemTime::default();
        unsafe { GetLocalTime(&mut st) };
        (st.hour as u8, st.minute as u8)
    }
}

#[cfg(not(windows))]
mod platform {
    use std::time::{SystemTime, UNIX_EPOCH};

    pub fn now_local() -> (u8, u8) {
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let minutes_total = (secs / 60) % (24 * 60);
        ((minutes_total / 60) as u8, (minutes_total % 60) as u8)
    }
}

/// The current wall-clock time as unix milliseconds. The header clock's
/// minute boundary is computed from this (see [`ms_until_next_minute`]);
/// std's `SystemTime` suffices — no platform FFI needed for a wall reading.
pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

/// Milliseconds from `unix_ms` until the next wall-clock minute boundary —
/// the idle loop's deadline for repainting the header clock
/// (idle-aware-event-loop design D2; application-shell "Header clock still
/// advances while idle"). Local-timezone offsets are whole minutes, so unix
/// arithmetic lands exactly on the boundary the rendered `h:mm a` flips at.
/// Always in `1..=60_000`, so the loop parks at most one minute.
pub fn ms_until_next_minute(unix_ms: u64) -> u64 {
    60_000 - unix_ms % 60_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ms_until_next_minute_at_known_instants() {
        assert_eq!(ms_until_next_minute(0), 60_000, "exactly on a boundary waits a full minute");
        assert_eq!(ms_until_next_minute(1), 59_999);
        assert_eq!(ms_until_next_minute(59_999), 1, "one ms before the flip");
        assert_eq!(ms_until_next_minute(60_000), 60_000, "the boundary itself starts a new minute");
        assert_eq!(ms_until_next_minute(61_500), 58_500);
        // A real wall-clock reading: 2026-10-12T15:04:23.250Z.
        assert_eq!(ms_until_next_minute(1_787_441_063_250), 36_750);
    }
}
