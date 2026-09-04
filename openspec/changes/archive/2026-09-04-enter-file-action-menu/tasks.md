# Tasks: enter-file-action-menu

## 1. Core menu state and Enter routing

- [x] 1.1 Add file-action-menu state to `filecommand-core` (target entry, entry list with conditional Run, highlight index) alongside the existing dialog state in `dialogs.rs` (file-action-menu: "Enter on a file opens the action menu")
- [x] 1.2 Route Enter in `update.rs::handle_enter` — empty command buffer + cursor on a file (executable or not) opens the menu; directories/`..` and non-empty buffer behavior unchanged (file-action-menu: "Enter on a file opens the action menu")
- [x] 1.3 Remove the direct spawn on Enter-on-executable and expose it as the menu's Run action via the existing suspended-spawn path (command-line: "Run command via shell in suspended-TUI mode")
- [x] 1.4 Implement menu navigation and dismissal in `core::update` — Up/Down highlight movement, Esc closes with no action, Enter activates, first-letter hotkeys activate directly (file-action-menu: "Menu contents, ordering, and navigation")

## 2. Action routing

- [x] 2.1 Route View and Edit to the existing F3 viewer and F4 edit-path commands for the target entry (file-action-menu: "Menu actions route to existing flows")
- [x] 2.2 Route Copy and Move to the existing destination-input dialog flows, scoped to the single target entry with the opposite panel path pre-filled (file-action-menu: "Menu actions route to existing flows")
- [x] 2.3 Route Delete to the existing delete-confirmation flow scoped to the single target entry (file-action-menu: "Menu actions route to existing flows")
- [x] 2.4 Verify no menu action mutates the filesystem before its dialog is accepted, and that Esc at menu or dialog aborts fully (file-action-menu: "No mutation without an intervening dialog")

## 3. In-place Rename

- [x] 3.1 Add the rename input dialog pre-filled with the current name, wired to the same-volume rename machinery in `fs_ops` including identity-aware case-only rename (file-action-menu: "In-place Rename")
- [x] 3.2 Surface rename collisions/failures through the existing overwrite-conflict and error-recovery dialogs and re-read the panel on success (file-action-menu: "In-place Rename")

## 4. TUI view and input

- [x] 4.1 Add the primary-style menu dialog view in `filecommand-tui/src/views/` per §4.4 (double-line frame, black on cyan, highlighted row) (file-action-menu: "Menu contents, ordering, and navigation")
- [x] 4.2 Add modal input routing for the menu in `input/` so panel/command-line keys are suppressed while the menu is open (file-action-menu: "Enter on a file opens the action menu")

## 5. Tests

- [x] 5.1 Reducer tests: Enter-on-file opens the menu (selection untouched, non-empty buffer precedence, directory Enter unchanged), navigation, Esc, hotkeys, and per-action routing (file-action-menu: "Enter on a file opens the action menu")
- [x] 5.2 Reducer tests: executable target gets Run first and Enter-Enter spawns; no direct spawn on first Enter (command-line: "Run command via shell in suspended-TUI mode")
- [x] 5.3 Rename tests: pre-fill, in-place rename, case-only rename, Esc no-op, collision surfaces existing dialogs (file-action-menu: "In-place Rename")
- [x] 5.4 `insta` snapshot tests for the menu view in executable and non-executable variants (file-action-menu: "Menu contents, ordering, and navigation")
