# Change: resize-preserves-session-state

## Why

Audit finding **R-03 (Critical)**: shrinking the terminal below the 60x16 floor throws away whatever screen was active. `Command::Resize` maps every phase to `UiPhase::Placeholder` (`(_, true) => UiPhase::Placeholder`, `update.rs:1202-1216`) and maps `Placeholder` back to `Panels` when the terminal grows again. `Editor`, `Viewer`, and `FileOpRunning` hold their state inside the phase, so that state is dropped:

- **Editor**: unsaved edits are lost with no prompt.
- **Running job**: later `JobProgress`, `JobConflict`, `JobError`, and `JobDone` hit the `other => state.phase = other` arm of `handle_file_op` (`update.rs:3084`) and are dropped. A job that then needs a conflict or error answer blocks forever in `reply_rx.recv()`. The panels are never re-read when it completes. A second F5 replaces `rt.active_job`, which drops the old reply sender, so the orphaned job resumes with default answers while the new job runs. Confirming quit from the placeholder emits no `CancelJob`, because `ConfirmQuit` only cancels when the phase is `FileOpRunning`.
- **Viewer**: `rt.viewer_source` is cleared only on `ViewerClose`, which can no longer be reached, so the memory map stays open for the rest of the session. On Windows, this stops other programs from truncating the file.

A Windows Terminal pane split, a window snap, or a font-size change is enough to trigger any of these. Work is lost silently, and a file operation can hang where the user cannot see it.

## What Changes

- "Terminal too small" becomes a flag beside the phase (`State::undersized`), no longer a phase. The placeholder is drawn over the active screen without discarding it. The panels, built-in editor, viewer, file-operation dialogs, running job, end-of-job summary, and open menus, dialogs, and overlays all persist while the terminal is undersized. They reappear unchanged when it is enlarged to at least 60x16.
- The startup splash is still dismissed when the terminal shrinks below the floor, and it does not return. A start below the floor still skips the splash.
- Worker results are still applied while the terminal is undersized: listing events, job progress, conflict, error, and completion, and replies to requests made before the shrink. A job that finishes while the placeholder shows still re-reads the affected panels. A conflict or error raised while undersized waits for an answer, and its dialog appears when the terminal is enlarged.
- Input never reaches the hidden screen. Mouse input is ignored, apart from the quit-confirmation dialog's buttons, as today. F10 and Esc open the quit-confirmation dialog over the placeholder, as they do today, except when the built-in editor is the hidden screen. There they are also ignored, so unsaved edits can only be discarded through the editor's own save-on-exit prompt. The quit-confirmation and startup-warning dialogs keep their keys, because both are drawn over the placeholder.
- `UiPhase::Placeholder` is removed. While undersized, rendering draws the placeholder (plus the quit or startup-warning dialog) in place of the phase.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `application-shell`: "Resize handling with 60x16 hard floor and placeholder". The placeholder preserves the active screen and its state, worker replies keep being applied, input routing while undersized is defined, and the splash exception is kept. All existing scenarios are kept verbatim, and new ones are added for each stateful screen.

## Impact

- `crates/filecommand-core/src/update.rs`:
  - `UiPhase` (`:151`): remove `Placeholder`.
  - `State`: new `undersized: bool`. `State::too_small` (`:403`) and `State::initial` (`:428`) change.
  - The `Command::Resize` arm of `update_impl` (`:1202-1216`) changes.
  - Remove the `UiPhase::Placeholder => {}` arm (`:1418`).
  - `drag_allowed` (`:1164`) must also refuse while undersized (mouse-drag "resize below the minimum" clears the drag).
  - `handle_editor` / `editor_viewport` (`:2234`) must not re-scroll against an undersized viewport.
  - The fall-through arm in `handle_file_op` (`:3066-3087`) stays, but only as a defensive guard.
- `crates/filecommand-tui/src/app.rs`:
  - The key dispatch in `run` (`:368`) must route through `input::map_key` while undersized, the same way it already does for `quit_confirm`.
  - Viewer/editor wheel handling in `apply_mouse_batch` (`:535`) is skipped while undersized.
  - `apply` (`:796-803`) adds a before/after check that releases `rt.viewer_source` when the phase leaves `Viewer` (or a `ViewerOpened` is discarded), not merely whenever the phase is not `Viewer`, because the synchronous open stores the source before the phase changes.
- `crates/filecommand-tui/src/input/mod.rs`: `map_key` (`:28`) gets an undersized arm, `map_undersized_key`. `input/mouse.rs` `context` (`:186`) returns `Context::Ignored` while undersized.
- `crates/filecommand-tui/src/views/mod.rs`: `render` and `render_phase` (`:264-307`, `:387-390`) draw the placeholder when `state.undersized` is set, whatever the phase. `build_hitmap` records nothing underneath it.
- Tests: regression tests for every stateful phase and overlay (shrink, then enlarge, then assert the phase is unchanged); job replies applied while undersized; input gating; the existing placeholder snapshots (`tests/snapshots.rs:138,154`, `tests/snapshot_views.rs:226`) re-expressed through the flag with byte-identical output.

**Audit findings:** R-03

**Coordination:**
- `reducer-overlay-structure` (B-02/B-08) restructures the phase and overlay model. Whichever change lands second must carry `undersized` into that model as a screen-level flag above every phase and overlay. R-03 is Critical, so this change should land first.
- `viewer-mapping-safety` (R-20) changes the `ByteSource` backend and the hidden-viewer mapping rule. This change only relies on the source being released when the viewer closes.
- `process-lifecycle-hardening` (R-11/R-18) changes the job lifecycle on quit and adds a conflict-dialog cancel. Its quit-time cancel must not depend on the core phase. Either order works.
