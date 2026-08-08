//! The F3 read-only viewer's full-screen renderer: header row, text/hex
//! body, and its own F-key bar. A frame-less view that replaces the panels
//! while open (viewer: Frame-less full-screen chrome), rendered as a pure
//! function of `ViewerState` plus an already-open `ByteSource` window into
//! the file (design D1). The byte reads it performs are bounded lookups
//! into a memory map (or a small positioned read on the chunk-read
//! fallback) — never a whole-file scan, matching the "instant open, no line
//! index" contract the core `viewer` module implements.

use filecommand_core::listing::{display_width, format_count, pad_to_width};
use filecommand_core::theme::{ColorDepth, Role, Theme};
use filecommand_core::viewer::decode::{clip_line, decode_lossy, sanitize_for_display, wrap_line};
use filecommand_core::viewer::hex::{hex_rows, HEX_BYTES_PER_ROW};
use filecommand_core::viewer::search::DEFAULT_CHUNK_SIZE;
use filecommand_core::viewer::{percent_through, ByteSource, ViewMode, ViewerState};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::style::role_style;

/// The viewer F-key bar's fixed slots; slot 4's label swaps `Hex`/`ASCII` by
/// mode (viewer: Frame-less full-screen chrome — "Viewer F-key bar
/// contents"; Text and hex modes — "F4 toggles mode and label").
const KEY_NUMBERS: [&str; 5] = ["1", "2", "4", "7", "10"];
const KEY_LABELS_STATIC: [&str; 5] = ["Help", "Unwrap", "", "Search", "Quit"];

/// Rows available for the viewer body at a given terminal height: the
/// header and the F-key bar each reserve one row. Exposed so the TUI's
/// input routing can compute the same page size the renderer uses.
pub fn body_rows(term_height: u16) -> usize {
    term_height.saturating_sub(2) as usize
}

/// Render the whole viewer: header, text/hex body, and F-key bar.
/// `source` is `None` only in the brief window before the first frame after
/// `Effect::OpenViewer` resolves (or if the byte source could not be kept
/// open) — the body then renders as blank rather than reading anything.
pub fn render_viewer(buf: &mut Buffer, area: Rect, viewer: &ViewerState, theme: &Theme, depth: ColorDepth, source: Option<&ByteSource>) {
    if area.width == 0 || area.height < 2 {
        return;
    }
    let header_style = role_style(theme, Role::ViewerHeader, depth);
    let text_style = role_style(theme, Role::ViewerText, depth);
    let match_style = role_style(theme, Role::ViewerMatch, depth);
    let w = area.width as usize;

    render_header(buf, area.x, area.y, w, viewer, header_style);

    let body_h = area.height.saturating_sub(2);
    let body_y = area.y + 1;
    for row in 0..body_h {
        buf.set_string(area.x, body_y + row, " ".repeat(w), text_style);
    }
    if let Some(source) = source {
        match viewer.mode {
            ViewMode::Text => render_text_body(buf, area.x, body_y, w, body_h as usize, viewer, source, text_style, match_style),
            ViewMode::Hex => render_hex_body(buf, area.x, body_y, w, body_h as usize, viewer, source, text_style, match_style),
        }
    }

    let keybar_area = Rect { x: area.x, y: area.y + area.height - 1, width: area.width, height: 1 };
    render_viewer_keybar(buf, keybar_area, viewer, theme, depth);
}

fn render_header(buf: &mut Buffer, x: u16, y: u16, w: usize, viewer: &ViewerState, style: Style) {
    let filename = viewer
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| viewer.path.display().to_string());
    let pct = percent_through(viewer.top_offset, viewer.file_len);
    let pos = match viewer.mode {
        ViewMode::Text => format!("Col {}", viewer.h_scroll),
        ViewMode::Hex => format!("Offset {:08X}", viewer.top_offset),
    };
    let right = format!("{pos}   Size {}   {pct}%", format_count(viewer.file_len as usize));
    let left = format!(" {filename}");
    let gap = w.saturating_sub(display_width(&left) + display_width(&right) + 1);
    let line = format!("{left}{}{right} ", " ".repeat(gap));
    buf.set_string(x, y, pad_to_width(&line, w), style);
}

/// The viewer F-key bar: `1Help 2Unwrap 4Hex 7Search 10Quit`, with slot 4's
/// label swapping to `ASCII` while in hex mode.
pub fn render_viewer_keybar(buf: &mut Buffer, area: Rect, viewer: &ViewerState, theme: &Theme, depth: ColorDepth) {
    if area.height == 0 {
        return;
    }
    let number_style = role_style(theme, Role::KeybarNumber, depth);
    let label_style = role_style(theme, Role::KeybarLabel, depth);
    buf.set_string(area.x, area.y, " ".repeat(area.width as usize), label_style);

    let mut labels = KEY_LABELS_STATIC;
    labels[2] = viewer.mode.toggle_label();

    let mut x = area.x;
    let right_edge = area.x + area.width;
    for (i, (num, label)) in KEY_NUMBERS.iter().zip(labels.iter()).enumerate() {
        if i > 0 {
            if x >= right_edge {
                break;
            }
            x += 1;
        }
        if x >= right_edge {
            break;
        }
        buf.set_string(x, area.y, num, number_style);
        x += num.chars().count() as u16;
        if x >= right_edge {
            break;
        }
        buf.set_string(x, area.y, label, label_style);
        x += label.chars().count() as u16;
    }
}

// ---------------------------------------------------------------------
// Text mode
// ---------------------------------------------------------------------

/// Split a byte window into logical lines, each paired with its absolute
/// starting offset, with a trailing `\r` excluded from the slice so CRLF and
/// LF line endings decode identically (mirrors
/// `filecommand_core::viewer::decode::logical_lines`, but byte-offset
/// aware so match highlighting can locate a hit within its source line).
fn raw_lines_stripped(raw: &[u8], base: u64) -> Vec<(u64, &[u8])> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (i, &b) in raw.iter().enumerate() {
        if b == b'\n' {
            let mut end = i;
            if end > start && raw[end - 1] == b'\r' {
                end -= 1;
            }
            out.push((base + start as u64, &raw[start..end]));
            start = i + 1;
        }
    }
    out.push((base + start as u64, &raw[start..]));
    out
}

struct BodyRow {
    text: String,
    origin_line: usize,
    /// The display-column offset (within the logical line, before
    /// clip/wrap) that `text`'s first character represents — `h_scroll` in
    /// unwrap mode, or the cumulative width of prior wrapped rows in wrap
    /// mode. Lets match highlighting map an absolute column back to a
    /// screen cell without re-deriving wrap/clip state.
    col_offset: usize,
}

fn build_text_rows(viewer: &ViewerState, lines: &[(u64, &[u8])], width: usize, max_rows: usize) -> Vec<BodyRow> {
    let width = width.max(1);
    let mut rows = Vec::new();
    for (idx, (_abs, bytes)) in lines.iter().enumerate() {
        if rows.len() >= max_rows {
            break;
        }
        let text = sanitize_for_display(&decode_lossy(bytes));
        if viewer.wrap {
            let mut col = 0usize;
            for w_row in wrap_line(&text, width) {
                if rows.len() >= max_rows {
                    break;
                }
                let w = display_width(&w_row);
                rows.push(BodyRow { text: w_row, origin_line: idx, col_offset: col });
                col += w;
            }
        } else {
            let clipped = clip_line(&text, viewer.h_scroll, width);
            rows.push(BodyRow { text: clipped, origin_line: idx, col_offset: viewer.h_scroll });
        }
    }
    rows
}

/// The last search match's location, expressed as display columns within
/// its source logical line, when that line falls inside the currently
/// decoded window. `None` when there is no match, or it does not (fully)
/// fall within this window/line — a cross-line match is not highlighted,
/// a deliberate simplification given literal search patterns are short and
/// this is a display nicety, not a correctness requirement.
fn compute_match_span(viewer: &ViewerState, lines: &[(u64, &[u8])]) -> Option<(usize, usize, usize)> {
    let (m_start, m_end) = viewer.last_match?;
    if m_end <= m_start {
        return None;
    }
    for (idx, (abs_start, bytes)) in lines.iter().enumerate() {
        let abs_end = abs_start + bytes.len() as u64;
        if m_start >= *abs_start && m_end <= abs_end {
            let rel_start = (m_start - abs_start) as usize;
            let rel_end = (m_end - abs_start) as usize;
            let pre = sanitize_for_display(&decode_lossy(&bytes[..rel_start]));
            let matched = sanitize_for_display(&decode_lossy(&bytes[rel_start..rel_end]));
            let col_start = display_width(&pre);
            let col_end = col_start + display_width(&matched).max(1);
            return Some((idx, col_start, col_end));
        }
    }
    None
}

/// Re-walk `row.text`'s characters to paint the sub-range that overlaps
/// `[col_start, col_end)` (in the logical line's display columns) with
/// `match_style`.
fn paint_match_overlay(buf: &mut Buffer, x: u16, y: u16, row: &BodyRow, col_start: usize, col_end: usize, match_style: Style) {
    let row_start = row.col_offset;
    let row_end = row.col_offset + display_width(&row.text);
    let overlap_start = col_start.max(row_start);
    let overlap_end = col_end.min(row_end);
    if overlap_start >= overlap_end {
        return;
    }
    let rel_start = overlap_start - row_start;
    let rel_end = overlap_end - row_start;
    let mut acc = 0usize;
    for ch in row.text.chars() {
        let cw = display_width(&ch.to_string()).max(1);
        if acc >= rel_start && acc < rel_end {
            buf.set_string(x + acc as u16, y, ch.to_string(), match_style);
        }
        acc += cw;
    }
}

/// Bytes of margin read on each side of the visible window before decoding,
/// so a multi-byte UTF-8 character split by `forward::next_line_start`'s /
/// `backward::previous_line_start`'s byte-count hard-split (when no newline
/// is found within their cap) is never fed to `decode_lossy` starting or
/// ending mid-sequence — the margin `viewer::decode::decode_lossy`'s own doc
/// comment calls for. 3 bytes covers the longest run of continuation bytes
/// in a valid UTF-8 sequence (a 4-byte encoding has 3).
const UTF8_BOUNDARY_MARGIN: usize = 3;

fn is_utf8_continuation_byte(b: u8) -> bool {
    b & 0b1100_0000 == 0b1000_0000
}

/// The encoded length of the UTF-8 sequence led by `b`, or `1` for an ASCII
/// byte or an invalid lead byte (`decode_lossy` handles the actual
/// substitution; this is only used to decide where to trim a possibly
/// truncated trailing sequence).
fn utf8_sequence_len(b: u8) -> usize {
    if b & 0b1000_0000 == 0 {
        1
    } else if b & 0b1110_0000 == 0b1100_0000 {
        2
    } else if b & 0b1111_0000 == 0b1110_0000 {
        3
    } else if b & 0b1111_1000 == 0b1111_0000 {
        4
    } else {
        1
    }
}

/// Read the visible text window with a small margin on each side (see
/// [`UTF8_BOUNDARY_MARGIN`]), then trim back to the nearest valid UTF-8
/// boundary at both ends. Returns the trimmed bytes and the absolute offset
/// its first byte sits at.
///
/// Without this, a hard-split anchor landing mid-character — which the
/// bounded backward/forward scans can do, since they split purely by byte
/// count when no newline is found within their cap — would decode as a
/// spurious replacement character right at the render window's edge.
fn read_text_window(source: &ByteSource, top_offset: u64, chunk_size: usize) -> (Vec<u8>, u64) {
    let margin_start = top_offset.min(UTF8_BOUNDARY_MARGIN as u64);
    let read_start = top_offset - margin_start;
    let raw = source.read_range(read_start, chunk_size + margin_start as usize + UTF8_BOUNDARY_MARGIN);

    // Skip forward past any continuation bytes at the front: they belong to
    // a character that started before the window (possibly mid-character at
    // `top_offset` itself), which cannot be decoded correctly without the
    // rest of that character. Landing on the next full character instead of
    // the fragment is what avoids the spurious replacement character.
    let mut start = margin_start as usize;
    while start < raw.len() && is_utf8_continuation_byte(raw[start]) {
        start += 1;
    }

    // Trim a truncated sequence at the back the same way: a lead byte near
    // the end whose continuation bytes were not all read is left for the
    // next window rather than decoded as a spurious replacement character.
    let mut end = raw.len();
    let scan_from = end.saturating_sub(UTF8_BOUNDARY_MARGIN + 1).max(start);
    for i in (scan_from..end).rev() {
        if is_utf8_continuation_byte(raw[i]) {
            continue;
        }
        let seq_len = utf8_sequence_len(raw[i]);
        if seq_len > 1 && i + seq_len > end {
            end = i;
        }
        break;
    }

    let window = if start < end { raw[start..end].to_vec() } else { Vec::new() };
    (window, read_start + start as u64)
}

/// `pub(crate)` rather than private: Quick View mode (additional-panel-
/// modes "Quick View preview of the opposite panel cursor file"; design D7)
/// reuses this exact renderer against a synthetic wrap-on `ViewerState`
/// rather than duplicating the text-layout logic.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_text_body(
    buf: &mut Buffer,
    x: u16,
    body_y: u16,
    w: usize,
    body_rows_count: usize,
    viewer: &ViewerState,
    source: &ByteSource,
    text_style: Style,
    match_style: Style,
) {
    let (raw, base) = read_text_window(source, viewer.top_offset, DEFAULT_CHUNK_SIZE);
    let lines = raw_lines_stripped(&raw, base);
    let rows = build_text_rows(viewer, &lines, w, body_rows_count);
    let match_span = compute_match_span(viewer, &lines);
    for (i, row) in rows.iter().enumerate() {
        let y = body_y + i as u16;
        buf.set_string(x, y, pad_to_width(&row.text, w), text_style);
        if let Some((m_line, col_start, col_end)) = match_span {
            if row.origin_line == m_line {
                paint_match_overlay(buf, x, y, row, col_start, col_end, match_style);
            }
        }
    }
}

// ---------------------------------------------------------------------
// Hex mode
// ---------------------------------------------------------------------

const HEX_OFFSET_W: usize = 8;
const HEX_GAP: usize = 2;
const HEX_FIELD_W: usize = HEX_BYTES_PER_ROW * 3 - 1;
const HEX_ASCII_START: usize = HEX_OFFSET_W + HEX_GAP + HEX_FIELD_W + HEX_GAP;

#[allow(clippy::too_many_arguments)]
fn render_hex_body(
    buf: &mut Buffer,
    x: u16,
    body_y: u16,
    w: usize,
    body_rows_count: usize,
    viewer: &ViewerState,
    source: &ByteSource,
    text_style: Style,
    match_style: Style,
) {
    let read_len = body_rows_count.saturating_mul(HEX_BYTES_PER_ROW);
    let raw = source.read_range(viewer.top_offset, read_len);
    let rows = hex_rows(&raw, viewer.top_offset);
    for (i, row) in rows.iter().take(body_rows_count).enumerate() {
        let y = body_y + i as u16;
        let line = format!("{:08X}{}{}{}{}", row.offset, " ".repeat(HEX_GAP), row.hex_field(), " ".repeat(HEX_GAP), row.ascii_gutter());
        buf.set_string(x, y, pad_to_width(&line, w), text_style);

        let Some((m_start, m_end)) = viewer.last_match else { continue };
        for (bi, byte) in row.bytes.iter().enumerate() {
            let abs = row.offset + bi as u64;
            if abs < m_start || abs >= m_end {
                continue;
            }
            let hex_col = HEX_OFFSET_W + HEX_GAP + bi * 3;
            if hex_col + 1 < w {
                buf.set_string(x + hex_col as u16, y, format!("{byte:02X}"), match_style);
            }
            let ascii_col = HEX_ASCII_START + bi;
            if ascii_col < w {
                let ch = if (0x20..=0x7e).contains(byte) { *byte as char } else { '.' };
                buf.set_string(x + ascii_col as u16, y, ch.to_string(), match_style);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use filecommand_core::viewer::ViewerState;
    use std::io::Write;
    use std::path::PathBuf;

    fn temp_source(name: &str, contents: &[u8]) -> ByteSource {
        let dir = std::env::temp_dir().join(format!("filecommand-tui-viewer-render-test-{}-{}", std::process::id(), name));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("file.bin");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(contents).unwrap();
        f.flush().unwrap();
        ByteSource::open(&path).unwrap()
    }

    fn render(viewer: &ViewerState, source: Option<&ByteSource>, area: Rect) -> String {
        let mut buf = Buffer::empty(area);
        render_viewer(&mut buf, area, viewer, &Theme::classic(), ColorDepth::Ansi16, source);
        (0..area.height).map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn text_mode_renders_decoded_lines_from_the_top_offset() {
        let src = temp_source("text-basic", b"first line\nsecond line\nthird line\n");
        let viewer = ViewerState::new(PathBuf::from("notes.txt"), src.len());
        let text = render(&viewer, Some(&src), Rect { x: 0, y: 0, width: 40, height: 6 });
        assert!(text.contains("first line"), "{text}");
        assert!(text.contains("second line"), "{text}");
        assert!(text.contains("notes.txt"), "header shows the filename:\n{text}");
    }

    #[test]
    fn hex_mode_renders_offset_hex_and_ascii_gutter() {
        let src = temp_source("hex-basic", b"AB\x00\xffCD");
        let mut viewer = ViewerState::new(PathBuf::from("bin.dat"), src.len());
        viewer.mode = ViewMode::Hex;
        let text = render(&viewer, Some(&src), Rect { x: 0, y: 0, width: 70, height: 6 });
        assert!(text.contains("00000000"), "offset column:\n{text}");
        assert!(text.contains("41 42 00 FF 43 44"), "hex bytes:\n{text}");
        assert!(text.contains("AB..CD"), "ascii gutter:\n{text}");
    }

    #[test]
    fn f4_label_swaps_between_hex_and_ascii() {
        let src = temp_source("label-swap", b"x");
        let mut viewer = ViewerState::new(PathBuf::from("f.txt"), src.len());
        let text_mode = render(&viewer, Some(&src), Rect { x: 0, y: 0, width: 40, height: 5 });
        assert!(text_mode.lines().last().unwrap().contains("4Hex"), "{text_mode}");

        viewer.mode = ViewMode::Hex;
        let hex_mode = render(&viewer, Some(&src), Rect { x: 0, y: 0, width: 40, height: 5 });
        assert!(hex_mode.lines().last().unwrap().contains("4ASCII"), "{hex_mode}");
    }

    #[test]
    fn keybar_matches_the_spec_string() {
        let src = temp_source("keybar", b"x");
        let viewer = ViewerState::new(PathBuf::from("f.txt"), src.len());
        let text = render(&viewer, Some(&src), Rect { x: 0, y: 0, width: 80, height: 5 });
        let last = text.lines().last().unwrap();
        assert!(last.trim_end().starts_with("1Help 2Unwrap 4Hex 7Search 10Quit"), "`{last}`");
    }

    #[test]
    fn wrap_reflows_a_long_line_across_multiple_rows() {
        let src = temp_source("wrap", b"0123456789ABCDEFGHIJ");
        let mut viewer = ViewerState::new(PathBuf::from("f.txt"), src.len());
        viewer.wrap = true;
        let text = render(&viewer, Some(&src), Rect { x: 0, y: 0, width: 10, height: 6 });
        assert!(text.contains("0123456789"), "{text}");
        assert!(text.contains("ABCDEFGHIJ"), "{text}");
    }

    #[test]
    fn unwrap_clips_at_the_horizontal_scroll_offset() {
        let src = temp_source("unwrap-scroll", b"0123456789ABCDEFGHIJ");
        let mut viewer = ViewerState::new(PathBuf::from("f.txt"), src.len());
        viewer.h_scroll = 10;
        let text = render(&viewer, Some(&src), Rect { x: 0, y: 0, width: 10, height: 5 });
        assert!(text.contains("ABCDEFGHIJ"), "{text}");
        assert!(!text.contains("0123456789"), "the pre-scroll content should be clipped away:\n{text}");
    }

    #[test]
    fn text_mode_match_is_highlighted_with_the_viewer_match_role() {
        let src = temp_source("match-text", b"the quick brown fox");
        let mut viewer = ViewerState::new(PathBuf::from("f.txt"), src.len());
        viewer.last_match = Some((4, 9)); // "quick"
        let area = Rect { x: 0, y: 0, width: 40, height: 5 };
        let mut buf = Buffer::empty(area);
        render_viewer(&mut buf, area, &viewer, &Theme::classic(), ColorDepth::Ansi16, Some(&src));
        let match_style = role_style(&Theme::classic(), Role::ViewerMatch, ColorDepth::Ansi16);
        // "quick" starts at column 4 on the body's first row (y=1).
        assert_eq!(Some(buf[(4, 1)].fg), match_style.fg);
        assert_eq!(Some(buf[(4, 1)].bg), match_style.bg);
        assert_eq!(Some(buf[(3, 1)].bg), role_style(&Theme::classic(), Role::ViewerText, ColorDepth::Ansi16).bg);
    }

    #[test]
    fn hex_mode_match_is_highlighted_in_both_hex_and_ascii_columns() {
        let src = temp_source("match-hex", b"ABCDEFGH");
        let mut viewer = ViewerState::new(PathBuf::from("f.bin"), src.len());
        viewer.mode = ViewMode::Hex;
        viewer.last_match = Some((2, 4)); // bytes 'C','D'
        let area = Rect { x: 0, y: 0, width: 70, height: 5 };
        let mut buf = Buffer::empty(area);
        render_viewer(&mut buf, area, &viewer, &Theme::classic(), ColorDepth::Ansi16, Some(&src));
        let match_style = role_style(&Theme::classic(), Role::ViewerMatch, ColorDepth::Ansi16);
        let hex_col = HEX_OFFSET_W + HEX_GAP + 2 * 3;
        assert_eq!(Some(buf[(hex_col as u16, 1)].fg), match_style.fg);
        let ascii_col = HEX_ASCII_START + 2;
        assert_eq!(Some(buf[(ascii_col as u16, 1)].fg), match_style.fg);
    }

    #[test]
    fn no_source_renders_header_and_keybar_but_a_blank_body() {
        let viewer = ViewerState::new(PathBuf::from("f.txt"), 100);
        let text = render(&viewer, None, Rect { x: 0, y: 0, width: 40, height: 5 });
        assert!(text.contains("f.txt"));
        assert!(text.contains("1Help"));
    }

    #[test]
    fn text_mode_render_does_not_corrupt_a_multibyte_char_split_by_the_hard_split_cap() {
        use filecommand_core::viewer::backward::DEFAULT_MAX_LINE_LEN;
        use filecommand_core::viewer::forward::next_line_start;

        // A long newline-free run of 3-byte CJK characters, comfortably past
        // the 64 KiB hard-split cap so `next_line_start` must hard-split
        // somewhere in the run; 65536 (the cap) is not a multiple of 3, so
        // the split necessarily lands mid-character.
        let ch = '\u{4e2d}'; // "中", 3 UTF-8 bytes
        let content: String = std::iter::repeat_n(ch, 30_000).collect();
        let src = temp_source("cjk-hard-split", content.as_bytes());

        let split_at = next_line_start(&src, 0, DEFAULT_MAX_LINE_LEN);
        assert_eq!(split_at, DEFAULT_MAX_LINE_LEN as u64, "sanity: the hard split lands exactly at the cap");
        assert_ne!(split_at % 3, 0, "sanity: the split point must land mid-character for this fixture to be meaningful");

        let mut viewer = ViewerState::new(PathBuf::from("cjk.txt"), src.len());
        viewer.top_offset = split_at;
        let text = render(&viewer, Some(&src), Rect { x: 0, y: 0, width: 40, height: 6 });
        assert!(!text.contains('\u{fffd}'), "spurious replacement character at the render window boundary:\n{text}");
    }

    #[test]
    fn body_rows_reserves_the_header_and_keybar_rows() {
        assert_eq!(body_rows(24), 22);
        assert_eq!(body_rows(1), 0);
        assert_eq!(body_rows(0), 0);
    }
}
