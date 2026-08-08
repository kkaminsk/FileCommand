//! Job and event types shared between the worker-thread engine
//! ([`super::worker`]) and the TUI event loop. A [`Job`] describes one
//! Copy/Move/Delete/Mkdir request; a [`JobSink`] is how the engine reports
//! progress and blocks for user decisions without knowing anything about
//! channels, threads, or the TUI.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::conflict::{ConflictChoice, ConflictInfo};
use super::error::{ErrorChoice, ErrorInfo, SkippedItem};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Copy,
    Move,
    Delete,
    Mkdir,
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

/// A single Copy/Move/Delete/Mkdir request. `dest_dir` is the destination
/// directory for Copy/Move and the parent directory for Mkdir; `Delete`
/// leaves it equal to `source_dir` (unused, but keeps the type uniform for
/// panel-re-read matching in `core::update`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub kind: JobKind,
    pub sources: Vec<SourceItem>,
    pub source_dir: PathBuf,
    pub dest_dir: PathBuf,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobOutcome {
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
