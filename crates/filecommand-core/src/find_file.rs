//! Alt+F7 find-file dialog state: the typed name pattern, the subtree walk's
//! streamed results, and the cursor over them.
//!
//! The walk itself ([`crate::listing::find_in_subtree`]) is terminal-free
//! and runs on a `filecommand-tui` worker thread, delivering matches
//! incrementally; this module only models the dialog's own state machine —
//! which stage it is in, what has arrived so far, and cursor movement over
//! the growing result list (find-file "Non-blocking search with static
//! progress").

use std::path::PathBuf;

use crate::listing::FindMatch;

/// The open find-file dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindFileState {
    /// The subtree root the walk searches — the active panel's directory at
    /// the moment the dialog opened.
    pub root: PathBuf,
    pub pattern: String,
    /// Results streamed in so far, in arrival order.
    pub results: Vec<FindMatch>,
    pub cursor: usize,
    /// `None` while the pattern is still being typed and no walk has been
    /// submitted yet. `Some(request)` once Enter has kicked off the walk —
    /// matched against `Command::FindFileMatch`/`FindFileSearchDone`'s own
    /// `request` so a reply from an abandoned/superseded search (the dialog
    /// was closed and reopened, or a new search was submitted) is dropped,
    /// mirroring `PanelState::info_request`'s staleness guard.
    pub request: Option<u64>,
    /// Whether the walk backing `request` has finished (find-file "Non-
    /// blocking search with static progress" — "results become selectable
    /// as the walk progresses or on completion").
    pub done: bool,
}

impl FindFileState {
    /// A freshly opened dialog rooted at `root`, with an empty pattern and
    /// no search yet submitted (find-file "Find-file invocation").
    pub fn new(root: PathBuf) -> FindFileState {
        FindFileState { root, pattern: String::new(), results: Vec::new(), cursor: 0, request: None, done: false }
    }

    /// Append `c` to the pattern while still in the input stage. A no-op
    /// once a search is in flight — the pattern is fixed for that search;
    /// Esc-then-reopen is how the user starts over.
    pub fn push(&mut self, c: char) {
        if self.request.is_none() {
            self.pattern.push(c);
        }
    }

    pub fn backspace(&mut self) {
        if self.request.is_none() {
            self.pattern.pop();
        }
    }

    /// Enter on the input stage: mint the walk's request id, clear any
    /// prior results, and mark the search in flight.
    pub fn submit(&mut self, request: u64) {
        self.results.clear();
        self.cursor = 0;
        self.request = Some(request);
        self.done = false;
    }

    /// Append a match arriving from the walk, only if `request` still names
    /// the outstanding search (find-file "Results become selectable as the
    /// walk progresses").
    pub fn push_match(&mut self, request: u64, m: FindMatch) {
        if self.request == Some(request) {
            self.results.push(m);
        }
    }

    /// Mark the walk backing `request` complete, only if it is still the
    /// outstanding one.
    pub fn mark_done(&mut self, request: u64) {
        if self.request == Some(request) {
            self.done = true;
        }
    }

    /// Move the result cursor by `delta`, clamped within the current result
    /// list.
    pub fn move_cursor(&mut self, delta: isize) {
        if self.results.is_empty() {
            self.cursor = 0;
            return;
        }
        let last = self.results.len() as isize - 1;
        self.cursor = (self.cursor as isize + delta).clamp(0, last) as usize;
    }

    pub fn selected(&self) -> Option<&FindMatch> {
        self.results.get(self.cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listing::{Entry, EntryKind};
    use std::ffi::OsString;

    fn a_match(name: &str) -> FindMatch {
        FindMatch {
            relative_path: PathBuf::from(name),
            entry: Entry { name: OsString::from(name), kind: EntryKind::File, size: 0, modified: None },
        }
    }

    #[test]
    fn typing_is_ignored_once_a_search_is_submitted() {
        let mut f = FindFileState::new(PathBuf::from("/root"));
        f.push('a');
        f.push('b');
        assert_eq!(f.pattern, "ab");
        f.submit(1);
        f.push('c');
        assert_eq!(f.pattern, "ab", "the pattern is fixed once the walk is submitted");
    }

    #[test]
    fn submit_clears_prior_results_and_marks_in_flight() {
        let mut f = FindFileState::new(PathBuf::from("/root"));
        f.push('x');
        f.submit(1);
        f.push_match(1, a_match("x1"));
        assert_eq!(f.results.len(), 1);
        f.submit(2);
        assert!(f.results.is_empty());
        assert!(!f.done);
        assert_eq!(f.request, Some(2));
    }

    #[test]
    fn a_match_for_a_superseded_request_is_dropped() {
        let mut f = FindFileState::new(PathBuf::from("/root"));
        f.submit(1);
        f.submit(2);
        f.push_match(1, a_match("stale"));
        assert!(f.results.is_empty(), "a match tagged with the old request must not be applied");
        f.push_match(2, a_match("fresh"));
        assert_eq!(f.results.len(), 1);
    }

    #[test]
    fn mark_done_only_applies_to_the_current_request() {
        let mut f = FindFileState::new(PathBuf::from("/root"));
        f.submit(1);
        f.mark_done(0);
        assert!(!f.done);
        f.mark_done(1);
        assert!(f.done);
    }

    #[test]
    fn cursor_clamps_within_the_result_list() {
        let mut f = FindFileState::new(PathBuf::from("/root"));
        f.submit(1);
        f.push_match(1, a_match("a"));
        f.push_match(1, a_match("b"));
        f.move_cursor(-5);
        assert_eq!(f.cursor, 0);
        f.move_cursor(5);
        assert_eq!(f.cursor, 1);
    }
}
