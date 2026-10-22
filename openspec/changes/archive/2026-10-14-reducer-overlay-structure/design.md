# Design: reducer-overlay-structure

## Context

All references were originally verified against `build/open-proposals-2026-10` (`1bada6f`). The audit quoted non-blank line counts. The physical counts are larger, and both are given here.

*Line references refreshed against the current branch (`Spec`, `03bdbd7`) on 2026-10-12.* Two features present on the verified branch have not landed on this one: the command-history dialog (`history_dialog`, `handle_history_dialog`, `HistoryDialogOpen`, from pulldown-menu-completion) and the dialog-iris stamp (`stamps.rs`, `State::dialog_opened_at_ms`, from ui-animations). Mentions of those, and the "ten fields" phrasing that counts `history_dialog`, describe the verified branch; every other reference below matches the current tree.

**Size.**
- `crates/filecommand-core/src/update.rs` is 3,707 lines (3,519 non-blank).
- `Command` spans `:409-877`, `Effect` spans `:908-1010`, and `update_impl` spans `:1063-1600` (538 lines).
- `update/tests.rs` is 5,118 lines (4,605 non-blank).

**Overlay state.** `State` holds ten independent `Option` overlay fields, each documented as a "one-shot `Option<T>` overlay" (`update.rs:190-252` on this branch, which has nine; see the refresh note):

| Field | Type |
|---|---|
| `menu` | `MenuState` |
| `drive_select` | `DriveSelect` |
| `fuzzy_jump` | `FuzzyJumpState` |
| `find_file` | `FindFileState` |
| `user_menu` | `UserMenuState` |
| `history_dialog` | `HistoryDialogState` |
| `theme_picker` | `ThemePickerState` |
| `file_action_menu` | `FileActionMenuState` |
| `help` | `HelpState` |
| `startup_warning` | `String` |

`quit_confirm: bool` is a separate layer (`:253-262`). Its documented contract is that it opens above any context and that cancelling it clears only this flag. The `State::dialog_opened_at_ms` doc (`update.rs:292-297` on the verified branch; the stamp `stamps.rs` maintains for the dialog iris — see the refresh note) also relies on "only one of these dialogs is ever visible at a time", as does the render comment in `views/mod.rs` `render_phase`.

**The four precedence encodings.**

| Where | Order, highest first |
|---|---|
| Reducer, `update_impl` `:1165-1277` | quit (`RequestQuit`/`ConfirmQuit`/`CancelQuit`) → file-op phases or job events → viewer → editor → drive select → menu → fuzzy jump → find file → user menu → history → theme picker → file-action menu → help → startup warning → phase `match` |
| Keys, `input::map_key` `:27-102` plus `app.rs:195-215` | quit → *(viewer, editor: dispatched directly by `app.rs` when the phase is `Viewer`/`Editor`)* → startup warning → drive select → menu → fuzzy jump → find file → user menu → history → theme picker → help → file-action menu → phase (file-op dialogs, then panels) |
| Mouse, `input/mouse.rs` `context` `:156-187` | quit → {startup warning, drive select, fuzzy jump, find file, user menu, history, theme picker, help} ignored → file-action menu → file-op phase → menu (inside `Panels`) → panels |
| Render z-order, `views/mod.rs` `render_phase` and `render` | (lowest to highest) panels → menu → drive select → fuzzy jump → find file → user menu → history → theme picker → help → file-action menu → file-op dialog → quit dialog → startup warning |

`build_hitmap` (`views/mod.rs:117-169`) repeats the "what is open" dispatch a fifth time.

**Why the disagreement is invisible today.** Every path that opens an overlay starts with the slot empty or empties it first:
- Overlays open only from the reducer's `Panels` arm, which is reached only when no overlay gate claimed the command.
- While an overlay is open, `map_key` routes every key to that overlay's own mapper. No overlay mapper emits another overlay's open command or `RequestQuit`. Unbound keys map to `None` and are ignored.
- The mouse context ignores clicks on every overlay except the file-action menu and the F9 menu.
- An overlay that leads to another closes itself first:
  - `handle_menu` sets `state.menu = None` before returning the item's command (`:2388-2395`);
  - the user menu's Themes slot takes `user_menu` before setting `theme_picker` (`:3372-3384`);
  - `handle_file_action_menu` takes the menu before `activate_file_action`.

Async replies could in principle open an overlay over another. `DriveListReady` is the only overlay opener among them. `Effect::EnumerateDrives` computes the drive list synchronously inside `run_effects` (`app.rs:689-694`) and posts `DriveListReady` on the channel, and `drain_events` applies it before the next key event is read. `ViewerOpened` and `EditorOpened` are produced the same way today. The startup warning is also raised mid-session, by `UserMenuReloaded` after the user-menu file was edited (`apply_user_menu_reload`; on the verified branch — see the refresh note); that command likewise follows the editor's return before any further input.

Startup warnings set before the event loop starts are last-writer-wins: `app::run` assigns `state.startup_warning` once (the theme-override warning, `app.rs:144-146`), and `apply_user_menu` once more (`:503`).

Two key-router tests nevertheless build two-overlay states on purpose, to pin an order:
- `the_drive_dialog_outranks_the_menu` (`input/tests.rs:473-478`): menu and drive select both open, and Esc goes to drive select.
- `startup_warning_outranks_every_other_modal_and_typing_sink` (`:1058-1067`): startup warning and menu both open.

A snapshot test draws the quit dialog over an open menu (`tests/snapshot_views.rs:275-284`, `quit_confirm_dialog_renders_above_an_open_pull_down_menu`). That combination stays legal, because quit is not part of the slot.

**The unwraps.** The overlay handlers in `update.rs` contain exactly 25 `.as_mut().unwrap()`, `.as_ref().unwrap()`, or `.take().unwrap()` calls on overlay fields, and they are the file's only `unwrap()` calls. They are in `handle_file_action_menu` (`:2627-2655`), `handle_fuzzy_jump` and `handle_find_file` (`:3263-3329`), `handle_user_menu` (`:3369-3373`), `handle_theme_picker` (`:3412-3416`), and `handle_help` (`:3445-3449`) — plus `handle_history_dialog` on the verified branch (see the refresh note). Each handler re-checks `is_none()` on entry, because nothing ties it to the gate.

**Dialog geometry.** Each clickable dialog has a `render_*` and a `hit_buttons`/`hit_items` that independently call `filecommand_core::dialogs::overlay_rect` with the same preferred and minimum sizes and recompute their content:
- `delete_confirm.rs:17-48` vs `:58-87` (the question text is rebuilt in both);
- `quit_dialog.rs:14-36` vs `:42-60`;
- `progress_dialog.rs:27-76` vs `:82-89`;
- the same pattern in `conflict_dialog.rs`, `error_dialog.rs`, `skipped_summary.rs`, `destination_input.rs`, `file_action_menu.rs`, and `menubar.rs`.

`overlay_rect` is the single geometry rule of responsive-layout "Unified overlay geometry". Each dialog's use of it, and the button positions inside the rectangle, is what is duplicated.

## Goals / Non-Goals

**Goals:**

- At most one modal overlay, enforced by the type system rather than by the shape of the open paths.
- One written-down precedence order that the reducer, key routing, mouse routing, rendering, and the hit map all follow.
- Overlay handlers that cannot run without their state, and no unwrap-after-gate.
- Reducer and test files split along functional lines, small enough to navigate.
- Click targets derived from the same layout that draws them.
- Zero change in observable behavior for every reachable state, with insta snapshots unchanged.

**Non-Goals:**

- Nesting overlay commands as `Command::Overlay(OverlayCmd)`, which the audit suggests (D4).
- Moving file-operation dialogs, the viewer, or the editor into the slot. They are `UiPhase` variants and already exclusive.
- Any precedence change that a user could observe. Where the current encodings disagree, it is only in unreachable states (Context).
- Deciding what an asynchronous viewer or editor open does when an overlay has opened in the meantime. That is `offload-blocking-io`, which makes those replies asynchronous and discards a late one (D5).
- Shortening `map_panel_key` (`input/mod.rs:238-431`), `app.rs::run`, or `app.rs::run_effects`. B-02 cites them as size evidence. Only the dispatch lines that route through the new precedence function change.
- Panel-row, tab-strip, and key-bar hit-testing. These already derive from `layout::compute`, the same function render uses.
- Removing the per-frame `OsString` clone of each on-screen row name in `views/panel.rs` `hit_test`, which B-08 cites as evidence. `streaming-listing-performance` lists it as out of its scope, so this change owns the decision, and keeps it. The clone is bounded by the panel height (a few dozen short names per frame, not the directory size), and `idle-aware-event-loop` (P-05) removes most idle frames. Panel hits are keyed by name on purpose, so a click stays correct when the listing changes under the pointer (the reasoning `hit_test`'s own comments give for rejecting row-index keys); keying by index to avoid the clone would give that up.
- The "too small" placeholder layer, which is `resize-preserves-session-state`.

## Decisions

### D1: `State::overlay: Option<Overlay>`, a single slot

```rust
pub enum Overlay {
    Menu(MenuState),
    DriveSelect(DriveSelect),
    FuzzyJump(FuzzyJumpState),
    FindFile(FindFileState),
    UserMenu(UserMenuState),
    HistoryDialog(HistoryDialogState),
    ThemePicker(ThemePickerState),
    FileActionMenu(FileActionMenuState),
    Help(HelpState),
    StartupWarning(String),
}
```

`quit_confirm` is **not** a variant. It is a layer above everything, and the existing application-shell contract ("Cancelling SHALL restore the prior context exactly — anything open stays open") requires it to coexist with an open overlay. The snapshot of the quit dialog over the menu stays valid.

The startup warning **is** a variant. It is raised only at session start or by `UserMenuReloaded` after the editor closes, both times with the slot empty, so it never coexists with another overlay. Its render placement (over whatever phase is showing, including the splash) is kept per variant (D8).

`State` gets read accessors (`overlay()`, plus typed `menu()`, `fuzzy_jump()`, and so on, returning `Option<&T>`). It also gets `open_overlay(Overlay)` and `close_overlay()`. The typed accessors keep the TUI and test migration mechanical. `drag_allowed` becomes `matches!(phase, Panels) && overlay.is_none() && !quit_confirm`.

*Alternative considered: keep the fields and add a runtime invariant check.* That catches violations only in tests, keeps the unwraps, and keeps four precedence encodings. **Rejected.**

### D2: One precedence function

Core exposes:

```rust
pub enum InputLayer { QuitConfirm, FileOp, Viewer, Editor, Overlay, Panels, Splash }
pub fn input_layer(state: &State) -> InputLayer
```

It encodes the reducer's current order, layer by layer:
1. quit confirmation;
2. file-operation phase;
3. viewer;
4. editor;
5. overlay slot;
6. the base phase (panels or splash).

Consumers:
- `update_impl` routes on it after its phase-independent preamble (`DialogButtonClick`, `Resize`, `Tick` bookkeeping, listing events, and stale-reply checks, all unchanged). Routing keeps today's fall-through: each layer handles only the commands it claims, and any other command continues to the next layer down. The quit layer claims only `ConfirmQuit` and `CancelQuit` (and `RequestQuit` opens it from anywhere). The file-op layer also claims job events in every phase. So `JobProgress`, `JobConflict`, and the other worker replies still reach a running job while the quit dialog is open, exactly as today.
- `input::map_key`, the direct viewer and editor dispatch in `app.rs::run`, `input::mouse::context`, and the merged render and hit-map dispatch (D8) all `match` on it, then on the slot's variant.

The reducer's order is chosen because the reducer is the authority on state, and the TUI's viewer and editor dispatch already agree with it. In every reachable state all five encodings agree, so the choice is not observable.

`resize-preserves-session-state` adds an undersized state. In its key routing the quit confirmation and then the startup warning are checked first, and only then the undersized mapper, which turns F10/Esc into a quit request and ignores every other key (its design D3). Here that is an `Undersized` layer directly below `QuitConfirm`, with one exception: an open `StartupWarning` keeps its keys above it. Rendering draws the placeholder in place of the phase and overlays, with the quit and startup-warning dialogs above it. Whichever change lands second adds this layer here.

*Alternative considered: the key router's order (overlays before the file-op phase).* It differs from the reducer's only in unreachable states, and adopting it would mean reordering the reducer, the larger and riskier side. **Rejected.**

### D3: Take, handle, restore

The reducer takes the overlay out of the slot (`state.overlay.take()`), matches on the variant, and calls the variant's handler with the overlay state and `&mut State`, for example `fn fuzzy_jump(fj: &mut FuzzyJumpState, state: &mut State, cmd) -> OverlayStep`. The slot is empty while the handler runs.

```rust
enum OverlayStep {
    Keep(Vec<Effect>),
    Close(Vec<Effect>),
    Replace(Overlay, Vec<Effect>),
    CloseThen(Command),
}
```

- `Keep` puts the state back.
- `Close` leaves the slot empty, as Esc, confirm, and cancel do today.
- `Replace` installs a different overlay: the user menu's Themes slot opening the theme picker.
- `CloseThen` closes and re-enters `update_impl` with a follow-up command: menu activation, exactly as `handle_menu` returns `Option<Command>` today.

This removes all 25 unwraps and the per-handler `is_none()` re-checks, and it sidesteps the double borrow of `state` and `state.<overlay>`.

*Alternative considered: `if let Some(Overlay::FuzzyJump(fj)) = &mut state.overlay` with split borrows.* The handlers need `&mut State` as well (`begin_listing`, `dir_history`, `clock_ms`), which a borrow of a field of `state` forbids. **Rejected.**

### D4: `Overlay::claims(&Command)` replaces the nine `is_*_command` predicates; `Command` stays flat

One `impl Overlay { fn claims(&self, cmd: &Command) -> bool }` holds every variant's command set in one `match`, moved verbatim from today's predicates. That includes `TextEdit` and `RequestPaste` for fuzzy jump and find file, and `FindFileMatch` and `FindFileSearchDone` for find file. The startup warning claims `DismissStartupWarning`. A command the open overlay does not claim falls through to the base layer, exactly as today: listing events, `Tick`, async replies, and so on.

*Alternative considered: `Command::Overlay(OverlayCmd)` nesting, as the audit suggests.* It would rewrite every overlay mapper in the TUI and roughly 300 test constructors, for no behavior or safety gain once `claims` sits in one place. **Rejected** for this change. It can be a later, purely mechanical change.

### D5: How overlays open, and replies that arrive for or during an overlay

- Synchronous openers stay where they are, in the `Panels` arm and the file-action and menu paths, and call `state.open_overlay(..)`. In a debug build `open_overlay` asserts that the slot is empty. Every current opener runs with an empty slot (Context).
- **Startup warnings keep last-writer-wins.** The assignments in `app::run` and `apply_user_menu` (Context) use a separate `State::set_startup_warning(String)` that installs `Overlay::StartupWarning` when the slot is empty and replaces the message when it already holds a startup warning. It debug-asserts that the slot holds nothing else. `error-diagnostics` introduces a logging helper of that name for the same assignments, and `config-parsing-robustness` adds its warning summary through it; whichever lands second merges the two into this setter.
- **`DriveListReady` replaces whatever overlay is in the slot.** This is unreachable today: the reply is synchronous and drained before the next input. The rule makes drive select the winner, the same winner today's key router gives in `the_drive_dialog_outranks_the_menu`.
- **Stale replies never touch another overlay.**
  - `FindFileMatch` and `FindFileSearchDone` are claimed only while the slot holds `FindFile`. Otherwise they fall to the base layer and are dropped, as today.
  - `DriveLabelResolved` updates the dialog only if the slot holds `DriveSelect` with the matching target and generation.
  - `UserMenuReloaded` with a malformed file opens the startup warning through `set_startup_warning`. The slot is empty there: the reload follows the return of the built-in or external editor, which was opened from the F9 menu after the menu closed, and no input is read in between.
- **`ViewerOpened` and `EditorOpened` keep the rule current when this change lands.** Today they change the phase from the base layer and leave the slot alone; an open overlay there is unreachable. `offload-blocking-io`, which lands first (proposal: Coordination), makes them asynchronous and discards a reply that arrives after a menu, dialog, or overlay has opened. With the slot, its overlay test becomes `state.overlay.is_some()`; this change does not alter the rule.

### D6: Tests

- **Core tests** (`update/tests/*.rs`) set overlays through `Overlay` constructors or `open_overlay`, and read them through the typed accessors. Assertions are unchanged.
- **New reducer tests:**
  - opening each overlay while another is in the slot replaces it only where a current path does (menu activation, user-menu Themes, `DriveListReady`), and is otherwise unreachable, which the debug assertion checks;
  - a bound command for a closed overlay is a no-op that does not panic;
  - late find-file results leave a different open overlay untouched.
- **Rewritten tests.** The two key-router tests that build two-overlay states are rewritten:
  - `the_drive_dialog_outranks_the_menu` becomes a reducer test: `DriveListReady` with the menu open leaves `Overlay::DriveSelect` in the slot.
  - `startup_warning_outranks_every_other_modal_and_typing_sink` keeps its typing-sink half (a quick search and a command line underneath) and drops the menu.
- **New precedence test.** `input_layer` is checked for each layer combination that can occur, and the key router, mouse context, and render dispatch agree with it.
- **Snapshots.** Every insta snapshot must pass unchanged. A changed snapshot is a regression in this change, not an update to accept.

### D7: Reducer and test split

The change starts with **pure moves**, kept as their own commits: functions move between files with no body changes. Then come the logic commits, one overlay at a time (D9).

| File | Contents |
|---|---|
| `update.rs` | module root: `State`, `UiPhase`, `PanelSide`, `update`, `update_impl` (routing only), `input_layer`, `drag_allowed`, re-exports |
| `update/command.rs` | `Command`, `Effect`, `ButtonId`, `ClickMods`, `DropTarget`, `DragState`, and the other reducer vocabulary types |
| `update/listing.rs` | `begin_listing`, `begin_listing_inner`, `apply_listing_event`, `git_info_query_effect`, `reconcile_panel_viewport`, and the viewport helpers |
| `update/file_op.rs` | `handle_file_op`, setup and running, `confirm_destination_input`, the clipboard export handlers, the source and selection helpers, the drag-and-drop helpers |
| `update/viewer.rs` | `handle_request_viewer`, `handle_viewer` |
| `update/editor.rs` | `handle_request_editor`, `handle_request_external_editor`, `handle_editor`, `editor_viewport`, `handle_edit_user_menu_file` |
| `update/overlays.rs` | `Overlay`, `OverlayStep`, `claims`, and each overlay's handler, plus `menu_action_command`, `button_command`, and `keybar_command` |
| `update/panels.rs` | the `Panels` arm as `handle_panels`, the command line (`run_command_line`, `dispatch_cd`, the parsers), tree mode, tabs, split, quick search, quick filter, swap |

`parse_cd`, `parse_del`, `parse_rmdir`, `resolve_cd_target`, `menu_action_command`, `button_command`, and `panel_viewport_rows` stay reachable at their current `filecommand_core::update::…` paths through `pub use`. `lib.rs`'s re-exports are unchanged.

`update/tests.rs` becomes `update/tests/mod.rs` (shared helpers) plus one file per module above, moved verbatim except for the overlay accessor changes in D6.

### D8: `DialogLayout`, and render plus hit map in one dispatch

Each clickable dialog and menu module exposes `pub fn layout(area: Rect, model: &…) -> DialogLayout`:

```rust
pub struct DialogLayout {
    pub rect: Rect,
    pub buttons: Vec<(Rect, ButtonId)>,
    pub rows: Vec<(Rect, usize)>,
    // plus the precomputed content strings the renderer needs (e.g. the
    // delete question), so they are built exactly once
}
```

Its renderer draws from that layout. Its old `hit_buttons` or `hit_items` becomes `layout(area, model).buttons` (or `.rows`), or is deleted.

The modules are `delete_confirm`, `destination_input`, `quit_dialog`, `progress_dialog`, `conflict_dialog`, `error_dialog`, `skipped_summary`, `file_action_menu`, and `menubar`. `menubar` has two layouts: the title bar, and the open pull-down, which uses responsive-layout's shift-left rule.

`views::render` fills the `HitMap` in the same per-layer dispatch that draws: each arm computes the layout once, draws, and records its hits. This replaces `build_hitmap`'s separate dispatch. The panel, tab-strip, key-bar, and command-line regions keep coming from `layout::compute`.

During a dialog's opening iris animation, hits keep using the final layout, as they do today. The geometry rule itself (`overlay_rect`) is unchanged.

*Alternative considered: renderers record hits while drawing (`&mut HitMap` threaded through every `render_*`).* It changes every renderer signature, including the ones snapshot tests call directly, and ties drawing to input bookkeeping. **Rejected** in favor of a layout value that both sides consume.

### D9: Incremental migration

1. Pure-move commits for the module split (D7).
2. Introduce `Overlay`, the slot, `input_layer`, and the accessors, with the slot initially unused.
3. Migrate one overlay per commit, in roughly increasing order of consumers: startup warning, help, theme picker, history dialog, user menu, drive select, fuzzy jump, find file, file-action menu, and finally menu (stamps, render, hit map, mouse). Each commit moves the field into the slot, its handler to `OverlayStep`, its predicate into `claims`, and its TUI readers onto accessors.
4. While both old fields and the slot exist, `update` debug-asserts after each call that at most one of them is set.
5. Switch the TUI routing (`map_key`, mouse `context`, `app.rs` dispatch) to `input_layer`.
6. Introduce `DialogLayout` one dialog at a time, then merge `build_hitmap` into `render`.

The full test suite and the snapshots must pass after every commit.

## Risks / Trade-offs

- [This is a large diff that touches the most-edited files in the repository.] → Land it after the Critical and High audit fixes (proposal: Coordination). Pure moves are separate commits. One overlay per commit keeps every step reviewable and bisectable.
- [A hidden reachable two-overlay state exists that the analysis missed.] → The debug assertion in `open_overlay` and in the transition-period `update` check (D9) turns it into a test failure. The full suite runs at every step.
- [Rendering order changes for the startup warning relative to the quit dialog.] → The two cannot both be open: the warning's mapper turns any key into a dismissal, and the mouse is ignored. No snapshot has both.
- [`Replace` semantics for `DriveListReady` is new code for an unreachable case.] → It is pinned by a reducer test and matches today's key-router winner.
- [The accessor layer adds indirection.] → It is thin `Option<&T>` getters. Handlers themselves use the taken state directly (D3).

## Migration Plan

None for users. Developers: code that set `state.<overlay> = Some(..)` uses `state.open_overlay(Overlay::…(..))`, and readers use the typed accessors. The `filecommand_core::update::…` public paths are preserved through re-exports.

## Open Questions

None.
