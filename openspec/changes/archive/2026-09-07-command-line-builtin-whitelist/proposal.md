## Why

The bottom command line currently runs any typed line through a real shell (`cmd.exe /C <text>` by default), with no parsing or allowlisting — a mistyped or unfamiliar command runs arbitrary shell functionality the user didn't intend. Compounding this, the one command that already IS special-cased outside the shell — `cd` — has a bug: navigating to a nonexistent directory switches the panel's current-directory into it anyway (empty listing, error text, but the header/prompt both show the nonexistent path as current). Tracked as Linear BIG-164.

## What Changes

- **BREAKING**: the command line no longer runs arbitrary typed text through a shell. Only three built-in verbs are recognized: `cd`, `del`, `rmdir`. Any other typed line is rejected (an error is shown, no process is spawned).
- `cd <path>`: unchanged navigation behavior for a valid target, but a target that doesn't exist (or isn't a directory) is now rejected outright — the panel is never switched into it. Fixes the reported bug.
- `del <target>` and `rmdir <target>` are new: they resolve the typed target against the active panel's directory and open the existing F8 delete-confirmation dialog (`del` for a file target, `rmdir` for a directory target — a type mismatch is rejected, not silently accepted). Neither deletes instantly.
- The file-action menu's **Run** entry and the **F2 user menu** are explicitly unaffected — both spawn through the same underlying suspended-shell mechanism as before, from their own call sites, independent of the command line's typed-buffer dispatch.

## Capabilities

### New Capabilities
(none)

### Modified Capabilities
- `command-line`: narrows "Run command via shell in suspended-TUI mode" so the suspended-shell spawn mechanism it describes is no longer reachable from typed command-line text (only from the file-action-menu Run entry and F2 user menu); adds the builtin-whitelist dispatch rule and `cd`'s existence-check behavior (closing an existing spec gap — `cd` was never documented despite already being implemented) and the new `del`/`rmdir` builtins.

## Impact

- `crates/filecommand-core/src/update.rs`: `run_command_line`'s `cd` branch gains a synchronous existence/is-directory check before `begin_listing`; two new builtin branches (`del`, `rmdir`) resolve a target and call the existing `enter_delete_confirm_for_sources`; the fallback branch (currently `Effect::RunShellCommand`) becomes a rejection instead of a shell spawn.
- `Effect::RunShellCommand`, `shell::build_command`, and the shell config (`crates/filecommand-core/src/shell.rs`) are unchanged and remain live for the file-action-menu Run entry (`update.rs` ~2515-2521) and F2 user menu (`update.rs` ~3182-3187).
- Existing tests asserting typed non-`cd` text produces `Effect::RunShellCommand` need rework (`running_a_command_records_history_clears_the_buffer_and_persists`, `running_a_command_uses_the_configured_shell`, `file_action_menu_does_not_open_when_the_command_buffer_is_non_empty`), and `an_unreachable_cd_target_surfaces_the_panel_error_state` needs to additionally assert `cwd` is left untouched.
