//! Dialog-state types consumed by `filecommand-tui`'s renderers and folded
//! by `core::update`'s `UiPhase`. Pure data — no rendering, no I/O.

use super::conflict::ConflictInfo;
use super::error::{ErrorInfo, SkippedItem};
use super::job::{JobKind, ProgressInfo, SourceItem};
use crate::line_edit::LineEdit;
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
    /// directory's name. The field is a shared text field per
    /// text-field-editing (a pre-filled `input` opens with the caret at its
    /// end). `buttons` is `None` for the plain keyboard-
    /// initiated F5/F6/Mkdir dialog (no button row, Enter/Esc only, unchanged
    /// from before `mouse-panel-drag`) and `Some` for a drop-initiated dialog,
    /// which adds a `[ Copy ] [ Move ] [ Cancel ]` row (operation-dialogs
    /// "Drop-initiated destination dialog"). `hidden_by_filter` is the same
    /// disclosure count as `DeleteConfirm`'s, rendered on the blank row
    /// below the field (selection "Operations disclose selected entries
    /// hidden by the quick filter"); 0 for Mkdir.
    DestinationInput {
        kind: JobKind,
        sources: Vec<SourceItem>,
        source_dir: PathBuf,
        input: LineEdit,
        buttons: Option<DropButtons>,
        hidden_by_filter: usize,
    },
    DeleteConfirm {
        sources: Vec<SourceItem>,
        source_dir: PathBuf,
        /// At least one source is a directory, so a second confirmation is
        /// required before the job actually runs.
        needs_second_confirm: bool,
        /// The first confirmation has already been given; a second Enter/Y
        /// now runs the job.
        confirmed_once: bool,
        /// How many of `sources` are selected entries of the source panel
        /// that its active quick filter hides (0 when there is no filter or
        /// nothing hidden) — computed once when the dialog opens and shown
        /// as its own row between the question and the buttons (selection
        /// "Operations disclose selected entries hidden by the quick
        /// filter"; filter-selection-safety design D2/D3).
        hidden_by_filter: usize,
    },
    /// The file-action menu's in-place Rename: a text-input dialog
    /// pre-filled with `original_name`, renaming within `source_dir`
    /// (file-action-menu "In-place Rename"). Reuses the same
    /// `Command::TextEdit`/`FileOpConfirm`/`FileOpCancel` commands
    /// `DestinationInput` uses.
    RenameInput { source_dir: PathBuf, original_name: OsString, is_dir: bool, input: LineEdit },
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

/// The hidden-selection disclosure line the delete confirmation and the
/// copy/move destination dialog both show when some of the operation's
/// sources are selected entries the source panel's quick filter hides
/// (selection "Operations disclose selected entries hidden by the quick
/// filter"; filter-selection-safety design D3). `None` when there is
/// nothing to disclose — the dialogs are then unchanged.
pub fn hidden_by_filter_disclosure(hidden_by_filter: usize, source_count: usize) -> Option<String> {
    if hidden_by_filter == 0 {
        None
    } else if source_count == 1 {
        Some("Selected item is hidden by filter".to_string())
    } else {
        Some(format!("{hidden_by_filter} of {source_count} selected hidden by filter"))
    }
}

/// Dialogs shown while a `Job` is in flight on the worker thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunningDialog {
    Progress {
        kind: JobKind,
        progress: ProgressInfo,
    },
    /// `rename_input` is a shared text field per text-field-editing (`Some`
    /// once the user picked Rename, starting empty).
    Conflict {
        kind: JobKind,
        info: ConflictInfo,
        progress: ProgressInfo,
        rename_input: Option<LineEdit>,
    },
    Error {
        kind: JobKind,
        info: ErrorInfo,
        progress: ProgressInfo,
    },
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
