# Tasks: move-reparse-point-safety

## 1. FakeFs fidelity (`crates/filecommand-core/src/fs_ops/fs.rs`)

- [ ] 1.1 Route the `FakeFs` path-taking mutations — `remove_file`, `remove_dir`, `set_readonly`, `set_modified`, `rename` (both ends), `create_dir`, and the copy's destination node — through `resolve_ancestors`, as `metadata` already does, so an ancestor link is traversed like NTFS (design D5)
- [ ] 1.2 Make `FakeFs::remove_file` fail with `PermissionDenied` on a `Dir` node or a `Reparse { is_dir: true }` node, matching `DeleteFileW` on a directory object (design D5)
- [ ] 1.3 Unit tests in `fs.rs`: `remove_file` through an ancestor junction removes the target-side node; `remove_dir` on a junction path removes only the link node; `remove_file` on a directory junction is refused
- [ ] 1.4 Write the R-01 regression test first, in `worker.rs` tests, and confirm it FAILS against the unmodified worker. The setup: volumes `/v1` and `/v2`; `/v1/work/proj/file.txt`; `/v1/shared/lib.dll` and `/v1/shared/x86/a.dll`; junction `/v1/work/proj/shared` → `/v1/shared`. Move `proj` to `/v2/backup`. Assert that `/v1/shared/lib.dll` and `/v1/shared/x86/a.dll` survive, that `/v2/backup/proj/shared/lib.dll` exists, and that `/v1/work/proj` is gone (file-operations: "Cross-volume move of a tree containing a junction to an outside directory leaves that directory's files intact")

## 2. RealFs directory-link classification (`fs_ops/fs.rs`, Windows)

- [ ] 2.1 In `windows_impl::to_fs_metadata`, derive `is_dir` from `FILE_ATTRIBUTE_DIRECTORY` (0x10) in `file_attributes()` instead of `md.is_dir()`. Document on `FsMetadata::is_dir` that for a non-followed reparse point it means "the link itself is a directory object" (design D4)
- [ ] 2.2 Leave `portable_impl::to_fs_metadata` unchanged: a Unix symlink is removed with `remove_file`. Note this in a comment (design D4)

## 3. Move plan and source removal (`crates/filecommand-core/src/fs_ops/worker.rs`)

- [ ] 3.1 Add `SourceRemoval { RemoveFile, RemoveLink { is_dir }, Keep }` to `PlannedFile`, and replace `CopyPlan::dirs: Vec<PathBuf>` with `Vec<PlannedDir { rel, src_abs, removal: DirRemoval { RemoveIfEmpty, RemoveLink { is_dir }, Keep } }>` (design D1)
- [ ] 3.2 Thread `under_link: bool` (plus the index of the enclosing link) through `walk_copy`. A traversed directory-type reparse point becomes `DirRemoval::RemoveLink` and everything beneath it becomes `Keep`. A file-type reparse point outside a link becomes `SourceRemoval::RemoveLink { is_dir: false }`. Copy totals and the cycle guard are unchanged (design D1)
- [ ] 3.3 Factor `remove_reparse_leaf(fs, path, is_dir)` out of `run_delete`: `remove_dir` when `is_dir`, otherwise `remove_file`, never clearing read-only. Use it from `run_delete` (design D3)
- [ ] 3.4 In `run_copy_or_move`'s `is_move` branch, act on `removal`. `RemoveFile` keeps today's read-only clear plus `remove_file`; the surfacing of its failures is `durable-file-copy`'s concern. `RemoveLink` calls `remove_reparse_leaf`. `Keep` does nothing (design D1, D3)
- [ ] 3.5 Track, per directory link, whether any file beneath it ended in anything other than `Done`, and mark that link incomplete (design D2)
- [ ] 3.6 Rewrite the post-loop cleanup over `PlannedDir`, deepest first, using each entry's recorded `src_abs` rather than `job.source_dir.join(rel)`. `RemoveIfEmpty` gets best-effort `remove_dir` as today. `Keep` is skipped. `RemoveLink` is removed with `remove_reparse_leaf` only when complete. A failed link removal is pushed as a `SkippedItem` reading "copied, but the link could not be removed: …" (design D2, D3; file-operations: "A link whose content was not fully copied is kept")

## 4. Tests

- [ ] 4.1 Confirm 1.4 now passes
- [ ] 4.2 `worker.rs`: cross-volume move of a selected junction removes only the link and copies the target's content (file-operations: "Cross-volume move of a selected junction removes only the link")
- [ ] 4.3 `worker.rs`: cross-volume move through a junction with an injected copy failure on one file beneath it and an error-recovery Skip. The junction survives, the target is intact, and the summary lists the skipped file (file-operations: "A link whose content was not fully copied is kept")
- [ ] 4.4 `worker.rs`: cross-volume move of a file symlink copies the target's content, removes only the link, and leaves the target file and its read-only attribute untouched (design D3)
- [ ] 4.5 `worker.rs`: an injected `RemoveDir` failure on the link during cleanup is reported as a skipped item (design D3)
- [ ] 4.6 `worker.rs`: the existing `reparse_point_delete_removes_link_not_target`, `reparse_point_copy_duplicates_target_content`, `reparse_point_cycle_is_skipped_not_infinite` and `move_cross_volume_*` tests still pass
- [ ] 4.7 `crates/filecommand-core/tests/realfs.rs` (`#[cfg(windows)]`): use the shared harness, or create the minimal one if `durable-file-copy` has not landed: a unique temp-dir guard under `std::env::temp_dir()`, a `cmd /C mklink /J` helper, and a `CrossVolume<RealFs>` wrapper that delegates everything and answers `same_volume = false` (design D6)
- [ ] 4.8 `realfs.rs` (`#[cfg(windows)]`): moving `proj`, which contains a junction to an outside directory, through `run_job` with `CrossVolume<RealFs>` leaves the outside directory's files intact, copies them under `dst\proj\shared`, and removes `src\proj` including the junction (design D6)
- [ ] 4.9 `realfs.rs` (`#[cfg(windows)]`): Delete of a junction through `run_job` with `RealFs` completes with no error dialog, removes the link and leaves the target intact (design D4; file-operations: "Delete removes the link, not the target")

## 5. Verify

- [ ] 5.1 `cargo test --workspace` green, on Windows so the `#[cfg(windows)]` `RealFs` tests run
- [ ] 5.2 `cargo clippy --workspace --all-targets` introduces no new warnings
- [ ] 5.3 `openspec validate move-reparse-point-safety --strict` clean
- [ ] 5.4 Manual (Windows, two volumes, e.g. a USB stick): create `C:\tmp\proj\shared` with `mklink /J` pointing to a scratch directory holding a few files. F6 `proj` to the other volume. Confirm the scratch directory's files are intact and the destination holds copies. Then F8 a junction and confirm it is removed without an error dialog and its target is intact
