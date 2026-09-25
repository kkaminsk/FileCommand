## Why

The `del`/`rmdir` builtins added by `command-line-builtin-whitelist` resolve their typed target against the active panel's directory and route it into the delete-confirmation dialog. Two degenerate targets slip through that resolution and produce dangerous dialogs:

- `rmdir ..` — `..` matches the panel's own parent-dir listing entry (`EntryKind::ParentDir`, `is_dir_like()`), so the verb passes every check and opens the delete-confirmation dialog **on the panel's parent directory**, offering (after the existing second confirmation) to delete the folder the user is browsing from.
- `rmdir .` — resolves to the panel's current directory, offering to delete the directory the panel itself is showing.

Both go through the confirmation dialog, so nothing deletes instantly — but a dialog offering to delete the user's parent or current directory is a footgun no other surface in the app exposes (F8 and the file-action menu's Delete entry scope to listed entries/selections and never propose `.`/`..`).

## What Changes

- `del` and `rmdir` reject `.` and `..` targets (including a trailing separator, e.g. `..\`): the panel's error state is set, no dialog opens.
- All other target handling is unchanged — a type mismatch still rejects, a valid type-matched target still opens the same F8 delete-confirmation dialog.

## Capabilities

### New Capabilities
(none)

### Modified Capabilities
- `command-line`: narrows "del and rmdir route into the existing delete-confirmation flow" so `.` and `..` are never valid targets, with rejection scenarios.

## Impact

- `crates/filecommand-core/src/update.rs`: `run_delete_builtin` gains one early guard rejecting `.`/`..` targets before resolution; everything after is untouched.
- New unit tests cover both verbs against `.` and `..` (no dialog, error set, no deletion).
