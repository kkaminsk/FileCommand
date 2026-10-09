# Design: pulldown-menu-completion

## Context

`menu.rs:150-215` item tables; `update.rs:2406-2427` `MenuAction → Command`; `MenuId::target_side` resolves Left/Right menus to their own side. `OpenTab`/`CloseTab`/`QuickFilterStart` act on `state.active`. `state.history` is the persisted command list (newest last, 200 cap). `handle_request_editor` opens the cursor entry via `Effect::OpenEditor{path}` (built-in) or `Effect::RunExternalEditor(EditorInvocation, PanelSide)` (external); both are built from a path, so a menu-file edit is one new command that supplies the user-menu path instead of the cursor entry. `startup_warning` is a dismissable dialog usable mid-session. In-flight Info/git replies carry a `PanelSide` and a request id; listing chunks carry only the `PanelSide`. The `F(8)` keymap arm has no modifier guard, so Alt+F8 currently opens the delete confirmation.

## Goals / Non-Goals

**Goals:**

- No greyed item left.
- Each new feature minimal and NC-faithful (Alt+F8 history, Ctrl+U swap).
- Left/Right menu items act on their own side.

**Non-Goals:**

- Attributes dialog, directory compare, a configuration dialog, editor selection UI, "Save setup" (settings already persist on change).
- Executing a history entry directly from the dialog (Enter places it; the user presses Enter again to run — one extra key, no accidental runs).

## Decisions

### D1: Side-carrying commands for the Left/Right items

`Command::OpenTab(PanelSide)`, `CloseTab(PanelSide)`, `QuickFilterStart(PanelSide)`; the keymap passes `state.active`. For the quick filter, typing must reach the filtered panel, so `QuickFilterStart(side)` also makes `side` the active panel (a visible but expected focus change, as with Left → Info today via `ToggleInfoMode(side)`).

Alternative rejected: keeping side-less commands and switching the active panel before dispatch — it would leave the keyboard focus moved after New tab/Close tab too, where nothing requires it.

### D2: History dialog is a list overlay like the user menu

`HistoryDialogState { cursor }` over `state.history` rendered newest-first; Up/Down/Home/End/PgUp/PgDn; Enter → `command_line = entry` (caret at end, `history_cursor = None`), close; Esc → close. Empty history renders a single `(no history)` row and Enter does nothing. Opened by Alt+F8 or Commands → History. Not mouse-enabled (joins the unsupported-overlay list in mouse-input).

### D3: Swap panels exchanges `PanelState`s and re-issues async queries

`std::mem::swap(&mut state.left, &mut state.right)`; `active` and `split_percent` unchanged (the left slot stays the left slot). Listing chunks are addressed only by `PanelSide` with no request id, so the swap is **refused while either panel's listing is streaming** (no-op; the mini-status already shows `Reading…`). Info/git replies do carry a request id, so after a swap the reducer re-issues `QueryInfo` (if in Info mode) and `QueryGitInfo` for both sides with fresh ids and any reply from before the swap is discarded as stale. Tree and Quick view modes keep working unchanged because both sides moved together. Drag state cannot exist while a menu is open; Ctrl+U during a drag is ignored.

Alternative rejected: re-targeting in-flight replies by remembering a swap generation — more state for a case the streaming guard removes.

### D4: Menu file edit reuses the F4 path and reloads on return

`Command::EditUserMenuFile` → `Effect::OpenEditor{path: state.user_menu_path}` (built-in) or `RunExternalEditor` when `editor =` is set — the same branch `handle_request_editor` takes. The TUI, on editor close/return for that path, calls `config::load_user_menu` and sends `Command::UserMenuReloaded(result)`: entries replace `state.user_menu_entries`; a malformed file raises the existing warning dialog and keeps the previous entries, never overwriting the file (user-menu "Create and recover").

### D5: Remove, don't hide

The five removed items disappear from the tables; Options becomes `Themes` alone (no separator). `MenuAction::Unimplemented` is retained for future proposals and the disabled-rendering unit tests build a local fixture menu instead of relying on shipped items.

## Risks / Trade-offs

- [Changing `OpenTab`/`CloseTab`/`QuickFilterStart` signatures touches existing tests] → Mechanical: every call site passes `state.active`.
- [A swap right after a job completes may race the automatic re-read] → Covered by D3's streaming guard; a swap while a job dialog is up cannot happen (the modal blocks menus and keys).

## Open Questions

- None.
