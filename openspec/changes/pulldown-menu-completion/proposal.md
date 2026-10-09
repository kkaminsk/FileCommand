# Change: pulldown-menu-completion

## Why

Thirteen pull-down items render greyed out (`MenuAction::Unimplemented` in `menu.rs`): Left/Right **Filter, New tab, Close tab**; Files **View, Edit, Attributes**; Commands **History, Swap panels, Compare directories, Menu file edit**; Options **Configuration, Editor selection, Save setup**. Eight of them front features that already exist (F3, F4, Ctrl+P, Ctrl+T, Ctrl+W) or are a few dozen lines away (a history list over `state.history`, a swap of two `PanelState`s, opening `usermenu.toml` in the editor). Greyed items in a shipping menu read as broken, and the pull-downs are the main discovery surface for keyboard users. The user chose to wire the cheap items and remove the rest.

## What Changes

- **Wired to existing commands:** Files → View (F3), Edit (F4); Left/Right → Filter (quick filter on that panel), New tab (Ctrl+T), Close tab (Ctrl+W), each acting on the menu's own panel side.
- **New, small:** Commands → **History** (new `Alt+F8` command-history dialog: pick an entry, Enter places it on the command line), **Swap panels** (new `Ctrl+U`: exchanges the two panels' entire state), **Menu file edit** (opens `usermenu.toml` with the F4 editor path and reloads the user menu on return).
- **Removed from the menus:** Files → Attributes; Commands → Compare directories; Options → Configuration, Editor selection, Save setup (Options now lists Themes only). They can return via their own proposals.
- The disabled-item rendering rule stays in the spec and renderer for future use; no shipped item is disabled after this change, so the "Not-yet-available feature renders disabled" scenario under "Menu contents" is dropped.
- Alt+F8 today falls through to the unguarded `F(8)` arm and opens the delete confirmation; the new Alt+F8 arm takes precedence, exactly as `F(7) if alt` does for find-file.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `pulldown-menus`: "Menu contents" rewritten to the final item lists; new requirement "Every listed item is wired" with per-item routing scenarios.
- `command-line`: new requirement "Command history dialog (Alt+F8)".
- `panel-navigation`: new requirement "Swap panels (Ctrl+U)".
- `user-menu`: new requirement "Edit the user menu file from the Commands menu".

## Impact

- `crates/filecommand-core/src/menu.rs` — item tables and `MenuAction` variants (`View`, `Edit`, `QuickFilter`, `NewTab`, `CloseTab`, `History`, `SwapPanels`, `EditMenuFile`); removed items.
- `crates/filecommand-core/src/update.rs` — `menu_action_command` mapping; `Command::OpenTab/CloseTab/QuickFilterStart` gain a `PanelSide` (the keymap passes the active side); `Command::SwapPanels`, `Command::HistoryDialogOpen/Move/Confirm/Cancel`, `Command::EditUserMenuFile`, `Command::UserMenuReloaded(..)`; `State.history_dialog: Option<HistoryDialogState>`, `State.user_menu_path: PathBuf`.
- `crates/filecommand-core/src/dialogs.rs` — `HistoryDialogState` (cursor over `state.history`, newest first).
- `crates/filecommand-tui` — `input/mod.rs` Alt+F8, Ctrl+U, history-dialog keymap; `views/history_dialog.rs` (primary style, like the user menu); `app.rs` reload of the user menu after the editor returns for the menu file; mouse: the history dialog is an unsupported overlay like the user menu.
- Tests across `menu.rs`, `update/tests.rs`, `input/tests.rs`, snapshots.
- Depends softly on `config-profile-directory` for the user-menu path (works with whichever path the app resolves).
