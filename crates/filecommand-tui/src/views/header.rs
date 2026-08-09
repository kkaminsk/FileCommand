//! Shared degradation logic for the viewer's and editor's full-screen
//! header rows: as the terminal narrows, trailing indicators drop
//! right-to-left before the file path itself is ever touched, and only
//! once every indicator is gone does the path left-truncate with a
//! leading `…` (responsive-layout "Full-screen surface degradation").

use filecommand_core::listing::{display_width, pad_to_width};

/// Compose `path` (already carrying any caller-supplied prefix/suffix,
/// e.g. the editor's `"Edit: "` lead-in and modified-buffer marker) with
/// as many of `indicators` as fit `width`, most-important-first —
/// `indicators` is tried in full, then with its last element dropped, and
/// so on down to none, so the *last* indicator in the slice is always the
/// first one to disappear as width runs out. The survivors are right-
/// appended after the path, joined by three spaces (matching the pre-
/// existing viewer/editor header spacing), with the remaining width
/// distributed as a gap between the path and the indicator block.
///
/// If even the bare path (with no indicators at all) does not fit, it
/// left-truncates with a leading `…` — the path is the last thing to give
/// up any of its own content, per the requirement's "keeping the file
/// path visible last, truncated from the left with a leading `…` when
/// even the path alone does not fit."
pub fn fit_header(path: &str, indicators: &[String], width: usize) -> String {
    for n in (0..=indicators.len()).rev() {
        let right = if indicators[..n].is_empty() { String::new() } else { format!("{} ", indicators[..n].join("   ")) };
        let left = format!(" {path}");
        let used = display_width(&left) + display_width(&right);
        if used <= width {
            let gap = width - used;
            return format!("{left}{}{right}", " ".repeat(gap));
        }
    }
    // Even the bare path doesn't fit: left-truncate it, reserving one
    // column for the leading space that keeps it clear of the frame.
    let budget = width.saturating_sub(1);
    let shown = left_truncate_with_ellipsis(path, budget);
    pad_to_width(&format!(" {shown}"), width)
}

/// Truncate `s` from the left, keeping its last `max_w - 1` display
/// columns and prefixing a leading `…` — mirrors
/// `command_line::left_truncate_with_ellipsis`, duplicated here rather
/// than shared across the two small modules (the same "tiny local
/// helper" precedent `panel.rs`'s `display_width`/`clip` wrappers set).
fn left_truncate_with_ellipsis(s: &str, max_w: usize) -> String {
    if display_width(s) <= max_w {
        return s.to_string();
    }
    if max_w == 0 {
        return String::new();
    }
    if max_w == 1 {
        return "\u{2026}".to_string();
    }
    let budget = max_w - 1;
    let mut kept: Vec<char> = Vec::new();
    let mut acc = 0usize;
    for ch in s.chars().rev() {
        let cw = display_width(&ch.to_string());
        if acc + cw > budget {
            break;
        }
        kept.push(ch);
        acc += cw;
    }
    kept.reverse();
    let mut out = String::from('\u{2026}');
    out.extend(kept);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_indicators_render_when_everything_fits() {
        let indicators = vec!["Col 5".to_string(), "Size 100   50%".to_string()];
        let line = fit_header("notes.txt", &indicators, 80);
        assert_eq!(display_width(&line), 80);
        assert!(line.contains("notes.txt"), "`{line}`");
        assert!(line.contains("Col 5"), "`{line}`");
        assert!(line.trim_end().ends_with("50%"), "`{line}`");
        // Path, then Col, then Size/pct, left to right.
        let path_pos = line.find("notes.txt").unwrap();
        let col_pos = line.find("Col 5").unwrap();
        let size_pos = line.find("Size 100").unwrap();
        assert!(path_pos < col_pos && col_pos < size_pos, "`{line}`");
    }

    #[test]
    fn the_last_indicator_drops_first_as_width_runs_out() {
        let indicators = vec!["Col 5".to_string(), "Size 100   50%".to_string()];
        // Content alone (path + both indicators, no gap) needs 33 columns;
        // narrow just below that so the full form no longer fits but the
        // path + first indicator alone (24 columns) still does.
        let narrow_w = 30;
        let narrowed = fit_header("notes.txt", &indicators, narrow_w);
        assert!(narrowed.contains("Col 5"), "`{narrowed}`");
        assert!(!narrowed.contains("50%"), "the size/pct indicator must drop first: `{narrowed}`");
        assert_eq!(display_width(&narrowed), narrow_w);
    }

    #[test]
    fn the_path_is_the_last_thing_to_drop() {
        let indicators = vec!["Col 5".to_string(), "Size 100   50%".to_string()];
        let line = fit_header("notes.txt", &indicators, 15);
        assert!(!line.contains("Col 5"), "`{line}`");
        assert!(line.contains("notes.txt"), "the path survives once all indicators are gone: `{line}`");
    }

    #[test]
    fn the_path_left_truncates_with_ellipsis_when_it_alone_does_not_fit() {
        let line = fit_header("a-very-long-nested-path/notes.txt", &[], 15);
        assert!(line.starts_with(" \u{2026}"), "`{line}`");
        assert!(line.ends_with("notes.txt"), "the tail (with the filename) survives: `{line}`");
        assert_eq!(display_width(&line), 15);
    }

    #[test]
    fn no_indicators_still_fits_exactly() {
        let line = fit_header("f.txt", &[], 20);
        assert_eq!(display_width(&line), 20);
        assert!(line.contains("f.txt"));
    }
}
