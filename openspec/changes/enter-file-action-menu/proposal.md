# Change: enter-file-action-menu

## Why

Pressing Enter on a non-executable file is currently a dead key — the panel does nothing — and Enter on an executable runs it immediately with no way to choose a different action. Surfacing the common per-file actions behind a single Enter press makes the panels discoverable and keyboard-efficient (§3.1 key model), while keeping every mutating action behind the existing confirmation dialogs so nothing commits by accident.

## What Changes

- Pressing Enter with the cursor on a **file** entry (and an empty command-line buffer) opens a modal **file-action menu** for that entry: **View, Edit, Copy, Rename, Move, Delete**, with a **Run** entry shown first when the entry is executable (PATHEXT match or `.lnk`).
- Each menu action routes into an existing flow — F3 viewer, F4 edit path, F5 copy destination dialog, F6 move destination dialog, F8 delete confirmation — so every filesystem mutation is confirmed via an intervening dialog before it commits, and Esc at any point aborts with no change.
- **Rename** is a new in-place variant: an input dialog pre-filled with the entry's current name, renaming within the current directory using the same-volume rename machinery (identity-aware case-only rename included).
- **BREAKING** (behavior): Enter on an executable no longer spawns it directly; it opens the menu, whose Run entry uses the existing suspended-TUI spawn path. Enter with a non-empty command-line buffer still runs the typed command, and Enter on a directory / `..` still navigates — both unchanged.
- The menu acts on the cursor entry only; it does not consume or alter the multi-entry selection.

## Capabilities

### New Capabilities

- `file-action-menu`: The modal Enter menu on file entries — contents and ordering (Run/View/Edit/Copy/Rename/Move/Delete), navigation (Up/Down, Enter, Esc, first-letter hotkeys), routing of each action into the existing viewer/editor/operation flows, the in-place Rename dialog, and the guarantee that no action mutates the filesystem without an intervening dialog.

### Modified Capabilities

- `command-line`: The "Run command via shell in suspended-TUI mode" requirement changes — Enter on an executable target no longer spawns directly; that spawn path is reached via the file-action menu's Run entry. Running a typed command with Enter is unchanged.

## Impact

- **Crates:** `filecommand-core` — new menu state + reducer routing in `update.rs` (Enter-on-file dispatch, menu open/navigate/activate, Rename job wiring through existing `fs_ops` rename machinery), dialog state in `dialogs.rs`. `filecommand-tui` — new menu dialog view (primary style per §4.4), input routing while the menu is modal, and adjusted Enter handling in `input/`.
- **Depends on:** M2 (`file-operations`, `operation-dialogs` — copy/move/delete jobs and dialogs), M3 (`command-line` suspended spawn), M4/M5 (`viewer`, `external-editor`, `builtin-editor` for View/Edit).
- **No changes** to `panel-navigation` (Enter on directories is untouched), selection semantics, or `usermenu.toml`/F2 user menu.
