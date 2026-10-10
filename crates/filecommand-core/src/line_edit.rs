//! The shared single-line text-field model behind every editable input line
//! (text-field-editing "Caret model and editing keys"; design D1): the F5/F6/
//! F7 destination and name field, the file-action Rename field, the
//! overwrite-conflict Rename field, the Alt+F7 find pattern, the Ctrl+J fuzzy
//! pattern, and the command line. One model, one key map, one renderer helper
//! — the days of six private "push a char / pop a char" `String`s are over.
//!
//! The caret is a **char index** into `text` (an insertion point between
//! chars, so `caret == len_chars()` is "at the end"), keeping `unicode-width`
//! out of the core logic; renderers map char indices to display columns with
//! `listing::display_width` (design D1).

/// One editing operation on a [`LineEdit`], as delivered by
/// `Command::TextEdit` (design D2). `Paste` carries already-read clipboard
/// text — the model never touches the OS clipboard itself; reading it is the
/// TUI's answer to `Effect::ReadClipboardText` (design D4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextEditOp {
    Insert(char),
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    WordLeft,
    WordRight,
    DeleteWordBack,
    DeleteWordForward,
    Paste(String),
}

impl TextEditOp {
    /// Whether the op can change the text (everything but the pure caret
    /// moves) — the reducer's cue to end history recall / re-narrow a
    /// filtered list, while a bare caret move leaves both alone.
    pub fn is_text_mutation(&self) -> bool {
        !matches!(self, TextEditOp::Left | TextEditOp::Right | TextEditOp::Home | TextEditOp::End | TextEditOp::WordLeft | TextEditOp::WordRight)
    }
}

/// A single line of text plus a caret (char index, `0..=len_chars()`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineEdit {
    text: String,
    /// Caret as a char index into `text`: `0` is before the first char,
    /// `len_chars()` is after the last.
    caret: usize,
}

impl LineEdit {
    /// The field's current text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The caret's char index (`0..=len_chars()`).
    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The text's length in chars — the caret's upper bound.
    pub fn len_chars(&self) -> usize {
        self.text.chars().count()
    }

    /// Replace the text wholesale, leaving the caret at its end — how every
    /// pre-filled field opens (text-field-editing "A field that opens
    /// pre-filled SHALL open with the caret at the end"), and how history
    /// recall and Ctrl+Enter/Ctrl+] leave the command line.
    pub fn set_text_caret_end(&mut self, text: String) {
        self.text = text;
        self.caret = self.len_chars();
    }

    /// Insert `c` at the caret, moving the caret after it.
    pub fn insert(&mut self, c: char) {
        let byte = self.byte_of_char(self.caret);
        self.text.insert(byte, c);
        self.caret += 1;
    }

    /// Insert `s` at the caret with any line breaks flattened to a single
    /// space (text-field-editing "Paste into a text field": "with any line
    /// breaks replaced by a single space"), moving the caret past what was
    /// inserted. The TUI's clipboard read normalizes before dispatching
    /// `TextEditOp::Paste` already; doing it here as well keeps the model's
    /// own contract independent of that path.
    pub fn insert_str(&mut self, s: &str) {
        let flattened = s.replace("\r\n", " ").replace('\r', " ").replace('\n', " ");
        let byte = self.byte_of_char(self.caret);
        self.text.insert_str(byte, &flattened);
        self.caret += flattened.chars().count();
    }

    /// Backspace: delete the char before the caret. A no-op at the start.
    pub fn backspace(&mut self) {
        if self.caret == 0 {
            return;
        }
        let start = self.byte_of_char(self.caret - 1);
        let end = self.byte_of_char(self.caret);
        self.text.replace_range(start..end, "");
        self.caret -= 1;
    }

    /// Delete: remove the char after the caret. A no-op at the end.
    pub fn delete(&mut self) {
        if self.caret >= self.len_chars() {
            return;
        }
        let start = self.byte_of_char(self.caret);
        let end = self.byte_of_char(self.caret + 1);
        self.text.replace_range(start..end, "");
    }

    pub fn left(&mut self) {
        self.caret = self.caret.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.caret = (self.caret + 1).min(self.len_chars());
    }

    pub fn home(&mut self) {
        self.caret = 0;
    }

    pub fn end(&mut self) {
        self.caret = self.len_chars();
    }

    /// Ctrl+Left: move the caret to the start of the current or previous
    /// word. Words are runs of alphanumerics; everything else is a separator
    /// run of its own, so `C:\Users\kevin` jumps at every `\` (design D1).
    pub fn word_left(&mut self) {
        self.caret = self.word_left_index();
    }

    /// Ctrl+Right: move the caret to the end of the current or next word.
    pub fn word_right(&mut self) {
        self.caret = self.word_right_index();
    }

    /// Ctrl+Backspace: delete from the caret back to where [`Self::word_left`]
    /// would land — e.g. `C:\Users\kevin\docs` at end becomes
    /// `C:\Users\kevin\` (text-field-editing "Word delete").
    pub fn delete_word_back(&mut self) {
        let target = self.word_left_index();
        if target == self.caret {
            return;
        }
        let start = self.byte_of_char(target);
        let end = self.byte_of_char(self.caret);
        self.text.replace_range(start..end, "");
        self.caret = target;
    }

    /// Ctrl+Delete: delete from the caret forward to where
    /// [`Self::word_right`] would land.
    pub fn delete_word_forward(&mut self) {
        let target = self.word_right_index();
        if target == self.caret {
            return;
        }
        let start = self.byte_of_char(self.caret);
        let end = self.byte_of_char(target);
        self.text.replace_range(start..end, "");
    }

    /// Empty the field and re-home the caret.
    pub fn clear(&mut self) {
        self.text.clear();
        self.caret = 0;
    }

    /// Apply one [`TextEditOp`] — the single entry point `Command::TextEdit`
    /// routes through for every field (design D2).
    pub fn apply(&mut self, op: &TextEditOp) {
        match op {
            TextEditOp::Insert(c) => self.insert(*c),
            TextEditOp::Backspace => self.backspace(),
            TextEditOp::Delete => self.delete(),
            TextEditOp::Left => self.left(),
            TextEditOp::Right => self.right(),
            TextEditOp::Home => self.home(),
            TextEditOp::End => self.end(),
            TextEditOp::WordLeft => self.word_left(),
            TextEditOp::WordRight => self.word_right(),
            TextEditOp::DeleteWordBack => self.delete_word_back(),
            TextEditOp::DeleteWordForward => self.delete_word_forward(),
            TextEditOp::Paste(s) => self.insert_str(s),
        }
    }

    /// The visible window of the text for a field `width` chars wide,
    /// scrolled so the caret is always visible (text-field-editing "Field
    /// rendering keeps the caret visible"; design D5). Returns
    /// `(start_char, caret_col)`: the char index the window starts at, and
    /// the caret's column within the window (`caret - start`).
    ///
    /// `caret_col` is in `[0, width]`: it equals `width` only when the caret
    /// sits at the end of a full window (an overlong line edited at its
    /// end), in which case the renderer clamps the terminal cursor onto the
    /// window's last cell. A mid-text caret always lands on a visible char:
    /// `caret_col <= width - 1` with the caret's own char inside the window.
    pub fn window(&self, width: usize) -> (usize, usize) {
        if width == 0 {
            return (0, 0);
        }
        if self.caret < width {
            // The caret sits inside the first window-full, so no scrolling:
            // the text's start stays visible (a Home press on a long line
            // shows the first `width` chars with the caret at the left edge).
            (0, self.caret)
        } else if self.caret == self.len_chars() {
            // At the end of an overlong line: show the last `width` chars,
            // the caret one column past the window's last char (clamped back
            // onto that cell by the renderer).
            (self.caret - width, width)
        } else {
            // Mid-text: scroll just far enough that the caret's own char is
            // the window's rightmost cell, keeping what follows in view.
            (self.caret - width + 1, width - 1)
        }
    }

    /// The byte offset of char index `idx` (`text.len()` when at the end).
    fn byte_of_char(&self, idx: usize) -> usize {
        self.text.char_indices().nth(idx).map(|(b, _)| b).unwrap_or(self.text.len())
    }

    /// Where [`Self::word_left`] would land: skip any separator run left of
    /// the caret, then the alphanumeric run before it.
    fn word_left_index(&self) -> usize {
        let mut i = self.caret;
        while i > 0 && !is_word_char(self.nth_char(i - 1)) {
            i -= 1;
        }
        while i > 0 && is_word_char(self.nth_char(i - 1)) {
            i -= 1;
        }
        i
    }

    /// Where [`Self::word_right`] would land: skip any separator run right
    /// of the caret, then the alphanumeric run after it.
    fn word_right_index(&self) -> usize {
        let len = self.len_chars();
        let mut i = self.caret;
        while i < len && !is_word_char(self.nth_char(i)) {
            i += 1;
        }
        while i < len && is_word_char(self.nth_char(i)) {
            i += 1;
        }
        i
    }

    /// The char at char index `idx`. Callers stay within `0..len_chars()`.
    fn nth_char(&self, idx: usize) -> char {
        self.text.chars().nth(idx).expect("char index within the text")
    }
}

/// Word characters for the word-motion ops: alphanumerics, so `\`, `/`,
/// `.`, and spaces are all separators (design D1).
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

/// A pre-filled field: text set, caret at its end (text-field-editing "A
/// field that opens pre-filled SHALL open with the caret at the end").
impl From<&str> for LineEdit {
    fn from(text: &str) -> LineEdit {
        LineEdit::from(text.to_string())
    }
}

/// Same as the `&str` conversion: text set, caret at its end.
impl From<String> for LineEdit {
    fn from(text: String) -> LineEdit {
        let mut edit = LineEdit::default();
        edit.set_text_caret_end(text);
        edit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(text: &str, caret: usize) -> LineEdit {
        let mut e = LineEdit::from(text);
        e.caret = caret;
        e
    }

    #[test]
    fn from_str_prefills_with_the_caret_at_the_end() {
        let e = LineEdit::from(r"D:\backup");
        assert_eq!(e.text(), r"D:\backup");
        assert_eq!(e.caret(), e.len_chars(), "a pre-filled field opens with the caret at the end");
    }

    #[test]
    fn insert_lands_at_the_caret_mid_line() {
        // text-field-editing "Insert in the middle of a pre-filled name":
        // `draft.txt` -> `drift.txt` by Backspace + `i` near the caret. The
        // scenario's "Left five times" is off by one in the spec: from the
        // end of a 9-char name, reaching the `a` takes six Lefts (five would
        // delete the `f` instead); the stated result `drift.txt` is what
        // pins the model down here.
        let mut e = LineEdit::from("draft.txt");
        for _ in 0..6 {
            e.left();
        }
        e.backspace();
        e.insert('i');
        assert_eq!(e.text(), "drift.txt");
        assert_eq!(e.caret(), 3, "the caret sits after the inserted i");
    }

    #[test]
    fn backspace_at_the_start_and_delete_at_the_end_are_noops() {
        let mut e = edit("ab", 0);
        e.backspace();
        assert_eq!((e.text(), e.caret()), ("ab", 0));
        e.end();
        e.delete();
        assert_eq!((e.text(), e.caret()), ("ab", 2));
    }

    #[test]
    fn delete_removes_the_char_after_the_caret() {
        // text-field-editing "Home then edit a destination prefix".
        let mut e = LineEdit::from(r"D:\backup\2026");
        e.home();
        e.delete();
        e.delete();
        assert_eq!(e.text(), r"\backup\2026");
        e.insert_str(r"\\server\share");
        assert_eq!(e.text(), r"\\server\share\backup\2026");
    }

    #[test]
    fn home_end_and_arrows_clamp_at_the_line_ends() {
        let mut e = edit("abc", 1);
        e.home();
        assert_eq!(e.caret(), 0);
        e.left();
        assert_eq!(e.caret(), 0, "left at the start holds");
        e.end();
        assert_eq!(e.caret(), 3);
        e.right();
        assert_eq!(e.caret(), 3, "right at the end holds");
    }

    #[test]
    fn word_left_jumps_at_backslashes_and_spaces() {
        let mut e = LineEdit::from(r"C:\Users\kevin");
        assert_eq!(e.caret(), 14);
        e.word_left();
        assert_eq!(e.caret(), 9, "start of `kevin`, just past the backslash");
        e.word_left();
        assert_eq!(e.caret(), 3, "start of `Users`");
        e.word_left();
        assert_eq!(e.caret(), 0, "start of `C`");
        e.word_left();
        assert_eq!(e.caret(), 0, "already at the start");

        let mut e = LineEdit::from("cd Program Files");
        e.word_left();
        assert_eq!(e.caret(), 11, "start of `Files`");
        e.word_left();
        assert_eq!(e.caret(), 3, "start of `Program`");
    }

    #[test]
    fn word_right_jumps_to_word_ends() {
        let mut e = edit(r"C:\Users\kevin", 0);
        e.word_right();
        assert_eq!(e.caret(), 1, "end of `C`");
        e.word_right();
        assert_eq!(e.caret(), 8, "end of `Users`");
        e.word_right();
        assert_eq!(e.caret(), 14, "end of the line");
        e.word_right();
        assert_eq!(e.caret(), 14, "already at the end");
    }

    #[test]
    fn delete_word_back_removes_to_the_previous_boundary() {
        // text-field-editing "Word delete": `C:\Users\kevin\docs` at end,
        // Ctrl+Backspace -> `C:\Users\kevin\`.
        let mut e = LineEdit::from(r"C:\Users\kevin\docs");
        e.delete_word_back();
        assert_eq!(e.text(), r"C:\Users\kevin\");
        assert_eq!(e.caret(), e.len_chars());
        e.delete_word_back();
        assert_eq!(e.text(), r"C:\Users\", "the separator run goes with the next word back");
    }

    #[test]
    fn delete_word_forward_removes_to_the_next_boundary() {
        let mut e = edit(r"C:\Users\kevin", 0);
        e.delete_word_forward();
        assert_eq!(e.text(), r":\Users\kevin", "only the word goes; its trailing separator stays");
        e.delete_word_forward();
        assert_eq!(e.text(), r"\kevin", "the next delete takes the separator and the word together");
        let mut e = edit(r"C:\Users\kevin", 9);
        e.delete_word_forward();
        assert_eq!(e.text(), r"C:\Users\");
    }

    #[test]
    fn clear_empties_the_field_and_rehomes_the_caret() {
        let mut e = LineEdit::from("dir *.txt");
        e.clear();
        assert!(e.is_empty());
        assert_eq!(e.caret(), 0);
    }

    #[test]
    fn paste_flattens_line_breaks_to_single_spaces() {
        let mut e = LineEdit::default();
        e.insert_str("one\r\ntwo\nthree\rfour");
        assert_eq!(e.text(), "one two three four");
        assert_eq!(e.caret(), e.len_chars(), "the caret lands past the pasted text");
    }

    #[test]
    fn paste_inserts_at_a_mid_line_caret() {
        let mut e = edit(r"D:\", 3);
        e.insert_str(r"E:\archive");
        assert_eq!(e.text(), r"D:\E:\archive");
        assert_eq!(e.caret(), e.len_chars());
    }

    #[test]
    fn apply_dispatches_every_op() {
        let mut e = edit("ab d", 4);
        e.apply(&TextEditOp::WordLeft);
        assert_eq!(e.caret(), 3);
        e.apply(&TextEditOp::Left);
        assert_eq!(e.caret(), 2);
        e.apply(&TextEditOp::Insert('c'));
        assert_eq!(e.text(), "abc d");
        e.apply(&TextEditOp::Home);
        e.apply(&TextEditOp::Delete);
        assert_eq!(e.text(), "bc d");
        e.apply(&TextEditOp::End);
        e.apply(&TextEditOp::Backspace);
        assert_eq!(e.text(), "bc ");
        e.apply(&TextEditOp::DeleteWordForward);
        assert_eq!(e.text(), "bc ", "nothing after the caret to delete");
        e.apply(&TextEditOp::WordRight);
        e.apply(&TextEditOp::DeleteWordBack);
        assert_eq!(e.text(), "");
        e.apply(&TextEditOp::Paste("x".to_string()));
        assert_eq!(e.text(), "x");
    }

    #[test]
    fn window_keeps_the_caret_visible_for_long_text() {
        // text-field-editing "Long destination scrolls to the caret": a
        // 60-char path in a 40-cell field with the caret at the end shows
        // the last 40 chars.
        let text: String = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ01234567".chars().collect();
        assert_eq!(text.chars().count(), 60);
        let mut e = LineEdit::from(text);
        assert_eq!(e.window(40), (20, 40), "at the end: the last 40 chars, caret one past the window");

        e.home();
        assert_eq!(e.window(40), (0, 0), "Home scrolls back to the first 40 chars, caret at the left edge");

        e.end();
        for _ in 0..21 {
            e.left();
        }
        assert_eq!(e.caret(), 39);
        assert_eq!(e.window(40), (0, 39), "a caret inside the first window needs no scrolling");

        e.right();
        assert_eq!(e.window(40), (1, 39), "a mid-text caret past the first window scrolls it into view at the right edge");
    }

    #[test]
    fn window_of_a_short_line_never_scrolls() {
        let e = LineEdit::from("short");
        assert_eq!(e.window(40), (0, 5), "the whole text fits; the caret sits just past it");
        let e = LineEdit::default();
        assert_eq!(e.window(40), (0, 0));
        assert_eq!(e.window(0), (0, 0), "a zero-width field has no window");
    }

    #[test]
    fn multibyte_chars_edit_and_move_by_char_not_byte() {
        let mut e = LineEdit::from("aé中");
        assert_eq!(e.len_chars(), 3);
        e.left();
        e.backspace();
        assert_eq!(e.text(), "a中");
        assert_eq!(e.caret(), 1);
        e.insert('é');
        assert_eq!(e.text(), "aé中");
        e.word_left();
        assert_eq!(e.caret(), 0, "é and 中 are alphanumeric word chars");
    }
}
