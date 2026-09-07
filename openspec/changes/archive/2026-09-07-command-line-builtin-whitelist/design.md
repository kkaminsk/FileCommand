## Context

`run_command_line` (`crates/filecommand-core/src/update.rs` ~1630-1659) is the single dispatch point for the command line's Enter handling. `cd` is already special-cased there (`parse_cd`/`resolve_cd_target`, pure path arithmetic, no existence check) because a subprocess can't change the app's own working directory; every other typed line falls through to `Effect::RunShellCommand(shell::build_command(...), side)` — a real shell spawn with the raw text appended, no parsing at all.

Two other call sites build the identical `Effect::RunShellCommand` independently: the file-action-menu's Run entry (~2515-2521) and the F2 user menu (~3182-3187). Both are deliberate, separate, already-specified uses of the same suspended-spawn mechanism and are unaffected by this change.

The `cd`-into-a-nonexistent-directory bug traces to `begin_new_listing` (`panel.rs` ~469-499) setting `panel.cwd` synchronously and optimistically, before the actual directory read runs (async, on a worker thread). `Command::ListingFailed` only sets `panel.last_error` on failure — `cwd` is never reverted, so the UI (which renders `panel.cwd` directly in both the panel header and the command prompt) shows the nonexistent path as current.

## Goals / Non-Goals

**Goals:**
- The command line only recognizes `cd`, `del`, `rmdir`; anything else typed is rejected without spawning a process.
- `cd` to a nonexistent (or non-directory) target is rejected outright — no navigation attempt, panel stays exactly where it was.
- `del`/`rmdir` reuse the existing delete-confirmation dialog and job machinery rather than deleting instantly or duplicating logic.
- The file-action-menu Run entry and F2 user menu keep working exactly as today.

**Non-Goals:**
- Fixing the identical optimistic-cwd/no-rollback pattern in drive-select navigation (same shape, different entry point — left as a possible separate future proposal).
- Adding command aliases (`erase`, `rd`) beyond the three verbs requested.
- Any change to command history semantics beyond what's needed to stay consistent with `cd`'s current (no-history) behavior.

## Decisions

- **Fix `cd` with a synchronous pre-check, not an async rollback.** Check the resolved target's existence/directory-ness before calling `begin_listing` at all; on failure, set `panel.last_error` and return without touching `cwd`, `entries`, or issuing any listing effect. Alternative considered: revert `cwd` generically in the `ListingFailed` handler for any failed listing (would also fix drive-select). Rejected for this change — plain listings have no request-generation guard today (unlike git-info/info-mode queries), so a stale failure could incorrectly revert a `cwd` that's since moved on to a newer, valid navigation; adding that guard is a larger, separate concern than what was asked.
- **`del`/`rmdir` route into the existing F8 confirmation flow, not an instant delete.** Resolve the typed target to a `SourceItem` (reusing the listed-entry's `is_dir_like()` when the target matches a currently-visible entry, otherwise a filesystem check — the same check used for the `cd` fix) and call the existing `enter_delete_confirm_for_sources`. This reuses the entire confirm/second-confirm/job/error-recovery pipeline unchanged, and matches the safety expectations already established for F8 and the file-action-menu's Delete entry (no destructive action without an interposed dialog).
- **Type enforcement matches classic `cmd.exe`**: `del` rejects a directory target, `rmdir` rejects a file target — a mismatched type is rejected immediately (no dialog), rather than silently accepting either, to keep the two verbs' meaning unambiguous.
- **Rejected/unrecognized input does not spawn anything and does not touch command history.** `cd` today does not push to history (confirmed in the current implementation); `del`/`rmdir` and rejected lines are kept consistent with that existing precedent rather than introducing new, inconsistent history behavior as a side effect of this change.
- **`Effect::RunShellCommand` and `shell::build_command` are untouched.** Only `run_command_line`'s own fallback branch changes (from "spawn a shell" to "reject"). The Run entry and F2 user menu keep constructing and dispatching the same effect from their own call sites, so no shared code is removed or altered in a way that affects them.

## Risks / Trade-offs

- [Risk] This is a **breaking change** for anyone currently using the command line as a lightweight shell (e.g. running `dir`, `git status`, or arbitrary tools without leaving the TUI). → Mitigation: this is the explicit, requested behavior change (Linear BIG-164); the file-action-menu Run entry and F2 user menu remain available for launching programs, and the F2 user menu can be configured with custom commands for anything used often enough to want quick access to.
- [Risk] `del`/`rmdir` needing a filesystem check for a target not in the current listing (e.g. a nested path, or a name not currently visible due to a quick-filter) adds a synchronous I/O call on the reducer thread. → Mitigation: this mirrors the same single-`stat` cost already accepted for the `cd` fix; typed delete targets are not a hot path.
