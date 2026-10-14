# Design: editor-encoding-safety

## Context

This was verified against `build/open-proposals-2026-10` at `1bada6f`.

**Open path.** F4 on a panel file runs `update::handle_request_editor`. If an external editor is configured, that wins. Otherwise the reducer emits `Effect::OpenEditor { path }`. `run_effects` in `crates/filecommand-tui/src/app.rs` executes it synchronously by calling `EditorState::open(&path)`:

- `open` stats the file. At or above `EDITOR_MAX_BYTES` (10 MiB) it returns `LoadResult::TooLarge { size }` without reading the file.
- Otherwise it reads the whole file and calls `EditorState::from_bytes`.
- `from_bytes` decodes with `viewer::decode::decode_lossy`, which is `String::from_utf8_lossy` (the audit says `from_utf8_lossy` directly; the call goes through the viewer's helper, with the same effect). It then splits lines with `logical_lines`, which strips one trailing `\r` per line, and detects the dominant line ending from the raw bytes.

The TUI turns `TooLarge` into `Command::EditorTooLarge { path, size }`. The reducer then:

1. sets the active panel's `last_error` to "`<name>` is too large for the editor (10 MB limit) — opened in the viewer". This is the panel mini-status, visible once the viewer closes.
2. emits `Effect::OpenViewer { path }`.

Nothing inspects the content. Invalid UTF-8 becomes U+FFFD, NUL bytes load as control characters, and a UTF-16 file loads as mojibake interleaved with NULs.

**UTF-8 BOM today.** `from_utf8_lossy` keeps the BOM, so U+FEFF becomes the first character of `lines[0]`. It is written back on save only because it is still in the buffer. It is a zero-width character that counts as a caret column, though. Home puts the caret in front of it, so typing there pushes the BOM into the middle of the line. One Right arrow from (0,0) moves the caret past it with no visible movement, and a Backspace there then deletes it without any visible change.

**Save path.** `Command::EditorSave` emits `Effect::SaveEditor { editor, then_quit }`, which `run_effects` executes synchronously with `EditorState::save()`:

- `save` calls `std::fs::write(to_extended_path(&self.path), self.to_bytes())`. This truncates the file, then writes it.
- There is no flush to stable storage, no temporary file, and no attribute handling.
- On a read-only file, `std::fs::write` fails with the OS's access-denied error. The TUI sends `Command::EditorSaveFailed { message }`, and the reducer stores the message in `EditorState::save_error`. The original file is untouched in that case, but `save_error` is **never rendered** (no view reads it), so the user only sees the ` *` marker stay. The field's doc comment also says it is "cleared by the next edit", but only `save()` clears it.

Mixed line endings are normalized to the dominant ending on save. That is specified behavior, not in scope here.

## Goals / Non-Goals

**Goals:**

- The built-in editor never loads content it cannot write back byte-exactly apart from edits and line-ending normalization. Files that are not UTF-8 go to the viewer, using the same mechanism and notice surface as the size redirect.
- A UTF-8 BOM survives a save, and the user can never edit it in or out by accident.
- F2 is crash-safe. At every point the file on disk is either the complete old content or the complete new content, and the new content is flushed before it replaces the old.
- Saving does not silently change the file's attributes, creation time, or (on Windows) DACL or alternate data streams. Today's truncate-and-write keeps all of them because the file object survives, so the atomic save must not regress them. Saving never makes a read-only file writable.
- A failed save is visible to the user.

**Non-Goals:**

- Editing legacy code pages or UTF-16. Supporting them needs encoding detection and a transcoding writer; the redirect is the safe floor. A read-only view of the decoded text already exists in F3.
- Preserving mixed line endings. Dominant-ending normalization stays as specified.
- Moving open or save off the UI thread. That is `offload-blocking-io` (R-09/P-09).
- Copy and Move attribute preservation (`durable-file-copy`, R-14) and `config.toml`/`history.json` durability (`config-parsing-robustness`, `history-persistence-hardening`). They only share the helper introduced here.
- Keeping hard-link identity (see Risks).

## Decisions

### D1: Classify bytes after the size gate, in core, before building the buffer

`EditorState::open` keeps its order: stat, size gate, read. After reading it calls a new pure function, `classify_editor_bytes(&[u8]) -> Result<EditorText<'_>, NotEditableReason>`. Checks run in this order, and the first match wins:

1. Starts with `FF FE` or `FE FF` → `NotEditableReason::Utf16`. This also catches UTF-32LE, which starts `FF FE 00 00`.
2. Contains a `0x00` byte anywhere → `NotEditableReason::Binary`. The check is a `memchr`-style scan of at most 10 MiB, already in memory.
3. Starts with `EF BB BF` → remember `has_bom = true` and validate the rest.
4. `std::str::from_utf8` on the remainder fails → `NotEditableReason::NotUtf8`.

Otherwise it returns the validated `&str` and the BOM flag. `open` returns the new variant `LoadResult::NotEditable { reason }`.

The order is chosen for the notice. A UTF-16 file is also invalid UTF-8 and usually contains NULs, but "UTF-16 text" tells the user more than "not UTF-8". UTF-16 without a BOM ends up as Binary because ASCII-range UTF-16 contains NULs, and that message is still accurate.

The check runs only on the built-in editor path. With an external editor configured, F4 never reaches `Effect::OpenEditor`, so external editors still get every file (existing "External editor takes precedence" scenario).

Alternatives considered:

- **Open read-only in the editor.** The audit suggests this as an option. It needs a new editor mode and a lossy display, and the viewer already does read-only display well. The user chose the viewer redirect.
- **Heuristic "looks binary" checks**, such as the share of control characters. These are not well defined and are not needed: NUL plus strict UTF-8 validation catches real binaries, and valid UTF-8 text with control characters round-trips exactly.

### D2: The redirect reuses the TooLarge mechanism with its own reply command

The new reply `Command::EditorNotEditable { path, reason }` is handled exactly like `Command::EditorTooLarge`: it sets the active panel's `last_error` notice and emits `Effect::OpenViewer { path }`. The notice text depends on the reason:

- `<name> is UTF-16 text — the editor only edits UTF-8; opened in the viewer`
- `<name> looks binary (contains NUL bytes) — the editor only edits UTF-8 text; opened in the viewer`
- `<name> is not valid UTF-8 — the editor only edits UTF-8 text; opened in the viewer`

`EditorTooLarge` stays as it is, so `offload-blocking-io` and the existing tests keep their shape.

Alternative considered: one `EditorRedirect { reason }` command that replaces `EditorTooLarge`. This is cleaner, but it renames a command that another in-flight proposal (offload-blocking-io) depends on, and the benefit is cosmetic.

### D3: Strip the UTF-8 BOM into a flag and write it back on save

`EditorState` gains `has_bom: bool`. The buffer is built from the text after the BOM, so `lines[0]` never contains U+FEFF. `to_bytes()` prefixes `EF BB BF` when `has_bom` is set. Because `to_bytes` is what `save` writes and what `byte_len` measures, the header's size figure still matches what F2 writes, and that figure includes the BOM.

Alternative considered: keeping the BOM as a buffer character. That is today's behavior, with the caret and editing problems described in Context.

### D4: Atomic save through a shared durable-replace helper

A new core module (working name `crate::durable`) provides `replace_file(path: &Path, bytes: &[u8]) -> io::Result<()>`. It writes through `to_extended_path` throughout, in these steps:

1. **Resolve the target.** If `path` is a symbolic link or another name-surrogate reparse point, it is resolved to its final target, and that file is replaced. The link stays a link. This mirrors how `std::fs::write` follows links today.
2. **Snapshot the original.** If the target exists, read its metadata: attributes, read-only flag, and on non-Windows the permission mode.
3. **Create the temporary file.** It goes in the target's directory with a unique name, `.<name>.<pid>.<n>.fc-tmp`, using `create_new`.
   - On Windows it is created with the original's HIDDEN, SYSTEM, ARCHIVE, and NOT_CONTENT_INDEXED attribute bits, through `std::os::windows::fs::OpenOptionsExt::attributes`.
   - On other platforms its permission mode is set to the original's.
4. **Write and flush.** Write all bytes, then call `File::sync_all()` and close the file.
5. **Replace the original.**
   - **Windows, original exists:** call `ReplaceFileW(target, temp, backup, REPLACEFILE_IGNORE_MERGE_ERRORS | REPLACEFILE_IGNORE_ACL_ERRORS)`, where `backup` is a sibling `.<name>.<pid>.<n>.fc-bak`. ReplaceFile documents that it carries over the original's creation time, DACL, named streams (ADS, for example `Zone.Identifier`), object ID, encryption, and compression. It does not carry over the owner or the SACL, so the spec promises the DACL only, and `IGNORE_ACL_ERRORS` makes the DACL best-effort on volumes without ACLs. A review probe through .NET's `File.Replace`, which calls `ReplaceFile` with these two flags, confirmed on NTFS that the creation time, the hidden attribute and a `Zone.Identifier` stream survive, that the result takes the *original's* attributes rather than the temporary file's, and that replacing a read-only original fails with "Access is denied." Setting the attribute bits on the temporary file (step 3) is therefore redundant on this path, though harmless. On success the backup is deleted, and a failure to delete it is ignored. The documented partial-failure codes are handled with the backup present, so neither copy is ever lost:
     - `ERROR_UNABLE_TO_REMOVE_REPLACED` and `ERROR_UNABLE_TO_MOVE_REPLACEMENT`: both files keep their names. The temp file is deleted and the error is returned.
     - `ERROR_UNABLE_TO_MOVE_REPLACEMENT_2`: the original was moved to the backup name. It is renamed back, the temp file is deleted, and the error is returned.
   - **Windows, original missing** (deleted while it was being edited): `ReplaceFileW` reports file-not-found, and the helper falls back to `std::fs::rename(temp, target)`.
   - **Non-Windows:** `std::fs::rename(temp, target)`, then a best-effort `sync_all` of the parent directory.
6. **Clean up on failure.** On any error before the replace succeeds, the temporary file is removed and the error is returned unchanged. The original is untouched.

`EditorState::save` makes two calls in order: `check_writable(target)`, then `durable::replace_file(target, &self.to_bytes())`. On success it clears `save_error` and resets `saved_snapshot` as it does today.

`ReplaceFileW` comes from `windows-sys` (features `Win32_Storage_FileSystem` and `Win32_Foundation`), added as a `cfg(windows)` dependency of `filecommand-core`. The TUI already depends on windows-sys 0.59. This follows the direction of audit B-07 (`ci-and-build-tooling`) instead of adding another hand-declared `extern "system"` block, and the `unsafe` call carries a `// SAFETY:` comment: the paths are NUL-terminated wide strings that live for the call, and the reserved pointers are null.

Alternatives considered:

- **Temp file, `sync_all`, `std::fs::rename` on every platform, then re-apply attributes and creation time with `File::set_times`/`FileTimesExt::set_created`.** This needs no FFI, but it loses the original's explicit DACL and its alternate data streams. Losing `Zone.Identifier` silently removes the Mark of the Web from downloaded scripts on edit. The file-operations spec already treats ADS as data worth keeping.
- **Keep `std::fs::write` and add `sync_all`.** This flushes data but does nothing about truncation: a crash between the truncate and the write still leaves a short file.
- **Fall back to an in-place write when the temporary file cannot be created**, for example when the directory denies create permission but the file allows write. That is exactly the unsafe path this change removes. The save fails with the OS error instead, and the buffer stays modified (see Risks).

The helper is deliberately generic, taking a path and bytes. `config-parsing-robustness` (`config.toml` rewrites) and `history-persistence-hardening` (`history.json`) reuse it instead of growing a third and fourth copy of the temp-file-and-rename code in `config.rs`.

### D5: Read-only files are refused, never made writable

`check_writable` reads the target's metadata. If the target exists and `permissions().readonly()` is true, `save` returns an `io::Error` of kind `PermissionDenied` with the message `<name> is read-only — not saved`, before any temporary file is created. Today the save also fails on a read-only file, but with an OS error string that is never shown. On Windows, `ReplaceFileW` would fail the same way ("Access is denied", D4). On other platforms `rename` would succeed and replace content the user had marked read-only, which is exactly what this decision rules out. The behavior is the same; the outcome is now deterministic and stated, and no temporary file is created only to be discarded.

Alternative considered: clearing read-only, saving, and setting it again, as the file-operations engine does for overwrite and delete. Those are explicit user operations on file entries with confirmation dialogs. An editor save that silently overrides a protection bit is a different contract. Users who want to edit the file can clear the attribute first.

### D6: Save failures are rendered, and an edit clears them

When `save_error` is set, `views/editor.rs` renders it as a one-row overlay on the last body row, directly above the F-key bar. The overlay reads `Save failed: <message>`, uses the `DialogError` role, and is truncated to the width. The header and F-key bar specified by "Full-screen editor chrome" stay unchanged, in the same way the save-on-exit confirmation overlays the body without changing the chrome. The buffer-mutating methods (`type_char`, `insert_newline`, `backspace`, `cut_selection`, `paste`, `replace_first`, `undo`) clear `save_error`, which matches the field's existing doc comment. A successful save clears it as it does today.

Alternatives considered:

- **Reporting the failure on the panel mini-status.** The panels are hidden while the editor is open, so the message would show up only after the editor closes, which is too late to act on.
- **Replacing the header's `Line/Col` indicator.** That changes the chrome that "Full-screen editor chrome" specifies.

## Risks / Trade-offs

- **[Risk] A legitimate UTF-8 text file containing a NUL byte now opens in the viewer instead of the editor.** → Such files are rare, and the editor could not have shown NUL sensibly anyway. The viewer shows them, and an external editor (`editor =`) still opens them.
- **[Risk] Hard links: the replace gives the saved name a new file identity**, so other hard-link names of the same file keep the old content. `ReplaceFileW` and `rename` both behave this way. → This is accepted. Text files edited inside FileCommand are rarely hard-linked. This change does not detect link counts.
- **[Risk] Directories that allow writing an existing file but not creating one** (some locked-down shares) can no longer be saved to. → The save reports the OS error and keeps the buffer. The external editor remains an option. This is the price of never truncating in place.
- **[Risk] A crash between the temp-file write and the replace leaves a `.fc-tmp` file** (or, in the rare `ReplaceFileW` recovery paths, a `.fc-bak` file) next to the edited file. → The names are distinctive and hidden-style (leading dot), and the original is intact. Automatic cleanup is not attempted, because another instance may be mid-save.
- **[Trade-off] Saving is slower:** an extra file create, a `sync_all`, and a replace instead of a single write. → This is unnoticeable on local disks for files under 10 MiB. `offload-blocking-io` later moves the save off the UI thread for slow media.
- **[Risk] `ReplaceFileW` edge semantics on network or FAT volumes**, which have no ACLs or streams. → The `IGNORE_MERGE_ERRORS` and `IGNORE_ACL_ERRORS` flags make missing features non-fatal. The integration tests cover NTFS, and the helper's error paths are covered by the fault-injection tests.

## Migration Plan

There is no data migration. Files that the previous build would have corrupted on save now open in the viewer. Rollback means reverting the change. Any `.fc-tmp` or `.fc-bak` leftovers are inert files.

## Open Questions

None.
