# Tasks: line-editing-in-text-fields

## 1. Core model (`crates/filecommand-core/src/line_edit.rs`, new)

- [ ] 1.1 `LineEdit` per design D1 with all operations and `window(width)`
- [ ] 1.2 Unit tests: insert mid-line, backspace/delete at ends, word jumps across `\` and spaces, delete-word, paste with CR/LF normalized, `window` keeps the caret visible for long text

## 2. State and reducer (`crates/filecommand-core`)

- [ ] 2.1 `fs_ops/dialog.rs`: `DestinationInput.input`, `RenameInput.input`, conflict `rename_input` become `LineEdit` (pre-filled → caret at end) (text-field-editing "Caret model and editing keys"; file-action-menu "In-place Rename")
- [ ] 2.2 `find_file.rs` `pattern`, `quicksearch.rs` `FuzzyJumpState.pattern` become `LineEdit`; search-narrowing reads `text()`
- [ ] 2.3 `update.rs`: `State.command_line: LineEdit`; `Command::TextEdit(TextEditOp)` routed by phase (design D2); `recall_history`, `paste_cursor_entry`, `CommandLineClear`, `run_command_line` use `text()`/`set_text_caret_end`/`clear` (command-line "Paste filename and path to command line": caret at end)
- [ ] 2.4 `Command::RequestPaste` → `Effect::ReadClipboardText`; `TextEdit(Paste)` inserts at the caret (text-field-editing "Paste into a text field"; design D4)
- [ ] 2.5 Tests in `update/tests.rs`: destination field edit in the middle; rename `draft.txt` → `drift.txt` via Left×5, Backspace, `i`; command line Left/Right only when non-empty; Home still moves the panel cursor mid-composition; history recall puts the caret at the end; paste normalizes newlines

## 3. Input mapping (`crates/filecommand-tui/src/input/mod.rs`)

- [ ] 3.1 `map_text_edit_key(key) -> Option<TextEditOp>` covering Left/Right/Home/End/Delete/Backspace/Ctrl+Left/Ctrl+Right/Ctrl+Backspace/Ctrl+Delete/Shift+Ins/Ctrl+V/printables; used by the destination, rename, conflict-rename, find-file, and fuzzy-jump arms
- [ ] 3.2 Panel arm: Left/Right → caret moves when `typing`; Ctrl+Backspace → `DeleteWordBack`; Ctrl+V → `RequestPaste`; Home/End/Delete/Shift+Ins/Ctrl+Left/Right unchanged (command-line "Command-line prompt and printable-key routing"; design D3, D4)
- [ ] 3.3 `input/tests.rs`: each mapping above, plus "Left on an empty line maps to nothing", "Shift+Ins over the panels still toggles selection", and "Ctrl+Shift+Ins is still clipboard paths"

## 4. Rendering (`crates/filecommand-tui/src/views/*`)

- [ ] 4.1 Shared `draw_text_field(buf, x, y, width, &LineEdit, style) -> caret_x` using `window()`; used by `destination_input.rs`, `find_file.rs`, `fuzzy_jump.rs` (text-field-editing "Field rendering keeps the caret visible"; design D5)
- [ ] 4.2 `views/command_line.rs`: window around the caret, terminal cursor at the caret replacing the drawn `_` (responsive-layout "Command line keeps the caret visible" still holds)
- [ ] 4.3 `app.rs`: set the terminal cursor position after drawing when a text field is focused; hide it otherwise; implement `Effect::ReadClipboardText` with `clipboard-win`/`arboard`
- [ ] 4.4 Snapshot tests: a destination field longer than 40 cells scrolled so the caret is visible; command line with the caret mid-text

## 5. Docs

- [ ] 5.1 README "Command line → Typing" table and "Keyboard reference": caret keys, Ctrl+Backspace, paste; F1 Help "Command line" and "File operations" pages

## 6. Verify

- [ ] 6.1 `cargo test --workspace` green; `openspec validate line-editing-in-text-fields --strict` clean
- [ ] 6.2 Manual: F6 on a file, Home, type a prefix, Enter; F5, End, Backspace×3, type, Enter; command line `cd ` + Shift+Ins of a copied path; Ctrl+J pattern edited mid-string
