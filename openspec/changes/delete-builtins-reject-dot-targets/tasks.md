## 1. Core: reject `.`/`..` delete targets

- [x] 1.1 In `crates/filecommand-core/src/update.rs`, add an early guard at the top of `run_delete_builtin`: a target whose `Path::components()` is exactly `CurDir` or `ParentDir` sets `last_error` to `"{verb}: invalid target {target}"` and returns — before any listed-entry lookup, filesystem check, or dialog.
- [x] 1.2 The guard must cover trailing-separator spellings (`..`, `..\`, `.`, `.\`) via the components normalization, and must not reject multi-component relative paths like `..\sibling`.

## 2. Tests

- [x] 2.1 Both verbs × both targets: `del .`, `del ..`, `rmdir .`, `rmdir ..` (plus a `..\` variant) leave the phase at Panels, open no dialog, emit no effects, and set the invalid-target error.
- [x] 2.2 Regression guard: `rmdir docs` on a listed directory still opens the delete-confirmation dialog, and `del notes.txt` on a listed file still opens it (the guard must not widen).
- [x] 2.3 A multi-component relative target (`del ..\sibling.txt`) is not caught by the guard (it follows the ordinary not-listed/filesystem-check path).

## 3. Verification

- [x] 3.1 Run `cargo test -p filecommand-core` and `cargo test -p filecommand-tui`.
- [x] 3.2 `openspec validate --all`.
- [x] 3.3 `detect_changes()` (GitNexus) against `main` to confirm only the expected symbols/flows are touched before opening the PR.
