//! Panel type-ahead jump (Alt+letter) and the fuzzy directory-jump
//! (Ctrl+J) frecency index. Both are terminal-free so their matching and
//! ranking logic is directly unit-testable (design D6; tasks 5.1-5.6).
//!
//! Type-ahead lives on [`crate::update::State::quick_search`] and is driven
//! by `Command::QuickSearch*` in `update.rs`; [`type_ahead_match`] is the
//! pure matching rule those handlers call into. Fuzzy jump's visited-
//! directory index — [`FrecencyEntry`], [`record_visit`], and
//! [`rank_directories`] — is the model a Ctrl+J dialog (landing with the
//! TUI-side `fuzzy-jump` capability) will read and mutate; persistence to
//! `history.json` lives alongside command history in [`crate::config`].

use std::path::{Path, PathBuf};

use crate::fuzzy::is_subsequence_match;
use crate::listing::Entry;

// ---------------------------------------------------------------------
// Type-ahead jump (Alt+letter)
// ---------------------------------------------------------------------

/// The index of the first entry whose displayed name starts with `pattern`
/// (case-insensitive), or `None` when nothing matches or `pattern` is empty
/// — in both cases the caller holds the cursor where it is (type-ahead-jump
/// "Entering type-ahead mode with Alt+letter", "Extending the pattern with
/// printable keys", "Alt+letter with no matching entry").
pub fn type_ahead_match(entries: &[Entry], pattern: &str) -> Option<usize> {
    if pattern.is_empty() {
        return None;
    }
    let needle = pattern.to_lowercase();
    entries.iter().position(|e| e.name.to_string_lossy().to_lowercase().starts_with(&needle))
}

// ---------------------------------------------------------------------
// Fuzzy directory jump (Ctrl+J) and frecency ranking
// ---------------------------------------------------------------------

/// One visited directory's frecency record: how many times it has been
/// navigated into, and when it was last visited (fuzzy-jump "Directory
/// history persistence").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrecencyEntry {
    pub path: PathBuf,
    pub visit_count: u32,
    pub last_visited_ms: u64,
}

/// Record a navigation into `path`: increments an existing entry's visit
/// count and refreshes its last-visit time, or inserts a fresh one-visit
/// entry when `path` has not been seen before (fuzzy-jump "Directory
/// history persistence" navigation-record scenario).
pub fn record_visit(history: &mut Vec<FrecencyEntry>, path: &Path, now_ms: u64) {
    match history.iter_mut().find(|e| e.path == path) {
        Some(entry) => {
            entry.visit_count += 1;
            entry.last_visited_ms = now_ms;
        }
        None => history.push(FrecencyEntry { path: path.to_path_buf(), visit_count: 1, last_visited_ms: now_ms }),
    }
}

/// Recency decays to zero over this span; visits older than this contribute
/// no recency bonus, leaving pure visit-count ordering among them.
const RECENCY_HALF_LIFE_MS: u64 = 7 * 24 * 60 * 60 * 1000; // one week

/// A directory's frecency score at `now_ms`: higher is more likely to be
/// the jump target. Both recency and visit frequency raise it — the score
/// is `visit_count * (a recency factor that falls linearly to 0 as the
/// entry ages past one week)` — deterministic under a pinned clock, no
/// wall-clock reads (fuzzy-jump "Frecency ranking").
pub fn frecency_score(entry: &FrecencyEntry, now_ms: u64) -> u64 {
    let age_ms = now_ms.saturating_sub(entry.last_visited_ms);
    let recency = RECENCY_HALF_LIFE_MS.saturating_sub(age_ms.min(RECENCY_HALF_LIFE_MS));
    entry.visit_count as u64 * (recency + 1)
}

/// The visited-directory list filtered to (case-insensitive) subsequence
/// matches of `pattern` — empty pattern matches everything — and ranked by
/// descending frecency score, ties keeping `history`'s relative order
/// (fuzzy-jump "Fuzzy matching of visited directories", "Frecency
/// ranking").
pub fn rank_directories(history: &[FrecencyEntry], pattern: &str, now_ms: u64) -> Vec<FrecencyEntry> {
    let mut matches: Vec<FrecencyEntry> = history.iter().filter(|e| is_subsequence_match(pattern, &e.path.to_string_lossy())).cloned().collect();
    matches.sort_by_key(|e| std::cmp::Reverse(frecency_score(e, now_ms)));
    matches
}

// ---------------------------------------------------------------------
// Ctrl+J fuzzy-jump dialog state
// ---------------------------------------------------------------------

/// The open Ctrl+J fuzzy-jump dialog: the typed pattern and the highlighted
/// row. The ranked/filtered list itself is never stored here — it is
/// recomputed on demand from `State::dir_history` via [`rank_directories`]
/// so there is exactly one source of truth for "what the list currently
/// shows" (fuzzy-jump "Fuzzy jump dialog invocation").
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FuzzyJumpState {
    pub pattern: String,
    pub cursor: usize,
}

impl FuzzyJumpState {
    /// A freshly opened dialog: empty pattern, cursor on the top (most
    /// frecent) entry.
    pub fn new() -> FuzzyJumpState {
        FuzzyJumpState::default()
    }

    /// Append `c` to the pattern. The cursor resets to the top of the
    /// newly narrowed list, mirroring how a fresh search naturally starts
    /// from the best match (fuzzy-jump "Typing narrows to subsequence
    /// matches").
    pub fn push(&mut self, c: char) {
        self.pattern.push(c);
        self.cursor = 0;
    }

    /// Backspace: shorten the pattern by one character and reset the
    /// cursor, mirroring [`Self::push`] (fuzzy-jump "Backspace re-widens
    /// the list").
    pub fn backspace(&mut self) {
        self.pattern.pop();
        self.cursor = 0;
    }

    /// Move the highlighted row by `delta`, clamped to `[0, len)`. A no-op
    /// on an empty list.
    pub fn move_cursor(&mut self, delta: isize, len: usize) {
        if len == 0 {
            self.cursor = 0;
            return;
        }
        let next = self.cursor as isize + delta;
        self.cursor = next.clamp(0, len as isize - 1) as usize;
    }
}

#[cfg(test)]
mod fuzzy_jump_state_tests {
    use super::*;

    #[test]
    fn typing_and_backspace_reset_the_cursor_to_the_top() {
        let mut fj = FuzzyJumpState::new();
        fj.cursor = 2;
        fj.push('a');
        assert_eq!(fj.pattern, "a");
        assert_eq!(fj.cursor, 0);

        fj.cursor = 3;
        fj.backspace();
        assert_eq!(fj.pattern, "");
        assert_eq!(fj.cursor, 0);
    }

    #[test]
    fn move_cursor_clamps_within_bounds() {
        let mut fj = FuzzyJumpState::new();
        fj.move_cursor(-5, 3);
        assert_eq!(fj.cursor, 0);
        fj.move_cursor(10, 3);
        assert_eq!(fj.cursor, 2);
        fj.move_cursor(-1, 3);
        assert_eq!(fj.cursor, 1);
    }

    #[test]
    fn move_cursor_on_an_empty_list_holds_at_zero() {
        let mut fj = FuzzyJumpState::new();
        fj.cursor = 0;
        fj.move_cursor(1, 0);
        assert_eq!(fj.cursor, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listing::EntryKind;
    use std::ffi::OsString;

    fn entry(name: &str) -> Entry {
        Entry { name: OsString::from(name), kind: EntryKind::File, size: 0, modified: None }
    }

    fn dir_entry(path: &str) -> FrecencyEntry {
        FrecencyEntry { path: PathBuf::from(path), visit_count: 0, last_visited_ms: 0 }
    }

    // -------------------------------------------------------------------
    // Type-ahead jump
    // -------------------------------------------------------------------

    #[test]
    fn alt_letter_jumps_to_the_first_prefix_match() {
        let entries = vec![entry("apple"), entry("banana"), entry("cherry")];
        assert_eq!(type_ahead_match(&entries, "b"), Some(1));
    }

    #[test]
    fn no_matching_entry_returns_none_holding_position() {
        let entries = vec![entry("apple"), entry("banana")];
        assert_eq!(type_ahead_match(&entries, "z"), None);
    }

    #[test]
    fn extending_the_pattern_re_matches_the_first_entry() {
        let entries = vec![entry("report.txt"), entry("readme.md")];
        assert_eq!(type_ahead_match(&entries, "r"), Some(0));
        assert_eq!(type_ahead_match(&entries, "re"), Some(0));
        assert_eq!(type_ahead_match(&entries, "rea"), Some(1));
    }

    #[test]
    fn extended_pattern_with_no_match_returns_none() {
        let entries = vec![entry("report.txt")];
        assert_eq!(type_ahead_match(&entries, "rez"), None);
    }

    #[test]
    fn empty_pattern_matches_nothing_so_the_cursor_holds() {
        let entries = vec![entry("apple")];
        assert_eq!(type_ahead_match(&entries, ""), None);
    }

    #[test]
    fn matching_is_case_insensitive() {
        let entries = vec![entry("Report.txt")];
        assert_eq!(type_ahead_match(&entries, "rep"), Some(0));
    }

    // -------------------------------------------------------------------
    // Fuzzy directory jump / frecency
    // -------------------------------------------------------------------

    #[test]
    fn record_visit_creates_then_increments() {
        let mut history = Vec::new();
        record_visit(&mut history, Path::new(r"C:\A"), 1_000);
        assert_eq!(history, vec![FrecencyEntry { path: PathBuf::from(r"C:\A"), visit_count: 1, last_visited_ms: 1_000 }]);

        record_visit(&mut history, Path::new(r"C:\A"), 2_000);
        assert_eq!(history, vec![FrecencyEntry { path: PathBuf::from(r"C:\A"), visit_count: 2, last_visited_ms: 2_000 }]);
    }

    #[test]
    fn record_visit_tracks_multiple_directories_independently() {
        let mut history = Vec::new();
        record_visit(&mut history, Path::new(r"C:\A"), 1_000);
        record_visit(&mut history, Path::new(r"C:\B"), 1_500);
        record_visit(&mut history, Path::new(r"C:\A"), 2_000);
        assert_eq!(history.len(), 2);
        let a = history.iter().find(|e| e.path == Path::new(r"C:\A")).unwrap();
        assert_eq!(a.visit_count, 2);
        let b = history.iter().find(|e| e.path == Path::new(r"C:\B")).unwrap();
        assert_eq!(b.visit_count, 1);
    }

    #[test]
    fn empty_pattern_shows_all_entries_in_frecency_rank_order() {
        let history = vec![
            FrecencyEntry { path: PathBuf::from(r"C:\Low"), visit_count: 1, last_visited_ms: 0 },
            FrecencyEntry { path: PathBuf::from(r"C:\High"), visit_count: 10, last_visited_ms: 0 },
        ];
        let ranked = rank_directories(&history, "", 0);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].path, PathBuf::from(r"C:\High"));
    }

    #[test]
    fn typing_narrows_to_subsequence_matches() {
        let history = vec![dir_entry(r"C:\Users\Me\Documents"), dir_entry(r"C:\Users\Me\Downloads")];
        let ranked = rank_directories(&history, "docu", 0);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].path, PathBuf::from(r"C:\Users\Me\Documents"));
    }

    #[test]
    fn backspace_shortened_pattern_re_widens_the_list() {
        let history = vec![dir_entry(r"C:\Users\Me\Documents"), dir_entry(r"C:\Users\Me\Downloads")];
        let narrow = rank_directories(&history, "docum", 0);
        assert_eq!(narrow.len(), 1);
        let wider = rank_directories(&history, "do", 0);
        assert_eq!(wider.len(), 2);
    }

    #[test]
    fn frequency_raises_rank_when_recency_is_equal() {
        let history = vec![
            FrecencyEntry { path: PathBuf::from(r"C:\Rare"), visit_count: 1, last_visited_ms: 1_000 },
            FrecencyEntry { path: PathBuf::from(r"C:\Frequent"), visit_count: 20, last_visited_ms: 1_000 },
        ];
        let ranked = rank_directories(&history, "", 1_000);
        assert_eq!(ranked[0].path, PathBuf::from(r"C:\Frequent"), "the more frequently visited directory must rank first");
    }

    #[test]
    fn recency_raises_rank_when_frequency_is_equal() {
        let history = vec![
            FrecencyEntry { path: PathBuf::from(r"C:\Old"), visit_count: 5, last_visited_ms: 0 },
            FrecencyEntry { path: PathBuf::from(r"C:\Recent"), visit_count: 5, last_visited_ms: 500_000 },
        ];
        let ranked = rank_directories(&history, "", 600_000);
        assert_eq!(ranked[0].path, PathBuf::from(r"C:\Recent"), "the more recently visited directory must rank first");
    }

    #[test]
    fn score_is_deterministic_under_a_pinned_clock() {
        let entry = FrecencyEntry { path: PathBuf::from(r"C:\A"), visit_count: 3, last_visited_ms: 10_000 };
        assert_eq!(frecency_score(&entry, 20_000), frecency_score(&entry, 20_000));
    }

    #[test]
    fn old_visits_beyond_the_half_life_still_rank_by_frequency() {
        let ancient = FrecencyEntry { path: PathBuf::from(r"C:\Ancient"), visit_count: 100, last_visited_ms: 0 };
        let very_ancient = FrecencyEntry { path: PathBuf::from(r"C:\VeryAncient"), visit_count: 100, last_visited_ms: 0 };
        let now = RECENCY_HALF_LIFE_MS * 10;
        // Both are far past the recency half-life, so their scores collapse
        // to the same frequency-only floor rather than diverging further.
        assert_eq!(frecency_score(&ancient, now), frecency_score(&very_ancient, now));
    }
}
