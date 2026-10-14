# Change: editor-encoding-safety

## Why

The built-in F4 editor can destroy the files it edits (audit finding **R-02**, severity **Critical**). `EditorState::from_bytes` decodes every file with a lossy UTF-8 decode, so each byte sequence that is not valid UTF-8 becomes U+FFFD in the buffer, and F2 writes the buffer back as UTF-8. Every non-ASCII character in a Windows-1252 `.bat` or `.ini` file, every character of a UTF-16 file (Windows PowerShell 5.1 `>` output, `.reg` exports), and every byte of a small binary file that is not valid UTF-8 is replaced with `EF BF BD` on the first save. Nothing detects or refuses these files. For example, a user fixes a one-character typo in an ANSI batch file, presses F2, and loses every accented character in the file for good.

The save itself is also unsafe. `EditorState::save` calls `std::fs::write`, which truncates the file and then rewrites it in place. A crash, a full disk, or a power loss during the write leaves a truncated file. The spec requirement is named "Save in place with line-ending and encoding preservation", but its text only promises UTF-8 output, so it does not cover any of this.

## What Changes

- **Refuse non-UTF-8 files at open.** A file that is not valid UTF-8, starts with a UTF-16 byte-order mark (`FF FE` or `FE FF`), or contains a NUL byte is not loaded into the editor. F4 opens it in the F3 viewer, and a notice explains that the built-in editor only edits UTF-8 text. This uses the same mechanism and notice surface as the existing ≥10 MB redirect. A file with a UTF-8 BOM is still editable.
- **Keep the UTF-8 BOM.** The BOM is removed from the buffer at load, so it is never an invisible first character the caret can type in front of or delete. It is written back on save.
- **Save atomically.** F2 writes the encoded buffer to a temporary file in the same directory, flushes it to stable storage, and then replaces the original in one rename. A failure at any step leaves the original file byte-for-byte intact, removes the temporary file, and keeps the buffer modified.
- **Keep file metadata on save.** The original file's attributes (hidden, system, archive), its creation time, and on Windows its alternate data streams and access-control list (DACL) carry over to the saved file. Today's in-place write keeps all of these, so the atomic save must not lose them. A read-only file is never made writable: the save is refused with a message, and the original stays untouched. Today's save of a read-only file also fails, but with a raw OS error.
- **Show save failures.** Today `EditorSaveFailed` stores its message in `EditorState::save_error`, but nothing renders it. The editor will show the message until the next edit or successful save.
- Mixed line endings are still normalized to the dominant ending on save. This is the documented existing behavior and is a non-goal here.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `builtin-editor`:
  - "Editor invocation and size cap" now also redirects non-UTF-8, UTF-16-BOM, and NUL-containing files to the viewer with a notice, and states that a UTF-8 BOM file is editable.
  - "Save in place with line-ending and encoding preservation" now requires an atomic, flushed save that leaves the original intact on failure. It also requires preserving file attributes, refusing read-only files, round-tripping the UTF-8 BOM, and showing save failures.

## Impact

- **Specs:** `builtin-editor` (two MODIFIED requirements).
- **Code (core):**
  - `crates/filecommand-core/src/editor.rs`:
    - `EditorState::open`: classify the bytes after the size gate.
    - `EditorState::from_bytes`: strip the UTF-8 BOM into a new `has_bom` field, and decode strictly instead of with `decode_lossy`.
    - New `LoadResult::NotEditable { reason }` and `NotEditableReason`.
    - `to_bytes` and `byte_len`: prefix the BOM.
    - `save`: atomic replace through the new helper; read-only refusal.
    - Edits clear `save_error`.
  - New durable-replace helper module in `crates/filecommand-core/src/` (temp file in the same directory, `sync_all`, replace, keep metadata, clean up on failure). On Windows it uses `ReplaceFileW` through a new `cfg(windows)` `windows-sys` dependency on `filecommand-core`.
  - `crates/filecommand-core/src/update.rs`: new `Command::EditorNotEditable { path, reason }` reply, handled like `Command::EditorTooLarge` (panel notice plus `Effect::OpenViewer`).
- **Code (TUI):**
  - `crates/filecommand-tui/src/app.rs`, `run_effects` → `Effect::OpenEditor`: map the new `LoadResult` variant.
  - `crates/filecommand-tui/src/views/editor.rs`: render `save_error`.
  - `crates/filecommand-tui/tests/integration_smoke.rs`: map the new `LoadResult` variant in its effect shim.
- **Docs:** README "Viewer (F3) and editor (F4)" bullet: non-UTF-8 files open in the viewer.
- **Tests:** unit tests in `editor.rs` for classification, BOM round-trip, and atomic save with failure injection. Reducer tests in `update/tests.rs` for the redirect notice. Windows integration tests (temp directory) for attribute, creation-time, ADS, and read-only behavior. An insta snapshot of the save-error row.
- **Compatibility:** files that used to open in the editor and then get corrupted on save now open in the viewer instead. UTF-8 files behave as before.

**Audit findings:** R-02

**Coordination:**
- `offload-blocking-io` (R-09/P-09) moves editor open and save onto a worker thread. **This change lands first.** Classification and the atomic save live in core's `EditorState::open`/`save`, so moving them is mechanical, and the worker replies with `EditorNotEditable` alongside `EditorOpened`/`EditorTooLarge`/`EditorSaved`. Its `Saving…` notice uses the same last body row as this change's save-failure row; the notice shows while a save is outstanding, and the failure row after a failed one.
- `config-parsing-robustness` and `history-persistence-hardening` need the same temp-file, `sync_all`, and rename durability for `config.toml` and `history.json`. This change introduces the shared durable-replace helper and they reuse it. If either lands first, it introduces the helper and this change reuses it.
- `ci-and-build-tooling` (B-07) moves core's hand-declared Windows FFI to `windows-sys`. This change adds `windows-sys` to `filecommand-core` for `ReplaceFileW`, so the two changes share the dependency entry.
