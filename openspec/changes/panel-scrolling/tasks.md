# Tasks: panel-scrolling

## 1. Core scroll state

- [ ] 1.1 Add the scroll offset to `PanelState` and `TreeState` in `crates/filecommand-core/src/panel.rs`, in visible-position space, snapshotted/restored by `to_tab_data`/`adopt_tab_data` (panel-navigation: "Scroll offset is core panel state")
- [ ] 1.2 Implement the pure minimal-shift reconciliation (ensure-cursor-visible clamp modeled on `EditorState::ensure_caret_visible`): no-op while the cursor is in the window, one-line shifts at the edges, Home/End pinning, jump re-clamp (panel-navigation: "Viewport scrolling keeps the cursor visible")
- [ ] 1.3 Derive per-panel body row counts core-side from `term_size`, split, display mode, and tab-strip visibility (mirroring `editor_viewport`): Full/Tree = body − header, Brief = full body, tab strip costs one row (panel-navigation: "Scroll offset is core panel state")
- [ ] 1.4 Invoke reconciliation from every cursor-writing and list-mutating path in `update.rs`/`panel.rs`: `move_cursor`/`move_cursor_filtered`, type-ahead's direct cursor set, find-file's settle on `ListingComplete`, `set_sort_mode` re-anchor, `insert_streamed` (keeping the pinned-top behavior), quick-filter push/backspace/clear, `adopt_tab_data`, `toggle_selection`'s advance, and `Resize` (panel-navigation: "Scroll offset is core panel state")
- [ ] 1.5 Core unit tests at fixed viewports: edge shifts, in-window no-ops, Home/End pinning, quick-filter/re-sort/resize/tab-restore re-clamps, streamed pin-to-top (panel-navigation: "Viewport scrolling keeps the cursor visible"; panel-navigation: "Scroll offset is core panel state")

## 2. Brief and Tree scrolling

- [ ] 2.1 Brief-mode column-window reconciliation: offset kept on `rows_h`-multiples, one-column shifts at the window edges, column count from the panel's interior width (additional-panel-modes: "Brief mode column scrolling")
- [ ] 2.2 Tree-mode reconciliation over the flattened node list via `TreeState`'s offset, including re-clamp after expand/collapse (additional-panel-modes: "Tree mode scrolling")
- [ ] 2.3 Core unit tests for Brief column-boundary invariants and Tree scrolling incl. expansion overflow (additional-panel-modes: "Brief mode column scrolling"; additional-panel-modes: "Tree mode scrolling")

## 3. Rendering

- [ ] 3.1 Render Full/Brief/Tree bodies from the scroll offset instead of position 0 in `crates/filecommand-tui/src/views/panel.rs` (panel-navigation: "Viewport scrolling keeps the cursor visible")
- [ ] 3.2 Add the `panel.scrollbar` role to `Role` and every built-in theme in `crates/filecommand-core/src/theme.rs` (panel-navigation: "Scrollbar indicator on overflow")
- [ ] 3.3 Draw the overflow-only scrollbar over the right border's entry rows — `░` track, `█` thumb, proportional length/position with exact top/bottom endpoint behavior; never on the top border, tab-strip row, header row, or bottom border (panel-navigation: "Scrollbar indicator on overflow")
- [ ] 3.4 Align the PgUp/PgDn paging step with core's per-mode row derivation, replacing `layout::compute(...).entries_visible` as the step source (panel-navigation: "Scroll offset is core panel state")

## 4. Snapshot verification

- [ ] 4.1 Audit existing goldens for fixtures that overflow at the matrix sizes (esp. 60×16); confirm every non-overflowing golden stays byte-identical and deliberately re-pin any that legitimately gain a scrollbar (panel-navigation: "Scrollbar indicator on overflow")
- [ ] 4.2 New snapshots: Full-mode overflow with thumb at top, middle, and bottom; scrolled window contents; Brief overflow with a shifted column window; Tree overflow after expansion (panel-navigation: "Scrollbar indicator on overflow"; additional-panel-modes: "Brief mode column scrolling"; additional-panel-modes: "Tree mode scrolling")
- [ ] 4.3 `cargo build --workspace` and `cargo test --workspace` green
