# Change: copy-onto-itself-guard

## Why

**Copying a file onto itself truncates it to zero bytes (Critical, data loss).** When both panels show the same directory, pressing F5 and accepting the pre-filled destination copies every selected file onto itself. `run_copy_or_move` sees that the target has the same file identity as the source and treats it as "no conflict". That rule exists so that case-only *renames* are not reported as overwrites. The worker then calls `copy_file_chunked(src, src)`. Its `File::open(from)` is followed by `File::create(to)` on the same path, and the create truncates the file before the first byte is read. A compiled probe of that exact sequence on a 2,048-byte file returned `Ok(0)` and left the file at 0 bytes. The job reports success, and no dialog appears. A directory copied into its own parent loses the content of every file in it the same way.

When a file identity cannot be read, the result is worse. The worker then raises an overwrite conflict for the file against itself, and choosing Overwrite truncates it the same way. `RealFs::same_volume` also answers `false` in that case, so a same-directory F6 Move takes the copy-then-delete path: the file is truncated by the self-copy and then deleted as the move's source.

The code audit does not list this defect. It was found while `durable-file-copy` (R-07, R-08) was being authored. It is tracked separately because it is a one-check fix that should not wait for the larger copy-engine rework.

## What Changes

- **A copy never writes to its own source.** Before copying a planned file, the copy/move worker checks whether the target resolves to the same file as the source. The target counts as the same file when its file identity equals the source's. When either identity cannot be read, it counts as the same file when the two normalized absolute paths are equal, compared case-insensitively on Windows.
- **Same-file items are skipped and reported.** Such an item is skipped without writing anything and without raising the overwrite-conflict dialog. It is listed in the end-of-job skipped-files summary with the reason "source and destination are the same file". A move performed as copy-then-delete also skips it and never deletes the source.
- **Regression tests:** `FakeFs` worker tests for a same-directory copy, a destination reached through a junction alias, a copy-then-delete move onto the same file, and an unknown identity; a unit test for the path fallback; and a `RealFs` integration test that copies a file into its own directory on a real temporary directory.
- Non-breaking. The case-only rename rule in `run_move_same_volume` and `run_rename` is unchanged.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `file-operations`: new requirement "A copy never overwrites its own source."

## Impact

- **Specs:** `file-operations` (one added requirement).
- **Code:**
  - `crates/filecommand-core/src/fs_ops/worker.rs`:
    - `run_copy_or_move` (lines 363-427): a same-file check before the conflict check, using a new helper next to `identity_matches`. The helper compares the followed identities of source and target, and falls back to `path::lexically_normalize` plus a case-insensitive comparison on Windows when an identity is unknown.
    - New tests in the `#[cfg(test)]` module.
  - `crates/filecommand-core/tests/realfs.rs`: a same-directory copy test. This change creates the file with a minimal harness if `durable-file-copy` and `move-reparse-point-safety` have not created it yet.
  - No `filecommand-tui` changes. The existing skipped-files summary renders the new reason.
- **Dependencies:** none.

**Audit findings:** none — discovered while authoring durable-file-copy (same-identity copy path, related to R-08)

**Coordination:**
- `durable-file-copy` rewrites `copy_one_file` around a temporary partial file and renames over the target, and edits the same `run_copy_or_move` loop. This change should land first because it is the smaller fix for a Critical data loss. `durable-file-copy` then keeps this check ahead of choosing the temporary file. If `durable-file-copy` lands first, this change rebases onto its loop unchanged. Both changes share `crates/filecommand-core/tests/realfs.rs`, and whichever lands first creates it.
- `move-reparse-point-safety` edits the same `run_copy_or_move` loop and also adds tests to `realfs.rs`. A same-file skip beneath a directory link counts as "not copied", so that change keeps the link in place. Whichever change lands second rebases.
- `relative-destination-resolution` (R-06) makes the job's destination absolute. Until it lands, a relative destination still reaches the worker, and the identity comparison catches the same-file case on its own. The path fallback relies on absolute paths.
- `error-diagnostics` (B-06) MODIFIES operation-dialogs "End-of-job skipped-files summary" so that its list of skip sources reads "for example". The same-file skip is one more skip recorded by the engine and fits that wording. There is no ordering constraint.
