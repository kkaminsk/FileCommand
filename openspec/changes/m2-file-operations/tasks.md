# Tasks: m2-file-operations

## 1. Module scaffolding and shared types

- [ ] 1.1 Create the `fs_ops` module in `filecommand-core` (submodules: `path` for the `\\?\` abstraction, `fs` for the trait seam, `job` for job/event types, `conflict` and `error` for the resolution state machines, `worker` for the worker thread).
- [ ] 1.2 Define the `Job` type (kind: Copy/Move/Delete/Mkdir, source list keyed by original `OsString`, destination, resolved options) and the worker→UI event enum (`Progress { current_file, bytes_done, bytes_total, files_done, files_total }`, `Conflict{..}`, `Error{..}`, `Done{..}`).
- [ ] 1.3 Define the request→reply channel types for pausing the worker on conflict/error/cancel decisions, plus a shared cancel flag observed at file boundaries and between chunk copies.
- [ ] 1.4 Add the core dialog-state enum consumed by the TUI (destination input, overwrite conflict, progress, error recovery, delete confirm, skipped-files summary) so rendering stays a pure function of core state.

## 2. Filesystem trait seam and long-path abstraction (`filecommand-core`)

- [ ] 2.1 Define the narrow internal fs trait: metadata/identity query (volume + file index), read-dir, create-dir, copy-file, rename, remove-file, remove-dir, set-attributes, reparse-point inspection. (filesystem-error-handling: "Narrow internal fs trait seam")
- [ ] 2.2 Implement the real Windows-backed fs implementation of the trait; ensure no `fs_ops` code path calls `std::fs`/platform APIs outside the trait. (filesystem-error-handling: "All fs access goes through the seam")
- [ ] 2.3 Implement the fake fs used by unit tests with deterministic injection of at least permission-denied, sharing-violation, and disk-full at chosen operations, and typed, path-attributed error classes. (filesystem-error-handling: "Injected permission-denied…", "Injected sharing-violation and disk-full are distinguishable")
- [ ] 2.4 Implement the `\\?\` path abstraction: full canonicalization (resolve `.`/`..`, forward slashes → backslashes, remove relative components) before applying the `\\?\` prefix, and `\\?\UNC\` for UNC paths; centralize so no caller hand-builds prefixed paths. (filesystem-error-handling: "`\\?\` long-path abstraction centralizes prefixing"; file-operations: "Long-path correctness")
- [ ] 2.5 Route every fs-trait call through the path abstraction so operations on > 260-char paths succeed without the registry opt-in. (file-operations: "Operation on a path longer than the legacy limit succeeds")

## 3. Selection state in `panel` (`filecommand-core`)

- [ ] 3.1 Add the per-panel selection set keyed by entry identity (original `OsString`), never by row index. (selection: "Selection persists across in-panel navigation and re-sort")
- [ ] 3.2 Implement `Ins` toggle-and-advance, with the parent-directory pseudo-entry (`▶UP--DIR◀`/`..`) never selectable and no cursor wrap on the last entry. (selection: "Toggle current entry with Ins")
- [ ] 3.3 Implement wildcard group-select (`+`) and group-deselect (`-`) matching against the original stored `OsString` name, additive/subtractive, excluding the parent entry; reuse the listing wildcard machinery. (selection: "Select a group by wildcard with `+`", "Deselect a group by wildcard with `-`")
- [ ] 3.4 Implement invert-selection (`*`) over selectable entries, leaving the parent entry unselected. (selection: "Invert selection with `*`")
- [ ] 3.5 Compute the `N files selected, X bytes` mini-status summing file bytes only (selected directories contribute 0 bytes), reverting to per-entry name/size/date/time when the selection is empty. (selection: "Selection mini-status summary")
- [ ] 3.6 Preserve selection across cursor movement, re-sort, and scroll; clear selection when the panel changes directory. (selection: "Selection persists across in-panel navigation and re-sort")

## 4. Worker-thread job engine and progress (`filecommand-core`)

- [ ] 4.1 Implement the worker thread: accept a `Job`, walk the tree, perform the operation, and emit progress/conflict/error/done events over the channel drained by the event loop and folded through `core::update`. (file-operations: "Cancellable file-operation jobs with progress events")
- [ ] 4.2 Compute `files_total`/`bytes_total` with selected directories contributing 0 bytes while their contained files count when the job recurses in; accumulate `files_done`/`bytes_done` across the job. (file-operations: "Progress totals accumulate…", "Selected directory adds no bytes to the total")
- [ ] 4.3 Observe the cancel signal at every file boundary and between chunk copies of a large file; emit a terminal cancelled `Done` event rather than completing remaining work. (file-operations: "Cancel is honored mid-job")

## 5. Windows filesystem semantics in `fs_ops` (`filecommand-core`)

- [ ] 5.1 Implement F5 Copy recursing into directories, preserving alternate NTFS data streams, attributes, and timestamps of the source. (file-operations: "Copy preserves alternate data streams, attributes, and timestamps")
- [ ] 5.2 Implement F6 Move: same-volume as a single instant `rename` (no byte copy); cross-volume as copy-then-delete, deleting the source only after the copy is verified; leave the source intact on a failed/unverified copy. (file-operations: "Same-volume move is a rename; cross-volume move is copy-then-verified-delete")
- [ ] 5.3 Implement the identity-aware target-exists check (volume + file index, not name string) so case-only renames (`foo` → `Foo`) succeed and distinct existing targets still raise an overwrite conflict. (file-operations: "Identity-aware case-only rename")
- [ ] 5.4 Implement read-only attribute detection and clearing (via the trait) before overwrite and before delete so the attribute alone never fails the operation. (file-operations: "Copy preserves… / Read-only target is cleared before overwrite"; filesystem-error-handling: "Read-only attribute handling before overwrite and delete")
- [ ] 5.5 Implement reparse-point (symlink/junction) semantics: delete removes the link not the target contents; copy duplicates the target's content by default; recursion-cycle protection via a visited file-identity set that never traverses a junction pointing inside the source tree. (file-operations: "Reparse-point (symlink/junction) semantics")
- [ ] 5.6 Implement F7 Make directory creating the named directory at the destination. (file-operations: "Same-volume move is a rename… / Make-directory creates the named directory")

## 6. Conflict and error resolution state machines (`filecommand-core`)

- [ ] 6.1 Implement the overwrite-conflict state machine (Overwrite/Skip/Rename/Overwrite All/Skip All) with "…All" choices latching a policy that auto-resolves subsequent conflicts without re-prompting; carry source and target size/date for display. (operation-dialogs: "Overwrite-conflict dialog")
- [ ] 6.2 Implement the per-file error-recovery state machine (Retry/Skip/Skip All/Abort) pausing the job until the user chooses; Retry re-attempts the same file, Skip All latches for same-class errors, Abort stops the job. (operation-dialogs: "Error-recovery dialog")
- [ ] 6.3 Accumulate skipped items (from Skip, Skip All, and latched conflict/error policies) into a list surfaced at end of job. (operation-dialogs: "End-of-job skipped-files summary")

## 7. Panel read-error state and auto re-read (`filecommand-core`)

- [ ] 7.1 Implement the inline panel read-error state: a listing failure (access denied, drive removed) enters an error state instead of panicking/exiting, offering re-read and drive-change actions, with successful re-read replacing it with the normal listing. (filesystem-error-handling: "Inline panel read-error state offering re-read or drive change")
- [ ] 7.2 Implement automatic re-read of the affected source/destination panel(s) on job completion — including cancellation after partial changes — reusing the streaming listing path, and reconcile the selection set against the fresh listing (vanished entries drop out). (file-operations: "Automatic panel re-read on completion")

## 8. TUI dialog views (`filecommand-tui`)

- [ ] 8.1 Render the destination input dialog (primary black-on-cyan, double-line frame, bracket-and-dots input field) pre-filled with the opposite panel's path, cursor positioned for edit; Enter starts the job, Esc aborts. (operation-dialogs: "Destination input dialog")
- [ ] 8.2 Render the overwrite-conflict dialog showing source vs target size/date with timestamps formatted through the injected `Clock`/formatting path, offering Overwrite/Skip/Rename/Overwrite All/Skip All. (operation-dialogs: "Overwrite-conflict dialog")
- [ ] 8.3 Render the progress dialog: file counts, current file path, and the byte gauge drawn with `█` (`dialog.gauge.filled`) and `░` (`dialog.gauge.empty`) per §4.11, plus a Cancel control that signals cancellation. (operation-dialogs: "Progress dialog with byte gauge and Cancel")
- [ ] 8.4 Render the error-recovery dialog in the bright-white-on-red error style offering Retry/Skip/Skip All/Abort. (operation-dialogs: "Error-recovery dialog")
- [ ] 8.5 Render the delete confirmation dialog: name the single targeted item or show the count for a multi-selection, warn that deletion is permanent (no recycle bin), and require a second confirmation for a non-empty directory. (operation-dialogs: "Delete confirmation dialog")
- [ ] 8.6 Render the end-of-job skipped-files summary dialog listing skipped items, shown only when one or more items were skipped. (operation-dialogs: "End-of-job skipped-files summary")

## 9. Input routing and event-loop wiring (`filecommand-tui`)

- [ ] 9.1 Route `Ins`, `+`, `-`, `*` to the core selection commands and F5/F6/F7/F8 to Copy/Move/Mkdir/Delete, mapping key presses to `Command`s per the §3.3 data flow.
- [ ] 9.2 Spawn fs jobs onto the worker thread and drain worker progress/conflict/error/done events back through the event queue into `core::update`, keeping the UI thread non-blocking and Cancel responsive throughout a long job. (file-operations: "UI stays interactive during a long job"; operation-dialogs: "Progress dialog stays interactive during a long job")
- [ ] 9.3 Wire conflict/error/cancel user choices from the dialogs back onto the worker reply channel and drive the auto panel re-read on job completion.

## 10. Testing

- [ ] 10.1 `filecommand-core` unit tests for selection semantics: Ins toggle/advance/no-wrap, parent non-selectable, `+`/`-` wildcard against original `OsString`, `*` invert (including invert-twice restores), mini-status counts/bytes with directories at 0, and persistence across move/re-sort plus clear-on-directory-change. (all `selection` scenarios; §8 "selection semantics")
- [ ] 10.2 `filecommand-core` fs_ops unit tests against temp dirs and the fake fs: multi-file copy/move/delete trees, same-volume rename vs cross-volume copy-then-delete (and source-survives-failed-copy), identity-aware case-only rename, read-only clear before overwrite/delete, reparse-point delete/copy/cycle-protection, cancellation mid-job, and error injection (permission-denied/sharing-violation/disk-full) surfacing distinct typed errors. (file-operations and filesystem-error-handling scenarios; §8 "fs_ops … error injection via the fs trait seam")
- [ ] 10.3 `filecommand-core` unit tests for the conflict and error state machines: "…All" latching for Overwrite All / Skip All / error Skip All, Retry re-attempt, Abort, and skipped-item accumulation for the summary. (operation-dialogs scenarios)
- [ ] 10.4 proptest for path joining including `\\?\` and `\\?\UNC\` prefixing (canonicalization precedes prefixing) and for the overwrite-conflict-resolution state machine. (§8 property-based tests; filesystem-error-handling "Canonicalization precedes prefixing", "UNC paths use the UNC prefix form")
- [ ] 10.5 `filecommand-tui` `insta` + `TestBackend` snapshot tests for each new dialog (destination input, overwrite conflict, progress with block gauge, error recovery, delete confirm, skipped-files summary), pinning time via the injected `Clock`, terminal size, and locale, with fixed-timestamp fixtures. (§8 TUI snapshot tests; operation-dialogs "Rendered timestamps are deterministic")
- [ ] 10.6 Integration test on real NTFS (CI `windows-latest`) verifying copy preserves alternate data streams, attributes, and timestamps (per design D5), plus a scripted smoke test: navigate → select → copy → verify result on disk and that affected panels re-read. (§8 integration smoke test; file-operations "Automatic panel re-read on completion")
- [ ] 10.7 Add the inline panel read-error state tests: access-denied listing enters the error state (no panic/exit), re-read recovers when the fault clears, and drive-change exits the error state. (filesystem-error-handling "Inline panel read-error state offering re-read or drive change")
