# Design: move-reparse-point-safety

## Context

All of the following was verified on `build/open-proposals-2026-10` @ `1bada6f`.

**How a cross-volume move is planned.** `run_copy_or_move(job, fs, sink, is_move)` (`fs_ops/worker.rs`) sends a move to `run_move_same_volume` only when every source is on the destination's volume. Otherwise it builds a `CopyPlan` with `plan_copy` → `walk_copy`:

- `walk_copy` takes the non-following `fs.metadata(abs)`.
- For a reparse point it decides `is_dir_like` from `fs.metadata_follow(abs)`.
- A directory-like reparse point is treated like a real directory. Its relative path is pushed to `plan.dirs`, and its children are enumerated through the link with `fs.read_dir(abs)`.
- Every child file becomes a `PlannedFile { src_abs: <link>\<child>, rel_path, size }`.
- A visited set of *followed* `FileId`s stops only cycles that lead back into the tree already being walked. A link pointing anywhere else is traversed.

**How the move deletes its source.** For each file whose copy returns `CopyOneResult::Done`, `is_move` runs `let _ = fs.set_readonly(&planned.src_abs, false); let _ = fs.remove_file(&planned.src_abs);`. For a file planned through a junction, `src_abs` is `C:\work\proj\shared\lib.dll`, and Windows resolves it to `C:\shared-libs\lib.dll` and deletes that file. The `set_readonly` call also follows the link: `RealFs::set_readonly` uses `std::fs::metadata` plus `set_permissions`, so it clears the target's read-only attribute first.

After the loop, when the job was not cancelled, the cleanup runs `let _ = fs.remove_dir(&job.source_dir.join(rel_dir))` for every planned directory, deepest first. Planned directories include the target's own subdirectories, reached through the link, so the target's emptied subdirectories are removed too. Finally the junction itself is removed.

**Contrast with Delete.** `walk_delete` checks `md.reparse.is_reparse_point()` before recursing and emits `DeleteItem::ReparseLeaf { path, is_dir: md.is_dir }`. It never enumerates through a link, and it removes the leaf with `remove_dir` or `remove_file` depending on `is_dir`, without read-only clearing.

**What `RealFs` reports for a junction (Windows).** A probe compiled with the audited toolchain (`rustc 1.98.1`) against a `mklink /J` junction printed the following:

| Call | Result |
|---|---|
| `symlink_metadata`, `read_dir` entry metadata | `is_dir=false is_symlink=true attrs=0x410` (DIRECTORY \| REPARSE_POINT) |
| `metadata` (follow) | `is_dir=true` |
| `std::fs::remove_file(junction)` | `Access is denied. (os error 5)` |
| `std::fs::remove_dir(junction)` | OK, the target's files intact |

The Windows `to_fs_metadata` sets `is_dir: md.is_dir()`. A junction's no-follow `FsMetadata` therefore has `is_dir = false`, and Delete's `ReparseLeaf` calls `remove_file`, which fails. The audit records Delete as CONFORMING because only `FakeFs` tests exercise it, and `FakeFs` models a junction as `Reparse { is_dir: true }`.

`reparse_kind_from_metadata` also never returns `Junction`: `file_type().is_symlink()` is true for every name-surrogate tag, including mount points. Only `is_reparse_point()` is ever consulted, so this is cosmetic.

**Why `FakeFs` cannot reproduce R-01 today.** `FakeFs::metadata`, `copy_file_chunked` (through `metadata`) and `read_dir` resolve ancestor reparse points, so the copy half reads through the link as NTFS does. `remove_file`, `remove_dir`, `set_readonly`, `set_modified`, `rename` and `create_dir` only lexically normalize the path. `remove_file("/v1/src/link/f.txt")` returns `NotFound` (ignored by `let _`) instead of deleting `/shared/f.txt`, so a `FakeFs` version of the R-01 scenario passes against the buggy code.

## Goals / Non-Goals

**Goals:**

- A cross-volume move never deletes, empties or changes the attributes of anything reached through a reparse point in the source. Only the link itself is removed, exactly as Delete removes it.
- Keep the existing copy rule: the destination receives the link target's content (user decision).
- Keep the source link while its content copy is incomplete.
- Make reparse-leaf removal work on real NTFS for directory junctions and directory symlinks, for both Delete and Move.
- Regression tests that fail against today's code: one with `FakeFs`, one with `RealFs` and a real junction (Windows only).

**Non-Goals:**

- Recreating junctions or symlinks at the destination (user decision). The destination keeps receiving a plain directory, or a plain file for a file symlink.
- Same-volume moves. `rename` moves a link as a link and never traverses it, so it is already safe. One quirk stays out of scope: the same-volume decision follows a *selected* top-level link to its target's volume. A selected junction whose target lives on the destination volume is therefore attempted as a `rename` across volumes, which fails with a recoverable error dialog, not data loss. See Open Questions.
- Copy behavior, the cycle guard, and the plan's byte or file totals. All are unchanged.
- Durability of the copy half (flush, length check, temp-file landing, surfacing of regular-file source-removal failures). These belong to `durable-file-copy`.
- Distinguishing `ReparseKind::Junction` from `Symlink` in `RealFs`. Nothing consumes the distinction.
- Panel listing of links (`link-aware-listing`).

## Decisions

### D1: The plan records how each source item is removed

`walk_copy` threads an `under_link: bool` through its recursion and records a removal disposition on every planned item:

- `PlannedFile` gains `removal: SourceRemoval`, one of `RemoveFile`, `RemoveLink { is_dir }` or `Keep`:
  - A regular file not beneath a traversed link is `RemoveFile`. This is today's behavior: clear read-only, then `remove_file`.
  - A file-type reparse point (a file symlink) not beneath a link is `RemoveLink { is_dir: false }`. Its content is copied by following it, as today, and the source half removes only the link.
  - Everything planned beneath a traversed directory link is `Keep`. That includes the link target's own files, nested links and nested directories.
- `plan.dirs` becomes `Vec<PlannedDir { rel, src_abs, removal: DirRemoval }>`, where `DirRemoval` is one of `RemoveIfEmpty`, `RemoveLink { is_dir }` or `Keep`:
  - A real directory not beneath a link is `RemoveIfEmpty`. This keeps today's best-effort cleanup: a directory left non-empty by a skipped file survives.
  - A directory-type reparse point is `RemoveLink`.
  - Directories beneath a link are `Keep`. They are still created at the destination, because `rel` is unchanged.
- `src_abs` is recorded during the walk instead of being rebuilt as `job.source_dir.join(rel)`. The link's own path is needed anyway, and this removes the assumption that a source's `original_name` sits directly in `source_dir`.

*Alternative considered:* skip directory links entirely in move mode, with a SkippedItem "links are not moved across volumes" (the audit's option b). Rejected because the user decided the copy rule stays: a move must still deliver the content, and refusing is a functional regression.

*Alternative considered:* recreate the link at the destination (the audit's option a). This is a non-goal by user decision. It would also need privilege for symlinks and a `DeviceIoControl` reparse-buffer writer for junctions.

### D2: A link is removed only after its content copied completely

During the file loop, every planned file carries the index of the nearest enclosing directory link, if any. If its outcome is anything other than `Done` (conflict skip, error skip), that link is marked incomplete. The cleanup pass, which already runs only when `!cancelled`, then works deepest first:

- `RemoveIfEmpty` directories get best-effort `remove_dir`, as today.
- `Keep` directories are never touched.
- `RemoveLink` directories are removed only when not marked incomplete.
- A file-type `RemoveLink` is removed right after its own copy succeeds, the same place a `RemoveFile` is removed today.

This mirrors how a real directory survives a skipped file. The user is never left with the link gone while the destination holds a partial copy. A cancelled move removes no links, consistent with today's "no cleanup on cancel."

*Alternative considered:* always remove the link once the walk finishes. That is safe for data but loses the link even when the user skipped half its content. Rejected.

### D3: Link removal reuses Delete's reparse-leaf rule, and a failure is reported

A small helper `remove_reparse_leaf(fs, path, is_dir)` is factored out of `run_delete`. It calls `remove_dir` for a directory-object link and `remove_file` otherwise, and never clears read-only, because `set_readonly` follows the link and would modify the target. `run_delete` and the move's `RemoveLink` items both use it, so both halves share one rule.

A link removal that fails (permission denied, sharing violation) is pushed as a `SkippedItem { path: <link>, reason: "copied, but the link could not be removed: …" }`. It then shows in the existing end-of-job summary, unlike the silently ignored `let _` used for directory cleanup. Directory cleanup stays silent because a non-empty directory is an expected outcome there, while a failed link removal is not. This matches `durable-file-copy`'s rule that source-removal failures surface.

### D4: RealFs classifies a directory link as a directory object (Windows)

In `windows_impl::to_fs_metadata`, `is_dir` comes from the attribute bits: `attrs & FILE_ATTRIBUTE_DIRECTORY (0x10) != 0`.

- For non-reparse entries this is identical to `md.is_dir()`.
- For followed metadata (`std::fs::metadata`) it is still the target's directory-ness.
- For the no-follow metadata of a junction or `mklink /D` symlink it becomes `true`. `FsMetadata::is_dir` on a reparse point then means "the link itself is a directory object, removed with `remove_dir`."

The probe confirmed that `remove_dir` on a junction removes only the link. The portable (non-Windows) implementation keeps `md.is_dir()`, which is false for a Unix symlink, and that is correct there: a Unix symlink to a directory is removed with `unlink` (`remove_file`), and `rmdir` would fail with `ENOTDIR`.

Other consumers of no-follow `is_dir` are `walk_copy` and the overwrite-conflict removals in `run_move_same_volume`/`run_rename`. `walk_copy` reads it only for non-reparse entries. The overwrite-conflict removals choose `remove_dir` when the conflicting target is a directory junction. Today they would call `remove_file` and ignore the "Access is denied." Both changes are correct.

*Alternative considered:* query the reparse tag (`FSCTL_GET_REPARSE_POINT` or `FindFirstFileW`'s `dwReserved0`). Rejected because it is more FFI for no benefit: the directory attribute is exactly the bit that decides `RemoveDirectoryW` versus `DeleteFileW`.

### D5: FakeFs resolves ancestor links in every path-taking operation, as NTFS does

`remove_file`, `remove_dir`, `set_readonly`, `set_modified`, `rename` (both ends), `create_dir` and the copy's destination all resolve their path with `resolve_ancestors` before touching the node map, matching `metadata`. The final component is left as is, so removing a link path removes the link node. `remove_file` on a `Dir` node or a `Reparse { is_dir: true }` node fails with `PermissionDenied`, matching `DeleteFileW` on a directory object.

With these two changes the `FakeFs` regression test deletes `/shared/f.txt` under today's code, so the test fails before the fix and passes after it. Existing `FakeFs` tests place no real nodes beneath links (other than the reparse tests, whose expectations hold), so they are unaffected.

### D6: RealFs coverage uses a real junction and a "cross-volume" wrapper

A single development machine or CI runner usually has one volume, which would route a `RealFs` move through `rename`. The Windows test therefore wraps `RealFs` in a test-local `CrossVolume<RealFs>` `FileSystem` implementation that delegates every call and answers `same_volume = false`. This drives the real copy-then-delete path on one volume.

The fixture builds `src\proj\file.txt`, `outside\keep.txt` and `src\proj\shared` = `mklink /J` → `outside`. Junctions need no privilege, unlike symlinks. The test then runs a Move of `proj` to `dst` through `run_job` and asserts:

- `outside\keep.txt` still exists with its content.
- `dst\proj\shared\keep.txt` exists.
- `src\proj` is gone.

A second test deletes a junction through `run_job` and asserts that the link is gone and the target is intact. This covers D4.

The harness lives in `crates/filecommand-core/tests/realfs.rs`, the shared `RealFs` suite that `durable-file-copy` fills out (`copy-onto-itself-guard` may create it first). If this change is the first to land, it creates that file with only the helpers it needs: a temp-dir guard, a `mklink /J` helper invoked through `cmd /C mklink /J`, and `CrossVolume`.

## Risks / Trade-offs

- [Behavior change for links inside a moved tree] The link's target content is still *copied*, but its originals now stay in place. A user who expected "move" to relocate the target's files will find them still at the target. → This is the intended, documented semantics (spec scenario), the same as Delete's leaf rule, and the only safe one when the target lies outside the selection.
- [Link kept on partial copy] A move with skipped files under a link leaves the link and the moved remainder. → The skipped items are listed in the summary, and the user can retry. This matches how real directories behave today.
- [D4 widens `is_dir = true` to directory links in no-follow metadata] Any future caller that treats no-follow `is_dir` as "recurse into" would traverse links. → `walk_delete` and `walk_copy` check `reparse` first. The field's meaning on reparse points is documented on `FsMetadata`.
- [FakeFs fidelity changes could break tests that relied on lexical-only mutation] → Run the full `fs_ops` suite. Only the reparse-point tests touch nodes beneath links.

## Open Questions

- Should the same-volume test for a *selected* top-level link use the link's own volume (no-follow) instead of its target's? Today a selected junction whose target is on the destination volume is attempted as a cross-volume `rename` and fails with a recoverable error. This is outside R-01, and a candidate for a follow-up.
