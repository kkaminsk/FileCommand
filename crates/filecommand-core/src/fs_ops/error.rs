//! Per-file error-recovery state machine: Retry/Skip/Skip All/Abort, plus
//! accumulation of skipped items for the end-of-job summary.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorInfo {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorChoice {
    Retry,
    Skip,
    SkipAll,
    Abort,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorResolution {
    Ask(ErrorInfo),
    Auto(ErrorChoice),
}

/// Whether a [`SkippedItem`] is work the job did not do (`Skipped`) or a
/// metadata-restore failure on a file whose content *was* copied
/// (`Warning`) — the end-of-job summary groups its rows and composes its
/// title from this distinction (error-diagnostics design D5;
/// operation-dialogs "Metadata-restore failures are reported in the
/// end-of-job summary").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Skipped,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedItem {
    pub path: PathBuf,
    pub reason: String,
    pub kind: ItemKind,
}

impl SkippedItem {
    /// An item the job did not process: a conflict Skip, an error Skip, or
    /// an engine guard such as a copy onto its own source.
    pub fn skipped(path: PathBuf, reason: String) -> SkippedItem {
        SkippedItem { path, reason, kind: ItemKind::Skipped }
    }

    /// A file whose content was copied but whose metadata (alternate data
    /// streams, modification time, read-only attribute) could not be fully
    /// restored — recorded, never silently discarded (error-diagnostics
    /// design D5).
    pub fn warning(path: PathBuf, reason: String) -> SkippedItem {
        SkippedItem { path, reason, kind: ItemKind::Warning }
    }
}

/// Latching policy for per-file errors: once "Skip All" is chosen, every
/// subsequent error in the job is skipped automatically. `Retry` and
/// `Abort` are never latched — they only make sense as one-shot, in-the-
/// moment decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ErrorPolicy {
    skip_all: bool,
}

impl ErrorPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn resolve(&self, info: ErrorInfo) -> ErrorResolution {
        if self.skip_all {
            ErrorResolution::Auto(ErrorChoice::Skip)
        } else {
            ErrorResolution::Ask(info)
        }
    }

    pub fn apply(&mut self, choice: &ErrorChoice) {
        if matches!(choice, ErrorChoice::SkipAll) {
            self.skip_all = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> ErrorInfo {
        ErrorInfo { path: PathBuf::from("/a.txt"), message: "permission denied".to_string() }
    }

    #[test]
    fn fresh_policy_always_asks() {
        let policy = ErrorPolicy::new();
        assert_eq!(policy.resolve(info()), ErrorResolution::Ask(info()));
    }

    #[test]
    fn skip_all_latches() {
        let mut policy = ErrorPolicy::new();
        policy.apply(&ErrorChoice::SkipAll);
        assert_eq!(policy.resolve(info()), ErrorResolution::Auto(ErrorChoice::Skip));
        assert_eq!(policy.resolve(info()), ErrorResolution::Auto(ErrorChoice::Skip));
    }

    #[test]
    fn retry_and_abort_do_not_latch() {
        let mut policy = ErrorPolicy::new();
        policy.apply(&ErrorChoice::Retry);
        assert_eq!(policy.resolve(info()), ErrorResolution::Ask(info()));
        policy.apply(&ErrorChoice::Abort);
        assert_eq!(policy.resolve(info()), ErrorResolution::Ask(info()));
    }

    #[test]
    fn plain_skip_does_not_latch() {
        let mut policy = ErrorPolicy::new();
        policy.apply(&ErrorChoice::Skip);
        assert_eq!(policy.resolve(info()), ErrorResolution::Ask(info()));
    }

    #[test]
    fn skipped_items_accumulate_in_order() {
        let skipped =
            [SkippedItem::skipped(PathBuf::from("/a"), "denied".into()), SkippedItem::skipped(PathBuf::from("/b"), "sharing violation".into())];
        assert_eq!(skipped.len(), 2);
        assert_eq!(skipped[0].path, PathBuf::from("/a"));
        assert_eq!(skipped[1].reason, "sharing violation");
    }

    /// error-diagnostics design D5: the constructors pin the `kind` field
    /// so the summary's grouping never has to guess from the reason text.
    #[test]
    fn skipped_item_constructors_set_the_kind() {
        let skipped = SkippedItem::skipped(PathBuf::from("/a"), "denied".into());
        assert_eq!(skipped.kind, ItemKind::Skipped);
        let warning = SkippedItem::warning(PathBuf::from("/b"), "modification time not restored: denied".into());
        assert_eq!(warning.kind, ItemKind::Warning);
    }
}
