## Context

`run_delete_builtin` (`crates/filecommand-core/src/update.rs`) determines the target's file-vs-directory type in two ways: a bare name that matches a currently-listed entry answers from the loaded `EntryKind`, and anything else gets a filesystem check. The panel's listing always contains a `..` row (`EntryKind::ParentDir`, and `is_dir_like()` is true for it), so:

- `del ..` is rejected today — but only incidentally, by the type check (`ParentDir` reads as a directory). The rejection message (".. is a directory") misdescribes the real problem.
- `rmdir ..` sails through: listed ⇒ typed-as-directory ⇒ delete-confirmation dialog opens naming the parent directory. Accepting it (plus the second confirmation) dispatches a real delete job against the parent.
- `rmdir .` resolves the target to the panel's own cwd and offers to delete the directory being browsed.

F8 and the file-action menu's Delete entry never reach this state: their scope comes from the listed entries/selection, and the `..` row is never a deletable source through those surfaces.

## Goals / Non-Goals

**Goals:**
- `.` and `..` (with or without a trailing separator) are rejected by `del`/`rmdir` up front, with an error that names the invalid target — no delete-confirmation dialog ever opens on the current or parent directory.
- The guard sits before target resolution so no filesystem I/O is spent on these inputs.

**Non-Goals:**
- Forbidding deleting a directory merely because a panel is displaying it (a real subdirectory named explicitly remains deletable, as today).
- Changing F8 / file-action-menu Delete scoping (they already cannot target `.`/`..`).
- Rejecting other pathological-but-explicit targets (e.g. `rmdir C:\` still routes to the dialog and fails inside the job machinery, as before).

## Decisions

- **Reject via `Path::components`, not string equality.** `Path::new("..\\").components()` yields just `ParentDir` (trailing separators are normalized), so one components check covers `..`, `..\`, `.`, and `.\` — string comparison would miss the separator variants. A multi-component path like `..\sibling` is unaffected (its first component is not the whole target).
- **Reject before the listed-entry lookup and the filesystem check.** Both degenerate names are pure syntax; spending an `is_dir()` stat or consulting the listing for them is wasted work and muddies which rule fired.
- **Message shape: `"{verb}: invalid target {target}"`** — the same wording `run_delete_builtin` already uses for the unresolvable-target case, keeping the verb's errors self-consistent.

## Risks / Trade-offs

- [Risk] A user who genuinely wants to delete an empty parent directory must type its real name (or use F8 on a listed entry elsewhere) — marginally less direct than `rmdir ..`, in exchange for not offering a one-typo directory deletion. Accepted: no other surface in the app offers `.`/`..` deletion, and cmd's own `rmdir ..` fails more often than it succeeds.
