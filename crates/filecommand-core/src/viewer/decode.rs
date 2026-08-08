//! Text-mode decoding and layout: lossy UTF-8 decode of the visible byte
//! window only, display-width-aware column math reusing `listing`'s
//! established rendering discipline, and the F2 wrap/unwrap re-flow
//! (design D5).

use crate::listing::display_width;

/// Decode `bytes` as UTF-8, substituting the replacement character for
/// invalid sequences and continuing to decode the rest of the window
/// (viewer: Text and hex modes — "Lossy UTF-8 decode of invalid bytes").
/// Callers pass only the visible byte window (plus a small margin to avoid
/// splitting a multi-byte sequence at the edge) — this never runs over the
/// whole file.
pub fn decode_lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Split decoded text into logical lines on `\n`, trimming a trailing `\r`
/// from each so CRLF and LF line endings render identically.
pub fn logical_lines(text: &str) -> Vec<String> {
    text.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line).to_string()).collect()
}

/// Replace control and zero-width characters with the replacement character
/// so display-width math (and the `Col` indicator) stays consistent
/// (design D5, reusing `listing`'s rendering discipline).
pub fn sanitize_for_display(s: &str) -> String {
    s.chars().map(|c| if c.is_control() || display_width(&c.to_string()) == 0 { '\u{fffd}' } else { c }).collect()
}

/// Clip a sanitized logical line to `width` display columns starting at
/// display column `h_scroll` (unwrap mode). A character whose width would
/// straddle either edge of the window is dropped rather than split (viewer:
/// F2 wrap and unwrap toggle — "Unwrap clips with horizontal scroll").
pub fn clip_line(line: &str, h_scroll: usize, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut col = 0usize;
    for ch in line.chars() {
        let cw = display_width(&ch.to_string()).max(1);
        if col >= h_scroll + width {
            break;
        }
        if col >= h_scroll && col + cw <= h_scroll + width {
            out.push(ch);
        }
        col += cw;
    }
    out
}

/// Re-flow a sanitized logical line into rows of at most `width` display
/// columns each (wrap mode). An empty line yields a single empty row so
/// blank lines still occupy a screen row (viewer: F2 wrap and unwrap toggle
/// — "Wrap re-flows at viewport width").
pub fn wrap_line(line: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![line.to_string()];
    }
    let mut rows = Vec::new();
    let mut current = String::new();
    let mut col = 0usize;
    for ch in line.chars() {
        let cw = display_width(&ch.to_string()).max(1);
        if col + cw > width && !current.is_empty() {
            rows.push(std::mem::take(&mut current));
            col = 0;
        }
        current.push(ch);
        col += cw;
    }
    rows.push(current);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_lossy_substitutes_replacement_chars_for_invalid_bytes() {
        let mut bytes = b"before ".to_vec();
        bytes.extend_from_slice(&[0xff, 0xfe]); // invalid UTF-8
        bytes.extend_from_slice(b" after");
        let text = decode_lossy(&bytes);
        assert!(text.contains('\u{fffd}'));
        assert!(text.starts_with("before "));
        assert!(text.ends_with(" after"));
    }

    #[test]
    fn decode_lossy_passes_through_valid_utf8_unchanged() {
        assert_eq!(decode_lossy("héllo wörld".as_bytes()), "héllo wörld");
    }

    #[test]
    fn logical_lines_splits_on_newline_and_strips_cr() {
        assert_eq!(logical_lines("a\r\nb\nc"), vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        assert_eq!(logical_lines(""), vec!["".to_string()]);
    }

    #[test]
    fn sanitize_replaces_control_chars_but_keeps_printable() {
        let sanitized = sanitize_for_display("a\u{7}b\u{0}c");
        assert_eq!(sanitized, "a\u{fffd}b\u{fffd}c");
    }

    #[test]
    fn sanitize_replaces_zero_width_characters() {
        // U+200B ZERO WIDTH SPACE has display width 0.
        let sanitized = sanitize_for_display("a\u{200b}b");
        assert_eq!(sanitized, "a\u{fffd}b");
    }

    #[test]
    fn clip_line_slices_at_the_horizontal_offset() {
        let line = "0123456789";
        assert_eq!(clip_line(line, 0, 4), "0123");
        assert_eq!(clip_line(line, 4, 4), "4567");
        assert_eq!(clip_line(line, 8, 4), "89");
        assert_eq!(clip_line(line, 20, 4), "");
    }

    #[test]
    fn wrap_line_re_flows_at_viewport_width() {
        let rows = wrap_line("0123456789", 4);
        assert_eq!(rows, vec!["0123".to_string(), "4567".to_string(), "89".to_string()]);
        for row in &rows {
            assert!(display_width(row) <= 4);
        }
    }

    #[test]
    fn wrap_line_empty_line_yields_one_empty_row() {
        assert_eq!(wrap_line("", 10), vec!["".to_string()]);
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn wrap_line_rows_never_exceed_width(line in "[a-zA-Z0-9 ]{0,80}", width in 1usize..40) {
                let rows = wrap_line(&line, width);
                for row in &rows {
                    prop_assert!(display_width(row) <= width);
                }
                // No character is lost or duplicated by re-flowing.
                let joined: String = rows.concat();
                prop_assert_eq!(joined, line);
            }

            #[test]
            fn clip_line_never_exceeds_requested_width(line in "[a-zA-Z0-9 ]{0,80}", h_scroll in 0usize..80, width in 1usize..40) {
                let clipped = clip_line(&line, h_scroll, width);
                prop_assert!(display_width(&clipped) <= width);
            }
        }
    }
}
