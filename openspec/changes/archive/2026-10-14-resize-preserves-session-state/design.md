# Design: resize-preserves-session-state

## Context

This is the code on `build/open-proposals-2026-10` (`1bada6f`).

- **Resize maps every phase to a placeholder phase.** `update_impl` handles `Command::Resize` before anything else (`update.rs:1202-1216`):

  ```rust
  state.phase = match (&state.phase, too_small) {
      (UiPhase::Placeholder, false) => UiPhase::Panels,
      (UiPhase::Placeholder, true) => UiPhase::Placeholder,
      (_, true) => UiPhase::Placeholder,
      (other, false) => other.clone(),
  };
  ```

  `UiPhase::Editor(EditorState)`, `Viewer(ViewerState)`, `FileOpSetup(..)`, `FileOpRunning { .. }` and `FileOpSummary(..)` own their state, so shrinking the terminal drops it. `State::initial` also starts in `Placeholder` when the first size is too small (`update.rs:428`), which skips the splash.
- **Job events are dropped while the placeholder shows.** They are routed to `handle_file_op` by command type, whatever the phase (`update.rs:1328-1333`). `handle_file_op` does `mem::replace(&mut state.phase, Panels)` and, for any phase other than the three file-op phases, falls into `other => state.phase = other` (`update.rs:3084`), which drops the event. This confirms the audit:
  - `JobConflict` and `JobError` are never shown, so the worker parks in `reply_rx.recv()` (`worker.rs:240`, `:250`).
  - `JobDone` never triggers the panel re-read.
  - After the user enlarges the terminal, the phase is `Panels`, so F5 starts a second job. `Effect::RunJob` overwrites `rt.active_job` (`app.rs:849`), which drops the first job's `reply_tx`. Its `recv()` then fails and it continues with `Skip` or `Abort`.
- **Quit from the placeholder does not cancel the job.** `ConfirmQuit` pushes `Effect::CancelJob` only when the phase is `FileOpRunning` (`update.rs:1302-1306`). From the placeholder, quit leaves the job thread running until the process exits.
- **The viewer's file stays open.** `rt.viewer_source` is set on `Effect::OpenViewer` and cleared only when `apply` sees `Command::ViewerClose` (`app.rs:801-803`). Once the phase has become `Placeholder`, `ViewerClose` can no longer be issued, so the `ByteSource` (an mmap on most files) lives for the rest of the session.
- **Input in the placeholder today.**
  - `run` dispatches keys through `input::map_key` unless the phase is `Viewer` or `Editor` (`app.rs:368-393`). `Placeholder` is not one of those, so `map_key` runs.
  - With no overlay open, `map_key` falls to `map_panel_key`. F10 and Esc produce `RequestQuit`, which `update_impl` handles globally (`update.rs:1293-1296`), so the quit-confirmation dialog opens. `render` draws it over the placeholder (`views/mod.rs:290-292`).
  - Every other panel command reaches `UiPhase::Placeholder => {}` (`update.rs:1418`) and is ignored.
  - With an overlay open (a menu, fuzzy jump, help, and so on — these fields survive a resize), `map_key` routes keys to that overlay's mapper, which the reducer applies to an overlay the user cannot see. This blind input is a latent hazard. It is not cited in the audit.
  - Mouse input is ignored: `input::mouse::context` returns `Context::Ignored` for `Placeholder` (`input/mouse.rs:186`). The one exception is the quit-confirmation dialog: `context` checks `quit_confirm` first and returns `Context::DialogButtons`, and `build_hitmap` records the quit buttons whatever the phase, so its Yes/No buttons are clickable over the placeholder today.
  - A drag in progress is cleared by the shrink, because `drag_allowed` requires `UiPhase::Panels`, so Esc never reaches `DragCancel` there.
- **Listing events and other replies.** Listing events, `DriveLabelResolved`, `InfoResolved`, `GitInfoResolved` and `TreeNodeExpanded` are already applied whatever the phase (`update.rs:1241-1287`). Replies that are handled inside the `Panels` arm, such as `EditorOpened`, `ViewerOpened`, `DriveListReady` and `ClipboardResult`, are swallowed by the `Placeholder` arm today.

## Goals / Non-Goals

**Goals:**

- No user-visible state is lost by shrinking and re-enlarging the terminal. The splash is the one documented exception.
- A job keeps running and reporting while the placeholder shows. A conflict or error waits for the user and does not resolve by default.
- Input cannot act on a screen the user cannot see.
- The viewer's file handle is held exactly as long as the viewer exists.

**Non-Goals:**

- Rendering anything below 60x16 other than the placeholder and the two top-level modals (quit confirmation and startup warning) that are already drawn over it.
- Changing the floor, the degraded band, or `responsive-layout`.
- Restructuring the phase and overlay model as a whole. That is `reducer-overlay-structure` (B-02/B-08).
- Changing the viewer backend or the mmap SAFETY argument. That is `viewer-mapping-safety` (R-20).
- Quit-time job cleanup, the conflict-dialog cancel, and worker panic handling. Those are `process-lifecycle-hardening` (R-10/R-11/R-12/R-18).

## Decisions

### D1: `State::undersized: bool` replaces `UiPhase::Placeholder`

Add a field `undersized` to `State` and remove the `Placeholder` variant. The `Command::Resize` arm becomes:

- `state.term_size = (w, h)`.
- `state.undersized = State::too_small((w, h))`.
- If the terminal is now undersized and the phase is `Splash`, set the phase to `Panels` (D2).
- Re-clamp both panel viewports as today.

No other phase is touched. With the phase intact, every existing handler keeps working on the hidden screen: `handle_file_op_running` for job events, `handle_editor` for `EditorSaved`/`EditorSaveFailed`, `handle_viewer` for `ViewerSearchResult`, and the `Panels` arm for `EditorOpened`/`ViewerOpened`/`DriveListReady`. The fall-through arm of `handle_file_op` stays as a defensive guard but is no longer reachable by a resize.

*Alternative considered:* `Placeholder { previous: Box<UiPhase> }`, which stashes the phase. This was rejected. Every handler that matches on the phase would have to look inside the box, including job events, editor saves and viewer search replies, which brings back the routing bug in a new form. The overlays that already live beside the phase (`menu`, `fuzzy_jump`, …) would still need separate handling.

*Alternative considered:* skip `update` for resizes in the TUI and keep a size flag there. This was rejected. The floor check, the splash exception and the drag-clearing rule belong in the pure reducer, where they are unit-testable with no terminal.

### D2: The splash exception and startup below the floor

Shrinking during `Splash` sets the phase to `Panels` and the flag to `true`. Enlarging only clears the flag, so the splash cannot return (startup-splash "Shrinking below the floor mid-splash"). `State::initial` with a too-small size starts with the phase `Panels` and the flag `true`, so the splash is skipped (startup-splash "Below the floor at startup"). Both behaviors are unchanged from today. Only their encoding changes.

### D3: Input gating lives in the TUI mapper

`input::map_key` gets a first-checked arm for `state.undersized`, after the `quit_confirm` and `startup_warning` checks, which keep their keys because both dialogs are drawn over the placeholder. That arm is `map_undersized_key`, which returns `Some(Command::RequestQuit)` for F10 and Esc and `None` for every other key.

- When the hidden phase is `Editor`, `map_undersized_key` returns `None` for F10 and Esc too. Otherwise a confirmed quit would discard unsaved edits without the editor's save-on-exit prompt. This matches full size, where the global quit dialog cannot be opened from inside the editor.
- In `app.rs`, the `run` key dispatch routes through `map_key` while `state.undersized` is set, ahead of the direct `map_viewer_key`/`map_editor_key` dispatch. This mirrors the existing `quit_confirm` precedence (`app.rs:368`).
- `apply_mouse_batch` skips the viewer/editor wheel arms while undersized and sends every event through `input::map_mouse`. In `input::mouse::context`, the undersized check goes after the existing `quit_confirm` check and returns `Context::Ignored`, so the quit dialog's buttons stay clickable exactly as today, and nothing else is.

Compared with today's placeholder, the input rules differ in exactly two ways, both deliberate:

- **Hidden overlay.** Today a key pressed while a menu, fuzzy jump, find file, help or another overlay is open is routed blindly to that overlay's mapper, which the reducer applies to an overlay the user cannot see. That stops. It was harmless only because nothing important survived the shrink; once the editor and dialogs survive, a blind Enter could confirm something.
- **Hidden editor.** F10 and Esc no longer open the quit dialog when the editor is the hidden screen. Today they do, but today the buffer is already gone by then.

Without an overlay and outside the editor, the rules are today's: F10 and Esc open the quit dialog, every other key does nothing, mouse input does nothing except the quit dialog's buttons, and the quit-confirmation and startup-warning dialogs keep their keys.

*Alternative considered:* gate in the reducer by ignoring key-derived commands while undersized. This was rejected. The reducer cannot tell a key-derived command from a reply of the same shape (`EditorSaved` and `JobConflict` must still be applied), and the TUI mapper already owns which keys reach which context.

### D4: Quit from the placeholder

`RequestQuit` and `ConfirmQuit` keep their global handling. Because the phase is preserved, `ConfirmQuit` during a hidden `FileOpRunning` now emits `Effect::CancelJob` as it does at full size. That fixes the quit-without-cancel path described in Context. Waiting for the cancellation to finish is `process-lifecycle-hardening`'s concern.

### D5: Rendering while undersized

`render_phase` draws `placeholder::render_placeholder` when `state.undersized` is set, whatever the phase, and returns no cursor. `render` still draws the quit-confirmation and startup-warning dialogs after it, unchanged. `build_hitmap` records nothing for the hidden phase while undersized; its existing phase-independent block still records the quit dialog's buttons when `quit_confirm` is set, which is the only part of the map consulted while undersized (D3). An otherwise empty map also keeps a stale panel hit map from outliving the shrink. The existing placeholder snapshots must stay byte-identical.

### D6: Drag state and editor scrolling while undersized

- `drag_allowed` adds `&& !state.undersized`. mouse-drag "Cancel and phase-change clear the drag" lists "resize below the minimum" as a trigger, and the phase no longer changes to signal it.
- `handle_editor` calls `editor.ensure_caret_visible(rows, cols)` from `editor_viewport(state.term_size)` on every command. `editor_viewport` already clamps to at least 1x1 (`update.rs:2234-2236`). Even so, a reply that arrives while undersized (`EditorSaved`/`EditorSaveFailed`) would re-scroll the buffer to fit a one- or two-row window. So `ensure_caret_visible` is skipped while undersized. On the first resize back above the floor, the `Resize` arm re-runs it for an `Editor` phase, so the caret is visible at the restored size. The buffer, caret and modified state are never touched by a resize.

### D7: The viewer's byte source follows the viewer phase

`apply` keeps the existing pre-`update` release on `ViewerClose`. It also records whether the phase was `Viewer` before `update` (the same before/after pattern it already uses for `editing_user_menu`) and drops `rt.viewer_source` when the phase was `Viewer` and no longer is. It additionally drops the source when a `ViewerOpened` was applied but the phase did not become `Viewer` (the reply was discarded).

The release must be keyed to the transition, not to "the phase is not `Viewer`". Today `Effect::OpenViewer` stores the source in `rt.viewer_source` inside `run_effects` while the phase is still `Panels`, and the phase only becomes `Viewer` when the `ViewerOpened` it queued is drained later. Any command applied in between, such as a listing chunk already queued ahead of it, would see a non-`Viewer` phase and drop the freshly opened source, leaving the viewer to open with no source.

With D1, the viewer phase survives a shrink, so the source is kept while the placeholder shows (the viewer must reappear at the same position) and is released when the viewer closes. The transition check makes "the handle is released when the viewer phase ends" an invariant, so a future phase transition cannot leak the mapping as `Placeholder` did. `viewer-mapping-safety` may change what `ByteSource` holds (an mmap or positioned reads). `offload-blocking-io` moves the open to a worker and installs the source only when `ViewerOpened` turns the phase into `Viewer`. This decision depends on neither.

## Risks / Trade-offs

- [The placeholder hides a conflict dialog the job is waiting on.] → The job waits, as at full size with the user away. Answering requires enlarging the terminal. Quitting from the placeholder cancels the job (D4). This is preferable to resolving conflicts by default without the user's answer.
- [`UiPhase::Placeholder` is used in tests and snapshot fixtures.] → Those fixtures move to `State { undersized: true, .. }`. The snapshot output is unchanged by construction (D5).
- [A future handler might read `term_size` and lay out at a sub-floor size.] → Rendering never runs a phase view while undersized (D5). Reducer-side viewport math clamps to at least 1, as `panel_viewport_rows` and `editor_viewport` already do.
- [Overlap with `reducer-overlay-structure`.] → That change must model `undersized` as a screen-level flag above all phases and overlays, not as an overlay that can be dismissed.

## Migration Plan

None. The flag lives only in memory, and no persisted format changes.

## Open Questions

None.
