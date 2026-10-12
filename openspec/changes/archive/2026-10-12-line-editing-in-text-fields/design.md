# Design: line-editing-in-text-fields

## Context

Six fields, six copies of "push a char / pop a char": `FileOpInputChar/Backspace` (destination, rename, conflict rename — `update.rs:2948-3060`), `FindFileChar/Backspace`, `FuzzyJumpChar/Backspace`, `CommandLineChar/Backspace`. All state is a `String`. Renderers draw the string left-aligned in a fixed-width bracket field (`destination_input.rs` `FIELD_WIDTH = 40`) and the command line shows the text followed by `_` (responsive-layout already requires "Command line keeps the caret visible"). Over the panels Left/Right are unbound; Home/End/PgUp/PgDn move the panel cursor; Delete requests delete; Ctrl+Left/Right adjust the split; Up/Down recall history only while the line is non-empty.

## Goals / Non-Goals

**Goals:**

- One model, one key map, one renderer helper.
- Classic NC input-line feel.
- No change to any existing panel key meaning.
- Paste from the system clipboard.

**Non-Goals:**

- Multi-line editing; selection within a field; mouse caret placement.
- The quick-filter/type-ahead patterns.
- The viewer/editor prompts (they share nothing with dialogs today and are short search patterns).
- Tab completion (separate proposal).

## Decisions

### D1: `LineEdit` in core, caret measured in chars

`pub struct LineEdit { text: String, caret: usize /* char index */ }` with `insert(char)`, `insert_str(&str)`, `backspace()`, `delete()`, `left()`, `right()`, `home()`, `end()`, `word_left()`, `word_right()`, `delete_word_back()`, `delete_word_forward()`, `clear()`, `set_text_caret_end(String)`, `text()`, `caret()`, and `window(width) -> (start_char, caret_col)` for renderers. Word boundaries: runs of alphanumerics vs. anything else, so `C:\Users\kevin` word-jumps at `\`. Char-indexed to keep `unicode-width` out of core logic; renderers map char index → display column with `display_width`.

Alternative rejected: byte-indexed caret — every operation would need UTF-8 boundary checks; char indexing is simpler and the fields are short.

### D2: One `TextEditOp` command, routed by phase

`Command::TextEdit(TextEditOp)` where `TextEditOp = Insert(char) | Backspace | Delete | Left | Right | Home | End | WordLeft | WordRight | DeleteWordBack | DeleteWordForward | Paste(String)`. `update` routes it to whichever field owns the keyboard (destination/rename/conflict-rename input, find-file, fuzzy-jump, else the command line). Existing `FileOpInputChar`/`FindFileChar`/`FuzzyJumpChar`/`CommandLineChar` and their Backspace commands become thin aliases of `TextEdit` (kept so current tests and the quick-search handoff compile unchanged) or are replaced in the same change — implementer's call, but no field may keep a private `String`.

### D3: Command-line rules preserve every panel key

Left/Right map to `TextEdit(Left/Right)` only when the line is non-empty (mirrors the Up/Down history rule); on an empty line they stay unbound. Home/End/PgUp/PgDn keep moving the panel cursor even mid-composition (unchanged, documented); Delete keeps opening the delete confirmation (unchanged, documented in the README today); Ctrl+Left/Right stay split keys; Ctrl+Backspace → `DeleteWordBack`. Printables insert at the caret. History recall (`recall_history`) and Ctrl+Enter/Ctrl+] (`paste_cursor_entry`) call `set_text_caret_end`.

Alternative rejected: Home/End moving the caret while composing — it would silently take two of the most-used panel keys away whenever a single character is on the line.

### D4: Paste is a TUI-side clipboard read turned into `Paste(String)`

The key map emits `Command::RequestPaste`; the event loop reads clipboard text (`clipboard-win` `get_clipboard_string` on Windows, `arboard` elsewhere), normalizes CR/LF to a single space, and feeds `TextEdit(Paste(text))`. An unavailable/non-text clipboard is a no-op. Inside dialog fields Shift+Ins and Ctrl+V both map to it (today the unguarded `Char(c)` arm types a literal `v` on Ctrl+V there). Over the panels only Ctrl+V maps to it (unbound today), because the plain `KeyCode::Insert` arm has no modifier guard, so Shift+Ins is already the selection toggle and stays so; Ctrl+Shift+Ins remains clipboard-paths.

Alternative rejected: Shift+Ins paste over the panels — it would silently take the selection toggle away from anyone who presses Insert with Shift held.

### D5: Field rendering shows the caret and scrolls

Each field renderer calls `window(field_w)` and draws `text[start..start+field_w]`, then positions the terminal cursor at the caret column (the frame's `set_cursor_position`; today's dialogs leave the cursor hidden — the plain command line already shows a `_` caret, which is replaced by the terminal cursor at the caret). Pre-filled fields open with the caret at the end so Enter-to-accept is unchanged.

## Risks / Trade-offs

- [Replacing `State.command_line: String` touches many tests] → A `text()` accessor and `set_text_caret_end` helper make the assertion changes mechanical.
- [Terminal cursor visibility inside dialogs is new chrome] → It only appears while a text field is focused and is hidden again on close.
- [Ctrl+Backspace on some terminals arrives as `Ctrl+H` or `0x7f`] → Both are mapped to `DeleteWordBack` when the Ctrl modifier is reported, otherwise plain Backspace; best-effort, documented.

## Open Questions

- None.
