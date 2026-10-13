# Tasks: resize-preserves-session-state

## 1. Core state and reducer (`crates/filecommand-core/src/update.rs`)

- [ ] 1.1 Add `pub undersized: bool` to `State` (`false` in `State::empty`). Remove `UiPhase::Placeholder` and its doc comment, and fix every match on `UiPhase` in both crates (design D1).
- [ ] 1.2 `State::initial`: when `too_small(term_size)`, start with `phase = Panels` and `undersized = true`, so the splash is skipped (design D2; startup-splash "Below the floor at startup").
- [ ] 1.3 `Command::Resize` arm: set `term_size` and `undersized`. Turn `Splash` into `Panels` only when shrinking below the floor, and leave every other phase untouched. Keep the two `reconcile_panel_viewport` calls. When the terminal is enlarged out of the undersized state and the phase is `Editor`, run `ensure_caret_visible` against the restored viewport (application-shell "Resize handling with 60x16 hard floor and placeholder"; design D1, D2, D6).
- [ ] 1.4 Delete the `UiPhase::Placeholder => {}` arm of the phase match. Keep the fall-through arm of `handle_file_op`, and update its comment to say it is defensive only (design D1).
- [ ] 1.5 `drag_allowed`: add `&& !state.undersized` (design D6; mouse-drag "Cancel and phase-change clear the drag").
- [ ] 1.6 `handle_editor`: skip `ensure_caret_visible` while `state.undersized` (design D6).

## 2. TUI input (`crates/filecommand-tui/src/input/`, `app.rs`)

- [ ] 2.1 `input::map_key`: after the `quit_confirm` and `startup_warning` checks, return `map_undersized_key(key, state)` when `state.undersized`. F10 and Esc map to `Command::RequestQuit` unless the phase is `Editor`. Everything else is `None` (design D3).
- [ ] 2.2 `app::run` key dispatch: route through `input::map_key` when `state.quit_confirm || state.undersized`, ahead of the direct `map_viewer_key`/`map_editor_key` dispatch (design D3).
- [ ] 2.3 `apply_mouse_batch`: when `state.undersized`, skip the viewer and editor wheel arms and route every event through `input::map_mouse`. `input::mouse::context` returns `Context::Ignored` when `state.undersized`, checked after the existing `quit_confirm` check so the quit dialog's buttons stay clickable (design D3).

## 3. TUI rendering and runtime (`crates/filecommand-tui/src/views/mod.rs`, `app.rs`)

- [ ] 3.1 `render_phase`: when `state.undersized`, draw `placeholder::render_placeholder` and return no cursor, whatever the phase. `render` keeps drawing the quit-confirmation and startup-warning dialogs over it (design D5).
- [ ] 3.2 `build_hitmap`: when `state.undersized`, record nothing for the phase; the existing quit-dialog block still records its buttons when `quit_confirm` is set (design D5).
- [ ] 3.3 `apply`: keep the existing pre-`update` `ViewerClose` release. Record whether the phase was `Viewer` before `update`, and set `rt.viewer_source = None` when it was and no longer is, or when the command was `ViewerOpened` and the phase did not become `Viewer`. Never drop the source merely because the phase is not `Viewer` (design D7).
- [ ] 3.4 TUI test for 3.3: run `Effect::OpenViewer`, apply an unrelated command (for example a `ListingChunk`) before the queued `ViewerOpened`, then apply `ViewerOpened`; `rt.viewer_source` is still set. Then shrink, enlarge and close the viewer; the source is `None` after the close and not before.

## 4. Regression tests: state survives a shrink (core, `update/tests.rs`)

- [ ] 4.1 A helper that applies `Resize(50, 12)` and then `Resize(80, 24)` and asserts that `state.phase` and every overlay field are equal before and after, with `undersized` set in between.
- [ ] 4.2 Run 4.1 for each stateful phase:
  - `Editor` with a modified buffer, caret on line 12, and overwrite mode (also assert `is_modified()` and the caret);
  - `Viewer` in hex mode with a non-zero top offset;
  - `FileOpSetup::DestinationInput` with typed text;
  - `FileOpSetup` delete confirmation;
  - `FileOpRunning` with each of the `Progress`, `Conflict` (with and without `rename_input`) and `Error` dialogs;
  - `FileOpSummary`.
- [ ] 4.3 Run 4.1 over `Panels` with each overlay open: F9 menu, drive select, fuzzy jump, find file, user menu, history dialog, theme picker, file-action menu, help, quick filter, type-ahead, and command-line text.
- [ ] 4.4 Splash: `Resize` below the floor during `Splash` gives `Panels` with `undersized` set, and enlarging stays in `Panels`. `State::initial` with a too-small size gives `Panels` with `undersized` set (startup-splash scenarios unchanged).
- [ ] 4.5 Job replies are applied while undersized:
  - `JobProgress` updates the hidden `Progress` dialog.
  - `JobConflict` and `JobError` switch the hidden dialog.
  - `JobDone` emits `StartListing` for the matching panels and moves to `Panels` or `FileOpSummary`, all with `undersized == true`.
- [ ] 4.6 `EditorSaved`, `ViewerOpened` and `EditorOpened` arriving while undersized are applied to the hidden screen.
- [ ] 4.7 `ConfirmQuit` while undersized over `FileOpRunning` emits `[CancelJob, Quit]`. Before this change it emitted only `Quit`.
- [ ] 4.8 A drag in progress is cleared by `Resize` below the floor.

## 5. TUI tests

- [ ] 5.1 `input` tests:
  - while undersized over `Editor`, letters, Enter, F2, F10 and Esc all map to `None`;
  - over `Panels` and over `Viewer`, F10 and Esc map to `RequestQuit` and other keys to `None`;
  - with `quit_confirm` set, its keys still map;
  - with `startup_warning` set, dismiss still maps.
- [ ] 5.2 `input::mouse` tests: `context` returns `Ignored` while undersized for every phase, and `DialogButtons` while undersized with `quit_confirm` set; a click on the quit dialog's No button over the placeholder maps to `DialogButtonClick(ButtonId::QuitNo)`. Update `input/mouse/tests.rs:593`.
- [ ] 5.3 Snapshots: rewrite the placeholder fixtures in `tests/snapshots.rs:138,154` and `tests/snapshot_views.rs:226` as `State { undersized: true, .. }`. Their accepted snapshots must stay byte-identical. Add a snapshot of the placeholder drawn while the phase is `Editor`, which must be identical to the panels case, and one of the quit dialog over the placeholder.

## 6. Verification

- [ ] 6.1 `cargo test --workspace` passes.
- [ ] 6.2 `cargo clippy --workspace --all-targets` reports no new warnings.
- [ ] 6.3 `openspec validate resize-preserves-session-state --strict` is clean.
- [ ] 6.4 Manual check on Windows Terminal:
  - Open the editor, type without saving, split the pane until the placeholder shows, then close the split. The edits are still there.
  - Start a large copy, shrink below the floor, wait for it to finish, then enlarge. The panels show the copied files.
  - Open F3, shrink, enlarge, then press Esc. The file can then be deleted by another program.
