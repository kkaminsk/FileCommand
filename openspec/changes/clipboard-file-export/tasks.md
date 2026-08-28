# Tasks: clipboard-file-export

## 1. Core

- [x] 1.1 Add `ClipboardPayloadKind`, `ClipboardPayload`, `Effect::SetClipboard`, `Command::CopyToClipboard`, and `Command::ClipboardResult` in `crates/filecommand-core/src/update.rs`; resolve scope via `active_selection_sources` (clipboard-export: "Clipboard payloads and scope")
- [x] 1.2 Mini-status feedback state with `Tick` expiry and next-key clearing (clipboard-export: "Clipboard feedback")
- [x] 1.3 Add `Send to clipboard` to the file-action menu model in `dialogs.rs` as a non-mutating entry with first-letter hotkey `S` (file-action-menu: "Menu contents, ordering, and navigation"; "No mutation without an intervening dialog")
- [x] 1.4 `Keys::clipboard_files` (default Ctrl+C) and `Keys::clipboard_paths` (default Ctrl+Shift+Ins) in `config.rs`, parsed as `key.clipboard_files` / `key.clipboard_paths` via `parse_binding` (clipboard-export: "Clipboard key bindings")
- [x] 1.5 Files menu items with hyphenated shortcut hints (`Ctrl-C`, `Ctrl-Sh-Ins`) and new `MenuAction` variants in `menu.rs` (pulldown-menus: "Menu contents"; clipboard-export: "Clipboard actions in menus")
- [x] 1.6 Help text in `dialogs.rs`: `MODERN_EXTRAS` documents the clipboard actions; `CONFIGURATION` lists the two new remappable keys (help-and-about: "Help topic pages")
- [x] 1.7 Core unit tests: scope rules, `..` exclusion, feedback expiry, menu and action-menu dispatch

## 2. TUI

- [x] 2.1 `crates/filecommand-tui/src/clipboard.rs`: `Clipboard` trait; Windows implementation with `clipboard-win` (`FileList`, `Preferred DropEffect`, `new_attempts` retry with back-off); text-only implementation elsewhere; recording implementation for tests (clipboard-export: "Windows file-object payload"; "Non-Windows fallback"; "Clipboard busy retry")
- [x] 2.2 Path normalisation: absolute, `\\?\` stripped, `\\?\UNC\` rewritten (clipboard-export: "Windows file-object payload")
- [x] 2.3 `run_effects` arm for `SetClipboard` feeding `ClipboardResult` back into the reducer
- [x] 2.4 Input mapping in `input/mod.rs`: Ctrl+C / Ctrl+Ins → Files and Ctrl+Shift+Ins → Paths over the panels, matched before the unguarded `Insert → ToggleSelectAtCursor` arm; remove every Ctrl+C → `RequestQuit` arm (panels, viewer, pull-down, fuzzy-jump, find-file, user-menu, theme-picker, file-action-menu, help, drive-select, quick-search, file-op setup/running) and the quit-dialog Ctrl+C → `ConfirmQuit` arm; add no-modifier guards to the progress-dialog `c` → `FileOpCancelJob` and Help `c` → `HelpCancel` arms so Ctrl+C is ignored there; update `input/tests.rs` (clipboard-export: "Clipboard key bindings"; application-shell: "Quit request keys and confirmation")
- [x] 2.5 TUI tests: Ctrl+C no longer requests quit in any context, quit dialog ignores Ctrl+C, progress dialog and Help ignore Ctrl+C, Ctrl+Ins no longer toggles selection

## 3. Verification

- [x] 3.1 Snapshot goldens: Files menu with the new group, action menu with `Send to clipboard`, mini-status feedback line
- [x] 3.2 `#[ignore]` Windows integration test: round-trip `FileList`, prefix stripping, DropEffect value
- [x] 3.3 `cargo build --workspace` and `cargo test --workspace` green
