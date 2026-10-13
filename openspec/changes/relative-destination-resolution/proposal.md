# Change: relative-destination-resolution

## Why

Audit finding **R-06 (High)**: a relative destination typed into the F5/F6 dialog resolves against the directory FileCommand was *launched* from, not the panel the user is working in. `confirm_destination_input` builds the job with `dest_dir: PathBuf::from(trimmed)` and never joins it to the source panel's directory. `fs_ops::path::to_extended_path` leaves a relative path relative, so Windows resolves it against the process working directory. FileCommand never changes that directory, so it stays at the launch directory for the whole session.

Typing `backup` or `..\archive` in the F6 dialog therefore moves the selection somewhere the user never intended, typically into the directory the app was started from. This also drifts from filesystem-error-handling "Canonicalization precedes prefixing," which requires the path abstraction to work on absolute paths.

The same dialog is reached from the file-action menu's Copy/Move and from a mouse drop, whose field the user can edit, so all of these share the bug. The F7 make-directory name and the in-place Rename name are joined onto the panel directory, so plain and `..\` names already land correctly. Drive-relative forms (`D:x`) and a bare `D:` still escape as relative paths there.

## What Changes

- The F5/F6 destination resolves against the **source panel's directory**, using the same rules `cd` uses (`resolve_cd_target`). This covers the keyboard dialog, the file-action menu's Copy/Move and the drop-initiated dialog:
  - A relative path is joined to the source directory: `backup` becomes `<source>\backup`.
  - `..` and `.` components are resolved: `..\archive` becomes `<source's parent>\archive`.
  - A bare drive letter means that drive's root: `D:` becomes `D:\`.
  - A drive-rooted path takes the source's drive: `\x` becomes `C:\x`.
  - UNC and absolute paths are used unchanged.
- `resolve_cd_target` now always returns an absolute, lexically normalized path. Two inputs change:
  - A drive-relative form `X:rest` resolves against the source directory when that directory is on drive `X`, and against `X:\` otherwise. It is no longer handed to the OS as a relative path.
  - Every result is lexically normalized, so it contains no `.`/`..` components.

  The second point also fixes `cd ..\x`, which today sets the panel to an unnormalized path whose `\\?\`-prefixed listing fails with OS error 123 (verified).
- The F7 make-directory name and the in-place Rename name go through the same resolver. Plain names behave exactly as today. Drive-relative names stop escaping, and a path naming a different directory refreshes that directory's panel when the job completes.
- A destination that resolves to nothing (`..` at a drive root) keeps the dialog open unchanged, as an empty field does today.
- `RealFs` debug-asserts that every path it receives is absolute, so a future relative-path leak fails loudly in debug builds and tests instead of acting on the launch directory.
- Non-breaking. An absolute or UNC destination, the common pre-filled case, is unchanged.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `operation-dialogs`: new requirement "Relative destinations resolve against the source panel directory" for the F5/F6 destination field (keyboard, menu and drop-initiated) and the F7 make-directory name. It is ADDED rather than modifying "Destination input dialog," which `line-editing-in-text-fields` already modifies.
- `command-line`: new requirement "cd, del and rmdir targets resolve to a normalized absolute path", which pins the shared resolver's new contract for the command-line builtins. It is ADDED rather than modifying "cd navigates the active panel or rejects a nonexistent target" and "del and rmdir route into the existing delete-confirmation flow", which `offload-blocking-io` already modifies.

## Impact

- **Specs:** `operation-dialogs` (one added requirement), `command-line` (one added requirement).
- **Code:**
  - `crates/filecommand-core/src/update.rs`:
    - `confirm_destination_input` (lines 2918-2943) resolves `trimmed` against `source_dir` for Copy/Move and Mkdir.
    - The `FileOpSetup::RenameInput` confirm arm (lines 2986-3018) does the same for Rename.
    - `resolve_cd_target` (lines 1832-1851) adds drive-relative handling and lexical normalization through `fs_ops::path::lexically_normalize`.
  - `crates/filecommand-core/src/fs_ops/fs.rs`: a `debug_assert!` on path absoluteness at the `RealFs` entry points, in both the Windows and portable implementations.
  - `crates/filecommand-core/src/fs_ops/job.rs`: doc comment on `Job::dest_dir` (Mkdir/Rename now carry the resolved parent).
  - Tests in `crates/filecommand-core/src/update/tests.rs`.
  - No `filecommand-tui` changes.
- **Compatibility:** relative, drive-relative and `..`-containing destinations now go where the dialog's panel context says. `cd` with a `..`-containing or drive-relative path now lands on a normalized absolute directory.

**Audit findings:** R-06

**Coordination:**
- `offload-blocking-io` (R-09) moves `dispatch_cd`'s and `dispatch_delete_builtin`'s existence probe off the UI thread and MODIFIES the command-line `cd` and `del`/`rmdir` requirements, keeping their "supported forms, exactly as today" wording. This change lands **first**: it is a "do now" data-safety fix, and it only changes the path `resolve_cd_target` hands to the probe, not the probe. Its ADDED command-line requirement defines how those same supported forms resolve, so the two deltas do not contradict each other. `offload-blocking-io` then rebases a small textual overlap in `update.rs` and probes the already-normalized path.
- `copy-onto-itself-guard` independently guarantees that a destination resolving to the source directory (for example `.`) never overwrites a file with itself. This change makes `.` mean the source directory but does not add that guard. That change's path fallback relies on the absolute destinations this change produces.
- `reducer-overlay-structure` (B-02) may move `confirm_destination_input` and the setup handlers into a sub-module. That is a mechanical rebase.
