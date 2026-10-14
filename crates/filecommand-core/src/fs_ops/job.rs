//! Job and event types shared between the worker-thread engine
//! ([`super::worker`]) and the TUI event loop. A [`Job`] describes one
//! Copy/Move/Delete/Mkdir request; a [`JobSink`] is how the engine reports
//! progress and blocks for user decisions without knowing anything about
//! channels, threads, or the TUI.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::conflict::{ConflictChoice, ConflictInfo};
use super::error::{ErrorChoice, ErrorInfo, SkippedItem};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Copy,
    Move,
    Delete,
    Mkdir,
    /// In-place rename within `source_dir`, including identity-aware
    /// case-only rename (file-action-menu "In-place Rename"). Uses the same
    /// `new_dir_name` field Mkdir uses for its new name, here holding the
    /// entry's new name instead.
    Rename,
}

/// One source item, keyed by its original on-disk name (selection identity)
/// plus its resolved absolute path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceItem {
    pub original_name: OsString,
    pub path: PathBuf,
    /// Known from the panel's already-loaded listing (no I/O needed) — used
    /// by `core::update` to decide whether a delete needs the non-empty-
    /// directory second confirmation.
    pub is_dir: bool,
}

/// A single Copy/Move/Delete/Mkdir/Rename request. `dest_dir` is the
/// destination directory for Copy/Move and the parent directory for Mkdir;
/// `Delete` leaves it equal to `source_dir` (unused, but keeps the type
/// uniform for panel-re-read matching in `core::update`). `Rename` likewise
/// sets `dest_dir` equal to `source_dir` — the renamed entry stays put — and
/// carries its single source's new name in `new_dir_name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub kind: JobKind,
    pub sources: Vec<SourceItem>,
    pub source_dir: PathBuf,
    pub dest_dir: PathBuf,
    /// The new directory's name for Mkdir, or the target entry's new name
    /// for Rename; unused by Copy/Move/Delete.
    pub new_dir_name: Option<OsString>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressInfo {
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub current_file: OsString,
}

impl ProgressInfo {
    pub fn starting(files_total: usize, bytes_total: u64) -> Self {
        ProgressInfo { files_done: 0, files_total, bytes_done: 0, bytes_total, current_file: OsString::new() }
    }
}

/// The minimum spacing between progress updates a sink should forward to the
/// UI — about 20 per second (idle-aware-event-loop "Progress updates are
/// rate-limited"; design D7).
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(50);

/// A pure rate-limiter for [`JobSink::progress`] (idle-aware-event-loop
/// design D7): `now` is injected, so engine tests stay deterministic and the
/// engine itself (`super::worker::run_job` with its recording sinks) is never
/// throttled. The first offer is always delivered; an offer inside
/// [`PROGRESS_INTERVAL`] of the last delivered one is held as `pending`, the
/// newest replacing it; [`flush`](Self::flush) hands out whatever is held.
/// The sink flushes before a conflict, an error, and completion, so a
/// decision point or the final frame never shows stale figures.
#[derive(Debug, Default)]
pub struct ProgressThrottle {
    last_delivered: Option<Instant>,
    pending: Option<ProgressInfo>,
}

impl ProgressThrottle {
    pub fn new() -> ProgressThrottle {
        ProgressThrottle::default()
    }

    /// Offer one progress update: returned (deliver it) when at least
    /// [`PROGRESS_INTERVAL`] has passed since the last delivered update — or
    /// none was delivered yet; otherwise held, replacing any previously held
    /// update, with `None` returned.
    pub fn offer(&mut self, info: ProgressInfo, now: Instant) -> Option<ProgressInfo> {
        let due = match self.last_delivered {
            None => true,
            Some(last) => now.duration_since(last) >= PROGRESS_INTERVAL,
        };
        if due {
            self.last_delivered = Some(now);
            self.pending = None;
            Some(info)
        } else {
            self.pending = Some(info);
            None
        }
    }

    /// The newest held update, if any. `last_delivered` is left alone: a
    /// flushed update goes out beside the message that triggered the flush,
    /// so it does not reset the cadence.
    pub fn flush(&mut self) -> Option<ProgressInfo> {
        self.pending.take()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobOutcome {
    /// `skipped` carries both kinds of `SkippedItem`: items the job did
    /// not process (`ItemKind::Skipped`) and metadata-restore warnings
    /// (`ItemKind::Warning`) for files whose content *was* copied
    /// (error-diagnostics design D5).
    Completed { skipped: Vec<SkippedItem> },
    /// Covers both an explicit user Cancel and an Abort chosen from the
    /// error-recovery dialog — both mean "stop early."
    Cancelled { skipped: Vec<SkippedItem> },
}

/// How the worker reports progress and asks the UI to make a decision. The
/// engine (`super::worker::run_job`) is generic over this trait so it never
/// needs to know about channels or threads; `filecommand-tui` supplies the
/// real implementation backed by an mpsc channel pair.
pub trait JobSink {
    fn progress(&mut self, info: ProgressInfo);
    /// Blocks until the UI supplies a resolution for this conflict.
    fn conflict(&mut self, info: ConflictInfo) -> ConflictChoice;
    /// Blocks until the UI supplies a resolution for this error.
    fn error(&mut self, info: ErrorInfo) -> ErrorChoice;
    fn done(&mut self, outcome: JobOutcome);
    fn is_cancelled(&self) -> bool;
}

/// Shared cancel flag observed at file boundaries and between copy chunks.
#[derive(Debug, Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    pub fn new() -> Self {
        CancelFlag(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(files_done: usize, bytes_done: u64) -> ProgressInfo {
        ProgressInfo { files_done, files_total: 10, bytes_done, bytes_total: 1_000, current_file: OsString::new() }
    }

    // idle-aware-event-loop (file-operations "Progress updates are
    // rate-limited"; design D7; task 1.4)

    #[test]
    fn the_first_offer_is_always_delivered() {
        let mut throttle = ProgressThrottle::new();
        assert_eq!(throttle.offer(info(0, 0), Instant::now()), Some(info(0, 0)));
    }

    #[test]
    fn offers_within_the_interval_are_held_and_only_the_latest_is_flushed() {
        let t0 = Instant::now();
        let mut throttle = ProgressThrottle::new();
        assert!(throttle.offer(info(1, 100), t0).is_some(), "the first offer goes out");
        assert_eq!(throttle.offer(info(2, 200), t0 + Duration::from_millis(10)), None, "inside the interval: held");
        assert_eq!(throttle.offer(info(3, 300), t0 + Duration::from_millis(20)), None, "held, replacing the previous one");
        assert_eq!(throttle.flush(), Some(info(3, 300)), "only the newest held update is flushed");
        assert_eq!(throttle.flush(), None, "nothing is held after the flush");
    }

    #[test]
    fn an_offer_after_the_interval_is_delivered() {
        let t0 = Instant::now();
        let mut throttle = ProgressThrottle::new();
        assert!(throttle.offer(info(1, 100), t0).is_some());
        assert_eq!(throttle.offer(info(2, 200), t0 + PROGRESS_INTERVAL - Duration::from_millis(1)), None, "one ms short is held");
        assert_eq!(throttle.offer(info(3, 300), t0 + PROGRESS_INTERVAL), Some(info(3, 300)), "exactly at the interval is due");
        // A delivered offer drops anything that was held before it.
        assert_eq!(throttle.flush(), None);
    }

    #[test]
    fn a_thousand_offers_over_one_second_deliver_at_most_twenty_one() {
        // file-operations "A fast copy does not flood the UI": the first
        // offer plus one per 50 ms interval is 20 in a 1 s span.
        let t0 = Instant::now();
        let mut throttle = ProgressThrottle::new();
        let mut delivered = 0;
        for i in 0..1_000u64 {
            if throttle.offer(info(i as usize, i), t0 + Duration::from_millis(i)).is_some() {
                delivered += 1;
            }
        }
        assert!(delivered <= 21, "{delivered} updates passed the throttle in one second");
        assert_eq!(delivered, 20, "t = 0, 50, ..., 950 ms");
        assert_eq!(throttle.flush(), Some(info(999, 999)), "the final figures are never lost");
    }
}
