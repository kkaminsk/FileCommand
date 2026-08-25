# Design: mouse-panel-drag

## Context

`mouse-basics` supplies the hit map, `MouseTracker`, and the semantic-command boundary between the TUI and core. Copy/Move start through `enter_file_op_setup_for_sources` (`crates/filecommand-core/src/update.rs`), which reads `state.active` for the source side and pre-fills the dialog with the opposite panel's path. crossterm reports modifier flags on `Drag` and `Up` events on both the Windows console backend and SGR terminals, but emulators intercept Shift (native selection) and sometimes Ctrl, and a bare modifier press produces no event at all. NC 5's drop opened the Copy dialog; Total Commander and Far default to Copy; Explorer defaults to Move on the same volume but to Copy across volumes — the common two-panel case.

## Goals / Non-Goals

**Goals:**

- Drag between panels feels like NC/TC.
- An accidental drop can never lose data: Copy is the default, and Move always passes through the dialog.
- All feedback is drawable in ANSI-16 / CP437 without compositing.
- The state machine is pure core and property-tested.

**Non-Goals:**

- A drag image following the pointer.
- Dragging within a panel to reorder (listings are sorted).
- Dragging to the command line (Ctrl+] / Ctrl+Enter already paste the path/name).
- Dragging out of or into the terminal window (not possible from a terminal application).
- Immediate (no-dialog) drops.

## Decisions

### D1: Plain drag is Copy; Move needs an explicit act

Copy is the non-destructive verb, the lineage default, and fails safe when a modifier is lost in transit. Move routes: Shift+drag where delivered, right-button drag, or `[ Move ]` in the drop dialog. Ctrl+drag = Copy (Explorer habit, harmless). Rejected: drag = Move (Explorer same-volume behaviour) — an accidental move is "where did my file go", and inverting Explorer's Ctrl is the one mapping that turns an intended copy into a move.

### D2: The verb is derived from the modifiers on each `Drag`/`Up` event

No key event exists for a bare modifier press, so the proposed verb is recomputed from `MouseEvent.modifiers` (and the button) on every drag event; the mini-status and key bar reflect the current value. Rejected: tracking Ctrl/Shift key events — not delivered on Windows.

### D3: Drop always opens the destination dialog, pre-filled with the exact drop path

Per the user's decision (no immediate mode). The dialog is the existing destination input (title, prompt, field — the keyboard F5/F6 dialog has no button row and is driven by Enter/Esc) plus a new button row `[ Copy ] [ Move ] [ Cancel ]` rendered with `button.normal`/`button.focused`, focused per D1/D2. The pre-fill is the drop target's path (`D:\BACKUP\OLD`, not the panel directory) so an overshoot onto a subdirectory row is visible before confirming. Downstream behaviour (overwrite conflicts, progress, error recovery, summary, re-read) is unchanged. `enter_file_op_setup_for_sources` is refactored to take `(source_side, prefill)` instead of reading `state.active`, so a drag from the inactive panel works. This is an ADDED `operation-dialogs` requirement rather than a modification of "Destination input dialog", so F5/F6 dialogs and their goldens are untouched.

### D4: `DragState` in core, freezing identity at begin

`DragState { source: PanelSide, source_dir: PathBuf, items: Vec<SourceItem>, op: JobKind, target: Option<DropTarget> }`; `DropTarget::{PanelDir(side), SubDir { side, name }, TreeNode { side, path }, Tab { side, index }}`. Items are captured at `DragBegin`, so streamed listing chunks or a re-sort mid-drag change nothing; at `DragDrop` the drop is cancelled if `source_dir != panel(source).cwd` or a `SubDir` name no longer resolves to a directory entry. The TUI-side `MouseTracker` owns pre-threshold bookkeeping and de-duplicates `DragOver`, so core only sees target changes. Rejected: keying items by row index — invalid after any listing change.

### D5: Drag cleared on every phase exit — a reducer post-condition

Any command that leaves the panels phase or opens a menu/overlay (job completion, listing failure, F9, quit request, resize below the minimum) clears `state.drag`. Enforced at the end of `update` and property-tested over random command interleavings: `drag` is never `Some` outside the panels phase and no `Effect::RunJob` is ever emitted except through the dialog path.

### D6: Feedback on the target, nothing on the pointer

The target panel's frame and title use a new `panel.frame.drop` role (bright-white on blue in `nc-classic`; inverted — black on white — in `nc-mono`, because that theme's frame is already white and theme-system forbids colour-only meaning; each other theme picks its own); the drop-target row uses `button.focused`, the focused-button colour; source rows are unchanged (they are already yellow when selected or under the cursor bar); the target mini-status shows the outcome; the key bar relabels `Drop=Copy  Shift/RightBtn=Move  Esc=Cancel` for the drag's duration (feasible because modifier flags arrive with every mouse event, unlike the keyboard modifier rows gated in the design spec §4.1). No hover highlight outside a drag. Rejected: a third "being dragged" treatment on source rows — noise.

### D7: Valid targets include same-panel subdirectories, tree nodes, and tabs

"F6 into a subfolder" is the classic use; tree nodes are the best use of Tree mode; a tab in the strip stands for its directory. Info and Quick View panels never light up. Self/descendant and same-directory drops cancel with `Can't drop here`.

## Risks / Trade-offs

- [Modifier interception by emulators] → the Copy default and the dialog's `[ Move ]` button make Move always reachable; a lost Shift yields a Copy dialog, never a surprise move.
- [Overshoot onto a subdirectory row] → the exact path in the dialog; one glance before Enter.
- [New theme role touches every built-in theme] → additive; tasks include every theme.
- [Snapshot churn] → drag visuals render only while `state.drag` is `Some`; existing goldens are untouched.

## Open Questions

- None blocking.
