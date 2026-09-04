## Context

`mouse-basics` built the entire hit-map/mouse-command architecture and already gives the pull-down menu bar full click-to-activate support, but explicitly scoped the file-action menu out: `crates/filecommand-tui/src/input/mouse.rs`'s `context()` function routes it to `Context::Ignored`, and no hit-map entries are ever recorded for it (`views/mod.rs::build_hitmap` has no file-action-menu branch, unlike every other overlay that accepts mouse input).

The pull-down menu's click pipeline is a near-exact structural match for what's needed here — both are fixed-height-row, click-to-activate, index-addressed lists:
1. `HitMap.menu_items: Vec<(Rect, usize)>` (`hitmap.rs`), populated by `menubar::hit_items` (`views/menubar.rs`), which mirrors `render_pulldown`'s box-geometry math exactly so a rect can never claim a cell that isn't actually drawn there this frame.
2. `input/mouse.rs`'s `map_pulldown`: on `Up(Left)`, hit-tests the item rects and emits `Command::MenuItemClick(index)`; a click outside the open pull-down closes it.
3. Core's `handle_menu_item_click` validates the index, activates the entry, closes the menu.

`views/file_action_menu.rs::render_file_action_menu` already computes exactly the row geometry a hit-test builder needs (one row per `dialog.entries[i]`, fixed inner width, box origin via `overlay_rect`) — directly parallel to `render_pulldown`'s math — but has no hit-test function today.

## Goals / Non-Goals

**Goals:**
- Clicking a file-action-menu row activates it, with identical downstream behavior to keyboard Enter.
- Clicking outside the open menu closes it (no action), matching the pull-down precedent.
- Reuse the pull-down's existing architecture and patterns as closely as possible rather than inventing new mechanisms.

**Non-Goals:**
- Hover-highlight-on-mouse-move (matches the pull-down precedent: click-to-activate only, no highlight-follows-cursor).
- Any change to right-click's existing meaning (opening the menu) while the menu is already open.
- Editing `mouse-basics` itself (its own artifacts/tasks are already authored and near-complete) — this is a new, stacked change.

## Decisions

- **Mirror `MenuItemClick(usize)` with a new `FileActionMenuItemClick(usize)` command**, rather than trying to generalize the pull-down's mechanism into a shared "clickable list" abstraction. The two lists (pull-down items, file-action-menu entries) differ enough in surrounding state (`MenuState` vs `FileActionMenuState`, enabled/disabled items vs none) that forcing a shared abstraction now would add indirection for a single reuse; mirroring the pattern keeps both call sites simple and independently testable, consistent with how the rest of the mouse-input surface already has one hit-map field + one mapping function per interactive area (panels, key bar, menu bar, dialog buttons).
- **Click-outside reuses the existing `Command::FileActionMenuCancel`** (the same command Esc already produces) rather than a new "close" variant — the two are behaviorally identical (close with no action taken), so reusing keeps the reducer's cancel logic as the single source of truth.
- **Reuse `FileActionMenuConfirm`'s activation logic for the click path.** The click reducer arm sets `cursor = index` on the menu state, then calls the same downstream activation function `FileActionMenuConfirm` invokes, so routed behavior (View/Edit/Copy/.../Send to clipboard/Run) is guaranteed identical regardless of input method — no duplicated dispatch logic.
- **Spec shape**: two changes to `mouse-input`. ADD a new requirement ("File-action menu entries are clickable") in the style of the existing "Key bar, menu bar, pull-down items, and dialog buttons are clickable" requirement. MODIFY "Mouse is honoured only where the key would be" — its current wording enumerates panels/pull-down/dialog-buttons/viewer-wheel as the only contexts where mouse is honored, with "all other overlays SHALL ignore mouse events"; the file-action menu needs to be added as an explicit exception, otherwise the new ADDED requirement would contradict this one.

## Risks / Trade-offs

- [Risk] Adding a new `Context` variant (or a new branch within the existing dispatch) in `input/mouse.rs` touches code shared by every overlay's mouse routing. → Mitigation: follow the exact same shape as the pull-down's existing, already-tested branch; the change is additive (one more recognized context), not a rewrite of the dispatch itself.
- [Risk] The hit-map geometry for the file-action menu must be recomputed identically to `render_file_action_menu`'s actual draw math, or clicks could register against the wrong row (or none) after a resize/scroll. → Mitigation: this is the same risk the pull-down's `hit_items` already manages successfully by mirroring `render_pulldown` line-for-line; the new builder follows the identical approach against `render_file_action_menu`.
