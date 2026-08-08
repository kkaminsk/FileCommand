//! The F4 built-in editor's core buffer model (design D1/D2).
//!
//! `EditorState` is an in-memory `Vec<String>` line buffer with no `mmap` —
//! unlike the viewer, which streams multi-GB files, the editor loads the
//! whole file up front and is gated by [`EDITOR_MAX_BYTES`] (design D1).
//! Column addressing is by `char` count, not byte or grapheme, matching the
//! deliberately minimal v1 floor (§4.6): no regex, single-level undo, and
//! whole-line-only selection so cut/copy/paste never has to reason about
//! grapheme boundaries.
//!
//! File I/O (`open`/`save`) is a thin wrapper around `std::fs`, routed
//! through [`crate::fs_ops::path::to_extended_path`] for the `\\?\`
//! long-path abstraction (design D9) — mirroring how [`crate::config`] and
//! the viewer's `byte_source` already do direct-but-extended-path-aware
//! whole-file I/O, while [`crate::fs_ops::fs::FileSystem`] remains reserved
//! for the copy/move/delete file-management engine it already models. Every
//! other method here is a pure buffer transformation, unit-testable without
//! touching a filesystem or a terminal (§8).

use std::io;
use std::path::{Path, PathBuf};

use crate::fs_ops::path::to_extended_path;
use crate::viewer::decode::{decode_lossy, logical_lines};

/// Files at or above this size are redirected to the F3 viewer instead of
/// being loaded into the editor (builtin-editor "Editor invocation and size
/// cap"; design D1). 10 MiB, informally "10 MB".
pub const EDITOR_MAX_BYTES: u64 = 10 * 1024 * 1024;

/// Whether a file of `size` bytes is small enough for the built-in editor to
/// load. A pure predicate over the byte count so the size-cap boundary is
/// unit-testable without writing a multi-megabyte fixture to disk.
pub fn fits_editor_cap(size: u64) -> bool {
    size < EDITOR_MAX_BYTES
}

/// The dominant line-ending convention detected on load and preserved on
/// save (design D2) so a CRLF file stays CRLF and an LF file stays LF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    Crlf,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::Crlf => "\r\n",
        }
    }

    /// Detect the dominant line ending in `bytes` by counting CRLF vs
    /// bare-LF terminators and picking the majority. Ties (including a file
    /// with no newlines at all) default to LF.
    pub fn detect(bytes: &[u8]) -> LineEnding {
        let mut crlf = 0usize;
        let mut lf_only = 0usize;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                if i > 0 && bytes[i - 1] == b'\r' {
                    crlf += 1;
                } else {
                    lf_only += 1;
                }
            }
        }
        if crlf > lf_only {
            LineEnding::Crlf
        } else {
            LineEnding::Lf
        }
    }
}

/// Insert vs overwrite text-entry mode (builtin-editor "Insert and
/// overwrite text entry").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryMode {
    Insert,
    Overwrite,
}

impl EntryMode {
    pub fn toggle(self) -> EntryMode {
        match self {
            EntryMode::Insert => EntryMode::Overwrite,
            EntryMode::Overwrite => EntryMode::Insert,
        }
    }
}

/// The caret position: a zero-based line index and a zero-based `char`
/// column within that line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Caret {
    pub line: usize,
    pub col: usize,
}

/// The result of a size-gated open attempt (builtin-editor "Editor
/// invocation and size cap").
#[derive(Debug)]
pub enum LoadResult {
    Loaded(EditorState),
    /// The file is `size` bytes, at or above [`EDITOR_MAX_BYTES`]; the
    /// caller should redirect to the F3 viewer instead.
    TooLarge { size: u64 },
}

/// The core, terminal-independent state of one open F4 editor session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorState {
    pub path: PathBuf,
    pub lines: Vec<String>,
    pub caret: Caret,
    pub mode: EntryMode,
    pub line_ending: LineEnding,
    /// The line-selection anchor set by F3 (Mark): `Some(line)` forms the
    /// `[anchor_line, caret.line]` range with the caret (design D1). `None`
    /// when no selection is active.
    pub selection_anchor: Option<usize>,
    /// The line clipboard: whole lines most recently cut or copied.
    pub clipboard: Vec<String>,
    /// The single prior-state snapshot single-level undo swaps to (design
    /// D1's "single snapshot batch"). Consumed (`take`n) by [`Self::undo`],
    /// so a second consecutive undo is a no-op rather than reaching further
    /// back (builtin-editor "Single-level undo").
    undo_snapshot: Option<Vec<String>>,
    /// The buffer as last saved (or as loaded, before any edit). The
    /// modified indicator is derived state: `lines != saved_snapshot`
    /// (builtin-editor "Modified indicator and save-on-exit prompt"; design
    /// D2).
    saved_snapshot: Vec<String>,
    /// The last-run literal search/replace pattern, if any.
    pub search_pattern: Option<String>,
}

impl EditorState {
    /// Build a fresh editor session from already-read file bytes: UTF-8
    /// lossy decode, split into logical lines, dominant line ending
    /// detected from the raw bytes, caret at the start of the first line.
    /// Pure — no I/O — so load behavior is unit-testable without a temp
    /// file (builtin-editor "Small file opens in the editor").
    pub fn from_bytes(path: PathBuf, bytes: &[u8]) -> EditorState {
        let text = decode_lossy(bytes);
        let lines = logical_lines(&text);
        EditorState {
            path,
            lines: lines.clone(),
            caret: Caret::default(),
            mode: EntryMode::Insert,
            line_ending: LineEnding::detect(bytes),
            selection_anchor: None,
            clipboard: Vec::new(),
            undo_snapshot: None,
            saved_snapshot: lines,
            search_pattern: None,
        }
    }

    /// Open `path` for editing: stat it first and redirect to
    /// [`LoadResult::TooLarge`] without reading its content when it is at
    /// or above [`EDITOR_MAX_BYTES`] (builtin-editor "Large file redirects
    /// to the viewer"), otherwise read and decode it in full. Routed
    /// through [`to_extended_path`] for the `\\?\` long-path abstraction
    /// (design D9).
    pub fn open(path: &Path) -> io::Result<LoadResult> {
        let extended = to_extended_path(path);
        let size = std::fs::metadata(&extended)?.len();
        if !fits_editor_cap(size) {
            return Ok(LoadResult::TooLarge { size });
        }
        let bytes = std::fs::read(&extended)?;
        Ok(LoadResult::Loaded(EditorState::from_bytes(path.to_path_buf(), &bytes)))
    }

    /// Whether the buffer differs from the last-saved (or loaded) state
    /// (builtin-editor "Modified indicator appears on edit").
    pub fn is_modified(&self) -> bool {
        self.lines != self.saved_snapshot
    }

    /// F2: write the buffer back to `self.path` in place, UTF-8 encoded,
    /// using the recorded line ending between lines (so a file that ended
    /// without a trailing newline still doesn't gain one, and vice versa),
    /// then clear the modified state (builtin-editor "Save in place …").
    pub fn save(&mut self) -> io::Result<()> {
        let mut bytes = Vec::new();
        for (i, line) in self.lines.iter().enumerate() {
            if i > 0 {
                bytes.extend_from_slice(self.line_ending.as_str().as_bytes());
            }
            bytes.extend_from_slice(line.as_bytes());
        }
        std::fs::write(to_extended_path(&self.path), &bytes)?;
        self.saved_snapshot = self.lines.clone();
        Ok(())
    }

    // -- text entry -------------------------------------------------------

    pub fn toggle_mode(&mut self) {
        self.mode = self.mode.toggle();
    }

    /// Insert or overwrite one character at the caret, per the active mode,
    /// and advance the caret one cell (builtin-editor "Insert and overwrite
    /// text entry").
    pub fn type_char(&mut self, c: char) {
        self.snapshot_for_undo();
        let mut chars: Vec<char> = self.lines[self.caret.line].chars().collect();
        match self.mode {
            EntryMode::Insert => {
                let at = self.caret.col.min(chars.len());
                chars.insert(at, c);
            }
            EntryMode::Overwrite => {
                if self.caret.col < chars.len() {
                    chars[self.caret.col] = c;
                } else {
                    chars.push(c);
                }
            }
        }
        self.lines[self.caret.line] = chars.into_iter().collect();
        self.caret.col += 1;
    }

    /// Split the current line at the caret into two lines; the caret moves
    /// to the start of the new (second) line.
    pub fn insert_newline(&mut self) {
        self.snapshot_for_undo();
        let chars: Vec<char> = self.lines[self.caret.line].chars().collect();
        let at = self.caret.col.min(chars.len());
        let tail: String = chars[at..].iter().collect();
        let head: String = chars[..at].iter().collect();
        self.lines[self.caret.line] = head;
        self.lines.insert(self.caret.line + 1, tail);
        self.caret.line += 1;
        self.caret.col = 0;
    }

    /// Delete the character before the caret; at column 0 on a line after
    /// the first, this joins the current line onto the end of the previous
    /// one instead.
    pub fn backspace(&mut self) {
        if self.caret.col == 0 && self.caret.line == 0 {
            return;
        }
        self.snapshot_for_undo();
        if self.caret.col == 0 {
            let current = self.lines.remove(self.caret.line);
            let prev_len = self.lines[self.caret.line - 1].chars().count();
            self.lines[self.caret.line - 1].push_str(&current);
            self.caret.line -= 1;
            self.caret.col = prev_len;
        } else {
            let mut chars: Vec<char> = self.lines[self.caret.line].chars().collect();
            chars.remove(self.caret.col - 1);
            self.lines[self.caret.line] = chars.into_iter().collect();
            self.caret.col -= 1;
        }
    }

    // -- line-based selection, cut/copy/paste ------------------------------

    /// F3 (Mark): anchor a line selection at the caret's current line.
    pub fn start_mark(&mut self) {
        self.selection_anchor = Some(self.caret.line);
    }

    pub fn clear_mark(&mut self) {
        self.selection_anchor = None;
    }

    /// The active selection's `[start, end]` inclusive line range, ordered
    /// regardless of whether the caret is above or below the anchor, or
    /// `None` when no selection is active (builtin-editor "Marking selects
    /// whole lines").
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        self.selection_anchor.map(|anchor| if anchor <= self.caret.line { (anchor, self.caret.line) } else { (self.caret.line, anchor) })
    }

    /// Remove the selected lines, capture them to the clipboard, and settle
    /// the caret on the line that followed the removed range (builtin-editor
    /// "Cut removes and captures selected lines"). A no-op if no selection
    /// is active.
    pub fn cut_selection(&mut self) {
        let Some((start, end)) = self.selection_range() else { return };
        self.snapshot_for_undo();
        let removed: Vec<String> = self.lines.drain(start..=end).collect();
        self.clipboard = removed;
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.caret.line = start.min(self.lines.len() - 1);
        self.caret.col = 0;
        self.selection_anchor = None;
    }

    /// Capture the selected lines to the clipboard without removing them
    /// from the buffer. A no-op if no selection is active.
    pub fn copy_selection(&mut self) {
        let Some((start, end)) = self.selection_range() else { return };
        self.clipboard = self.lines[start..=end].to_vec();
    }

    /// Insert the clipboard's whole lines at the caret's line, without
    /// splitting the surrounding lines (builtin-editor "Paste inserts
    /// captured lines"). The caret settles on the first pasted line. A
    /// no-op if the clipboard is empty.
    pub fn paste(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        self.snapshot_for_undo();
        let at = self.caret.line.min(self.lines.len());
        for (i, line) in self.clipboard.iter().enumerate() {
            self.lines.insert(at + i, line.clone());
        }
        self.caret.line = at;
        self.caret.col = 0;
    }

    // -- literal search / search-and-replace ------------------------------

    /// F7: move the caret to the next literal occurrence of `pattern`
    /// after the caret's current position, wrapping to the start of the
    /// buffer if nothing is found past it. Returns whether a match was
    /// found (builtin-editor "Search moves to the next match", "Search
    /// treats input literally").
    pub fn find_next(&mut self, pattern: &str) -> bool {
        self.search_pattern = Some(pattern.to_string());
        let found = self.search_from(self.caret.line, self.caret.col + 1, pattern).or_else(|| self.search_from(0, 0, pattern));
        match found {
            Some((line, col)) => {
                self.caret.line = line;
                self.caret.col = col;
                true
            }
            None => false,
        }
    }

    /// F4 replace: find the next literal occurrence of `pattern` at or
    /// after the caret (wrapping if needed) and substitute it with
    /// `replacement`, marking the buffer modified. Returns whether a
    /// replacement happened (builtin-editor "Replace substitutes a match").
    pub fn replace_first(&mut self, pattern: &str, replacement: &str) -> bool {
        self.search_pattern = Some(pattern.to_string());
        let found = self.search_from(self.caret.line, self.caret.col, pattern).or_else(|| self.search_from(0, 0, pattern));
        let Some((line, col)) = found else { return false };
        self.snapshot_for_undo();
        let chars: Vec<char> = self.lines[line].chars().collect();
        let pat_len = pattern.chars().count();
        let mut new_chars = chars[..col].to_vec();
        new_chars.extend(replacement.chars());
        new_chars.extend(chars[col + pat_len..].iter());
        self.lines[line] = new_chars.into_iter().collect();
        self.caret.line = line;
        self.caret.col = col + replacement.chars().count();
        true
    }

    /// Literal (non-regex) forward scan for `pattern`, starting at
    /// `start_line`/`start_col` (a `char` column). Returns the first match's
    /// `(line, col)`, or `None` if `pattern` does not occur anywhere from
    /// there to the end of the buffer.
    fn search_from(&self, start_line: usize, start_col: usize, pattern: &str) -> Option<(usize, usize)> {
        if pattern.is_empty() {
            return None;
        }
        for (i, line) in self.lines.iter().enumerate().skip(start_line) {
            let from_col = if i == start_line { start_col } else { 0 };
            let chars: Vec<char> = line.chars().collect();
            if from_col > chars.len() {
                continue;
            }
            let remainder: String = chars[from_col..].iter().collect();
            if let Some(byte_idx) = remainder.find(pattern) {
                let char_idx = remainder[..byte_idx].chars().count();
                return Some((i, from_col + char_idx));
            }
        }
        None
    }

    // -- single-level undo --------------------------------------------------

    /// Capture the buffer as it stands right now as the one prior-state
    /// snapshot, overwriting any snapshot already held. Called before every
    /// mutating edit.
    fn snapshot_for_undo(&mut self) {
        self.undo_snapshot = Some(self.lines.clone());
    }

    /// Swap the buffer to the single prior-state snapshot, if one is held.
    /// The snapshot is consumed, so invoking this twice in a row only
    /// undoes the most recent edit — the second call is a no-op
    /// (builtin-editor "Undo does not go back more than one level").
    pub fn undo(&mut self) {
        if let Some(prev) = self.undo_snapshot.take() {
            self.lines = prev;
            if self.lines.is_empty() {
                self.lines.push(String::new());
            }
            self.caret.line = self.caret.line.min(self.lines.len() - 1);
            let line_len = self.lines[self.caret.line].chars().count();
            self.caret.col = self.caret.col.min(line_len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(lines: &[&str]) -> EditorState {
        EditorState {
            path: PathBuf::from("/edit.txt"),
            lines: lines.iter().map(|s| s.to_string()).collect(),
            caret: Caret::default(),
            mode: EntryMode::Insert,
            line_ending: LineEnding::Lf,
            selection_anchor: None,
            clipboard: Vec::new(),
            undo_snapshot: None,
            saved_snapshot: lines.iter().map(|s| s.to_string()).collect(),
            search_pattern: None,
        }
    }

    fn temp_file(name: &str, contents: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("filecommand-editor-test-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("file.txt");
        std::fs::write(&path, contents).unwrap();
        path
    }

    // -- size cap ------------------------------------------------------

    #[test]
    fn fits_editor_cap_boundary_is_strictly_less_than_10_mib() {
        assert!(fits_editor_cap(0));
        assert!(fits_editor_cap(EDITOR_MAX_BYTES - 1));
        assert!(!fits_editor_cap(EDITOR_MAX_BYTES));
        assert!(!fits_editor_cap(EDITOR_MAX_BYTES + 1));
    }

    #[test]
    fn open_redirects_a_too_large_file_without_reading_it() {
        // A file just at the cap must redirect, and must not be read into
        // memory as editor content.
        let big = vec![b'a'; EDITOR_MAX_BYTES as usize];
        let path = temp_file("too-large", &big);
        match EditorState::open(&path).unwrap() {
            LoadResult::TooLarge { size } => assert_eq!(size, EDITOR_MAX_BYTES),
            LoadResult::Loaded(_) => panic!("expected TooLarge"),
        }
    }

    #[test]
    fn open_loads_a_small_file_with_caret_at_the_start() {
        let path = temp_file("small", b"line one\nline two\n");
        match EditorState::open(&path).unwrap() {
            LoadResult::Loaded(editor) => {
                assert_eq!(editor.lines, vec!["line one".to_string(), "line two".to_string(), "".to_string()]);
                assert_eq!(editor.caret, Caret { line: 0, col: 0 });
                assert!(!editor.is_modified());
            }
            LoadResult::TooLarge { .. } => panic!("expected Loaded"),
        }
    }

    // -- line-ending detection & CRLF/LF round trip ---------------------

    #[test]
    fn detects_crlf_and_lf_dominance() {
        assert_eq!(LineEnding::detect(b"a\r\nb\r\nc\r\n"), LineEnding::Crlf);
        assert_eq!(LineEnding::detect(b"a\nb\nc\n"), LineEnding::Lf);
        assert_eq!(LineEnding::detect(b"no newlines here"), LineEnding::Lf);
    }

    #[test]
    fn crlf_file_round_trips_through_save() {
        let path = temp_file("crlf", b"alpha\r\nbeta\r\ngamma");
        let LoadResult::Loaded(mut editor) = EditorState::open(&path).unwrap() else { panic!("expected Loaded") };
        assert_eq!(editor.line_ending, LineEnding::Crlf);
        editor.save().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"alpha\r\nbeta\r\ngamma");
    }

    #[test]
    fn lf_file_round_trips_through_save() {
        let path = temp_file("lf", b"alpha\nbeta\ngamma\n");
        let LoadResult::Loaded(mut editor) = EditorState::open(&path).unwrap() else { panic!("expected Loaded") };
        assert_eq!(editor.line_ending, LineEnding::Lf);
        editor.save().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"alpha\nbeta\ngamma\n");
    }

    #[test]
    fn save_clears_the_modified_state() {
        let path = temp_file("save-clears", b"hello\n");
        let LoadResult::Loaded(mut editor) = EditorState::open(&path).unwrap() else { panic!("expected Loaded") };
        editor.type_char('!');
        assert!(editor.is_modified());
        editor.save().unwrap();
        assert!(!editor.is_modified());
    }

    #[cfg(unix)]
    #[test]
    fn save_preserves_a_non_unicode_filename() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let dir = std::env::temp_dir().join(format!("filecommand-editor-test-nonunicode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 0x80 alone is not valid UTF-8, but perfectly valid as a Unix
        // filename byte sequence.
        let raw_name = OsStr::from_bytes(&[b'f', 0x80, b'o']).to_os_string();
        let path = dir.join(&raw_name);
        std::fs::write(&path, b"content\n").unwrap();

        let LoadResult::Loaded(mut editor) = EditorState::open(&path).unwrap() else { panic!("expected Loaded") };
        assert_eq!(editor.path, path);
        editor.type_char('X');
        editor.save().unwrap();
        let mut on_disk = std::fs::read_dir(&dir).unwrap();
        let entry = on_disk.next().unwrap().unwrap();
        assert_eq!(entry.file_name(), raw_name, "the on-disk filename must survive unmangled");
    }

    // -- insert / overwrite ---------------------------------------------

    #[test]
    fn insert_mode_shifts_existing_text_right() {
        let mut e = state(&["ac"]);
        e.caret.col = 1;
        e.type_char('b');
        assert_eq!(e.lines[0], "abc");
        assert_eq!(e.caret.col, 2);
    }

    #[test]
    fn overwrite_mode_replaces_in_place_and_advances() {
        let mut e = state(&["abc"]);
        e.mode = EntryMode::Overwrite;
        e.caret.col = 1;
        e.type_char('X');
        assert_eq!(e.lines[0], "aXc");
        assert_eq!(e.caret.col, 2);
    }

    #[test]
    fn overwrite_past_end_of_line_appends() {
        let mut e = state(&["ab"]);
        e.mode = EntryMode::Overwrite;
        e.caret.col = 2;
        e.type_char('c');
        assert_eq!(e.lines[0], "abc");
    }

    #[test]
    fn toggling_mode_flips_insert_and_overwrite() {
        let mut e = state(&["a"]);
        assert_eq!(e.mode, EntryMode::Insert);
        e.toggle_mode();
        assert_eq!(e.mode, EntryMode::Overwrite);
        e.toggle_mode();
        assert_eq!(e.mode, EntryMode::Insert);
    }

    #[test]
    fn insert_newline_splits_the_line_at_the_caret() {
        let mut e = state(&["hello world"]);
        e.caret.col = 5;
        e.insert_newline();
        assert_eq!(e.lines, vec!["hello".to_string(), " world".to_string()]);
        assert_eq!(e.caret, Caret { line: 1, col: 0 });
    }

    #[test]
    fn backspace_joins_lines_at_column_zero() {
        let mut e = state(&["foo", "bar"]);
        e.caret = Caret { line: 1, col: 0 };
        e.backspace();
        assert_eq!(e.lines, vec!["foobar".to_string()]);
        assert_eq!(e.caret, Caret { line: 0, col: 3 });
    }

    #[test]
    fn backspace_at_very_start_of_buffer_is_a_no_op() {
        let mut e = state(&["abc"]);
        e.backspace();
        assert_eq!(e.lines, vec!["abc".to_string()]);
        assert_eq!(e.caret, Caret { line: 0, col: 0 });
    }

    // -- line-based selection: mark, cut, copy, paste --------------------

    #[test]
    fn marking_selects_whole_lines_from_anchor_to_caret() {
        let mut e = state(&["a", "b", "c", "d"]);
        e.start_mark();
        e.caret.line = 2;
        assert_eq!(e.selection_range(), Some((0, 2)));
    }

    #[test]
    fn selection_range_is_ordered_even_when_caret_moves_above_anchor() {
        let mut e = state(&["a", "b", "c"]);
        e.caret.line = 2;
        e.start_mark();
        e.caret.line = 0;
        assert_eq!(e.selection_range(), Some((0, 2)));
    }

    #[test]
    fn cut_removes_captures_and_settles_caret_on_the_following_line() {
        let mut e = state(&["a", "b", "c", "d"]);
        e.start_mark();
        e.caret.line = 1; // selects "a", "b"
        e.cut_selection();
        assert_eq!(e.lines, vec!["c".to_string(), "d".to_string()]);
        assert_eq!(e.clipboard, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(e.caret.line, 0); // settled on the line that followed the removed range
        assert_eq!(e.selection_anchor, None);
    }

    #[test]
    fn cut_the_entire_buffer_leaves_one_empty_line() {
        let mut e = state(&["only"]);
        e.start_mark();
        e.cut_selection();
        assert_eq!(e.lines, vec!["".to_string()]);
        assert_eq!(e.clipboard, vec!["only".to_string()]);
    }

    #[test]
    fn copy_captures_without_removing() {
        let mut e = state(&["a", "b", "c"]);
        e.start_mark();
        e.caret.line = 1;
        e.copy_selection();
        assert_eq!(e.lines, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        assert_eq!(e.clipboard, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn paste_inserts_whole_lines_at_the_caret_without_splitting_neighbors() {
        let mut e = state(&["x", "y"]);
        e.clipboard = vec!["m".to_string(), "n".to_string()];
        e.caret.line = 1;
        e.paste();
        assert_eq!(e.lines, vec!["x".to_string(), "m".to_string(), "n".to_string(), "y".to_string()]);
        assert_eq!(e.caret, Caret { line: 1, col: 0 });
    }

    #[test]
    fn paste_with_empty_clipboard_is_a_no_op() {
        let mut e = state(&["x"]);
        e.paste();
        assert_eq!(e.lines, vec!["x".to_string()]);
    }

    #[test]
    fn cut_then_paste_round_trips_the_lines() {
        let mut e = state(&["a", "b", "c"]);
        e.start_mark();
        e.caret.line = 1;
        e.cut_selection();
        e.caret.line = 0;
        e.paste();
        assert_eq!(e.lines, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    // -- literal search / search-and-replace ------------------------------

    #[test]
    fn find_next_moves_the_caret_to_the_next_occurrence() {
        // The caret starts at (0, 0), exactly on the first match, so
        // "next" (searching strictly after the caret) lands on the second
        // occurrence first, then wraps back around to the first.
        let mut e = state(&["needle here", "another needle"]);
        assert!(e.find_next("needle"));
        assert_eq!(e.caret, Caret { line: 1, col: 8 });
        assert!(e.find_next("needle"));
        assert_eq!(e.caret, Caret { line: 0, col: 0 });
    }

    #[test]
    fn find_next_wraps_to_the_start_when_nothing_remains_after_the_caret() {
        let mut e = state(&["needle"]);
        e.caret.col = 6;
        assert!(e.find_next("needle"));
        assert_eq!(e.caret, Caret { line: 0, col: 0 });
    }

    #[test]
    fn find_next_returns_false_when_the_pattern_is_absent() {
        let mut e = state(&["nothing matches here"]);
        assert!(!e.find_next("zzz"));
    }

    #[test]
    fn search_treats_regex_metacharacters_literally() {
        let mut e = state(&["a.b*c literal.here"]);
        assert!(e.find_next("."));
        assert_eq!(e.caret, Caret { line: 0, col: 1 }, "`.` must match only the literal dot, not any character");
        assert!(e.find_next("*"));
        assert_eq!(e.lines[0].chars().nth(e.caret.col), Some('*'));
    }

    #[test]
    fn replace_substitutes_the_match_and_marks_modified() {
        let mut e = state(&["hello world"]);
        assert!(e.replace_first("world", "there"));
        assert_eq!(e.lines[0], "hello there");
        assert!(e.is_modified());
    }

    #[test]
    fn replace_treats_the_pattern_literally() {
        let mut e = state(&["3.14 is pi"]);
        assert!(e.replace_first(".", "_"));
        assert_eq!(e.lines[0], "3_14 is pi");
    }

    #[test]
    fn replace_returns_false_when_no_match_exists() {
        let mut e = state(&["abc"]);
        assert!(!e.replace_first("zzz", "y"));
        assert!(!e.is_modified());
    }

    // -- single-level undo --------------------------------------------------

    #[test]
    fn undo_restores_the_snapshot_taken_before_the_edit() {
        let mut e = state(&["abc"]);
        e.caret.col = 3;
        e.type_char('d');
        assert_eq!(e.lines[0], "abcd");
        e.undo();
        assert_eq!(e.lines[0], "abc");
    }

    #[test]
    fn undo_twice_does_not_go_back_more_than_one_level() {
        let mut e = state(&["abc"]);
        e.caret.col = 3;
        e.type_char('d'); // "abc" -> "abcd", snapshot = "abc"
        e.type_char('e'); // "abcd" -> "abcde", snapshot = "abcd"
        e.undo(); // restores "abcd"
        assert_eq!(e.lines[0], "abcd");
        e.undo(); // no snapshot left: no-op
        assert_eq!(e.lines[0], "abcd", "a second undo must not reach back to the original \"abc\"");
    }

    #[test]
    fn undo_with_no_prior_edit_is_a_no_op() {
        let mut e = state(&["abc"]);
        e.undo();
        assert_eq!(e.lines[0], "abc");
    }

    #[test]
    fn undo_after_cut_restores_the_removed_lines() {
        let mut e = state(&["a", "b", "c"]);
        e.start_mark();
        e.caret.line = 1;
        e.cut_selection();
        assert_eq!(e.lines.len(), 1);
        e.undo();
        assert_eq!(e.lines, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    // -- modified indicator -----------------------------------------------

    #[test]
    fn fresh_buffer_is_not_modified() {
        let e = state(&["a"]);
        assert!(!e.is_modified());
    }

    #[test]
    fn an_edit_marks_the_buffer_modified() {
        let mut e = state(&["a"]);
        e.caret.col = 1;
        e.type_char('b');
        assert!(e.is_modified());
    }
}
