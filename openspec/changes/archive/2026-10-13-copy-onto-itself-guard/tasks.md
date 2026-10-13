# Tasks: copy-onto-itself-guard

## 1. Regression tests first (`crates/filecommand-core/src/fs_ops/worker.rs`, `#[cfg(test)]`)

- [x] 1.1 Same-directory copy with `FakeFs`: `/work/report.txt` is created with `add_file_with_stream` and given a last-write time with `set_modified`. A Copy job runs with `source_dir == dest_dir == /work`. Assert that the job completes with `report.txt` in `skipped`, with a reason containing "same file". Assert that the node keeps its size, stream and last-write time, and that `RecordingSink` saw no conflict. `FakeFs` does not model truncation, so the stream and time are what detect today's self-copy. Confirm the test FAILS against the unmodified worker (file-operations: "F5 into the directory the file is already in leaves it unchanged")
- [x] 1.2 Junction alias with `FakeFs`: junction `/alias` → `/work`, then copy `/work/report.txt` to `/alias`. Same assertions as 1.1 (file-operations: "The same file reached through a link is not overwritten")
- [x] 1.3 Copy-then-delete move with `FakeFs`: `set_volume("/v2", 2)` and a junction `/v2/alias` → `/v1/work`, then Move `/v1/work/report.txt` to `/v2/alias`. `same_volume` answers `false`, so the copy-then-delete path runs. Assert that `/v1/work/report.txt` still exists unchanged and is listed as skipped for "same file". Today's code deletes it (file-operations: "A copy-then-delete move onto the same file keeps the source")
- [x] 1.4 Identity unknown: a test-local `NoIdentity<FakeFs>` wrapper delegates every call and clears `file_id`. A same-directory Copy through it must skip `report.txt` for "same file" without raising a conflict. Today it raises a conflict, and the sink's default Overwrite rewrites the file (design D1)

## 2. Worker guard (`crates/filecommand-core/src/fs_ops/worker.rs`)

- [x] 2.1 Add `same_file(fs, src, target) -> bool` next to `identity_matches`. It answers `true` when the followed identities of `src` and `target` are both known and equal. When either is unknown, it answers `true` when `path::lexically_normalize` of the two paths are equal, compared case-insensitively under `cfg(windows)` and exactly elsewhere. A missing target answers `false` (design D1)
- [x] 2.2 In `run_copy_or_move`'s file loop, before the conflict check, skip a `same_file` item: push `SkippedItem { path: planned.src_abs, reason: "source and destination are the same file" }` and continue. Nothing is written, no conflict is raised, the move's source removal is not reached, and `files_done`/`bytes_done` do not advance (file-operations: "A copy never overwrites its own source"; design D1)
- [x] 2.3 Leave `run_move_same_volume` and `run_rename` unchanged. Their identity-equals-no-conflict rule stays for case-only renames. Add a comment at the new check pointing to that difference
- [x] 2.4 Unit-test the path fallback directly: on Windows, `C:\Work\Report.txt` and `c:\work\.\report.txt` are the same file; `C:\work\a.txt` and `C:\work\b.txt` are not. On other platforms the comparison is exact (design D1)

## 3. RealFs integration test (`crates/filecommand-core/tests/realfs.rs`)

- [x] 3.1 If the file does not exist yet, because `durable-file-copy` and `move-reparse-point-safety` have not landed, create it with a minimal harness: a per-test directory under `std::env::temp_dir()` named with the process id and test name, a drop guard that removes it, and a recording `JobSink` that answers conflicts with Overwrite. Otherwise reuse the existing harness
- [x] 3.2 Cross-platform: write a 2,048-byte `report.txt` with known content into a temp directory, then run a Copy job through `run_job` with `RealFs` and `source_dir == dest_dir`. Assert that the file still holds the same 2,048 bytes, and that the outcome lists it as skipped for "same file" (file-operations: "F5 into the directory the file is already in leaves it unchanged")

## 4. Verify

- [x] 4.1 1.1-1.4 pass, and the existing `case_only_rename_*`, `move_cross_volume_*`, `overwrite_all_*` and `skip_all_*` worker tests still pass
- [x] 4.2 `cargo test --workspace` green, including `tests/realfs.rs`
- [x] 4.3 `cargo clippy --workspace --all-targets` introduces no new warnings
- [x] 4.4 `openspec validate copy-onto-itself-guard --strict` clean
- [ ] 4.5 Manual (Windows): open the same directory in both panels, select a file and a directory, press F5 and accept the pre-filled destination. Confirm that every file keeps its size and content and that the summary lists them as "same file"
