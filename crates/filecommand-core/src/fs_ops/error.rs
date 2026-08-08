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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedItem {
    pub path: PathBuf,
    pub reason: String,
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
        let mut skipped = Vec::new();
        skipped.push(SkippedItem { path: PathBuf::from("/a"), reason: "denied".into() });
        skipped.push(SkippedItem { path: PathBuf::from("/b"), reason: "sharing violation".into() });
        assert_eq!(skipped.len(), 2);
        assert_eq!(skipped[0].path, PathBuf::from("/a"));
        assert_eq!(skipped[1].reason, "sharing violation");
    }
}
