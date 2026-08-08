# Change: quit-keys

## Why

Quitting FileCommand today requires F10 or Files → Quit. Esc — the key every terminal user reaches for first — is a dead key over idle panels, and Ctrl+C, the universal terminal "get me out" chord, does nothing outside the built-in editor. Wiring both into the existing quit-confirmation flow makes exiting discoverable and fast while the confirmation dialog keeps it safe.

## What Changes

- **BREAKING** (behavior): **Esc at panel level always opens the quit-confirmation dialog** — regardless of command-line content, an active quick filter, or type-ahead mode. Esc no longer clears the command line, exits the quick filter, or exits type-ahead; cancelling the dialog returns with all of that state intact. Esc keeps its cancel/dismiss role inside pull-down menus, modal dialogs and overlays, the viewer (Esc still closes the viewer), and editor sub-prompts.
- Displaced Esc roles get natural fallbacks: **Backspace to empty** is now the mechanism that releases Up/Down from history navigation back to the panel cursor; **the quick-filter activation key (Ctrl+P by default) becomes a toggle** that also exits an active filter; type-ahead continues to exit on any movement key (already specified).
- **Ctrl+C opens the quit-confirmation dialog from every context except the built-in editor** (where it remains Copy): panels in any state, the viewer, open pull-down menus, and modal dialogs/overlays — including while a file operation is running, in which case confirming the quit aborts the job through its existing cancel semantics before exiting.
- **Inside the quit-confirmation dialog**: Esc cancels (unchanged), and **Ctrl+C confirms** — pressing Ctrl+C twice from anywhere (outside the editor) exits the application.
- The quit-confirmation dialog becomes reachable from non-panel contexts, and **cancelling it restores the prior context exactly** — open viewer stays open, open dialog stays open, typed command-line text and active filters survive.

## Capabilities

### Modified Capabilities

- `application-shell`: Adds a "Quit request keys and confirmation" requirement codifying the previously unspecified F10 / Files → Quit confirmation flow and extending it with the Esc and Ctrl+C triggers, the in-dialog key semantics, cancel-restores-context, and quit-during-file-operation behavior.
- `command-line`: The "Command history navigation" requirement changes — Esc no longer clears the command-line buffer; backspacing to empty is the mechanism that hands Up/Down back to the panel.
- `quick-filter`: The "Clearing the quick filter" requirement changes — the activation key toggles the filter off; Esc no longer exits the filter.
- `type-ahead-jump`: The "Exiting type-ahead and restoring command-line routing" requirement changes — movement keys are the exit; Esc no longer exits type-ahead.

## Impact

- **Crates:** `filecommand-core` — promote the quit-confirmation from `UiPhase::QuitConfirm` to an overlay usable above panels, viewer, menus, and dialogs in `update.rs`; route `RequestQuit` from those contexts; abort-running-job-on-confirm wiring. `filecommand-tui` — key-mapping changes in `input/mod.rs` (panel-level Esc, global-except-editor Ctrl+C, in-dialog Ctrl+C-confirms), quit-dialog rendering above non-panel contexts.
- **Tests:** existing reducer/input tests asserting Esc-clears-command-line, Esc-exits-filter, Esc-exits-type-ahead, and the `UiPhase::QuitConfirm` phase shape must be updated to the new behavior.
- **Depends on:** M1 (`application-shell` quit flow, quit-confirm dialog), M3 (`command-line`), M5 (`quick-filter`, `type-ahead-jump`).
- The design doc's Esc statements (§4.1 "Esc clears the command line", §4.7 quick-filter/type-ahead Esc exits) become historical on this point; OpenSpec specs govern and the design doc is not edited by this change.
