## 1. Core: fix the `cd`-to-nonexistent-directory bug

- [x] 1.1 In `crates/filecommand-core/src/update.rs`, in `run_command_line`'s `cd` branch (~1630-1659), add a synchronous existence/is-directory check on the `resolve_cd_target` result before calling `begin_listing`.
- [x] 1.2 On a failed check, set the active panel's `last_error` to a "not found" / "not a directory" message and return without calling `begin_listing`, without touching `cwd`/`entries`.
- [x] 1.3 Update `an_unreachable_cd_target_surfaces_the_panel_error_state` (`update/tests.rs` ~1441-1449) to assert `cwd` is unchanged and no `Effect::StartListing`/listing effects are produced (this test currently exercises the UNC-unreachable case via `ListingFailed`, which is a different, still-valid scenario — a race where a target existed at check time but became unreachable before the read completed; add a new test for the synchronous nonexistent-target rejection path specifically).
- [x] 1.4 Add a test for `cd` to an existing file (not a directory) being rejected the same way.

## 2. Core: builtin whitelist dispatch

- [x] 2.1 Replace `run_command_line`'s fallback branch (currently `Effect::RunShellCommand`) with a rejection: set `last_error` on the active panel to a "not recognized" message, spawn nothing, push no `Effect::RunShellCommand`.
- [x] 2.2 Confirm case-insensitive matching for `cd`/`del`/`rmdir` (mirroring `parse_cd`'s existing `cd `/`CD `/`Cd ` handling) and add/extend a shared parse helper if that reduces duplication across the three verbs.
- [x] 2.3 Do not push `Effect::PersistHistory` for `del`/`rmdir`/rejected input, consistent with `cd`'s existing no-history behavior (design.md decision).

## 3. Core: `del`/`rmdir` builtins

- [x] 3.1 Add `parse_del`/`parse_rmdir` mirroring `parse_cd`'s pattern.
- [x] 3.2 Resolve the typed argument to a path against the active panel's cwd.
- [x] 3.3 Determine the target's `is_dir`: reuse the listed-entry's `is_dir_like()` when the target matches a currently-visible entry (same helper pattern used near `panels_matching`, `update.rs` ~2085); otherwise do a filesystem check (reuse the same check added in task 1.1).
- [x] 3.4 Reject on nonexistent target, or on a type mismatch (`del` + directory, `rmdir` + file) — set `last_error`, open no dialog.
- [x] 3.5 On a valid, type-matched target, build the `SourceItem` and call the existing `enter_delete_confirm_for_sources` (`update.rs` ~2427-2434) — do not add any new deletion logic.
- [x] 3.6 Tests: `del` on a file opens the confirm dialog naming it; `rmdir` on a non-empty directory requires the existing second confirmation; `del` on a directory rejected; `rmdir` on a file rejected; nonexistent target rejected for both.

## 4. Tests: rework existing shell-execution assertions

- [x] 4.1 Update `running_a_command_records_history_clears_the_buffer_and_persists` (~946-965) and `running_a_command_uses_the_configured_shell` (~967-980) — these currently assert arbitrary typed text produces `Effect::RunShellCommand`; since that's no longer true for the command line, either repurpose them to test the Run entry/F2 user menu's still-unchanged use of `Effect::RunShellCommand`/`shell::build_command`, or remove/replace with builtin-dispatch equivalents as appropriate.
- [x] 4.2 Update `file_action_menu_does_not_open_when_the_command_buffer_is_non_empty` (~1075-1084) to use a still-valid non-empty-buffer example (e.g. `cd somewhere`, or a rejected line) instead of relying on `Effect::RunShellCommand`.
- [x] 4.3 Grep for any other test asserting `Effect::RunShellCommand` fires from a typed command-line buffer and update similarly.

## 5. Verification

- [x] 5.1 Run `cargo test -p filecommand-core` and `cargo test -p filecommand-tui`.
- [ ] 5.2 Manual check via the `run` skill: `cd` to a real subdirectory (navigates); `cd` to a nonexistent path (rejected, panel unchanged); `del`/`rmdir` on real targets (opens confirm dialog, deletes only on accept); type a random command like `dir` or `notepad` (rejected, nothing spawns); confirm the file-action menu's Run entry and an F2 user-menu command still spawn normally.
- [ ] 5.3 `detect_changes()` (GitNexus) against `main` to confirm only the expected symbols/flows are touched before opening the PR.
