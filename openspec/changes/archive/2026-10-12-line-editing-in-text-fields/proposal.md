# Change: line-editing-in-text-fields

## Why

Every single-line text field in FileCommand is append-only: the F5/F6/F7 destination and name field, the file-action Rename field, the overwrite-conflict Rename field, the Alt+F7 find pattern, the Ctrl+J fuzzy pattern, and the command line accept only printable characters and Backspace (`input/mod.rs` `map_file_op_setup_key`, `map_file_op_running_key`, and the find/fuzzy/command-line arms). Changing one character in the middle of `report_final_v2.txt` means backspacing through the suffix and retyping it; a long destination path cannot be edited except from its end; nothing can be pasted. Norton Commander's input lines had a movable cursor, Home/End, and insert-at-cursor. This is the single largest everyday friction in the dialogs.

## What Changes

- A shared **line-edit model** in `filecommand-core` (`LineEdit { text, caret }`) with insert-at-caret, Backspace, Delete, Left/Right, Home/End, word left/right, delete word back/forward, clear, and paste. Every single-line field above is backed by it.
- **Dialog fields** (destination/mkdir, Rename, conflict Rename, find-file pattern, fuzzy-jump pattern) honor Left/Right, Home/End, Delete, Ctrl+Left/Ctrl+Right, Ctrl+Backspace/Ctrl+Delete, and paste via Shift+Ins or Ctrl+V. A pre-filled field opens with the caret at its end.
- **Command line**: printable keys insert at the caret; Left/Right move the caret while the line is non-empty (on an empty line they have no command-line meaning, exactly like Up/Down today); Ctrl+Backspace deletes the word before the caret; Ctrl+V pastes. Home/End, PgUp/PgDn, Delete, Shift+Ins, and Ctrl+Left/Right keep their panel meanings (cursor jump, delete-file confirmation, selection toggle, split adjust). History recall and Ctrl+Enter/Ctrl+] paste leave the caret at the end.
- **Rendering**: each field shows the terminal cursor at the caret and scrolls its visible window horizontally so the caret is always visible; the bracket-and-dots field style is unchanged.
- Out of scope: the quick-filter and type-ahead patterns (search-as-you-type, not editable lines), the viewer F7 and editor F4/F7 prompts, mouse caret placement, Tab completion.

## Capabilities

### New Capabilities

- `text-field-editing`: the caret model, the key set, paste, and the scrolling field rendering shared by every single-line text field.

### Modified Capabilities

- `command-line`: "Command-line prompt and printable-key routing" gains caret insertion and the non-empty-line Left/Right rule; "Paste filename and path to command line" states the caret lands at the end.
- `operation-dialogs`: "Destination input dialog" references the shared editing keys.
- `file-action-menu`: "In-place Rename" opens with the caret at the end of the pre-filled name and honors the shared keys.
- `find-file`: "Find-file invocation" — the pattern field is a shared text field.
- `fuzzy-jump`: "Fuzzy jump dialog invocation" — the pattern field is a shared text field.

## Impact

- `crates/filecommand-core/src/line_edit.rs` (new) — model + unit tests; `fs_ops/dialog.rs` (`DestinationInput.input`, `RenameInput.input`, conflict `rename_input`), `find_file.rs` (`pattern`), `quicksearch.rs` (`FuzzyJumpState.pattern`), `update.rs` (`State.command_line` becomes a `LineEdit`; new `Command::TextEdit(TextEditOp)` routed by phase; `Command::RequestPaste` and `Effect::ReadClipboardText`).
- `crates/filecommand-tui/src/input/mod.rs` — one `map_text_edit_key` helper used by every field; command-line arms for Left/Right/Ctrl+Backspace/paste. `app.rs` — clipboard **read** (text) via the existing `clipboard-win`/`arboard` dependencies; terminal cursor placement while a field is focused.
- `crates/filecommand-tui/src/views/{destination_input,find_file,fuzzy_jump,command_line}.rs` — caret placement and horizontal window.
- Tests: `update/tests.rs`, `input/tests.rs`, snapshot additions for a scrolled field.
- Non-breaking for existing key meanings over the panels (Left/Right were unbound there).
