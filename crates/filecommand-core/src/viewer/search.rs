//! F7 literal streaming search (design D4).
//!
//! Scans forward from an offset in fixed chunks, carrying a
//! `pattern.len() - 1` byte overlap across chunk boundaries so a match
//! straddling two chunks is never missed. Never loads the whole file — each
//! step is bounded to a fixed chunk window (viewer: F7 streaming search
//! with chunk-boundary overlap).

use super::byte_source::ByteSource;

/// The default forward-scan chunk size.
pub const DEFAULT_CHUNK_SIZE: usize = 64 * 1024;

/// Search `source` for the first occurrence of `pattern` at or after
/// `start_offset`, streaming forward in `chunk_size`-byte windows with
/// `pattern.len() - 1` bytes of overlap between consecutive windows so a
/// match straddling a chunk boundary is still found (viewer: F7 streaming
/// search — "Match straddling a chunk boundary is found"). Returns the
/// match's absolute byte range `[start, end)`, or `None` if the pattern does
/// not occur before the source's snapshot length. An empty pattern never
/// matches.
pub fn find_forward(source: &ByteSource, start_offset: u64, pattern: &[u8], chunk_size: usize) -> Option<(u64, u64)> {
    if pattern.is_empty() {
        return None;
    }
    let overlap = pattern.len().saturating_sub(1);
    let mut pos = start_offset;
    let file_len = source.len();
    let read_len = chunk_size.max(pattern.len());

    while pos < file_len {
        let chunk = source.read_range(pos, read_len);
        if chunk.is_empty() {
            break;
        }
        if let Some(rel) = find_subslice(&chunk, pattern) {
            let start = pos + rel as u64;
            return Some((start, start + pattern.len() as u64));
        }
        if chunk.len() < read_len {
            // This window reached the snapshot's end; nothing more to scan.
            break;
        }
        let advance = read_len - overlap;
        if advance == 0 {
            // A pathological chunk_size <= overlap would never advance;
            // bail out rather than loop forever.
            break;
        }
        pos += advance as u64;
    }
    None
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_source(name: &str, contents: &[u8]) -> ByteSource {
        let dir = std::env::temp_dir().join(format!("filecommand-viewer-search-test-{}-{}", std::process::id(), name));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("file.bin");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(contents).unwrap();
        f.flush().unwrap();
        ByteSource::open(&path).unwrap()
    }

    #[test]
    fn finds_a_match_within_a_single_chunk() {
        let src = temp_source("single-chunk", b"the quick brown fox jumps over the lazy dog");
        let found = find_forward(&src, 0, b"brown", DEFAULT_CHUNK_SIZE);
        assert_eq!(found, Some((10, 15)));
    }

    #[test]
    fn returns_none_when_pattern_absent() {
        let src = temp_source("absent", b"the quick brown fox");
        assert_eq!(find_forward(&src, 0, b"zebra", DEFAULT_CHUNK_SIZE), None);
    }

    #[test]
    fn search_starts_at_the_given_offset() {
        let src = temp_source("start-offset", b"aaa needle aaa needle aaa");
        // First "needle" is at offset 4; searching from offset 5 must skip
        // it and find the second occurrence at offset 15.
        let found = find_forward(&src, 5, b"needle", DEFAULT_CHUNK_SIZE);
        assert_eq!(found, Some((15, 21)));
    }

    #[test]
    fn match_straddling_a_chunk_boundary_is_found() {
        // Pattern "abcdef" (len 6, overlap 5) placed so it starts at offset
        // 5 and ends at offset 11 — with a small chunk size the first
        // window (0..8) only contains part of the pattern.
        let mut content = vec![b'x'; 5];
        content.extend_from_slice(b"abcdef");
        content.extend_from_slice(&[b'y'; 20]);
        let src = temp_source("boundary", &content);
        let found = find_forward(&src, 0, b"abcdef", 8);
        assert_eq!(found, Some((5, 11)));
    }

    #[test]
    fn empty_pattern_never_matches() {
        let src = temp_source("empty-pattern", b"anything");
        assert_eq!(find_forward(&src, 0, b"", DEFAULT_CHUNK_SIZE), None);
    }

    #[test]
    fn each_search_step_reads_only_a_bounded_chunk_window() {
        // A large file with the match near the very start: search must
        // report it after touching only a small, bounded prefix, not the
        // whole file (viewer: F7 streaming search — "Search is bounded and
        // streaming").
        let mut content = b"NEEDLE".to_vec();
        content.extend(std::iter::repeat_n(b'z', 5_000_000));
        let src = temp_source("bounded-large", &content);
        let found = find_forward(&src, 0, b"NEEDLE", DEFAULT_CHUNK_SIZE);
        assert_eq!(found, Some((0, 6)));
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// Planting a known pattern at a known offset inside otherwise
            /// arbitrary content must always be found at that offset,
            /// regardless of chunk size relative to the pattern length.
            #[test]
            fn planted_pattern_is_always_found(
                prefix in prop::collection::vec(prop::sample::select(vec![b'x', b'y', b'z']), 0..300),
                suffix in prop::collection::vec(prop::sample::select(vec![b'x', b'y', b'z']), 0..300),
                chunk_size in 4usize..64,
            ) {
                let pattern = b"NEEDLE".to_vec();
                let mut content = prefix.clone();
                content.extend_from_slice(&pattern);
                content.extend_from_slice(&suffix);
                let expected_start = prefix.len() as u64;

                let dir = std::env::temp_dir().join(format!(
                    "filecommand-viewer-search-proptest-{}-{}",
                    std::process::id(),
                    expected_start
                ));
                let _ = std::fs::create_dir_all(&dir);
                let path = dir.join("file.bin");
                std::fs::write(&path, &content).unwrap();
                let src = ByteSource::open(&path).unwrap();

                let found = find_forward(&src, 0, &pattern, chunk_size);
                prop_assert_eq!(found, Some((expected_start, expected_start + pattern.len() as u64)));
            }
        }
    }
}
