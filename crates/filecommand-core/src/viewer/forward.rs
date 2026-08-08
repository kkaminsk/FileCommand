//! Bounded forward line-start navigation, the downward mirror of
//! [`super::backward`] (design D3's cap logic applied in the other
//! direction). Because the viewer builds no line index, moving the
//! top-of-screen anchor downward by one line cannot look up the next line's
//! offset either — it scans forward from the current offset for the next
//! newline, bounded by the same max-line-length cap so the read performed
//! per keystroke stays bounded regardless of file content.
//!
//! This module exists to drive the TUI's Down/PageDown navigation (`update`
//! itself never does file I/O; the caller computes the new top offset via
//! this module, exactly as it does for Up via [`super::backward`], before
//! issuing `Command::ViewerSetTop`).

use super::byte_source::ByteSource;

/// Scan forward from `offset` for the start of the next line, reading at
/// most `cap` bytes regardless of file content. Returns the byte after the
/// next newline found, or `offset + cap` (hard-split, clamped to the file's
/// snapshot length) if none was found within the cap. Returns `offset`
/// unchanged if it is already at or past the end of the file — there is no
/// next line to move to.
pub fn next_line_start(source: &ByteSource, offset: u64, cap: usize) -> u64 {
    let len = source.len();
    if offset >= len {
        return len;
    }
    let cap = cap as u64;
    let window = source.read_range(offset, cap as usize);
    match window.iter().position(|&b| b == b'\n') {
        Some(rel_pos) => offset + rel_pos as u64 + 1,
        None => {
            if (window.len() as u64) < cap {
                // The window ran out at the snapshot's end without another
                // newline: the current line runs to EOF, so there is no
                // next line — land exactly at the end.
                len
            } else {
                // A newline-free stretch at least `cap` bytes long: hard
                // split at the cap boundary, mirroring the backward scan's
                // hard-split (design D3).
                (offset + cap).min(len)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_source(name: &str, contents: &[u8]) -> ByteSource {
        let dir = std::env::temp_dir().join(format!("filecommand-viewer-forward-test-{}-{}", std::process::id(), name));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("file.bin");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(contents).unwrap();
        f.flush().unwrap();
        ByteSource::open(&path).unwrap()
    }

    #[test]
    fn forward_scan_finds_the_next_line_start() {
        let src = temp_source("basic", b"line1\nline2\nline3\n");
        // From the start of "line1" (offset 0), the next line starts right
        // after its newline, at offset 6 ("line2").
        assert_eq!(next_line_start(&src, 0, 64 * 1024), 6);
        assert_eq!(next_line_start(&src, 6, 64 * 1024), 12);
    }

    #[test]
    fn forward_scan_from_mid_line_lands_on_the_next_lines_start() {
        let src = temp_source("mid", b"line1\nline2\n");
        // Offset 2 is inside "line1"; the next line start is still 6.
        assert_eq!(next_line_start(&src, 2, 64 * 1024), 6);
    }

    #[test]
    fn forward_scan_on_the_last_line_lands_at_end_of_file() {
        let src = temp_source("last-line-no-trailing-newline", b"line1\nline2");
        // "line2" has no trailing newline: there is no next line.
        assert_eq!(next_line_start(&src, 6, 64 * 1024), 11);
    }

    #[test]
    fn forward_scan_at_or_past_end_of_file_stays_put() {
        let src = temp_source("at-end", b"line1\nline2\n");
        assert_eq!(next_line_start(&src, 12, 64 * 1024), 12);
        assert_eq!(next_line_start(&src, 100, 64 * 1024), 12);
    }

    #[test]
    fn newline_free_content_is_hard_split_at_the_cap() {
        let content = vec![b'x'; 200_000];
        let src = temp_source("hardsplit", &content);
        let cap = 64 * 1024;
        assert_eq!(next_line_start(&src, 0, cap), cap as u64);
    }

    #[test]
    fn newline_free_content_shorter_than_the_cap_lands_at_end_of_file() {
        let content = vec![b'x'; 1000];
        let src = temp_source("short-no-newline", &content);
        assert_eq!(next_line_start(&src, 0, 64 * 1024), 1000);
    }

    #[test]
    fn forward_read_never_exceeds_the_cap_even_for_a_large_newline_free_file() {
        let content = vec![b'y'; 1_000_000];
        let src = temp_source("large-newline-free", &content);
        let cap = 64 * 1024;
        let anchor = next_line_start(&src, 100_000, cap);
        assert_eq!(anchor - 100_000, cap as u64);
    }

    #[test]
    fn forward_and_backward_scans_agree_on_line_boundaries() {
        let src = temp_source("roundtrip", b"aaa\nbbb\nccc\n");
        // The line starting at 4 ("bbb"), scanned forward from its start,
        // lands on the following line's start; scanning backward from
        // there returns to 4.
        let next = next_line_start(&src, 4, 64 * 1024);
        assert_eq!(next, 8);
        assert_eq!(super::super::backward::previous_line_start(&src, next, 64 * 1024), 4);
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        fn arb_content() -> impl Strategy<Value = Vec<u8>> {
            prop::collection::vec(prop::sample::select(vec![b'a', b'b', b'\n']), 0..2000)
        }

        proptest! {
            /// The scan never reads more than `cap` bytes forward, and the
            /// resulting anchor is always at or past `offset`, at or before
            /// the file's end.
            #[test]
            fn anchor_is_bounded_and_never_moves_backward(content in arb_content(), off_frac in 0.0f64..1.0, cap in 8usize..500) {
                if content.is_empty() {
                    return Ok(());
                }
                let offset = ((content.len() as f64) * off_frac) as u64;
                let dir = std::env::temp_dir().join(format!(
                    "filecommand-viewer-forward-proptest-{}-{}",
                    std::process::id(),
                    offset
                ));
                let _ = std::fs::create_dir_all(&dir);
                let path = dir.join("file.bin");
                std::fs::write(&path, &content).unwrap();
                let src = ByteSource::open(&path).unwrap();

                let anchor = next_line_start(&src, offset, cap);

                prop_assert!(anchor >= offset);
                prop_assert!(anchor <= content.len() as u64);
                prop_assert!(anchor - offset <= cap as u64);
            }
        }
    }
}
