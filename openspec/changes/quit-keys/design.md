# Design: quit-keys

## Context

Quit today: F10 (`crates/filecommand-tui/src/input/mod.rs`) or Files → Quit (`crates/filecommand-core/src/update.rs`, menu dispatch) emit `Command::RequestQuit`; the `UiPhase::Panels` arm handles it by entering `UiPhase::QuitConfirm`, a phase that *replaces* the panels; `ConfirmQuit` pushes `Effect::Quit`, `CancelQuit` returns to `UiPhase::Panels`, and the dialog maps `n`/`N`/`Esc` to cancel. Esc elsewhere is the universal dismisser: it clears a non-empty command line, exits quick filter and type-ahead, collapses menus, cancels every modal dialog, and closes the viewer. Ctrl+C is mapped only inside the built-in editor, as Copy.

## Goals / Non-Goals

**Goals:**

- Esc and Ctrl+C become first-class quit triggers routed through the existing confirmation dialog.
- Esc at panel level is unconditional (user decision) — its displaced roles get non-Esc fallbacks.
- Ctrl+C works from panels, viewer, menus, and dialogs; only the built-in editor (Ctrl+C = Copy) is excluded.
- Cancelling the dialog is always a perfect no-op: every bit of prior context survives.

**Non-Goals:**

- No instant/unconfirmed quit path; every trigger goes through the dialog.
- No change to the editor's own F10 quit/save-prompt flow or its Ctrl+C Copy binding.
- No change to Esc's cancel/dismiss role inside menus, dialogs, the viewer, or editor sub-prompts.
- No new configuration surface; the bindings are fixed (quick-filter key remains remappable as before).

## Decisions

### D1: Esc at panel level always requests quit

Whenever the panels own input (no pull-down menu, no modal dialog or overlay open), Esc opens the quit-confirmation dialog — even mid-command-line-edit, under an active quick filter, or during type-ahead. Rationale: one unconditional, discoverable meaning for Esc over panels, per explicit user decision. Alternative considered — a priority model where Esc first clears/exits and only a "bare" Esc asks to quit — rejected by the user.

### D2: Displaced Esc roles get natural fallbacks

Backspacing the buffer to empty replaces Esc as the mechanism that hands Up/Down back to the panel cursor (Backspace already deletes; no new binding). The quick-filter activation key (Ctrl+P by default, remappable per its existing "Overridable binding" requirement) becomes a toggle: pressing it with a filter active exits and clears the filter. Type-ahead needs nothing new — movement keys already exit it. Rationale: zero invented bindings; each fallback is the most adjacent existing affordance.

### D3: Ctrl+C is global except the built-in editor

Ctrl+C opens the quit-confirmation dialog from panels (any command-line state), the viewer, open pull-down menus, and every modal dialog/overlay — including the file-operation dialogs while a job runs. Confirming a quit while a job is running aborts the job through the existing cancel machinery (stop at the current item boundary) before `Effect::Quit`. The built-in editor is the sole exclusion: Ctrl+C remains Copy there, and the editor keeps its own F10 quit/save flow. Rationale: terminal interrupt muscle-memory, per explicit user decision; the editor conflict is real and the editor already has a guarded quit path.

### D4: In the dialog, Esc cancels and Ctrl+C confirms

Esc keeps its universal dialog-cancel meaning (Esc-Esc nets out to a no-op). Ctrl+C pressed again confirms — the terminal "press Ctrl+C again to exit" convention, making Ctrl+C-Ctrl+C a two-keystroke exit from anywhere outside the editor. Existing confirm/cancel keys are unchanged. Alternative — both keys cancel — rejected as breaking the Ctrl+C idiom; both confirming rejected as making habitual double-Esc destructive.

### D5: The quit confirmation becomes an overlay

`UiPhase::QuitConfirm` currently replaces the panels, which cannot represent "quit dialog above the viewer" or "above an open menu/dialog". The confirmation moves to an overlay (drawn topmost, handled first in `core::update`, same pattern as the M5 overlay dialogs) so it can open above any permitted context and cancel restores that context untouched. Existing panel-context behavior, including its snapshot, is preserved; tests asserting the phase shape are migrated.

### D6: The design doc's Esc statements diverge

`docs/superpowers/specs/2026-08-06-filecommand-design.md` states in §4.1 that "Esc clears the command line to return Up/Down to the panel", and in §4.7 that Esc clears the quick filter and exits type-ahead. Those statements become historical; the OpenSpec specs govern and the frozen design doc is not edited (same treatment as enter-file-action-menu's D6).

## Risks / Trade-offs

- [Muscle-memory break: Esc-to-clear-line/filter now opens a dialog] → cancel is a perfect no-op with all state intact; the change is flagged **BREAKING** in the proposal and the fallbacks are one keystroke away.
- [Accidental exits via Ctrl+C-Ctrl+C] → accepted deliberately by the user; the first press always shows the dialog, so a single stray Ctrl+C never quits.
- [Overlay refactor churn: `UiPhase::QuitConfirm` is load-bearing in tests] → migrate mechanically; the dialog view itself is unchanged, so snapshots should hold except for new-context variants.
- [Quit during a running job] → reuses the job-cancel machinery; no new cancellation semantics are introduced.

## Open Questions

- None. Esc scope, Ctrl+C scope, fallbacks, and in-dialog semantics were all settled with the user before authoring.
