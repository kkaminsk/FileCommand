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
