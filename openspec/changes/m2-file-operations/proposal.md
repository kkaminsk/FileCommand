# Change: m2-file-operations

## Why

M1 delivers a navigable dual-panel shell but no way to act on files. M2 makes FileCommand actually useful for everyday work by adding the core Norton Commander operations — selection and F5/F6/F7/F8 (Copy, Rename/Move, Make directory, Delete) — with the safe, cancellable, error-recoverable semantics the spec demands (§4.4, §7, §9 M2). Doing this now, on top of M1's rendering and event loop, unblocks a demoable "manage files entirely from the keyboard" milestone before command-line, menus, and viewer work land.

## What Changes

- Add multi-entry **selection**: `Ins` toggles the current entry and advances the cursor; `+` / `-` open a wildcard dialog to select / deselect a group; `*` inverts the selection; the mini-status shows `N files selected, X bytes` (directories contribute 0 bytes); selection is preserved across in-panel navigation.
- Add the four **file operations** — Copy (F5), Rename/Move (F6), Make directory (F7), Delete (F8) — implemented as cancellable jobs on a worker thread that emit progress and error events consumed by the UI event loop.
- Add the **operation dialogs**: destination input (pre-filled with the opposite panel path), overwrite-conflict resolution (Overwrite/Skip/Rename/Overwrite All/Skip All), progress (block byte gauge + Cancel), per-file error recovery (Retry/Skip/Skip All/Abort), delete confirmation (with non-empty-directory second confirm and permanent-delete warning), and an end-of-job skipped-files summary.
- Add correct Windows **file-system semantics**: same-volume move as an instant rename vs cross-volume copy-then-delete (delete only after a verified copy); identity-aware case-only rename; ADS/attribute/timestamp preservation on copy; reparse-point (symlink/junction) handling with recursion-cycle protection; `\\?\` long-path abstraction; read-only-attribute handling; and a narrow injectable fs trait seam for deterministic failure testing plus an inline panel read-error state.
- Re-read the affected panel(s) automatically when an operation completes.

## Capabilities

### New Capabilities

- `selection`: `Ins` toggles the current entry and advances the cursor, `+`/`-` group-select/deselect via a wildcard dialog, `*` inverts, the `N files selected, X bytes` mini-status (directories count 0 bytes), and selection preserved across in-panel navigation.
- `file-operations`: Copy/move/delete/mkdir as cancellable worker-thread jobs emitting progress events, with same-volume rename vs cross-volume copy-then-delete, identity-aware case-only rename, ADS/attribute/timestamp preservation, reparse-point handling with cycle protection, and automatic panel re-read on completion.
- `operation-dialogs`: The destination input dialog, overwrite-conflict dialog, progress dialog with block byte gauge and Cancel, Retry/Skip/Skip All/Abort error dialog, delete confirmation with second confirm and permanent-delete warning, and the end-of-job skipped-files summary.
- `filesystem-error-handling`: The narrow injectable fs trait seam for deterministic failure injection, the `\\?\` long-path abstraction, read-only-attribute handling, and the inline panel read-error state offering re-read or drive change without crashing.

### Modified Capabilities

- None (greenfield project; no existing specs).

## Impact

- **Crates:** `filecommand-core` — new `fs_ops` module (jobs, worker thread, progress/error events, conflict/error state machines, `\\?\` abstraction, fs trait seam) and additions to `panel` (selection set, mini-status totals, read-error state, auto re-read). `filecommand-tui` — new dialog views (input, confirm, overwrite, progress, error, summary) and input routing for `Ins`/`+`/`-`/`*` and F5–F8, plus consuming job events in the event loop.
- **Dependencies:** worker-thread job/event plumbing (std threads + channels); Windows fs APIs behind the internal trait; `insta` + ratatui `TestBackend` for dialog snapshots; the injected `Clock` trait for pinnable timestamps in conflict/mini-status rendering. No new heavyweight crates required for this milestone.
- **Depends on:** M1 (event loop, panel state, theme/rendering, `Clock` seam). Does not touch command line, menus, viewer, or editor.
