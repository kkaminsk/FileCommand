## 1. TUI: hit-testing

- [ ] 1.1 Add `file_action_menu_items: Vec<(Rect, usize)>` to `HitMap` (`crates/filecommand-tui/src/hitmap.rs`).
- [ ] 1.2 Add a `hit_items`-equivalent function in `crates/filecommand-tui/src/views/file_action_menu.rs`, mirroring `render_file_action_menu`'s exact row/box geometry (same `overlay_rect`, `visible_rows`, `box_h`/`x`/`y` computation) so recorded rects always match what's actually drawn this frame.
- [ ] 1.3 Wire it into `build_hitmap` (`crates/filecommand-tui/src/views/mod.rs`) when `state.file_action_menu` is `Some`.

## 2. TUI: mouse mapping

- [ ] 2.1 In `crates/filecommand-tui/src/input/mouse.rs`, move the file-action menu off its current `Context::Ignored` membership into its own click-mapping path.
- [ ] 2.2 On `Up(Left)` over a `file_action_menu_items` rect (via `find_hit`), emit a new `Command::FileActionMenuItemClick(usize)`.
- [ ] 2.3 On `Up(Left)` while the menu is open but outside its rect, emit the existing `Command::FileActionMenuCancel`.
- [ ] 2.4 No mouse-move/hover handling added for this context (design.md non-goal).

## 3. Core: reducer

- [ ] 3.1 Add `Command::FileActionMenuItemClick(usize)` to the `Command` enum and to `is_file_action_menu_command` (`crates/filecommand-core/src/update.rs` ~1261-1264 area).
- [ ] 3.2 Reducer arm: validate the index is within `entries.len()`; on a valid index, set `cursor = index` then invoke the same activation logic `FileActionMenuConfirm` uses; on an out-of-range index (shouldn't normally happen given hit-testing, but guard anyway), no-op.

## 4. Tests

- [ ] 4.1 TUI-side: new test mirroring `pulldown_item_click_dispatches_menu_item_click` for `FileActionMenuItemClick`.
- [ ] 4.2 TUI-side: new test mirroring `pulldown_click_elsewhere_closes_the_bar` asserting a click outside the open file-action menu yields `Command::FileActionMenuCancel`.
- [ ] 4.3 TUI-side: confirm `an_ignored_overlay_returns_none_even_over_a_hit_row` still passes unchanged (it tests `state.help`, not the file-action menu) — add an equivalent negative test for another still-genuinely-ignored overlay if useful, but no change needed to the existing one.
- [ ] 4.4 Core-side: new tests mirroring `menu_item_click_activates_the_item_exactly_like_menu_activate` / `..._with_no_menu_open_is_a_no_op` for `FileActionMenuItemClick`, matching the assertion style of the existing file-action-menu test suite (`open_action_menu_at_opens_the_menu_for_a_file`, etc.).
- [ ] 4.5 Core-side: test that clicking a non-highlighted row activates that row (not the previously-highlighted one).

## 5. Verification

- [ ] 5.1 Run `cargo test -p filecommand-core` and `cargo test -p filecommand-tui`.
- [ ] 5.2 Manual check via the `run` skill: open the file-action menu (Enter or right-click), click a row (activates it), reopen and click outside the menu (closes with no action), confirm no visual hover-highlight appears on mouse-move alone.
- [ ] 5.3 `detect_changes()` (GitNexus) against `main` to confirm only the expected symbols/flows are touched before opening the PR.
