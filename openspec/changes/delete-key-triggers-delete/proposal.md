## Why

Pressing the **Delete** key in a panel currently does nothing — a bug. Users expect Delete to delete the cursored file (or the current selection), matching the modern-file-manager convention Norton Commander users reach for reflexively. The delete machinery itself works; only the physical Delete key was never wired to it.

## What Changes

- Bind the plain **Delete** key in the panel keymap (`map_panel_key`) as a fixed alias for **F8**, issuing the existing `Command::RequestDelete` so it enters the identical delete-confirmation flow.
- The alias is **not user-rebindable** (matching the existing fixed `Ctrl+Ins` clipboard alias) and fires only on an **unmodified** Delete press, leaving `Shift+Delete` / `Ctrl+Delete` free for the future.
- No change to the delete flow, confirmation dialogs, selection scope, or the permanent (no recycle bin) semantics. F8, the `Files` pull-down, the file-action menu, mouse activation, and the `del`/`rmdir` command-line verbs are all unchanged.
- Add regression test coverage asserting the Delete key maps to `Command::RequestDelete`.

Non-breaking and purely additive.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `operation-dialogs`: The "Delete confirmation dialog" requirement, which today names **F8** as the sole trigger, is widened to state the confirmation is triggered by **F8 or the Delete key**, with a new scenario proving the Delete key opens the same dialog.

## Impact

- **Specs:** `operation-dialogs` (one modified requirement, one added scenario).
- **Code:** `crates/filecommand-tui/src/input/mod.rs` (`map_panel_key` — one added match arm) and `crates/filecommand-tui/src/input/tests.rs` (one added assertion). No core, dialog, or filesystem changes.
- **Compatibility:** Additive; no existing binding or behavior is altered or removed.
