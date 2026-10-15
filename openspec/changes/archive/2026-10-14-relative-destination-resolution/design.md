# Design: relative-destination-resolution

## Context

All of the following was verified on `build/open-proposals-2026-10` @ `1bada6f`.

**Every destination prompt funnels through one function.** F5/F6 (`enter_file_op_setup`), the file-action menu's Copy/Move, and a mouse drop (`Command::DragDrop` → `enter_file_op_setup_for_sources` with `buttons: Some(..)`) all open the same `FileOpSetup::DestinationInput { kind, sources, source_dir, input, buttons }`:

- `source_dir` is the source panel's `cwd`. For a drop it is the panel the drag started on, which may be the inactive one.
- Enter (`FileOpConfirm`) and the drop dialog's `[ Copy ]`/`[ Move ]` (`FileOpConfirmAs`) both call `confirm_destination_input`.
- For Copy/Move that function builds `Job { dest_dir: PathBuf::from(trimmed), .. }`, with no join against `source_dir`.
- F7 opens the same setup with `kind: Mkdir` and builds `Job { dest_dir: source_dir, new_dir_name: Some(trimmed) }`, which `run_mkdir` joins as `dest_dir.join(name)`.
- The file-action menu's in-place Rename uses a separate `FileOpSetup::RenameInput` arm that builds `Job { dest_dir: source_dir, new_dir_name: Some(trimmed) }`, joined the same way by `run_rename`.

**What a relative path does today.**

- **The destination path.** `RealFs` passes every path through `fs_ops::path::to_extended_path`. That function lexically normalizes the path, prefixes `\\?\` only when the result `is_absolute()`, and otherwise returns it relative. Windows then resolves a relative path against the process's current directory. FileCommand never calls `set_current_dir`, so that directory is the launch directory for the whole session, and `backup` lands in `<launch dir>\backup`.
- **F7 and Rename.** These are mostly right, because `Path::join` handles relative names, `\x` (rooted: keeps the source's drive), absolute paths and UNC paths, and `to_extended_path` normalizes a joined `..`. Two forms still escape: a drive-relative `D:x` (`join` yields the relative `D:x`, resolved against the process's per-drive directory) and a bare `D:`.
- **The refresh after a job.** `FileOpRunning` keeps `dest_dir` verbatim, and `JobDone` refreshes panels by exact `PathBuf` equality (`panels_matching`). An unnormalized `C:\work\..\archive` therefore never matches a panel showing `C:\archive`.

**The `cd` resolver.** `resolve_cd_target(cwd, target)` handles these cases:

| Input | Result |
|---|---|
| `..` | `parent_path(cwd)`, or `None` at a root |
| `.` | `cwd` |
| UNC or absolute | as-is |
| Bare `X:` | `X:\` |
| Anything else | `cwd.join(target)` |

It never normalizes, so `cd ..\x` sets the panel's `cwd` to `C:\work\..\x`. The listing's `to_long_path` then prefixes that as `\\?\C:\work\..\x`, and a probe confirmed that a verbatim path with a `..` component fails with OS error 123 ("The filename, directory name, or volume label syntax is incorrect"). The existence probe passes first, because it uses the non-verbatim path. A drive-relative `cd D:x` likewise yields the relative `D:x`.

**The absolute-path invariant.** filesystem-error-handling "Canonicalization precedes prefixing" requires the abstraction to produce an absolute, normalized path before prefixing. Nothing enforces that the input is absolute. `to_extended_path` has callers outside `fs_ops`, though: `shell::spawn_safe_path` passes configured program names, which may legitimately be relative (`cmd.exe`), once they reach 240 characters, and `editor.rs` passes the panel path joined with the file name.

## Goals / Non-Goals

**Goals:**

- A destination typed or edited in the F5/F6 dialog, reached from the keyboard, the file-action menu or a drop, is interpreted relative to the source panel's directory, with the same forms `cd` accepts.
- The resolved destination is absolute and normalized, so the job acts on the intended directory and the completion re-read finds the panel that shows it.
- F7 and in-place Rename names cannot escape to the process's working directory either.
- Relative paths reaching `RealFs` fail loudly in debug builds.

**Non-Goals:**

- Stripping surrounding quotes from a pasted destination (`"C:\x y"`, as Explorer's "Copy as path" produces). Today's literal handling is kept. A quoted path still fails at the filesystem with an error dialog, as it does now.
- Interpreting a destination naming a *file* as "copy under a new name" (NC's single-file rename-on-copy). The destination keeps meaning a directory.
- Case-insensitive panel matching for the completion re-read (`d:\Backup` vs `D:\backup`).
- Changing the `cd` existence probe or moving it off the UI thread (`offload-blocking-io`).
- Guarding against a destination that resolves to the source directory (`.`). `copy-onto-itself-guard` makes that a skipped "same file" item.

## Decisions

### D1: Resolve in the reducer, with `resolve_cd_target`, against the source panel's directory

`confirm_destination_input` replaces `PathBuf::from(trimmed)` with `resolve_cd_target(&source_dir, trimmed)`:

- `None` (only `..` at a drive or UNC root) returns the same `DestinationInput` phase unchanged, exactly like the existing empty-field branch.
- `Some(path)` becomes the job's `dest_dir` and the `FileOpRunning.dest_dir` used for the completion refresh.

The resolution belongs in the reducer for two reasons. `source_dir` is already in hand there, and the pure function can be unit-tested without I/O. The audit's suggested `to_extended_path` join, by contrast, has no notion of the panel. Reusing `cd`'s resolver gives one set of rules users already know from the command line, and keeps `D:` meaning "that drive's root."

*Alternative considered:* resolve in the worker, against `job.source_dir`. Rejected because the refresh matching in the reducer would still see the unresolved string.
*Alternative considered:* reject relative input with an error. Rejected: NC accepts relative destinations, and the field is a free-form path.

### D2: `resolve_cd_target` always returns an absolute, normalized path

Two additions make the helper's contract "absolute and normalized whenever `cwd` is":

- **Drive-relative input (Windows only).** A target matching `X:` followed by a non-separator, such as `D:x`, resolves to `cwd.join(rest)` when `cwd` is on drive `X` (compared case-insensitively). Otherwise it resolves to `drive_root(X).join(rest)`. FileCommand has no per-drive current directory, and the source panel is the closest equivalent: when it is on that drive, it acts as the drive's current directory. On other platforms `D:x` is an ordinary file name and stays relative to `cwd`.
- **Normalization.** Every `Some` result is passed through `fs_ops::path::lexically_normalize`, whether it was joined or typed absolute (`C:\a\..\b` → `C:\b`). An already-clean absolute or UNC path comes out unchanged, and `..\archive` from `C:\work\proj` yields `C:\work\archive`. On Windows this matches what Win32 path normalization would do to the non-verbatim path. Lexical `..` on Unix can differ from the kernel's physical resolution through a symlink, an accepted difference on a best-effort platform.

These also change `cd`/`del`/`rmdir`, which share the helper:

- `cd ..\x` now lands on `C:\x` and lists successfully, instead of failing with error 123.
- `cd D:x`, `del D:x` and `rmdir D:x` act on a well-defined absolute path, instead of whatever the process's per-drive directory happens to be.

The command-line spec's "resolve ... exactly as today" lists `.`, `..`, bare drive letters, UNC and relative paths, and those keep their meaning. `offload-blocking-io` keeps that wording in its MODIFIED `cd` and `del`/`rmdir` requirements. Neither requirement can be modified here as well, so this change ADDs a command-line requirement, "cd, del and rmdir targets resolve to a normalized absolute path". It states the two refinements, `..` removal and drive-relative resolution, without adding or removing a supported form, so it reads consistently with both the current and the `offload-blocking-io` wording. `resolve_cd_target`'s doc comment is updated to state the new contract.

*Alternative considered:* a separate `resolve_destination` wrapper that leaves `cd` untouched. Rejected because `cd ..\x` is broken by the same missing normalization, and D4's debug assertion needs every path source, `del`/`rmdir` included, to produce absolute paths. Two resolvers with subtly different rules would also be worse than one.

### D3: F7 and in-place Rename use the same resolver

For Mkdir and Rename, the confirm handler computes `target = resolve_cd_target(&source_dir, trimmed)` and builds `Job { dest_dir: target.parent(), new_dir_name: target.file_name(), .. }`:

- A plain name resolves to `source_dir\name`, giving exactly today's job.
- A path names its real parent directory. That makes `FileOpRunning.dest_dir` accurate, so a panel showing that parent re-reads on completion, which it does not today.
- A result with no file name (a drive or UNC root, or `None`) keeps the dialog open, like an empty field.

The `Job` doc comment changes from "Rename sets `dest_dir` equal to `source_dir`" to "the resolved parent of the new name, which is `source_dir` for a plain name."

The spec delta covers F7, because it is the same destination dialog. Rename's behavior for every name the file-action-menu spec describes is unchanged. Rename is routed through the resolver only so D4's invariant holds, and no file-action-menu delta is needed. All of file-action-menu's requirements are in flight elsewhere.

### D4: `RealFs` debug-asserts absolute paths at its own boundary, not inside `to_extended_path`

Every `RealFs` method, in both the Windows and the portable implementation, obtains its OS path through a private helper:

```rust
fn ext(p: &Path) -> PathBuf {
    debug_assert!(p.is_absolute(), "fs_ops received a relative path: {p:?}");
    to_extended_path(p)
}
```

On Windows, `is_absolute()` is true for drive-absolute, UNC and `\\?\` paths, and false for `\x` and `D:x`, both of which depend on process state.

The assertion is not placed in `to_extended_path` itself, because `shell::spawn_safe_path` legitimately routes relative program names such as `cmd.exe` through it once they reach the 240-character threshold. The existing `extended_path_leaves_relative_alone` test keeps documenting that pass-through. The assertion costs nothing in release builds and turns any future relative-path leak into a test failure.

## Risks / Trade-offs

- [Behavior change for `cd`/`del`/`rmdir` with drive-relative or `..` input] → Both were unreliable before: they depended on process state, or produced an unlistable `\\?\…\..\…` path. The new results are what a user would expect. Unit tests pin them.
- [The debug assertion could fire in debug builds if a panel `cwd` were ever relative] One case: `app.rs` falls back to `PathBuf::from(".")` when `std::env::current_dir()` fails at startup. → This only matters in debug builds. A relative panel path already misbehaves in release builds, and the assertion makes it visible rather than hiding it.
- [Lexical normalization through a link] `C:\link\..\x` normalizes to `C:\x`, not to the link target's parent. → This is identical to Win32's own normalization of non-verbatim paths, which is what users already get from Explorer and `cmd`.

## Open Questions

None.
