//! Overwrite-conflict resolution state machine: given a policy possibly
//! latched by a prior "…All" answer, decide whether a fresh conflict can be
//! auto-resolved or must be surfaced to the user.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::listing::DateTime;

/// Source vs. target size/date, enough for the conflict dialog to render a
/// side-by-side comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictInfo {
    pub source_name: OsString,
    pub source_size: u64,
    pub source_modified: Option<DateTime>,
    pub target_path: PathBuf,
    pub target_size: u64,
    pub target_modified: Option<DateTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictChoice {
    Overwrite,
    OverwriteAll,
    Skip,
    SkipAll,
    Rename(OsString),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictResolution {
    Ask(ConflictInfo),
    Auto(ConflictChoice),
}

/// Latching auto-resolve policy: once the user answers "Overwrite All" or
/// "Skip All", every subsequent conflict in the same job is resolved
/// automatically without asking again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConflictPolicy {
    overwrite_all: bool,
    skip_all: bool,
}

impl ConflictPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn resolve(&self, info: ConflictInfo) -> ConflictResolution {
        if self.overwrite_all {
            ConflictResolution::Auto(ConflictChoice::Overwrite)
        } else if self.skip_all {
            ConflictResolution::Auto(ConflictChoice::Skip)
        } else {
            ConflictResolution::Ask(info)
        }
    }

    /// Fold a user (or auto-resolved) choice into the policy, latching
    /// "…All" answers for future conflicts in this job.
    pub fn apply(&mut self, choice: &ConflictChoice) {
        match choice {
            ConflictChoice::OverwriteAll => self.overwrite_all = true,
            ConflictChoice::SkipAll => self.skip_all = true,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> ConflictInfo {
        ConflictInfo {
            source_name: OsString::from("a.txt"),
            source_size: 1,
            source_modified: None,
            target_path: PathBuf::from("/dst/a.txt"),
            target_size: 2,
            target_modified: None,
        }
    }

    #[test]
    fn fresh_policy_always_asks() {
        let policy = ConflictPolicy::new();
        assert_eq!(policy.resolve(info()), ConflictResolution::Ask(info()));
    }

    #[test]
    fn overwrite_all_latches_and_auto_resolves_future_conflicts() {
        let mut policy = ConflictPolicy::new();
        policy.apply(&ConflictChoice::OverwriteAll);
        assert_eq!(policy.resolve(info()), ConflictResolution::Auto(ConflictChoice::Overwrite));
        // A second, unrelated conflict is also auto-resolved.
        assert_eq!(policy.resolve(info()), ConflictResolution::Auto(ConflictChoice::Overwrite));
    }

    #[test]
    fn skip_all_latches_and_auto_resolves_future_conflicts() {
        let mut policy = ConflictPolicy::new();
        policy.apply(&ConflictChoice::SkipAll);
        assert_eq!(policy.resolve(info()), ConflictResolution::Auto(ConflictChoice::Skip));
    }

    #[test]
    fn plain_overwrite_and_skip_do_not_latch() {
        let mut policy = ConflictPolicy::new();
        policy.apply(&ConflictChoice::Overwrite);
        assert_eq!(policy.resolve(info()), ConflictResolution::Ask(info()));
        policy.apply(&ConflictChoice::Skip);
        assert_eq!(policy.resolve(info()), ConflictResolution::Ask(info()));
    }

    #[test]
    fn rename_choice_does_not_latch() {
        let mut policy = ConflictPolicy::new();
        policy.apply(&ConflictChoice::Rename(OsString::from("b.txt")));
        assert_eq!(policy.resolve(info()), ConflictResolution::Ask(info()));
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        #[derive(Debug, Clone, Copy)]
        enum Choice {
            Overwrite,
            OverwriteAll,
            Skip,
            SkipAll,
        }

        fn arb_choice() -> impl Strategy<Value = Choice> {
            prop_oneof![
                Just(Choice::Overwrite),
                Just(Choice::OverwriteAll),
                Just(Choice::Skip),
                Just(Choice::SkipAll),
            ]
        }

        proptest! {
            #[test]
            fn once_latched_always_auto_resolves_for_rest_of_sequence(choices in prop::collection::vec(arb_choice(), 1..20)) {
                let mut policy = ConflictPolicy::new();
                let mut latched = false;
                for c in choices {
                    let as_choice = match c {
                        Choice::Overwrite => ConflictChoice::Overwrite,
                        Choice::OverwriteAll => ConflictChoice::OverwriteAll,
                        Choice::Skip => ConflictChoice::Skip,
                        Choice::SkipAll => ConflictChoice::SkipAll,
                    };
                    policy.apply(&as_choice);
                    if matches!(as_choice, ConflictChoice::OverwriteAll | ConflictChoice::SkipAll) {
                        latched = true;
                    }
                    if latched {
                        prop_assert!(matches!(policy.resolve(info()), ConflictResolution::Auto(_)));
                    }
                }
            }
        }
    }
}
