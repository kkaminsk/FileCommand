## Why

`mouse-basics` gave the pull-down menu bar full click-to-activate support but deliberately left the file-action menu out — its own code documents this as an overlay mouse input does not support yet, and today it ignores every mouse event while open, including a click directly on one of its rows. Since the pull-down menu already proves the exact interaction pattern works well in this app, extending it to the file-action menu is a natural, low-risk completion of that work. Tracked as Linear BIG-165.

## What Changes

- Left-clicking a file-action-menu row activates that entry — identical routed behavior (View/Edit/Copy/Rename/Move/Delete/Send to clipboard/Run) to highlighting it and pressing Enter.
- Left-clicking outside the open menu closes it with no action taken, matching the pull-down menu's existing "click elsewhere closes it" behavior.
- No hover-highlight-on-mouse-move is added — click-to-activate only, matching the pull-down precedent.
- Right-click behavior while the menu is already open, and every other existing mouse behavior, is unaffected.

## Capabilities

### New Capabilities
(none)

### Modified Capabilities
- `mouse-input`: adds a new requirement making file-action-menu rows clickable and click-outside dismiss the menu, and narrows the existing "Mouse is honoured only where the key would be" requirement's overlay exclusion so the file-action menu is no longer blanket-ignored.

## Impact

- `crates/filecommand-tui/src/hitmap.rs`: new `file_action_menu_items: Vec<(Rect, usize)>` field.
- `crates/filecommand-tui/src/views/file_action_menu.rs`: new hit-test builder mirroring `render_file_action_menu`'s row geometry; wired into `build_hitmap` (`views/mod.rs`).
- `crates/filecommand-tui/src/input/mouse.rs`: the file-action menu moves off `Context::Ignored` into its own click-mapping path (mirroring `map_pulldown`), emitting a new click command on a row hit, and the existing `Command::FileActionMenuCancel` on a click outside the menu.
- `crates/filecommand-core/src/update.rs`: a new reducer arm for the click command, validating the index and reusing the exact activation logic `FileActionMenuConfirm` already uses.
- Existing test `an_ignored_overlay_returns_none_even_over_a_hit_row` (`input/mouse/tests.rs`) is unaffected (it tests `state.help`, not the file-action menu) but a new equivalent negative test is needed for whichever overlays remain genuinely ignored.
