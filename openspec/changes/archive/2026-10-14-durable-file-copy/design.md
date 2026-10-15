# Design: durable-file-copy

## Context

All of the following was verified on `build/open-proposals-2026-10` @ `1bada6f`.

**The copy primitive.** `FileSystem::copy_file_chunked(from, to, chunk_bytes, on_chunk)` is implemented on Windows by `copy_chunked_impl` (`fs_ops/fs.rs`), and by an identical loop in `portable_impl`:

- It opens `from`, then `File::create(to)`. That truncates an existing target immediately.
- It loops `read` → `write_all` → `on_chunk(total)`.
- On `ChunkControl::Cancel` it drops the destination, removes it, and returns `Interrupted`.
- Any other error returns through `?` and leaves the partially written destination in place.
- There is no `sync_all`, no length check, and the handle is closed by `Drop`, which ignores `CloseHandle` errors.

**The per-file loop.** `copy_one_file` (`fs_ops/worker.rs`) calls the primitive with `CHUNK_BYTES = 256 KiB`. On success it restores metadata, every call best-effort through `let _`:

- named ADS, through `list_streams` (hand-rolled `FindFirstStreamW` FFI) and `copy_stream_chunked`
- the last-write time, through `set_modified`
- read-only, through `set_readonly`

Hidden, system and archive attributes and the creation time are not restored. On error it asks the error-recovery dialog (Retry/Skip/Skip All/Abort) through `ErrorPolicy`.

**Copy/move orchestration.** `run_copy_or_move`:

- Computes a conflict only when the target exists **and** has a different file identity (`identity_matches`). A target with the *same* identity is silently copied over, which truncates the file to 0 bytes. That defect is fixed separately by `copy-onto-itself-guard` (see D6).
- On Overwrite, it clears read-only on the target and lets `File::create` truncate it.
- For a move, after `CopyOneResult::Done`, it runs `let _ = fs.set_readonly(src, false); let _ = fs.remove_file(src);`.

**What `CopyFileExW` preserves.** Measured with .NET's `File.Copy`, which calls `CopyFileEx`, on this machine. A source with creation time 2001-02-03, last-write time 2002-03-04, attributes ReadOnly|Hidden|System|Archive, and a `Zone.Identifier` ADS copied to:

| Kept | Not kept |
|---|---|
| Last-write time | Creation time (set to the time of the copy) |
| All four attributes | |
| The ADS | |

A separate probe showed:
- A read-only + hidden file can be renamed over a hidden target (`MoveFileEx` replace).
- Renaming over a **read-only** target fails with "Access is denied." The existing "clear read-only before overwrite" step is therefore still needed. `std::fs::rename`, which `RealFs::rename` uses, behaves the same: a review probe got `os error 5` over a read-only target and success over a hidden one.

**Test reality (B-01).** `RealFs` is referenced only by the `fs_ops::mod` re-export and `filecommand-tui/src/worker.rs:416`, and no test constructs it. `FakeFs`, `InjectableOp`, `FakeNode` and `Injection` are compiled unconditionally and re-exported `pub`. No crate outside `filecommand-core`'s own `#[cfg(test)]` modules uses `FakeFs`: neither `filecommand-tui/src` nor `filecommand-tui/tests`. `filecommand-core` has no `tests/` directory today.

## Goals / Non-Goals

**Goals:**

- A cross-volume move deletes a source only after its copy is flushed to stable storage and length-checked. Flush and close failures are copy failures, and a source that cannot be removed is reported.
- No copy ever leaves a truncated file under a target's name. An Overwrite that fails leaves the old target intact, and Cancel, Skip and Abort leave no temporary file behind.
- On Windows, a copy preserves data, named ADS, the read-only, hidden, system and archive attributes, and the creation and last-write times.
- `RealFs` is exercised by real-filesystem integration tests, and `FakeFs` is not in the release binary.

**Non-Goals:**

- ACLs, owner and other security-descriptor fields. The copy inherits the destination directory's permissions, as an Explorer copy does. This also applies to the copy half of a cross-volume move.
- Last-access time, sparse and compression flags (these follow `CopyFileExW`'s own behavior and are not guaranteed), and directory attributes and timestamps. Directories are still created fresh at the destination, so a hidden directory's copy is not hidden. This is unchanged from today.
- An "encryption will be lost" confirmation. EFS is kept where the destination supports it, and is otherwise copied decrypted (see D3).
- Detecting a copy onto its own source (`copy-onto-itself-guard`), recreating links (`move-reparse-point-safety`), progress-report throttling (`idle-aware-event-loop`, P-11), typed errors and logging for metadata-restore failures (`error-diagnostics`, B-06), and waiting for cleanup on quit or cancelling a blocked job (`process-lifecycle-hardening`).
- Flushing plain Copy jobs (D4) and unbuffered I/O for very large files.

## Decisions

### D1: Land every copied file through `<name>.~fc-partial` in the destination directory

`copy_one_file` no longer copies straight to the target. It works in four steps:

1. Choose a temporary path in the target's own directory. This puts it on the target's volume, so the final step is a same-volume rename.
2. Call `fs.copy_file(src, temp, …)`.
3. On success, `fs.rename(temp, final_target)`. This replaces an existing target on Overwrite.
4. On **every** other path, best-effort `fs.remove_file(temp)` (a `NotFound` is ignored). That covers copy error, rename error, cancellation, a Skip or Abort chosen from the error dialog, and a panic-free early return.

Retry repeats steps 2-4 from scratch.

**Name.** The temporary file is `<target file name>.~fc-partial`:

- The suffix keeps the file next to its target in a name-sorted panel.
- It gives the file an extension no shell association claims, so a leftover is never "opened" or executed by accident.
- It is self-describing if a crash leaves it behind: visible, recognizable, and safe to delete.
- If that name already exists (a leftover, or a user file of that name), the worker uses the first free candidate in `<name>.~fc-partial-2`, `-3`, … (bounded, e.g. 100 tries). The copy creates the chosen file **exclusively**, so an existing file is never overwritten or deleted to make room.
- If `<name>.~fc-partial` would exceed the 255-UTF-16-unit component limit, the base name is shortened, keeping the suffix. The temporary name never needs to round-trip.

*Alternative considered:* a hidden or dot-prefixed name (`.~fc-<name>`). On Windows a dot prefix does not hide a file, and a leftover would sort away from its target.
*Alternative considered:* a temp file in `%TEMP%`. It would be on the wrong volume, and the final step would be another copy, not an atomic rename.
*Alternative considered:* a per-process unique name (pid or random). It is harder to recognize as "FileCommand's leftover" and is not needed once creation is exclusive.

### D2: `FileSystem::copy_file` copies data *and* metadata, creates its destination exclusively, and optionally flushes

The trait method becomes `copy_file(&self, from, to, opts: &CopyOptions, on_progress: &mut dyn FnMut(u64) -> ChunkControl) -> io::Result<CopyReport>`, with `CopyOptions { chunk_bytes: usize, flush: bool }` and `CopyReport { bytes: u64, unrestored: Vec<Unrestored { what, error }> }`. The contract has six parts:

- **Exclusive create.** `to` must not exist. If it does, the call fails with `AlreadyExists`.
- **Metadata.** The call preserves metadata as far as the platform allows (D3 and D8).
- **Byte count.** `bytes` is the main data stream's byte count, the same quantity progress reports.
- **Length check.** Before returning `Ok`, the implementation checks that the destination's length equals `bytes`. A mismatch is an error.
- **Flush.** With `flush: true`, the destination is also flushed to stable storage before returning, and a flush or close failure is an error.
- **Unrestored metadata.** Metadata the implementation could not carry over is listed in `unrestored` (for example "creation time", "stream Zone.Identifier", "read-only attribute"), with the OS message. It never fails the call, and it is never silently swallowed inside the filesystem layer.

This change does not display `unrestored`. The worker ignores it, which equals today's `let _`. It is the hand-off point for `error-diagnostics`' metadata warnings, whichever of the two lands second.

The worker's restore block moves into the implementations. `list_streams`, `copy_stream_chunked` and `set_modified` leave the trait, because `CopyFileExW` copies ADS natively. The Windows `stream_enum` FFI stays as a `RealFs`-internal helper, used only for the post-copy stream check (D3). `set_readonly` stays on the trait: it is still needed to clear a target's or source's read-only attribute. The trait still covers the operations the "Narrow internal fs trait seam" requirement lists.

*Alternative considered:* keep the restore in the worker and add only `sync`. That keeps the per-stream copy loop and splits one platform concern across two layers. With `CopyFileExW`, the OS already did that work during the copy, and only a verification of the result is left.

### D3: Windows copies with `CopyFileExW`

`RealFs::copy_file` on Windows calls `CopyFileExW(from, to, Some(progress_routine), lpData, null, COPY_FILE_FAIL_IF_EXISTS | COPY_FILE_ALLOW_DECRYPTED_DESTINATION)` on `to_extended_path` forms.

**Progress routine.**
- `lpData` points at the caller's `&mut dyn FnMut(u64) -> ChunkControl`.
- The routine reports the cumulative bytes of **stream 1** (`dwStreamNumber == 1`, `StreamBytesTransferred`). ADS bytes therefore never push the gauge past the planned total.
- It returns `PROGRESS_CANCEL` when the callback answers `Cancel`, and `PROGRESS_CONTINUE` otherwise, including on `CALLBACK_STREAM_SWITCH`. Cancellation is still polled on every callback.
- A cancelled copy fails with `ERROR_REQUEST_ABORTED` (1235), which is mapped to the existing `Interrupted` "job cancelled" error. `CopyFileExW` deletes its partial destination itself, and the worker's cleanup (D1) is a harmless second attempt.
- The routine must not unwind across the FFI boundary. Its body runs inside `std::panic::catch_unwind`, a panic answers `PROGRESS_CANCEL` and is re-raised with `resume_unwind` after `CopyFileExW` returns.
- `CopyOptions::chunk_bytes` is ignored: the OS chooses the chunk size, and progress cadence follows it.

**Creation time.** `CopyFileExW` does not preserve it (Context). After a successful copy, the destination is opened with `OpenOptionsExt::access_mode(FILE_WRITE_ATTRIBUTES)`, which works even though the copy already carries the read-only attribute. Then `File::set_times(FileTimes::new().set_created(src_created))` is called through `std::os::windows::fs::FileTimesExt`. A failure here does not fail the copy. It is returned as `Unrestored { what: "creation time", .. }`.

**Stream check.** `CopyFileExW` copies named streams itself. It is not documented to report a stream it could not create, for example on a destination without named-stream support. So when the source has named streams (`stream_enum::list_streams`, the existing helper), the copy's stream list is compared after the copy, and every missing name is returned as `Unrestored { what: "stream <name>", .. }`. Files without named streams, the common case, pay one `FindFirstStreamW` on the source, as today.

**Flush and length (`flush: true`).** `FlushFileBuffers` needs a write handle, and opening a read-only file for write fails. The sequence is therefore:

1. If the copy is read-only, clear the bit.
2. Open the copy with write access.
3. Compare `len()` with the stream-1 byte count.
4. `sync_all()`.
5. Close.
6. Restore read-only.

Without `flush`, only the length check runs, on the `FILE_WRITE_ATTRIBUTES` handle used for the creation time.

**EFS.** `COPY_FILE_ALLOW_DECRYPTED_DESTINATION` keeps today's outcome: an encrypted file copied to a volume that cannot hold EFS still succeeds, unencrypted. Where the destination supports EFS, `CopyFileExW` keeps the copy encrypted, which is an improvement on today's plaintext read/write loop. A confirmation prompt is a non-goal.

**FFI.** `CopyFileExW` and the `LPPROGRESS_ROUTINE` signature are declared by hand in the crate's existing style, with a `// SAFETY:` comment on each `unsafe` block. If `ci-and-build-tooling` (B-07) has already moved core's FFI to `windows-sys`, the declarations come from there instead.

*Alternative considered:* keep the manual loop and add attribute and creation-time restore. More hand-written metadata code for strictly less fidelity, since `CopyFileExW` also handles EFS and future NTFS metadata. The application design doc (`docs/superpowers/specs/2026-08-06-filecommand-design.md`, the `fs_ops` paragraph) already chose `CopyFileEx`. It asked for an integration test to confirm that attributes and timestamps survive, and the Context measurement shows the creation time does not, hence the explicit restore.
*Alternative considered:* `std::fs::copy`, which wraps `CopyFileExW` on Windows. It has no progress or cancel hook, so the 256 KiB-granular cancel contract would regress to whole-file granularity.
*Alternative considered:* `CopyFile2`. Equivalent capability, but a different callback struct, and nothing here needs its extra features.

### D4: Verification means flush + length check + surfaced errors, and only moves flush

The copy half of a move passes `flush: true`. Plain Copy passes `flush: false` but still gets the length check, which costs one stat. The rationale:

- A move deletes the only other copy, so it must not proceed on data still sitting in a write-behind cache.
- `FlushFileBuffers` per file is expensive on slow media. Copying thousands of small files to a USB stick would pay it on every file, while the source still exists, so the cost buys little.

The R-07 audit recommendation is the same: `sync_all` plus a size check in move mode. Any verification failure is a copy failure: the temp is removed, the source is untouched, and the error-recovery dialog is shown.

### D5: Source removal goes through the error-recovery dialog

After the rename lands, a move removes its source inside `attempt_with_recovery(src, …)`: clear read-only, then `remove_file`. The first failure shows the existing error-recovery dialog:

- **Retry** retries the removal.
- **Skip** and **Skip All** record a `SkippedItem { path: src, reason: "copied, but the source could not be removed: <os error>" }`. The destination copy stays.
- **Abort** stops the job.

The `let _ =` at worker.rs:412-415 is gone. Directory cleanup stays best-effort and silent, because a directory left non-empty by a skipped file is expected. Link removal is governed by `move-reparse-point-safety`.

### D6: The same-file check runs before a temporary file is chosen

`copy-onto-itself-guard` adds a check to `run_copy_or_move` that skips any planned file whose target is the same file as its source. This change keeps that check where it is, ahead of D1's temporary-path chooser. A same-file item therefore never creates a `.~fc-partial` file, and a copy-then-delete move never renames a copy over its own source and then deletes it.

*Alternative considered:* rely on D1 alone. Temp-and-rename onto the same file keeps the content, but a copy would still rewrite the file pointlessly, and a copy-then-delete move would then delete it.

### D7: Overwrite clears read-only, then renames over the target

On Overwrite (or Overwrite All), the worker still clears the target's read-only attribute up front. The probe showed `MoveFileEx` cannot replace a read-only file, while a hidden target is replaced fine. The target is then left untouched until step 3 of D1. A failure before that point leaves the target byte-for-byte intact, though with read-only cleared. Re-applying read-only on the failure path is not attempted: the error dialog is already in front of the user and the target's content is safe.

### D8: The non-Windows fallback keeps the manual loop

`portable_impl::copy_file`:

1. Opens `to` with `OpenOptions::new().write(true).create_new(true)`.
2. Copies chunk by chunk with the same cancel handling.
3. Sets the last-write time on the open handle (`File::set_modified`).
4. `sync_all()` when `flush`, and compares `metadata().len()` with the bytes written.
5. Closes, then applies the read-only state.

A failure in step 3 or step 5 is returned in `unrestored`, not swallowed. Creation time, extended attributes and ADS do not exist or are not exposed portably. This matches the spec's "on platforms without these attributes" clause.

### D9: B-01 — a `RealFs` integration suite, with `FakeFs` gated to `cfg(test)`

**The suite.** `crates/filecommand-core/tests/realfs.rs` drives `fs_ops::run_job` with `RealFs` and a small recording `JobSink` defined in the test file, which needs only the public API. If `copy-onto-itself-guard` or `move-reparse-point-safety` already created the file, this change extends its harness. Each test gets its own directory under `std::env::temp_dir()`, named with the process id and test name (the crate's existing convention, so no `tempfile` dependency), with a drop guard that clears read-only and removes the tree. Coverage:

- **Cross-platform:**
  - copy round-trip
  - cancel cleanup: a multi-chunk file, a sink that cancels on the first progress event, then no target and no `.~fc-partial`
  - read-only target overwrite
  - copy into a destination that already holds `<name>.~fc-partial` uses `-2` and leaves the existing file alone
- **`#[cfg(windows)]`:**
  - ADS, attributes (set with `attrib +r +h +s +a`) and creation and last-write times survive a copy, read back through `MetadataExt::file_attributes`, `created()`, `modified()` and `File::open("<path>:<stream>")`
  - a failed overwrite keeps the old target: the source is held open with `share_mode(0)` so `CopyFileExW` fails with a sharing violation, the answer is Skip, and the old target content survives with no `.~fc-partial`
  - paths longer than 260 characters, built under `\\?\`, copy and delete
  - a case-only rename through a `Rename` job
  - a cross-volume move through a `CrossVolume<RealFs>` wrapper that answers `same_volume = false`, so the copy-then-verified-delete path runs on one volume: the source is gone and the destination is intact
  - the junction-inside-a-moved-tree test from `move-reparse-point-safety`, which shares the `mklink /J` helper

**Gating.** `FakeFs`, `FakeNode`, `Injection`, `InjectableOp` and the `pub use fs::FakeFs` re-export in `fs_ops/mod.rs` become `#[cfg(test)]`. The module docs stop intra-linking `FakeFs` from non-test items, to avoid broken rustdoc links. No `test-support` cargo feature is added: no other crate, and no integration test, uses `FakeFs` (checked: `filecommand-tui` and its `tests/` only construct `RealFs`). The feature can be introduced the day a cross-crate consumer appears.

*Alternative considered:* `#[cfg(any(test, feature = "test-support"))]` now, as the audit suggests. It adds a feature nothing enables yet.

### D10: FakeFs models partial files and flush failure

`FakeFs::copy_file`:

- creates the destination node at the start, then grows its size chunk by chunk
- respects exclusive creation (`AlreadyExists`)
- copies the source node's modeled streams, read-only and modified fields

An injected `CopyAfterChunks` failure therefore leaves a partial node, as a real filesystem does, and the worker tests prove D1's cleanup. A new `InjectableOp::Flush` makes a `flush: true` copy fail after the last chunk, with the node fully written, so the "flush failure keeps the source" path is testable without hardware.

## Risks / Trade-offs

- [An overwritten target is replaced, not rewritten in place] Hard links to the old target keep the old content. The new file carries the copy's inherited ACL rather than the old target's ACL, and the old target's creation time is replaced by the source's (which is what "preserve the source's timestamps" means). → This is the price of never destroying the target on failure, and it is documented here and in the spec scenario for a failed overwrite.
- [Moves get slower on slow media] Moves pay one `FlushFileBuffers` per file. → It applies to moves only, where the source is about to be deleted. Plain Copy is unaffected.
- [Leftover `.~fc-partial` after a crash or power loss] → Leftovers are visible, recognizable and never mistaken for the real file. The next copy of the same file uses a numbered variant rather than touching them.
- [`CopyFileExW` behavior varies by Windows version and filesystem, e.g. FAT32 has no ADS or creation-time precision] → The `RealFs` suite asserts only what NTFS guarantees, and the spec states what is not guaranteed.
- [FFI callback correctness] → `catch_unwind` in the progress routine, `SAFETY` comments, and the cancel test in the `RealFs` suite.
- [Temp name visible mid-copy] A user who presses Ctrl+R on the destination mid-copy sees `name.~fc-partial`. → This is intentional transparency. The panel re-reads automatically when the job completes, and the file is gone by then.

## Migration Plan

None. No persisted data or configuration changes. Leftover `.~fc-partial` files cannot exist before this change ships.

## Open Questions

None.
