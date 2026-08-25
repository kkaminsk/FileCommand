# Change: mouse-panel-drag

## Why

With `mouse-basics` in place a mouse user can click, but the gesture every dual-panel user reaches for — dragging entries from one panel onto the other — does nothing. Drag-and-drop is the mouse's native expression of Copy/Move, and NC 5, Total Commander, and Far Manager all support it. Because a terminal cannot composite a drag image or start an OS drag, the design puts all feedback on the *target* and always lands in the existing destination dialog, so nothing can mutate the file system without the same confirmation chain F5/F6 use.

## What Changes

- **Drag = Copy.** Pressing on an entry and moving the pointer begins a drag of the F5 scope (the selection set if the pressed entry is selected, else that entry; never `..`). Releasing over a valid target opens the destination-input dialog pre-filled with the *exact* drop path and offering `[ Copy ] [ Move ] [ Cancel ]`. Ctrl+drag is also Copy; Shift+drag (where the emulator delivers it) and right-button drag open the same dialog with `[ Move ]` focused. Ctrl never means Move. The keyboard F5/F6 dialog has no button row today; the drop-initiated dialog adds one.
- **Targets:** the other panel's directory (title, blank area, or non-directory row); a subdirectory row or `..` in either panel; a Tree-mode node; the other panel's tab. Info and Quick View panels are never targets. Dropping onto the items' own directory, or onto a dragged directory itself or its descendant, cancels.
- **Feedback:** the target panel's frame and title switch to a new `panel.frame.drop` theme role; the drop-target row renders in `button.focused`; the target mini-status reads `Copy 3 files ► OLD\` or `Can't drop here` (CP437 glyphs only); the key bar relabels `Drop=Copy  Shift/RightBtn=Move  Esc=Cancel` for the duration; source rows are untouched.
- **Cancel:** release anywhere invalid, or Esc mid-drag; any phase change clears the drag (a reducer post-condition).
- **Robustness:** items are frozen at drag start; a source panel that navigated away, or a target row that no longer resolves to a directory, cancels the drop.

## Capabilities

### New Capabilities

- `mouse-drag`: lifecycle, verb rules, valid targets, drop-to-dialog, cancel paths, robustness against listing changes, visual feedback.

### Modified Capabilities

- `operation-dialogs`: new requirement — the drop-initiated destination dialog offers Copy/Move/Cancel and is pre-filled with the drop path; keyboard F5/F6 dialogs are unchanged.

*(`theme-system` is not modified: the new `panel.frame.drop` role slots in under its existing every-role-in-every-theme rule, as `panel.scrollbar` did in `panel-scrolling`.)*

## Impact

- `crates/filecommand-core/src/update.rs` — `DragState`, `DropTarget`, commands `DragBegin`/`DragOver`/`DragDrop`/`DragCancel`; `enter_file_op_setup_for_sources` refactored to take an explicit `(source_side, prefill)` instead of reading `state.active`; the drag cleared on every phase exit.
- `crates/filecommand-core/src/theme.rs` — `panel.frame.drop` role in `Role` and every built-in theme.
- `crates/filecommand-tui/src/input/mod.rs` — `MouseTracker` drag threshold and verb-from-modifiers on each `Drag`/`Up`; `DragOver` de-duplication.
- `crates/filecommand-tui/src/views/panel.rs`, `views/keybar.rs`, `views/tab_strip.rs`, `views/destination_input.rs` — drag visuals, key-bar relabel, three-button drop dialog.
- Snapshot goldens for drag-in-progress and the drop dialog; core proptests for the state machine.
- Depends on `mouse-basics` (hit map, `MouseTracker`, semantic-command boundary).
