# Change: durable-file-copy

## Why

The copy engine treats "the last `write_all` returned `Ok`" as proof that a file arrived. Everything the user cares about beyond the bytes is best-effort or missing. Four audit findings describe the result:

- **R-07 (High) — cross-volume Move deletes its source without verifying the copy.** `copy_chunked_impl` never calls `sync_all` and never compares lengths, and it closes the destination by `Drop`, which discards `CloseHandle` errors. The move then removes the source with `let _ = fs.remove_file(..)`. Deferred write errors on SMB shares and removable media surface only at flush or close, so a move to a USB stick followed by a yank or power loss can leave the destination incomplete with the source already gone. A source that *cannot* be removed (for example, open in another program) is silently left behind, and the job still reports success.
- **R-08 (Medium) — failed copies leave partial files, and Overwrite destroys the target first.** The partial file is removed only on cancel. A disk-full or sharing-violation error leaves a truncated file under the target's name. After a Skip, that file looks like a successful copy. Overwrite opens the target with `File::create`, which truncates it before a single byte is copied, so a mid-copy failure loses the user's previous version too.
- **R-14 (Medium) — Copy loses attributes and the creation time.** The engine restores only the last-write time, read-only and named ADS. Copied hidden or system files become visible, archive state is lost, and backups lose their creation dates. The spec requirement "Copy preserves alternate data streams, attributes, and timestamps" is only partly met.
- **B-01 (High) — the real filesystem engine has no tests, and the test double ships in production.** No test constructs `RealFs`. Every copy, move and delete test runs against `FakeFs`, whose path model differs from NTFS exactly where R-01, R-05, R-07, R-08 and R-14 live. `FakeFs` and `InjectableOp` are `pub` and compiled into the release binary.

Verifying R-08 also turned up a defect the audit does not list: copying a file onto itself truncates it to zero bytes. It is Critical data loss with a one-check fix, so it is tracked separately in `copy-onto-itself-guard` and is not part of this change.

## What Changes

- **Verified move.** The copy half of a cross-volume move flushes each destination file to stable storage and checks that its length equals the bytes copied, before the source is touched. A flush, close or length failure is a copy failure: the source is kept, and the error-recovery dialog offers Retry/Skip/Skip All/Abort. Removing the source goes through the same error-recovery dialog, and a source that cannot be removed is reported in the end-of-job summary instead of being ignored.
- **Copies land via a temporary partial file.** Each file is written to `<name>.~fc-partial` in the destination directory and renamed over the target name only after the copy succeeded. The temporary file is removed on every error, Skip, Abort and Cancel path. An Overwrite never destroys the existing target when the copy fails, and a failed copy never leaves a truncated file under the target's name.
- **Full metadata on Windows via `CopyFileExW`.** On Windows the copy uses `CopyFileExW`. It copies the data, every named ADS, the read-only, hidden, system and archive attributes, and the last-write time. Its progress routine maps the existing `ChunkControl` callback, with `Cancel` returning `PROGRESS_CANCEL`. The creation time, which `CopyFileExW` does not preserve, is set explicitly afterwards. The manual read/write loop remains the non-Windows fallback. The worker's per-stream copy loop goes away. The `FindFirstStreamW` enumeration stays only to report a named stream that did not arrive on the copy. Metadata the copy could not carry over is returned to the worker, not discarded inside the filesystem layer, so `error-diagnostics` can surface it. ACLs and owner are not copied: the copy inherits the destination's permissions, as with Explorer. EFS is kept where the destination supports it.
- **Real-filesystem integration tests; test-only fault injection.** A new `crates/filecommand-core/tests/realfs.rs` suite runs `run_job` on temporary directories against `RealFs`. Windows-only behavior is gated with `#[cfg(windows)]`. The suite covers:
  - copy with ADS, attributes and timestamps
  - a junction inside a moved tree (shared with `move-reparse-point-safety`)
  - paths longer than 260 characters
  - a read-only target
  - a case-only rename
  - cancel cleanup
  - a failed overwrite

  `FakeFs`, `InjectableOp` and their re-export become `#[cfg(test)]`.
- Non-breaking for users, apart from the documented trade-offs in design.md: an overwritten target is replaced rather than rewritten in place, and moves pay one flush per file.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `file-operations`:
  - "Same-volume move is a rename; cross-volume move is copy-then-verified-delete" defines *verified*: flushed, length-checked, with close and flush errors surfaced. It also requires source-removal failures to surface.
  - "Copy preserves alternate data streams, attributes, and timestamps" enumerates exactly what is preserved and what is not.
  - New requirement: "Copies land via a temporary partial file."
- `filesystem-error-handling`: new requirement "Real filesystem engine is integration-tested; fault injection is test-only."

## Impact

- **Specs:** `file-operations` (two modified, one added requirement), `filesystem-error-handling` (one added requirement).
- **Code:**
  - `crates/filecommand-core/src/fs_ops/fs.rs`:
    - `FileSystem::copy_file_chunked` becomes a metadata-preserving `copy_file(from, to, &CopyOptions { chunk_bytes, flush }, on_progress) -> io::Result<CopyReport { bytes, unrestored }>` with exclusive creation of `to`.
    - `list_streams`, `copy_stream_chunked` and `set_modified` leave the trait.
    - The Windows `copy_chunked_impl` (lines 247-271) is replaced by a `CopyFileExW` path.
    - The portable `copy_file_chunked` (lines 324-350) gets temp-safe exclusive create, `set_modified`, optional `sync_all`, and a length check.
    - `stream_enum` (lines 491-579) stays as a `RealFs`-internal helper for the post-copy stream check.
    - `FakeFs` gets partial-file modeling, a flush-failure injection, and `#[cfg(test)]` gating (lines 581-955).
  - `crates/filecommand-core/src/fs_ops/worker.rs`:
    - `copy_one_file` (lines 267-318) gets temp naming, rename-over, and cleanup on every path. Its metadata-restore block (lines 289-299) moves into the `FileSystem` implementations.
    - `run_copy_or_move` (lines 324-439) gets `flush` for moves and source removal through `attempt_with_recovery` (replacing `let _` at 412-415).
  - `crates/filecommand-core/src/fs_ops/mod.rs`: the `FakeFs` re-export becomes `#[cfg(test)]`.
  - New `crates/filecommand-core/tests/realfs.rs`, or an extension of it if `copy-onto-itself-guard` or `move-reparse-point-safety` created it first.
  - No `filecommand-tui` changes. Its only `fs_ops` filesystem use is `RealFs::new()` in `worker.rs:416`.
- **Dependencies:** none new. `CopyFileExW` is declared by hand, matching the crate's existing FFI style, or taken from `windows-sys` if `ci-and-build-tooling` (B-07) has already moved core's FFI there.

**Audit findings:** R-07, R-08, R-14, B-01

**Coordination:**
- `copy-onto-itself-guard` (Critical, not in the audit) adds a same-file check to the same `run_copy_or_move` loop and should land first. This change keeps that check ahead of choosing the temporary file, so a same-file item never creates a `.~fc-partial` file. Both changes add tests to `crates/filecommand-core/tests/realfs.rs`, and whichever lands first creates it.
- `move-reparse-point-safety` changes the same `run_copy_or_move` loop and `FakeFs`, and adds the junction test to the shared `realfs.rs` suite. Its link removal follows this change's "source-removal failures surface" rule. Whichever change lands second rebases. Neither depends on the other.
- `idle-aware-event-loop` (P-11) throttles the progress reports `copy_one_file`'s callback sends. With `CopyFileExW`, the callback cadence is chosen by the OS, and throttling stays in the worker callback.
- `error-diagnostics` (B-06) turns best-effort metadata-restore failures into summary warnings, and today it targets the worker's restore loop, which this change removes. `CopyReport::unrestored` is the hand-off. Every piece of metadata the copy could not carry over is returned there, never swallowed: the creation time, a named stream missing on the copy, and on the portable path the last-write time or read-only state. A metadata failure never fails the copy. Whichever change lands second maps `unrestored` onto `error-diagnostics`' warning items. If this change lands first, the worker drops `unrestored`, which is no worse than today's `let _`.
- `process-lifecycle-hardening` (R-11, R-18) waits for cancellation cleanup on quit and makes a blocked job observe cancel. Removing the `.~fc-partial` file is part of that cleanup.
- `ci-and-build-tooling` (B-07, B-09): if core's FFI moves to `windows-sys` first, `CopyFileExW` comes from there. The README's "file ops against temp directories are tested" claim becomes true with this change.
