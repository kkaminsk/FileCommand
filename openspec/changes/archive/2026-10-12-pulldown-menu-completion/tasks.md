# Tasks: pulldown-menu-completion

## 1. Menu tables (`crates/filecommand-core/src/menu.rs`)

- [x] 1.1 Add `MenuAction::{View, Edit, QuickFilter, NewTab, CloseTab, History, SwapPanels, EditMenuFile}`; wire Left/Right Filter/New tab/Close tab, Files View/Edit, Commands History/Swap panels/Menu file edit (pulldown-menus "Every listed item is wired")
- [x] 1.2 Remove Attributes, Compare directories, Configuration, Editor selection, Save setup; Options = `Themes` (pulldown-menus "Menu contents"; design D5)
- [x] 1.3 Tests: item lists match the spec; no shipped item is disabled; disabled-rendering tests use a local fixture (design D5)

## 2. Reducer (`crates/filecommand-core/src/update.rs`, `dialogs.rs`)

- [x] 2.1 `menu_action_command` maps the new actions; `OpenTab/CloseTab/QuickFilterStart` take `PanelSide`; `QuickFilterStart(side)` activates `side` (design D1; scenarios "Right → Filter on the inactive panel", "Left → New tab on the inactive panel")
- [x] 2.2 `HistoryDialogState` + `Command::HistoryDialogOpen/Move/Confirm/Cancel`; Enter places the entry with the caret at the end and clears `history_cursor` (command-line "Command history dialog (Alt+F8)"; design D2)
- [x] 2.3 `Command::SwapPanels`: streaming guard, swap, `active`/split unchanged, re-issue Info/git queries with fresh ids (panel-navigation "Swap panels (Ctrl+U)"; design D3)
- [x] 2.4 `State.user_menu_path`; `Command::EditUserMenuFile` → editor effect for that path; `Command::UserMenuReloaded(UserMenuLoadResult)` applies entries or the malformed warning (user-menu "Edit the user menu file from the Commands menu"; design D4)
- [x] 2.5 Tests: each menu item issues its command; history dialog navigation/confirm/cancel/empty; swap preserves the active side and split, refuses during streaming, re-queries git/Info; menu-file edit effect path and reload (well-formed and malformed)

## 3. TUI (`crates/filecommand-tui`)

- [x] 3.1 `input/mod.rs`: `F(8) if alt` → `HistoryDialogOpen` placed before the plain `F(8)` arm (which has no modifier guard today); Ctrl+U → `SwapPanels`; history-dialog keymap; `OpenTab/CloseTab/QuickFilterStart` pass `state.active`; `input/tests.rs` coverage
- [x] 3.2 `views/history_dialog.rs`: primary-style list (reuse the user-menu renderer's geometry); snapshot tests (populated, empty)
- [x] 3.3 `app.rs`: set `user_menu_path`; after the built-in editor closes or the external editor returns on that path, reload and send `UserMenuReloaded`
- [x] 3.4 Mouse: the history dialog is added to the unsupported-overlay set (no clicks); verify existing mouse tests

## 4. Docs

- [x] 4.1 README "Pull-down menus (F9)" rewritten; keyboard reference gains Alt+F8 and Ctrl+U; F1 Help "Menus" and "Keyboard reference" pages

## 5. Verify

- [x] 5.1 `cargo test --workspace` green; `openspec validate pulldown-menu-completion --strict` clean
- [ ] 5.2 Manual: every F9 item does something; Alt+F8 → pick → Enter → Enter runs; Ctrl+U in a git repo swaps and the branch suffix follows; Commands → Menu file edit, add an entry, save → F2 shows it
