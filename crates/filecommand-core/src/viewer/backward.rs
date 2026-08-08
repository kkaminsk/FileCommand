//! Bounded backward line-start navigation (design D3).
//!
//! Because the viewer builds no line index, moving the top-of-screen anchor
//! upward cannot look up a previous line offset. Instead it scans backward
//! from the current top offset for the previous newline, capped at a
//! maximum line length so the read performed is bounded regardless of file
//! content; if no newline is found within the cap the line is hard-split at
//! the cap boundary (viewer: Bounded backward navigation with hard-split
//! cap).

use super::byte_source::ByteSource;

/// The default maximum line length the backward scan (and the hard-split
/// fallback) is capped at, per §4.5's suggested 64 KB.
pub const DEFAULT_MAX_LINE_LEN: usize = 64 * 1024;

/// Scan backward from `top_offset` for the start of the line above it,
/// reading at most `cap` bytes regardless of file content. Returns the new
/// top-of-screen anchor: the byte after the newline found, or the
/// hard-split cap boundary if none was found within `cap` (viewer: Bounded
/// backward navigation — "Backward scan finds the previous line start",
/// "Newline-free content is hard-split at the cap").
pub fn previous_line_start(source: &ByteSource, top_offset: u64, cap: usize) -> u64 {
    if top_offset == 0 {
        return 0;
    }
    let cap = cap as u64;

    // If `top_offset` already sits right after a newline (i.e. it is
    // already a line start), that newline terminates the line *above* it —
    // the one we're trying to find the start of. Skip it so the search
    // looks strictly before it, rather than reporting it right back.
    let search_end = if source.read_range(top_offset - 1, 1).first() == Some(&b'\n') { top_offset - 1 } else { top_offset };
    if search_end == 0 {
        return 0;
    }

    let scan_start = search_end.saturating_sub(cap);
    let window = source.read_range(scan_start, (search_end - scan_start) as usize);
    match window.iter().rposition(|&b| b == b'\n') {
        Some(rel_pos) => scan_start + rel_pos as u64 + 1,
        None => scan_start,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_source(name: &str, contents: &[u8]) -> ByteSource {
        let dir = std::env::temp_dir().join(format!("filecommand-viewer-backward-test-{}-{}", std::process::id(), name));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("file.bin");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(contents).unwrap();
        f.flush().unwrap();
        ByteSource::open(&path).unwrap()
    }

    #[test]
    fn backward_scan_finds_the_previous_line_start() {
        let src = temp_source("basic", b"line1\nline2\nline3\n");
        // Top at start of "line3" (offset 12) -> previous line start is
        // "line2" at offset 6.
        assert_eq!(previous_line_start(&src, 12, DEFAULT_MAX_LINE_LEN), 6);
        // Top at start of "line2" (offset 6) -> previous line start is
        // "line1" at offset 0.
        assert_eq!(previous_line_start(&src, 6, DEFAULT_MAX_LINE_LEN), 0);
    }

    #[test]
    fn backward_scan_at_offset_zero_stays_at_zero() {
        let src = temp_source("zero", b"line1\nline2\n");
        assert_eq!(previous_line_start(&src, 0, DEFAULT_MAX_LINE_LEN), 0);
    }

    #[test]
    fn backward_scan_from_mid_line_returns_current_lines_start() {
        let src = temp_source("mid", b"line1\nline2\n");
        // Offset 8 is inside "line2" ('n'); scrolling up lands on the start
        // of the line containing the anchor.
        assert_eq!(previous_line_start(&src, 8, DEFAULT_MAX_LINE_LEN), 6);
    }

    #[test]
    fn newline_free_content_is_hard_split_at_the_cap() {
        let content = vec![b'x'; 200_000];
        let src = temp_source("hardsplit", &content);
        let cap = 64 * 1024;
        let anchor = previous_line_start(&src, 150_000, cap);
        assert_eq!(anchor, 150_000 - cap as u64);
    }

    #[test]
    fn hard_split_near_start_of_file_clamps_to_zero() {
        let content = vec![b'x'; 1000];
        let src = temp_source("near-start", &content);
        // Top offset is within the cap of byte 0, so the hard split lands
        // exactly at 0 rather than underflowing.
        let anchor = previous_line_start(&src, 500, DEFAULT_MAX_LINE_LEN);
        assert_eq!(anchor, 0);
    }

    #[test]
    fn backward_read_never_exceeds_the_cap_even_for_a_large_newline_free_file() {
        // A newline-free file far larger than the cap: the scan must only
        // ever touch `cap` bytes, never the whole file.
        let content = vec![b'y'; 1_000_000];
        let src = temp_source("large-newline-free", &content);
        let cap = 64 * 1024;
        let anchor = previous_line_start(&src, 900_000, cap);
        assert_eq!(900_000 - anchor, cap as u64);
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        fn arb_content() -> impl Strategy<Value = Vec<u8>> {
            prop::collection::vec(prop::sample::select(vec![b'a', b'b', b'\n']), 0..2000)
        }

        proptest! {
            /// The scan never reads more than `cap` bytes back from the
            /// (newline-skip-adjusted) search point, and the resulting
            /// anchor is always a valid line start: either byte 0, the byte
            /// right after a newline, or exactly the hard-split cap
            /// boundary.
            #[test]
            fn anchor_is_bounded_and_a_valid_line_start(content in arb_content(), top_frac in 0.0f64..1.0, cap in 8usize..500) {
                if content.is_empty() {
                    return Ok(());
                }
                let top_offset = ((content.len() as f64) * top_frac) as u64;
                let dir = std::env::temp_dir().join(format!(
                    "filecommand-viewer-backward-proptest-{}-{}",
                    std::process::id(),
                    top_offset
                ));
                let _ = std::fs::create_dir_all(&dir);
                let path = dir.join("file.bin");
                std::fs::write(&path, &content).unwrap();
                let src = ByteSource::open(&path).unwrap();

                let anchor = previous_line_start(&src, top_offset, cap);

                prop_assert!(anchor <= top_offset);
                // Bounded: the anchor can only be as far back as `cap` bytes
                // from the (possibly newline-adjusted) search point, plus
                // the one-byte peek used to detect that adjustment.
                prop_assert!(top_offset - anchor <= cap as u64 + 1);

                let is_valid_line_start = anchor == 0
                    || content.get(anchor as usize - 1) == Some(&b'\n')
                    || top_offset.saturating_sub(anchor) >= cap as u64;
                prop_assert!(is_valid_line_start, "anchor {anchor} for top {top_offset} cap {cap} is not a valid line start");
            }
        }
    }
}
