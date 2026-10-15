# Design: copy-onto-itself-guard

## Context

All of the following was verified on `build/open-proposals-2026-10` @ `1bada6f`.

**How the destination reaches the worker.** F5 and F6 open the destination dialog pre-filled with the opposite panel's directory (`enter_file_op_setup`). `confirm_destination_input` builds `Job { dest_dir: PathBuf::from(trimmed), .. }` with no comparison against `source_dir`. With both panels on the same directory, Enter therefore starts a job whose destination is the source directory.

**How the worker decides there is no conflict.** In `run_copy_or_move` (`fs_ops/worker.rs`), each planned file's target is `job.dest_dir.join(rel_path)`. A conflict is raised only when `fs.metadata(&target)` succeeds **and** `!identity_matches(fs, &planned.src_abs, target_md.file_id)`. `identity_matches` compares the source's followed identity with the target's no-follow identity, and answers `false` unless both are known. A target with the same identity as the source is therefore "no conflict". That rule exists for case-only renames, but here it sends the file straight to `copy_one_file(src, src)`.

**What the copy primitive does to a file copied onto itself.** The Windows `copy_chunked_impl` and the portable `copy_file_chunked` both run `File::open(from)` and then `File::create(to)`. `File::create` truncates the existing file, because Rust's default share mode lets the second handle open a file the first handle is reading. The read loop then sees end-of-file at once. A probe compiled with the audited toolchain (`rustc 1.98.1`), which replicates `copy_chunked_impl` on a 2,048-byte file under a `\\?\` path, printed `before=2048 result=Ok(0) after=0`. The worker then runs its best-effort metadata restore against that same, now-empty file and reports `Done`.

**When the identity is unknown.** The Windows `RealFs` reads identity through `file_id_via_handle`, which returns `None` when the handle cannot be opened. In that case:

- `identity_matches` answers `false`, so the same file raises an overwrite conflict against itself, and Overwrite truncates it as above.
- `RealFs::same_volume` answers `false`, so a same-directory Move skips `run_move_same_volume` and takes the copy-then-delete path. After the self-copy, `let _ = fs.remove_file(src)` deletes the file.

**Aliases.** `FakeFs::metadata` resolves ancestor links, so a target reached through a junction to the source directory reports the source's identity. NTFS does the same. A file symlink as the target is different: its no-follow identity is the link's, so today it raises a conflict, and Overwrite truncates the source through the link.

## Goals / Non-Goals

**Goals:**

- No Copy, and no copy half of a move, ever opens its own source for writing.
- The user learns why the file was not copied. The item appears in the end-of-job skipped-files summary.
- Regression tests that fail against today's code, using `FakeFs` and `RealFs`.

**Non-Goals:**

- Temporary-file landing, flushing and metadata preservation (`durable-file-copy`).
- Rejecting a same-directory destination in the destination dialog before the job starts. This could be added later on top of the worker check.
- Copying a directory into its own subdirectory, for example `C:\work\sub` to `C:\work\sub\inner`. That is a different defect and is not changed here.
- Same-volume moves and in-place renames. `run_move_same_volume` and `run_rename` keep their identity-equals-no-conflict rule, which case-only renames rely on (file-operations "Identity-aware case-only rename").

## Decisions

### D1: The worker checks for "same file" before the conflict check

For each planned file, `run_copy_or_move` calls a new `same_file(fs, src, target)` helper before computing the conflict. The helper answers `true` when either of these holds:

- **Identity.** `fs.metadata_follow(target)` and `fs.metadata_follow(src)` both report a file identity, and the two are equal. The target is *followed* here, unlike the conflict check, so that a file symlink pointing at the source also counts as the same file.
- **Path fallback.** Either identity is unknown, and the two paths are equal after `path::lexically_normalize`. The comparison is case-insensitive on Windows, where a case-insensitive volume is the norm, and exact elsewhere.

A target that does not exist is never the same file.

When the helper answers `true`, the item gets `SkippedItem { path: src, reason: "source and destination are the same file" }`. Nothing is written, no conflict dialog is shown, and in move mode the source is not removed. The loop then continues with the next file. As with a conflict Skip, `files_done` and `bytes_done` do not advance for the skipped item.

*Alternative considered:* rely on `durable-file-copy`'s temp-and-rename. Copying onto itself through a temporary file keeps the content, but a copy would still rewrite the file pointlessly, and a copy-then-delete move would then delete it. That change is also much larger and should not hold up a Critical fix.

*Alternative considered:* refuse in the reducer when the destination equals the source directory. Rejected as the only guard, because it misses aliases that only identity catches: junctions, `subst` drives, and a UNC path to a local share. A dialog-level check can be added later on top.

*Alternative considered:* raise the overwrite-conflict dialog for the same file. Rejected, because every choice in that dialog either destroys the file (Overwrite) or means "skip" anyway. A rename to a new name is the one useful outcome, and the user can get that by typing a different destination.

### D2: The path fallback assumes an absolute destination

The path comparison is meaningful only for absolute paths. `relative-destination-resolution` (R-06) makes the job's destination absolute. Until it lands, a relative destination reaches the worker unchanged. The identity comparison is unaffected, because the OS resolves both paths before the identity is read. The fallback then simply does not match, which is no worse than today.

## Risks / Trade-offs

- [The path fallback is lexical] Two different spellings of the same file, such as a `subst` drive versus its real path, are not caught when identity is unknown. → Identity is normally readable on local NTFS volumes, the common case. The fallback only covers the rare unknown case.
- [Case-insensitive comparison on a case-sensitive directory] On a Windows directory with per-directory case sensitivity enabled, `a.txt` and `A.txt` can be distinct files. → The fallback runs only when identity is unknown, and then the cost is a skipped item listed in the summary, not data loss.
- [A file symlink target is now skipped instead of raising a conflict] Overwriting a file symlink with a copy of its own target is no longer possible through F5. → That operation truncated the source through the link anyway, so no working behavior is lost.

## Migration Plan

None. No persisted data or configuration changes.

## Open Questions

None.
