## Why

The file-action menu currently lists Send to clipboard last, after every mutating operation (Copy, Rename, Move, Delete). Send to clipboard is a common, non-destructive action used alongside View and Edit, and having to skip past four other entries to reach it slows down that common path. Tracked as Linear BIG-163.

## What Changes

- Reorder the file-action menu's entries so Send to clipboard sits immediately after Edit, ahead of the mutating operations: `[Run], View, Edit, Send to clipboard, Copy, Rename, Move, Delete`.
- No change to which entries appear (Run still conditional on the target being executable; View/Edit/Run still omitted for directories), to first-letter hotkey activation, or to what each entry does when activated — this is a display/cursor-order change only.

## Capabilities

### New Capabilities
(none)

### Modified Capabilities
- `file-action-menu`: changes the "Menu contents, ordering, and navigation" requirement's listed entry order (file targets), and the "Directory targets and selection-scoped invocation" requirement's directory-menu order to match. Note: this capability's spec has not yet been archived into `openspec/specs/` — the requirement text this change modifies currently lives as pending deltas in `openspec/changes/clipboard-file-export/` (base ordering) and `openspec/changes/mouse-basics/` (directory-menu ordering), and this proposal's delta is written to stack on top of both once archived.

## Impact

- `crates/filecommand-core/src/dialogs.rs`: `FileActionMenuState::open` builds one shared `entries` sequence — an `if !is_dir` block pushes `[Run?, View, Edit]`, then a common `entries.extend([Copy, Rename, Move, Delete, SendToClipboard])` suffix. Moving `SendToClipboard` out of that suffix to immediately after the `if !is_dir` block (before `Copy`) naturally produces the correct order for both the file-target case (`View, Edit, SendToClipboard, Copy, ...`) and the directory-target case (`SendToClipboard, Copy, ...`) with one change.
- Existing unit tests in `dialogs.rs` that assert the full entry order (e.g. the executable-menu-contents test) need their expected order updated to match.
- No changes expected to `crates/filecommand-tui` rendering beyond picking up the new order from core state; no changes to hotkey handling (`hotkey_action`) since it looks up by letter, not position.
