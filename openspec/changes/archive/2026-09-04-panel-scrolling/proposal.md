# Change: panel-scrolling

## Why

The file panels render their entry lists from index 0 and clip at the panel height — no scroll offset exists anywhere in the codebase. In any directory longer than the panel body, moving the cursor past the last visible row makes it vanish off-screen and the user navigates blind. This affects all three list display modes (Full, Brief, Tree).

## What Changes

- **Viewport scrolling in every list display mode**: the visible window follows the cursor so it can never leave view. Full mode and Tree mode scroll by lines; Brief mode scrolls by whole columns. Scrolling is minimal-shift (Norton Commander behavior): the window moves only when the cursor would cross its edge, and jump movements (Home/End/PgUp/PgDn, fuzzy-jump, find-file "go to", quick-filter snaps) re-clamp so the cursor lands in view.
- **Scroll offset as core panel state**: a per-panel (and per-tree) scroll offset lives in `filecommand-core` and is reconciled through `core::update` after every cursor-moving or list-mutating path, following the editor's existing `ensure_caret_visible` pattern — core derives each panel's body row count from the `term_size` it already receives via `Resize`, its split, display mode, and tab-strip state; the renderer just reads the offset.
- **Scrollbar indicator on overflow**: when (and only when) a panel's list overflows its body, a vertical scrollbar renders over the right border of the body region — CP437 glyphs, ANSI-16 theme roles, thumb size proportional to the visible fraction and position proportional to the offset. When the list fits, the right border stays the unbroken double-line `║`, byte-identical to today.
- **Scrolling operates on the filtered view**: offsets are positions in the quick-filter-narrowed `visible_indices()` list, not raw entry indices, and re-clamp on reload, re-sort, filter changes, and tab switches.

## Capabilities

### New Capabilities

*(none)*

### Modified Capabilities

- `panel-navigation`: new requirements — viewport scrolling keeps the cursor visible in Full mode; the scroll offset is core panel state reconciled through `core::update` with a TUI-supplied viewport height; a scrollbar indicator renders on the body's right border only when the list overflows.
- `additional-panel-modes`: Brief display mode gains column-wise scrolling; Tree display mode gains line scrolling of the flattened node list; both gain the same overflow-only scrollbar.

*(`theme-system` is not modified: its role model already requires every renderer role to be defined in every theme, so the new `panel.scrollbar` role slots in under existing requirements — no spec-level change.)*

## Impact

- `crates/filecommand-core/src/panel.rs` — scroll offset fields on `PanelState` and `TreeState`, pure ensure-cursor-visible reconciliation, re-clamp on list mutations (reload, sort, quick-filter, tab restore).
- `crates/filecommand-core/src/update.rs` — per-panel viewport derivation from `term_size`/split/mode/tab-strip (mirroring `editor_viewport`); reconciliation invoked from every cursor-moving and list-mutating path (including type-ahead's direct cursor set and find-file's deferred settle on `ListingComplete`).
- `crates/filecommand-core/src/theme.rs` — new `panel.scrollbar` role added to `Role` and every built-in theme (under theme-system's existing "every role required by renderers is defined" rule).
- `crates/filecommand-tui/src/views/panel.rs` — Full/Brief/Tree bodies render from the offset instead of position 0; scrollbar drawn over the right border on overflow.
- `crates/filecommand-tui/src/input/` / `app.rs` — the PgUp/PgDn paging step aligns with core's per-mode row derivation (replacing the layout-level `entries_visible`, which over-counts by one when the tab strip is visible and mismatches Brief).
- Snapshot tests (`crates/filecommand-tui/tests/`) — new goldens for overflowing lists and the scrollbar; existing short-list goldens must remain byte-identical.
