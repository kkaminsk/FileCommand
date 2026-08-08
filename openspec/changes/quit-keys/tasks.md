# Tasks: quit-keys

## 1. Quit-confirmation overlay

- [x] 1.1 Promote the quit confirmation from `UiPhase::QuitConfirm` to an overlay in `filecommand-core` (state field + handled-first routing in `update.rs`, M5-overlay pattern), preserving existing confirm/cancel semantics and updating tests that assert the old phase shape (application-shell: "Quit request keys and confirmation")
- [x] 1.2 Route `RequestQuit` from viewer, open-menu, and modal-dialog/overlay contexts — not just Panels — and render the dialog topmost above each (application-shell: "Quit request keys and confirmation")
- [x] 1.3 Verify cancel restores the prior context exactly: viewer still open, menu/dialog still open, command-line text, quick filter, and type-ahead state intact (application-shell: "Quit request keys and confirmation")

## 2. Key triggers

- [x] 2.1 Map Esc at panel level to `RequestQuit` unconditionally (replacing `CommandLineClear`-on-Esc, quick-filter-exit-on-Esc, and type-ahead-exit-on-Esc) in `filecommand-tui/src/input/mod.rs` (application-shell: "Quit request keys and confirmation")
- [x] 2.2 Map Ctrl+C to `RequestQuit` from panels, viewer, menus, and modal dialogs/overlays, excluding the built-in editor where Ctrl+C remains Copy (application-shell: "Quit request keys and confirmation")
- [x] 2.3 In the quit dialog, keep Esc as cancel and add Ctrl+C as confirm (application-shell: "Quit request keys and confirmation")
- [x] 2.4 Confirming quit while a file operation is running aborts the job via the existing cancel path before `Effect::Quit` (application-shell: "Quit request keys and confirmation")

## 3. Displaced-role fallbacks

- [x] 3.1 Remove Esc-clears-buffer from the command line; backspacing to empty releases Up/Down to the panel (command-line: "Command history navigation")
- [x] 3.2 Make the quick-filter activation key a toggle that exits and clears an active filter (quick-filter: "Clearing the quick filter")
- [x] 3.3 Remove Esc from type-ahead's exit keys; movement keys remain the exit (type-ahead-jump: "Exiting type-ahead and restoring command-line routing")

## 4. Tests

- [x] 4.1 Reducer tests: Esc from idle panels, mid-command-line, under quick filter, and during type-ahead all open the dialog; cancel restores each context bit-for-bit (application-shell: "Quit request keys and confirmation")
- [x] 4.2 Reducer tests: Ctrl+C from panels/viewer/menu/dialog opens the dialog; Ctrl+C in the editor still copies; Ctrl+C-Ctrl+C quits; Esc in the dialog cancels (application-shell: "Quit request keys and confirmation")
- [x] 4.3 Reducer tests: quit-confirm during a running job aborts via cancel semantics before quitting (application-shell: "Quit request keys and confirmation")
- [x] 4.4 Update existing tests asserting Esc-clears-line / Esc-exits-filter / Esc-exits-type-ahead / `UiPhase::QuitConfirm` to the new behavior; add toggle-off and backspace-release tests (command-line: "Command history navigation"; quick-filter: "Clearing the quick filter"; type-ahead-jump: "Exiting type-ahead and restoring command-line routing")
- [x] 4.5 `insta` snapshot: quit dialog rendered above the viewer and above an open pull-down (application-shell: "Quit request keys and confirmation")
