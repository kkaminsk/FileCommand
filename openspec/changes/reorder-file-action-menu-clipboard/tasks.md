## 1. Core: reorder the menu entries

- [x] 1.1 In `crates/filecommand-core/src/dialogs.rs::FileActionMenuState::open` (~lines 206-223), move the `FileActionMenuEntry::SendToClipboard` push out of the trailing `entries.extend([Copy, Rename, Move, Delete, SendToClipboard])` and place it immediately after the `if !is_dir { ... }` block, before `Copy` — yielding `[Run?, View, Edit, SendToClipboard, Copy, Rename, Move, Delete]` for files and `[SendToClipboard, Copy, Rename, Move, Delete]` for directories.
- [x] 1.2 Update the doc comment above `SendToClipboard` in the `FileActionMenuEntry` enum (~line 133) that references "Menu contents, ordering, and navigation" if it describes the old position.

## 2. Core: update existing tests

- [x] 2.1 Update `file_action_menu_lists_run_first_only_when_executable` (`dialogs.rs` ~line 553) — the expected `m.entries` vec for `setup.exe` — to the new order.
- [x] 2.2 Add a test asserting the directory-target order (`FileActionMenuState::open` with `is_dir: true`) is `[SendToClipboard, Copy, Rename, Move, Delete]`.
- [x] 2.3 Search `crates/filecommand-core/src/update/tests.rs` for any file-action-menu order assertions and update them if present.

## 3. TUI: regenerate snapshots

- [x] 3.1 Regenerate `crates/filecommand-tui/tests/snapshots/snapshot_views__file_action_menu_dialog_executable.snap` and `..._non_executable.snap` (via `cargo insta review` or equivalent) to reflect the new entry order.
- [x] 3.2 Check `snapshot_views__menu_bar_files_pulldown.snap` and `snapshot_views__quit_confirm_dialog_over_pulldown_menu.snap` for any file-action-menu order dependency; regenerate if affected.

## 4. Verification

- [x] 4.1 Run `cargo test -p filecommand-core` and `cargo test -p filecommand-tui`.
- [ ] 4.2 Manual check via the `run` skill: open the menu (Enter) on a file and confirm order `View, Edit, Send to clipboard, Copy, Rename, Move, Delete`; right-click a directory and confirm order `Send to clipboard, Copy, Rename, Move, Delete`.
- [ ] 4.3 `detect_changes()` (GitNexus) against `main` to confirm only the expected symbols/flows are touched before opening the PR.
