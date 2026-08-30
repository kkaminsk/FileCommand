//! Dialog-state types consumed by `filecommand-tui`'s renderers and folded
//! by `core::update`'s `UiPhase`. Pure data — no rendering, no I/O.

use super::conflict::ConflictInfo;
use super::error::{ErrorInfo, SkippedItem};
use super::job::{JobKind, ProgressInfo, SourceItem};
use std::ffi::OsString;
use std::path::PathBuf;

/// Pre-job setup dialogs: gathering a destination/name (Copy, Move, Mkdir),
/// a new in-place name (Rename), or a delete confirmation, before any `Job`
/// has been dispatched to the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileOpSetup {
    /// Text-input dialog. For Copy/Move, `input` is the destination path
    /// (pre-filled with the opposite panel's cwd, or — for a drop-initiated
    /// dialog — the exact drop path); for Mkdir, `input` is the new
    /// directory's name. `buttons` is `None` for the plain keyboard-
    /// initiated F5/F6/Mkdir dialog (no button row, Enter/Esc only, unchanged
    /// from before `mouse-panel-drag`) and `Some` for a drop-initiated dialog,
    /// which adds a `[ Copy ] [ Move ] [ Cancel ]` row (operation-dialogs
    /// "Drop-initiated destination dialog").
    DestinationInput { kind: JobKind, sources: Vec<SourceItem>, source_dir: PathBuf, input: String, buttons: Option<DropButtons> },
    DeleteConfirm {
        sources: Vec<SourceItem>,
        source_dir: PathBuf,
        /// At least one source is a directory, so a second confirmation is
        /// required before the job actually runs.
        needs_second_confirm: bool,
        /// The first confirmation has already been given; a second Enter/Y
        /// now runs the job.
        confirmed_once: bool,
    },
    /// The file-action menu's in-place Rename: a text-input dialog
    /// pre-filled with `original_name`, renaming within `source_dir`
    /// (file-action-menu "In-place Rename"). Reuses the same
    /// `Command::FileOpInputChar`/`FileOpInputBackspace`/`FileOpConfirm`/
    /// `FileOpCancel` commands `DestinationInput` uses.
    RenameInput { source_dir: PathBuf, original_name: OsString, is_dir: bool, input: String },
}

/// The drop-initiated destination dialog's button row (operation-dialogs
/// "Drop-initiated destination dialog"; mouse-drag design D1/D2/D3).
/// `focused` names which of `[ Copy ]`/`[ Move ]` starts out visually
/// highlighted — the verb the drag proposed — independent of the setup's own
/// `kind` field, which is what `Command::FileOpConfirm` (Enter) actually
/// commits: clicking the other button instead reaches
/// `Command::FileOpConfirmAs` and commits that verb directly, without first
/// having to change which one is focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropButtons {
    pub focused: JobKind,
}

/// Dialogs shown while a `Job` is in flight on the worker thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunningDialog {
    Progress { kind: JobKind, progress: ProgressInfo },
    Conflict { kind: JobKind, info: ConflictInfo, progress: ProgressInfo, rename_input: Option<String> },
    Error { kind: JobKind, info: ErrorInfo, progress: ProgressInfo },
}

impl RunningDialog {
    pub fn progress(&self) -> &ProgressInfo {
        match self {
            RunningDialog::Progress { progress, .. } => progress,
            RunningDialog::Conflict { progress, .. } => progress,
            RunningDialog::Error { progress, .. } => progress,
        }
    }

    pub fn kind(&self) -> JobKind {
        match self {
            RunningDialog::Progress { kind, .. } => *kind,
            RunningDialog::Conflict { kind, .. } => *kind,
            RunningDialog::Error { kind, .. } => *kind,
        }
    }
}

/// Re-exported for callers that only need the end-of-job summary shape.
pub type SkippedSummary = Vec<SkippedItem>;
