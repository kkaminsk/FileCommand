# Tasks: durable-file-copy

## 1. FileSystem copy contract (`crates/filecommand-core/src/fs_ops/fs.rs`)

- [ ] 1.1 Replace `copy_file_chunked` with `copy_file(from, to, opts: &CopyOptions { chunk_bytes, flush }, on_progress) -> io::Result<CopyReport { bytes, unrestored }>`, documenting the contract on the trait (design D2):
  - `to` is created exclusively (`AlreadyExists` if present).
  - Metadata is preserved.
  - `bytes` is the main-stream byte count, and a length mismatch is an error.
  - With `flush`, the data is flushed to stable storage, and flush or close errors are returned.
  - Metadata that could not be carried over is listed in `unrestored` and never fails the call.
- [ ] 1.2 Remove `list_streams`, `copy_stream_chunked` and `set_modified` from the trait. Keep `set_readonly`. The Windows `stream_enum` helper stays as `RealFs`-internal code (design D2)
- [ ] 1.3 Windows `RealFs::copy_file`: declare `CopyFileExW` and the progress-routine type, with a `// SAFETY:` comment per `unsafe` block, or use `windows-sys` if `ci-and-build-tooling` landed first. Call it with `COPY_FILE_FAIL_IF_EXISTS | COPY_FILE_ALLOW_DECRYPTED_DESTINATION`. The progress routine reports stream-1 cumulative bytes, maps `ChunkControl::Cancel` to `PROGRESS_CANCEL`, and wraps the callback in `catch_unwind`, re-raising after return. `ERROR_REQUEST_ABORTED` maps to the existing `Interrupted` cancelled error (design D3)
- [ ] 1.4 Windows: after a successful `CopyFileExW`, set the source's creation time through a `FILE_WRITE_ATTRIBUTES` handle (`OpenOptionsExt::access_mode`, `File::set_times` + `FileTimesExt::set_created`). A failure goes to `unrestored`. Without `flush`, check the length on that handle. With `flush`: clear read-only if set, open for write, check the length, `sync_all`, close, and restore read-only (design D3, D4)
- [ ] 1.5 Windows: when the source has named streams (`stream_enum::list_streams`, now `RealFs`-internal), list the copy's streams after the copy and report each missing name in `unrestored` (design D3)
- [ ] 1.6 Delete the Windows `copy_chunked_impl` (design D3)
- [ ] 1.7 Portable `RealFs::copy_file`: `create_new(true)`, the chunk loop with cancel handling, `File::set_modified`, `sync_all` when `flush`, a length check via `metadata()`, then read-only applied after close. `set_modified` and read-only failures go to `unrestored` (design D8)

## 2. FakeFs (`fs_ops/fs.rs`, `fs_ops/mod.rs`)

- [ ] 2.1 `FakeFs::copy_file`: fail with `AlreadyExists` if `to` exists. Create the destination node before the first chunk and grow it per chunk, so an injected `CopyAfterChunks` failure or a cancel leaves a partial node. Copy the source node's streams, read-only and modified fields on success (design D10)
- [ ] 2.2 Add `InjectableOp::Flush`: a `flush: true` copy of the injected path fails after its last chunk, leaving the fully written node (design D10)
- [ ] 2.3 Gate `FakeNode`, `Injection`, `InjectableOp`, `FakeFs` and its `impl FileSystem` with `#[cfg(test)]`, and the `FakeFs` item in `fs_ops/mod.rs`'s `pub use`. Reword the module docs so no non-test item intra-links `FakeFs` (filesystem-error-handling: "The release binary carries no fault-injection double"; design D9)
- [ ] 2.4 Update the existing `fs.rs` `FakeFs` unit tests to the new `copy_file` API. Drop or rewrite `fake_fs_streams_roundtrip` against `copy_file`'s stream propagation

## 3. Worker (`crates/filecommand-core/src/fs_ops/worker.rs`)

- [ ] 3.1 Add the temp-path chooser: `<name>.~fc-partial` in the target's directory, the first free numbered variant `-2`, `-3`, … (bounded), and the base name shortened to fit 255 UTF-16 units (file-operations: "Copies land via a temporary partial file"; design D1)
- [ ] 3.2 Rewrite `copy_one_file` as: choose temp → `fs.copy_file(src, temp, …)` → `fs.rename(temp, final_target)`. Best-effort `fs.remove_file(temp)` runs on every non-success path: copy or verify error, rename error, cancel, Skip, Skip All, Abort. Retry restarts from a fresh temp. Delete the old metadata-restore block. `CopyReport::unrestored` is not displayed by this change: either map it onto `error-diagnostics`' warning items if that change has landed, or drop it with a comment pointing at that change (design D1, D2)
- [ ] 3.3 `run_copy_or_move`: pass `flush: is_move` (design D4)
- [ ] 3.4 `run_copy_or_move`: keep `copy-onto-itself-guard`'s same-file check ahead of the temp-path chooser, so a same-file item never creates a `.~fc-partial` file. If that change has not landed yet, land it first (design D6)
- [ ] 3.5 Overwrite keeps clearing the target's read-only attribute up front. The target is otherwise untouched until the final rename (design D7)
- [ ] 3.6 Move source removal: replace `let _ = fs.set_readonly(..); let _ = fs.remove_file(..)` with `attempt_with_recovery` (clear read-only, then `remove_file`). Skip and Skip All record `SkippedItem { path: src, reason: "copied, but the source could not be removed: …" }`, and Abort cancels the job (file-operations: "A source that cannot be removed is reported"; design D5)

## 4. Worker tests (FakeFs, `fs_ops/worker.rs`)

- [ ] 4.1 A successful copy leaves only the target, with no `.~fc-partial` node (file-operations: "A successful copy appears under the target name with no partial file left")
- [ ] 4.2 Overwrite plus `CopyAfterChunks` failure plus Skip: the original target node is unchanged (size and modified time) and no `.~fc-partial` node remains (file-operations: "A failed overwrite keeps the existing target")
- [ ] 4.3 A new-target copy with `CopyAfterChunks` failure plus Skip: no target and no partial; the summary lists the source (file-operations: "Skip after a failed copy leaves nothing behind")
- [ ] 4.4 A cancel mid-copy leaves no partial node and no target (file-operations: "Cancel removes the partial file")
- [ ] 4.5 An existing `report.txt.~fc-partial` node is untouched, and the copy lands via `-2` (file-operations: "An existing file with the temporary name is left alone")
- [ ] 4.6 A cross-volume move with a `Flush` injection plus Skip keeps the source and leaves no partial node (file-operations: "A flush failure keeps the source")
- [ ] 4.7 A cross-volume move with a `RemoveFile` injection on the source plus Skip: the destination node exists, and the summary lists the source with "could not be removed" (file-operations: "A source that cannot be removed is reported")
- [ ] 4.8 The existing `move_cross_volume_*`, `overwrite_all_*`, `skip_all_*`, `error_*`, `cancellation_*` and `permission_denied_*` tests pass, adjusted only for the API rename

## 5. RealFs integration suite (`crates/filecommand-core/tests/realfs.rs`)

- [ ] 5.1 Harness: a per-test temp directory named with the process id and test name under `std::env::temp_dir()`, with a drop guard that clears read-only recursively and removes the tree. A recording `JobSink` with scripted conflict and error answers and a cancel-after-N-progress option. On Windows only: a `cmd /C mklink /J` helper and a `CrossVolume<RealFs>` wrapper answering `same_volume = false`. If `copy-onto-itself-guard` or `move-reparse-point-safety` already created this file, extend its harness instead (design D9)
- [ ] 5.2 Cross-platform: a copy round-trip; cancel cleanup (multi-chunk file, cancel on first progress → no target, no `*.~fc-partial*`); a read-only target overwritten through an Overwrite answer; an existing `name.~fc-partial` file is left alone (filesystem-error-handling: "The suite exercises the real implementation on temporary directories")
- [ ] 5.3 `#[cfg(windows)]`: ADS (`std::fs::write("f:stream")`), `attrib +r +h +s +a`, and creation and last-write times set with `File::set_times` all survive a Copy and a `CrossVolume` Move. Read back via `file_attributes()`, `created()`, `modified()` and `File::open("<copy>:<stream>")` (file-operations: "Hidden and system attributes survive a copy", "Creation time survives a copy", "A moved file keeps its metadata")
- [ ] 5.4 `#[cfg(windows)]`: failed overwrite. Hold the source open with `share_mode(0)`, answer Overwrite then Skip, and assert the old target's content is intact and no partial remains (file-operations: "A failed overwrite keeps the existing target")
- [ ] 5.5 `#[cfg(windows)]`: a path over 260 characters, built under `\\?\`, copies, moves (`CrossVolume`) and deletes successfully (file-operations: "Operation on a path longer than the legacy limit succeeds")
- [ ] 5.6 `#[cfg(windows)]`: a case-only rename (`foo.txt` → `Foo.txt`) through a `Rename` job succeeds with no conflict (file-operations: "Case-only rename succeeds")
- [ ] 5.7 `#[cfg(windows)]`: a cross-volume move through `CrossVolume<RealFs>` removes the source only after the destination exists with full length (file-operations: "Cross-volume move copies then deletes the source")
- [ ] 5.8 Hold a slot for `move-reparse-point-safety`'s junction-in-moved-tree test, which shares the `mklink /J` helper. If that change has not landed, add a TODO pointing at it

## 6. Verify

- [ ] 6.1 `cargo test --workspace` green on Windows, with the `RealFs` suite running and no leftover temp directories
- [ ] 6.2 `cargo build --workspace --release` succeeds, and the release `filecommand-core` exposes no `FakeFs`/`InjectableOp` (a non-test reference fails to compile)
- [ ] 6.3 `cargo clippy --workspace --all-targets` introduces no new warnings
- [ ] 6.4 `openspec validate durable-file-copy --strict` clean
- [ ] 6.5 Manual (Windows):
  - F5 a hidden+system file and confirm it stays hidden at the destination with its creation date.
  - Fill a small USB stick, F5 Overwrite a file there, and confirm the old file survives the disk-full error.
