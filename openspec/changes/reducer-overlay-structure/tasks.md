# Tasks: reducer-overlay-structure

Land after `resize-preserves-session-state`, `listing-request-guard`, `process-lifecycle-hardening`, and `offload-blocking-io` (proposal: Coordination). Every step below must leave `cargo test --workspace` green with **no insta snapshot changes**. A snapshot diff is a regression to fix, not one to accept.

## 1. Pure-move module split (design D7)

- [ ] 1.1 Move `Command`, `Effect`, `ButtonId`, `ClickMods`, `DropTarget`, `DragState`, and the other reducer vocabulary types from `update.rs` to `update/command.rs`, re-exported from `update.rs` so every `filecommand_core::update::…` and `filecommand_core::…` path still resolves. Commit with no body changes
- [ ] 1.2 Move handlers verbatim into `update/listing.rs`, `update/file_op.rs`, `update/viewer.rs`, `update/editor.rs`, `update/overlays.rs`, and `update/panels.rs` per the D7 table. Extract the `Panels` arm of `update_impl` into `panels::handle_panels`. Keep the `pub` helpers (`parse_cd`, `parse_del`, `parse_rmdir`, `resolve_cd_target`, `menu_action_command`, `button_command`, `panel_viewport_rows`) re-exported at their current paths. Commit with no body changes
- [ ] 1.3 Split `update/tests.rs` into `update/tests/mod.rs` (shared helpers) and one file per module above, moving test functions verbatim. Commit with no body changes
- [ ] 1.4 Confirm that `cargo test --workspace` passes and that the test count is unchanged from before 1.1

## 2. Overlay slot and precedence (design D1, D2)

- [ ] 2.1 Add `enum Overlay` (ten variants), `State::overlay: Option<Overlay>`, the typed read accessors, `open_overlay` (debug-asserting an empty slot), and `close_overlay`. Re-export `Overlay` from `lib.rs` (application-shell: "Modal overlays are mutually exclusive by construction")
- [ ] 2.2 Add `enum InputLayer` and `pub fn input_layer(&State) -> InputLayer` encoding: quit, then file-op phase, then viewer, then editor, then overlay, then panels or splash. Add a unit test per layer (application-shell: "One precedence order everywhere")
- [ ] 2.3 Add `enum OverlayStep { Keep, Close, Replace, CloseThen }` and the take, handle, restore routing step in `update_impl`, unused until 3.x (design D3)
- [ ] 2.4 During the transition, debug-assert after every `update` call that at most one of the not-yet-migrated `Option` fields and the slot is set (design D9)

## 3. Migrate one overlay per commit (design D3, D4, D9)

For each overlay:
- move the field into the slot;
- convert its handler to receive its own state and return an `OverlayStep`, which removes its `unwrap()`s and the `is_none()` re-check;
- move its `is_*_command` predicate into `Overlay::claims`;
- switch its openers to `open_overlay`;
- switch `stamps.rs`, `drag_allowed`, the TUI views, input, and mouse readers, and the tests to the accessors.

- [ ] 3.1 Startup warning (`DismissStartupWarning`, `apply_user_menu_reload`), keeping its render placement over any phase, splash included (design D8). Route the last-writer-wins assignments in `app::run` and `apply_user_menu` through `State::set_startup_warning` (replaces an existing warning, debug-asserts no other overlay), merged with `error-diagnostics`' logging helper of the same name if that change has landed, and add a test that two successive startup warnings leave the second in the slot (design D5)
- [ ] 3.2 Help window (`handle_help`, `HelpOpen`)
- [ ] 3.3 Theme picker (`handle_theme_picker`, `ThemePickerOpen`), keeping `State::render_theme`'s live preview working
- [ ] 3.4 History dialog (`handle_history_dialog`, `HistoryDialogOpen`)
- [ ] 3.5 User menu (`handle_user_menu`, `UserMenuOpen`). The Themes slot returns `OverlayStep::Replace(Overlay::ThemePicker(..))` (application-shell: "The user menu's Themes entry replaces the user menu")
- [ ] 3.6 Drive select (`handle_drive_select`, `DriveListReady`, `DriveLabelResolved`). `DriveListReady` replaces whatever overlay is in the slot, and `DriveLabelResolved` applies only to a `DriveSelect` in the slot with the matching target and generation (design D5)
- [ ] 3.7 Fuzzy jump (`handle_fuzzy_jump`, `FuzzyJumpOpen`)
- [ ] 3.8 Find file (`handle_find_file`, `FindFileOpen`). `FindFileMatch` and `FindFileSearchDone` are claimed only while the slot holds `FindFile` (application-shell: "Late results of a dismissed search touch no other overlay")
- [ ] 3.9 File-action menu (`handle_file_action_menu`, `handle_enter`, `handle_open_action_menu_at`)
- [ ] 3.10 F9 menu (`handle_menu`, `MenuOpen`, `MenuTitleClick`, `MenuItemClick`). Activation returns `OverlayStep::CloseThen(command)` (application-shell: "A menu item opens its dialog in place of the menu")
- [ ] 3.11 Remove the transition assertion (2.4) and the last per-overlay `Option` field. Confirm that `grep -n "unwrap()" crates/filecommand-core/src/update` finds none of the 25 overlay unwraps

## 4. TUI routing on one precedence order (design D2)

- [ ] 4.1 `input::map_key`: dispatch on `input_layer`, then on the slot's variant, replacing the field-by-field `if` chain (application-shell: "One precedence order everywhere")
- [ ] 4.2 `app.rs::run` key dispatch: choose `map_viewer_key`, `map_editor_key`, or `map_key` from `input_layer` instead of the `quit_confirm` check plus the phase `match`
- [ ] 4.3 `input::mouse::context`: derive the context from `input_layer` and the slot's variant, keeping today's per-overlay mouse policy (ignored, file-action rows, pull-down, dialog buttons) (mouse-input: "Mouse is honoured only where the key would be")
- [ ] 4.4 `views::render_phase` and `render`: draw the slot's overlay in one `match` at its current z-position (D8 placement per variant)

## 5. Shared dialog layout (design D8)

- [ ] 5.1 Add `DialogLayout { rect, buttons, rows, … }`. For each of `delete_confirm`, `destination_input`, `quit_dialog`, `progress_dialog`, `conflict_dialog`, `error_dialog`, `skipped_summary`, and `file_action_menu`, add `pub fn layout(area, model) -> DialogLayout` and make the renderer draw from it. The old `hit_buttons`/`hit_items` become thin wrappers over `layout(..)`, or are removed (mouse-input: "Click targets match the rendered geometry")
- [ ] 5.2 `menubar`: title-bar and pull-down layouts shared by `render_*`, `hit_titles`, and `hit_items`, keeping the shift-left rule of responsive-layout "Unified overlay geometry"
- [ ] 5.3 Merge `build_hitmap` into `views::render`: each layer's arm computes its layout once, draws, and records its hits. Panel, tab-strip, key-bar, and command-line hits keep coming from `layout::compute`
- [ ] 5.4 Property test (proptest) over terminal sizes from 60x16 to 200x60 and representative models, including long, truncated names and each conflict and error variant. For every listed dialog and menu, render into a `Buffer` and assert that the cells of each hit rectangle are exactly the cells holding that button's label or that row (mouse-input: "Geometry agrees at every size")
- [ ] 5.5 Mouse tests: delete-confirm `(Y)es`/`(N)o` at 60x16 with a 200-character name; quit `Y`/`N` at 80x24 and 60x16; progress `[ Cancel ]` after a resize from 120x40 to 80x24; and clicks on non-button cells do nothing (mouse-input: "Delete confirmation buttons at the size floor", "Quit dialog hotkey cells", "Progress dialog Cancel follows a resize")

## 6. Reducer and router tests (design D6)

- [ ] 6.1 Menu activation of Commands → Find file leaves only `Overlay::FindFile` in the slot (application-shell: "A menu item opens its dialog in place of the menu")
- [ ] 6.2 Fuzzy jump open: F9 and Alt+F1 map to no command, and the slot and pattern are unchanged (application-shell: "Another overlay's shortcut is ignored while an overlay is open")
- [ ] 6.3 User menu open: an unbound printable key leaves the command line and cursor unchanged (application-shell: "Unbound keys never reach the panels or the command line")
- [ ] 6.4 Find file dismissed, then Help opened, then late `FindFileMatch` and `FindFileSearchDone` arrive: the Help state is unchanged and no find-file state appears (application-shell: "Late results of a dismissed search touch no other overlay")
- [ ] 6.5 Each overlay's confirm or move command applied with the slot empty is a no-op returning no effects (application-shell: "An overlay command with its overlay closed does nothing")
- [ ] 6.6 Quit confirmation over an open menu, then `CancelQuit`: the menu is still in the slot and the next key maps to a menu command (application-shell: "The quit dialog stacks above an open overlay")
- [ ] 6.6a With a running job's progress dialog and `quit_confirm` set, a `JobProgress` updates the progress and leaves `quit_confirm` set (application-shell: "Worker replies pass beneath the quit dialog"; design D2)
- [ ] 6.7 Rewrite `input/tests.rs` `the_drive_dialog_outranks_the_menu` as a reducer test (`DriveListReady` with the menu open leaves `Overlay::DriveSelect`), and drop the menu half of `startup_warning_outranks_every_other_modal_and_typing_sink`, keeping its typing-sink assertions (design D6)
- [ ] 6.8 A cross-check test that, for a set of states covering every `InputLayer` and every overlay variant, `map_key`, mouse `context`, and the render dispatch agree with `input_layer` (application-shell: "One precedence order everywhere")

## 7. Verify

- [ ] 7.1 `cargo test --workspace` green, with `cargo insta test` reporting no changed snapshots
- [ ] 7.2 `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] 7.3 `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` clean (the split moves items that rustdoc links point at)
- [ ] 7.4 Manual smoke test on Windows: open and close each overlay by keyboard and mouse, activate menu items that open dialogs, use the user menu → Themes, and click every dialog button at 60x16 and at a large size
- [ ] 7.5 `openspec validate reducer-overlay-structure --strict` clean
