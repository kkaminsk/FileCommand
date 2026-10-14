# Tasks: editor-encoding-safety

## 1. Byte classification and BOM handling (`crates/filecommand-core/src/editor.rs`)

- [x] 1.1 Add `NotEditableReason { Utf16, Binary, NotUtf8 }` and `LoadResult::NotEditable { reason }`. Add the pure `classify_editor_bytes(&[u8]) -> Result<EditorText<'_>, NotEditableReason>`, which checks the UTF-16 BOM, then NUL, then the UTF-8 BOM, then strict `from_utf8` (builtin-editor "Editor invocation and size cap"; design D1).
- [x] 1.2 Add `EditorState::has_bom`. Build the buffer from the validated text after the BOM, and stop using `decode_lossy` on the editor path. `LineEnding::detect` still runs on the raw bytes. Make `from_bytes` strict, returning `Result<EditorState, NotEditableReason>` (design D1, D3).
- [x] 1.3 In `EditorState::open`, after the existing size gate and read, classify the bytes and return `LoadResult::NotEditable { reason }` instead of loading (design D1).
- [x] 1.4 Make `to_bytes()` prefix `EF BB BF` when `has_bom` is set, so `save` and `byte_len` both include it (builtin-editor "UTF-8 byte-order mark is written back"; design D3).
- [x] 1.5 Update the `state()` test fixture and any other struct-literal construction for the new `has_bom` field.

## 2. Durable replace helper (new core module, e.g. `crates/filecommand-core/src/durable.rs`)

- [x] 2.1 Add `windows-sys` (features `Win32_Storage_FileSystem`, `Win32_Foundation`) as a `[target.'cfg(windows)'.dependencies]` entry in `crates/filecommand-core/Cargo.toml`, with the same version as the TUI (0.59) (design D4).
- [x] 2.2 Implement `replace_file(path, bytes)`:
  - resolve a symlink target;
  - snapshot the original's metadata;
  - create a uniquely named `.fc-tmp` sibling with `create_new`, carrying the original's HIDDEN/SYSTEM/ARCHIVE/NOT_CONTENT_INDEXED attributes on Windows (`OpenOptionsExt::attributes`) or its permission mode elsewhere;
  - write all bytes, then `sync_all`;
  - replace the original;
  - remove the temp file on any failure.

  All paths go through `to_extended_path` (design D4).
- [x] 2.3 Windows replace step: `ReplaceFileW` with a `.fc-bak` backup and `REPLACEFILE_IGNORE_MERGE_ERRORS | REPLACEFILE_IGNORE_ACL_ERRORS`; delete the backup on success. Recover from `ERROR_UNABLE_TO_REMOVE_REPLACED`, `ERROR_UNABLE_TO_MOVE_REPLACEMENT`, and `ERROR_UNABLE_TO_MOVE_REPLACEMENT_2` as design D4 describes. Fall back to `std::fs::rename` when the original no longer exists. Add a `// SAFETY:` comment on the `unsafe` call.
- [x] 2.4 Non-Windows replace step: `std::fs::rename`, then a best-effort `sync_all` on the parent directory.
- [x] 2.5 Add a fault-injection seam (a test-only hook that fails after N bytes written, or before the replace) so the failure paths are unit-testable without filling a disk.

## 3. Editor save (`crates/filecommand-core/src/editor.rs`)

- [x] 3.1 Add `check_writable`: an existing target whose `permissions().readonly()` is set yields `PermissionDenied` with `<name> is read-only — not saved`, before any temp file is created (builtin-editor "Read-only file is not saved"; design D5).
- [x] 3.2 Make `EditorState::save` call `check_writable`, then `durable::replace_file(&self.path, &self.to_bytes())`. On success, reset `saved_snapshot` and clear `save_error` as today (builtin-editor "Save in place with line-ending and encoding preservation"; design D4).
- [x] 3.3 Clear `save_error` in `type_char`, `insert_newline`, `backspace`, `cut_selection`, `paste`, `replace_first` (on a replacement), and `undo` (design D6).

## 4. Reducer and TUI wiring

- [x] 4.1 `crates/filecommand-core/src/update.rs`: add `Command::EditorNotEditable { path, reason }`. Handle it like `Command::EditorTooLarge`: set the active panel's `last_error` to the reason-specific notice from design D2 and emit `Effect::OpenViewer { path }` (builtin-editor "Non-UTF-8 file redirects to the viewer"; design D2).
- [x] 4.2 `crates/filecommand-tui/src/app.rs` `run_effects` → `Effect::OpenEditor`: map `LoadResult::NotEditable { reason }` to `Command::EditorNotEditable { path, reason }`.
- [x] 4.3 `crates/filecommand-tui/tests/integration_smoke.rs`: map the new variant in the effect shim.
- [x] 4.4 `crates/filecommand-tui/src/views/editor.rs`: render `save_error` as a one-row `Save failed: <message>` overlay on the last body row in the `DialogError` role, without changing the header or the F-key bar (builtin-editor "A failed save leaves the original intact"; design D6).

## 5. Tests

- [x] 5.1 `editor.rs` unit tests for `classify_editor_bytes`:
  - `FF FE …` → Utf16;
  - `FE FF …` → Utf16;
  - `FF FE 00 00` → Utf16;
  - ASCII with an embedded `00` → Binary;
  - a lone `E9` byte → NotUtf8;
  - truncated multi-byte sequence at EOF → NotUtf8;
  - empty file → editable;
  - `EF BB BF` + `hello` → editable with the BOM flag;
  - `EF BB BF` + invalid UTF-8 → NotUtf8.
- [x] 5.2 `editor.rs` (temp dir): `open` on a Windows-1252 file returns `NotEditable(NotUtf8)` and leaves the file byte-identical; `open` on a ≥10 MiB file still returns `TooLarge` without classifying (the existing test stays).
- [x] 5.3 `editor.rs` (temp dir): a BOM file's `lines[0]` has no U+FEFF; Home then type `X` and save produce `EF BB BF X…`; `byte_len()` includes the 3 BOM bytes (builtin-editor "UTF-8 file with a byte-order mark is editable", "UTF-8 byte-order mark is written back").
- [x] 5.4 `durable.rs` unit tests with the fault-injection seam:
  - failure mid-write leaves the original byte-identical and no `.fc-tmp` sibling;
  - failure before the replace leaves the original byte-identical;
  - success leaves no temp or backup files;
  - a missing original is created (builtin-editor "A failed save leaves the original intact", "Content is flushed before it replaces the original").
- [x] 5.5 `editor.rs`: saving a read-only file returns `PermissionDenied`, the file stays unchanged and read-only, `is_modified()` stays true, and no temp file is created (builtin-editor "Read-only file is not saved").
- [x] 5.6 `editor.rs`: after a failed save sets `save_error`, `type_char` clears it (builtin-editor "Save failure message clears on the next edit").
- [x] 5.7 Windows-only integration test (`#[cfg(windows)]`, temp dir on NTFS):
  - hidden and system attributes survive a save;
  - creation time is unchanged;
  - a `:Zone.Identifier` stream survives with the same content;
  - the existing CRLF/LF round-trip tests still pass through the new path (builtin-editor "Attributes and creation time survive a save", "Alternate data streams survive a save on Windows").
- [x] 5.8 Unix-only test: saving through a symlink replaces the link target and leaves the link a symlink; the permission mode is preserved.
- [x] 5.9 `update/tests.rs`: `Command::EditorNotEditable` for each reason sets the reason-specific notice on the active panel, emits `Effect::OpenViewer` for the same path, and does not enter `UiPhase::Editor` (builtin-editor "UTF-16 file redirects to the viewer", "File containing a NUL byte redirects to the viewer").
- [x] 5.10 `update/tests.rs`: with `state.editor` set (external editor configured), F4 on any file still dispatches the external editor and never emits `Effect::OpenEditor` (builtin-editor "External editor receives non-UTF-8 files unchanged").
- [x] 5.11 `views/editor.rs`: add an insta snapshot of the editor with `save_error` set, showing the overlay row, unchanged header, and unchanged F-key bar.

## 6. Docs

- [x] 6.1 README "Viewer (F3) and editor (F4)": state that the built-in editor edits UTF-8 text only. Files that are not UTF-8, that are UTF-16, or that contain NUL bytes open in the viewer with a notice. Saves are atomic and keep file attributes; read-only files are not saved.

## 7. Verify

- [x] 7.1 `cargo test --workspace` green, including `cargo insta test` review of the new snapshot.
- [x] 7.2 `cargo clippy --workspace --all-targets` with no new warnings.
- [x] 7.3 `openspec validate editor-encoding-safety --strict` clean.
- [ ] 7.4 Manual check on Windows:
  - F4 on an ANSI `.bat` with `é` opens the viewer with the notice;
  - F4 on a PowerShell 5.1 `>`-redirected file (UTF-16LE) opens the viewer;
  - F4 on a UTF-8-BOM file edits and saves with the BOM intact;
  - F2 on a read-only file shows the read-only message;
  - F2 on a hidden file keeps it hidden.
