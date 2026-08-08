//! Hand-rolled, terminal-free subsequence fuzzy matcher (design D6 open
//! question; task 1.2), shared scaffolding for the `fuzzy-jump` (Ctrl+J
//! visited-directory jump) and `find-file` (Alt+F7) capabilities. No crate
//! dependency was pulled in for this — the matcher is a small enough
//! algorithm that a hand-rolled version keeps the scorer in
//! `filecommand-core`, dependency-free, and trivially unit-testable (§8).
//!
//! A pattern "subsequence-matches" text when every pattern character
//! appears in the text in the same relative order, not necessarily
//! contiguously — e.g. `"cmd"` matches `"C:\Users\Me\Documents"` because
//! C, M, and D occur in that order somewhere in the string. Matching is
//! case-insensitive throughout, since directory and file names are matched
//! by eye, not by exact case.

/// Whether `pattern` is a (case-insensitive) subsequence of `text`. An empty
/// pattern matches everything, matching the "empty pattern shows all"
/// requirement shared by `fuzzy-jump` and `find-file`.
pub fn is_subsequence_match(pattern: &str, text: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }
    let mut haystack = text.chars().map(|c| c.to_ascii_lowercase());
    'needle: for pc in pattern.chars().map(|c| c.to_ascii_lowercase()) {
        for tc in haystack.by_ref() {
            if tc == pc {
                continue 'needle;
            }
        }
        return false;
    }
    true
}

/// A ranking score for a subsequence match: higher is better. Rewards
/// matches that start earlier in `text` and matches whose characters run
/// contiguously (so `"abc"` scores `"abcdef"` above `"a-b-c-def"`). Returns
/// `None` when `pattern` is not a subsequence of `text` at all.
pub fn subsequence_score(pattern: &str, text: &str) -> Option<i64> {
    if pattern.is_empty() {
        return Some(0);
    }
    let haystack: Vec<char> = text.chars().map(|c| c.to_ascii_lowercase()).collect();
    let mut cursor = 0usize;
    let mut score = 0i64;
    let mut last_match: Option<usize> = None;
    for pc in pattern.chars().map(|c| c.to_ascii_lowercase()) {
        let found = haystack[cursor..].iter().position(|&tc| tc == pc)? + cursor;
        score += 10;
        match last_match {
            Some(last) if found == last + 1 => score += 5,
            None => score -= found as i64, // earlier first match ranks higher
            _ => {}
        }
        last_match = Some(found);
        cursor = found + 1;
    }
    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_pattern_matches_everything() {
        assert!(is_subsequence_match("", "anything"));
        assert!(is_subsequence_match("", ""));
        assert_eq!(subsequence_score("", "anything"), Some(0));
    }

    #[test]
    fn matches_a_non_contiguous_subsequence_case_insensitively() {
        assert!(is_subsequence_match("cmd", r"C:\Users\Me\Documents"));
        assert!(is_subsequence_match("CMD", r"c:\users\me\documents"));
    }

    #[test]
    fn rejects_out_of_order_or_missing_characters() {
        assert!(!is_subsequence_match("ba", "abc"), "'b' precedes 'a' in the pattern but follows it in the text");
        assert!(!is_subsequence_match("xyz", "abc"));
    }

    #[test]
    fn score_prefers_earlier_matches() {
        let early = subsequence_score("ab", "ab-----").unwrap();
        let late = subsequence_score("ab", "-----ab").unwrap();
        assert!(early > late, "earlier match should score higher: {early} vs {late}");
    }

    #[test]
    fn score_prefers_contiguous_matches() {
        let contiguous = subsequence_score("abc", "abcxxx").unwrap();
        let scattered = subsequence_score("abc", "a-b-c-xxx").unwrap();
        assert!(contiguous > scattered, "contiguous match should score higher: {contiguous} vs {scattered}");
    }

    #[test]
    fn score_is_none_for_a_non_match() {
        assert_eq!(subsequence_score("zzz", "abc"), None);
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn a_string_always_matches_itself_as_a_pattern(s in "[a-zA-Z0-9]{0,12}") {
                prop_assert!(is_subsequence_match(&s, &s));
            }

            #[test]
            fn matching_implies_a_score(pattern in "[a-c]{0,4}", text in "[a-c]{0,10}") {
                let matched = is_subsequence_match(&pattern, &text);
                let scored = subsequence_score(&pattern, &text).is_some();
                prop_assert_eq!(matched, scored);
            }
        }
    }
}
