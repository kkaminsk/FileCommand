# Change: move-reparse-point-safety

## Why

Audit finding **R-01 (Critical)**: a cross-volume Move deletes files the user never selected. When the moved tree contains a junction or directory symlink, `walk_copy` follows the link and plans every file beneath it with a source path *through* the link (`src\link\file`). After each file is copied, the move half calls `remove_file` on that path, and the OS resolves it through the junction and deletes the real file in the link's target. The empty-directory cleanup then removes the target's subdirectories the same way. Moving `C:\work\proj` to `D:\backup` when `proj\shared` is a junction to `C:\shared-libs` empties `C:\shared-libs`. Delete already treats reparse points as leaves (`walk_delete`); the move's delete half does not. No test covers this, and the in-memory `FakeFs` cannot reproduce it, because its `remove_file`/`remove_dir` do not resolve a path through an ancestor link the way NTFS does.

A related defect sits on the same path: on Windows, `RealFs` reports a junction's own (non-followed) metadata with `is_dir = false`, because `std::fs::Metadata::is_dir()` is false for every name-surrogate reparse point. The reparse-leaf removal therefore calls `remove_file` on a directory junction, which Windows refuses with "Access is denied" (verified with a `mklink /J` probe). Today that makes Delete of a junction fail with an error dialog. It would make the fixed move fail the same way.

## What Changes

- A cross-volume Move treats every reparse point in the source as a leaf for its source-removal half. The copy half is unchanged: the destination still receives a plain copy of the link target's content, per the existing copy rule. The source half removes only the link itself, exactly as Delete does. It never deletes, empties, or changes the attributes of anything reached through the link.
- A link is removed only after everything copied through it succeeded. If any file beneath it was skipped or failed, the link stays in place, the same way a real directory survives when it is left non-empty. A link that cannot be removed is reported in the end-of-job skipped-files summary instead of being ignored.
- On Windows, the `RealFs` no-follow metadata classifies a directory junction or directory symlink as a directory object, using the link's own `FILE_ATTRIBUTE_DIRECTORY` bit. Delete and the move's link removal then use `remove_dir`, which removes the link and leaves the target intact. This also fixes Delete of a junction, which currently fails with "Access is denied."
- `FakeFs` resolves ancestor reparse points in its mutating operations the way NTFS does, and refuses `remove_file` on directory objects. The regression test therefore fails against today's code.
- New regression tests: a `FakeFs` cross-volume move of a tree containing a junction to an outside directory, and a `#[cfg(windows)]` `RealFs` test with a `mklink /J` fixture.
- Non-breaking. Recreating links at the destination is out of scope: the destination still receives the target's content.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `file-operations`: "Reparse-point (symlink/junction) semantics" adds the rule that a cross-volume move's source-removal half treats every reparse point as a leaf. It removes the link, never anything reached through it, and only after the content copied through the link succeeded. New scenarios cover a tree containing a junction to an outside directory, a selected junction, and a link whose content copy was incomplete.

## Impact

- **Specs:** `file-operations` (one modified requirement).
- **Code:**
  - `crates/filecommand-core/src/fs_ops/worker.rs`:
    - `PlannedFile`/`CopyPlan`: each planned file and directory records how its source is removed.
    - `walk_copy` (lines 208-257): marks content reached through a link as "keep" and the link itself as "remove link."
    - `run_copy_or_move` (lines 324-439): the `is_move` `set_readonly` and `remove_file` calls at 412-415 apply only to removable files. The empty-directory cleanup at 430-436 skips kept directories and removes links with the reparse-leaf rule.
    - A shared reparse-leaf removal helper factored out of `run_delete` (`DeleteItem::ReparseLeaf`, lines 93-110 and 146-162).
  - `crates/filecommand-core/src/fs_ops/fs.rs`:
    - Windows `to_fs_metadata` (lines 149-158) takes `is_dir` from `FILE_ATTRIBUTE_DIRECTORY`.
    - `FakeFs` `remove_file`, `remove_dir`, `set_readonly`, `set_modified`, `rename` and `create_dir` resolve ancestor links via `resolve_ancestors`. `FakeFs::remove_file` rejects directory nodes.
  - Tests in `fs_ops/worker.rs` and `fs_ops/fs.rs`, plus a Windows-only `RealFs` junction test in `crates/filecommand-core/tests/realfs.rs`.
  - No `filecommand-tui` changes.
- **Compatibility:** the only observable changes are that a cross-volume move no longer deletes through links, and that Delete of a junction on Windows now succeeds instead of raising "Access is denied."

**Audit findings:** R-01

**Coordination:**
- `durable-file-copy` edits the same `run_copy_or_move` copy/move loop and `fs.rs` `RealFs`/`FakeFs`, and creates the shared `RealFs` integration-test harness (`crates/filecommand-core/tests/realfs.rs`). Its source-removal error surfacing applies to removable regular files; this change's link removal follows the same "report, don't ignore" rule. Whichever change lands second rebases onto the other. If this change lands first, it adds its own minimal harness (temp-dir helper, `mklink /J` helper, cross-volume `RealFs` wrapper) in that file, and `durable-file-copy` extends it.
- `copy-onto-itself-guard` adds a same-file skip to the same `run_copy_or_move` file loop and may create `realfs.rs` first. A same-file skip beneath a link counts as "not copied", so the link is kept (D2). Whichever change lands second rebases.
- `link-aware-listing` adds an internal link flag to panel `Entry` values. It is independent: `fs_ops` classifies reparse points from its own metadata and does not read the panel flag.
