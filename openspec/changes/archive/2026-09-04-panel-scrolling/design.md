# Design: panel-scrolling

## Context

`PanelState` (in `filecommand-core`) holds a `cursor` index but no scroll offset; its own doc comment states cursor moves are "resolvable without knowing the viewport." Rendering in `filecommand-tui/src/views/panel.rs` slices the entry list from position 0 in all three list modes — Full (`visible.get(row)`, `rows_h = body_h − 1` for the header), Brief (`pos = c * rows_h + row` across ≥12-cell columns, `rows_h = body_h`), and Tree (flat node list, `rows_h = body_h − 1`) — so any entry past the body height is simply unreachable visually. Quick-filter narrows the rendered body to `visible_indices()` positions rather than raw entry indices, and Tree mode keeps its own cursor in `TreeState`.

Two viewport-bridging precedents already exist. The TUI computes `page_size` from `layout::compute(...).entries_visible` and passes it into `Command::MoveCursor` for PgUp/PgDn (note: `entries_visible` currently over-counts by one when the tab strip is shown, and doesn't match Brief's row count). The editor is the stronger model: core receives `Command::Resize` and stores `term_size`, derives the editor viewport core-side (`editor_viewport(term_size)`), and the reducer calls `EditorState::ensure_caret_visible(rows, cols)` — a pure minimum-scroll clamp — after every caret-moving command, while the renderer just reads `top_line`/`left_col`.

The `panel-navigation` spec requires all panel-state mutations to flow through `core::update`, and its "Full display mode layout" requirement **normatively mandates the double-line border** — so a scrollbar overlaying `║` is a spec modification, not just an addition. The in-flight `responsive-layout` change pins byte-identical width anchors at 80×24 (Full-mode column ladder over interior 38; Brief = 3 columns of 12/12/14), all functions of the panel's interior width. Rendering is pinned by ~47 insta snapshot goldens whose last column is the panel right border.

## Goals / Non-Goals

**Goals:**

- The cursor can never be outside the rendered window in Full, Brief, or Tree mode, no matter how the cursor moved (arrows, paging, Home/End, type-ahead, find-file's settle-on-listing, quick-filter snapping, tab restore, Tree navigation) or how the list changed (streamed load, re-sort, filter, reload).
- Norton Commander scroll feel: the window shifts only when the cursor would cross its edge — no centering, no anchor-to-bottom while moving up.
- A scrollbar on the body's right border that appears exactly when the list overflows, sized and positioned proportionally.
- Zero rendering change for lists that fit: existing short-list snapshot goldens stay byte-identical.

**Non-Goals:**

- No mouse interaction with the scrollbar (the application is keyboard-driven).
- No horizontal scrolling of long names, and no scrolling for Info/QuickView panels (no entry list; the viewer already scrolls itself).
- No smooth/partial-row scrolling — the terminal grid scrolls by whole rows/columns.
- No fix for the pre-existing `page_size` over-count with a visible tab strip beyond what the viewport derivation naturally corrects.

## Decisions

### D1: Scroll offset is core state, not TUI state

`PanelState` gains a scroll offset (top visible position), and `TreeState` gains its own. Alternative — a TUI-side offset cache keyed by panel/tab — was rejected: `panel-navigation` requires panel mutations to flow through `core::update`, tabs already snapshot/restore panel state in core (`to_tab_data`/`adopt_tab_data`), and a parallel TUI-side map would have to mirror tab switching, streamed listings, and filter changes to avoid stale offsets. Keeping the offset next to the cursor makes every reconciliation site testable in core's pure tests.

### D2: Reconciliation follows the editor pattern — core derives the viewport and clamps in the reducer

The model is `EditorState::ensure_caret_visible`: a pure, minimum-scroll, no-op-when-visible clamp invoked from the reducer after every relevant mutation. Core already knows everything the panel viewport depends on — `term_size` (via the existing `Command::Resize`), the split, each panel's display mode, and tab-strip visibility — so it derives per-panel body rows itself, the way `editor_viewport(term_size)` already does for the editor; no new TUI→core message is required. Per-mode row counts follow the renderer's geometry: Full and Tree bodies are `body_h − 1` (header row), Brief is `body_h`, and a visible tab strip costs one row. Minimal-shift semantics: `offset ≤ cursor_pos ≤ offset + rows − 1`, restored by moving the offset the smallest distance; Home pins the window to the top, End to the bottom; other jumps only guarantee the cursor lands in view. The rejected alternative — TUI passes rows on each movement command like today's `page_size` — was set aside because `entries_visible` already disagrees with real per-mode row counts (tab strip, Brief), and duplicating per-mode math in the TUI invites drift.

### D3: Offsets live in visible-position space

The offset counts positions in the quick-filter-narrowed `visible_indices()` list — the same space the renderer iterates — not raw entry indices. The spec makes the consequences explicit: quick-filter push/backspace/clear re-clamps; streamed listing keeps the pinned-to-top behavior while the cursor is unmoved and re-clamps otherwise; re-sort re-clamps after the cursor re-anchors to its entry name; find-file's deferred cursor settle (on `ListingComplete`) re-clamps; adopting a tab re-clamps against the current viewport, which may differ from when the tab was stashed.

### D4: Brief mode scrolls by whole columns

Brief renders `n_cols` columns of `rows_h` entries; its window is `n_cols × rows_h` consecutive positions starting at a column (i.e. `rows_h`-multiple) boundary. When the cursor crosses the right edge the window shifts one column left (and symmetrically at the left edge), keeping every rendered column full-height and aligned — matching NC 5.5. Line-wise shifting was rejected: it would misalign column boundaries with position boundaries and re-flow every column on every step.

### D5: Scrollbar overlays the right border column — never a reserved interior column

When a list overflows, the body entry rows of the panel's right border column render as a scrollbar: CP437 glyphs only — `░` track, `█` thumb — with thumb length `max(1, round(rows × rows / total))`, position proportional to the offset, touching the top exactly when the offset is 0 and the bottom exactly when the last position is visible. It occupies only entry rows: never the top border, tab-strip row, header row, or bottom border (the clock and mini-status are untouched). In Brief mode the same vertical scrollbar reflects linear position through the list (a horizontal one was rejected: the bottom border carries the mini-status). The rejected alternative — reserving an interior column for the scrollbar — would shrink the interior width that the Full-mode column ladder and Brief's `max(1, ⌊inner_w/12⌋)` column count are functions of, breaking the in-flight responsive-layout change's byte-identical 80×24 anchors; overlaying the border leaves all width math untouched. A single new `panel.scrollbar` role is added to the theme model and every built-in theme; track and thumb stay distinguishable through glyph density (`░` shows the role's background through the speckle, `█` is solid foreground), so no second role is needed — and since theme-system's requirements already demand every renderer role be defined in every theme, this is an implementation addition, not a theme-system spec change. When the list fits, the border renders exactly as today — this keeps existing goldens byte-identical. Because the in-flight responsive-layout change already carries a MODIFIED delta for `panel-navigation`'s "Full display mode layout" (which normatively mandates the double-line border) and for `additional-panel-modes`' "Brief display mode", this change deliberately uses **ADDED requirements only** — the scrollbar requirement states explicitly that on overflow it takes precedence over the unbroken-border mandate — so the two changes cannot clobber each other's requirement text regardless of archive order.

### D6: Every cursor-writing path funnels through reconciliation

Cursor mutations are scattered across core: `move_cursor`/`move_cursor_filtered`, `clamp_cursor`, `snap_cursor_to_visible`, `insert_streamed`'s pin-to-top, `set_sort_mode`'s re-anchor, `begin_new_listing`'s reset, type-ahead's direct assignment, find-file's `pending_cursor_target` settle in `apply_listing_event`, `toggle_selection`'s advance, tab `adopt_tab_data`, and `TreeState::move_cursor`. Rather than giving each site its own scroll math, core applies one reconciliation after any mutation that can move the cursor or change the list — enforced by spec scenarios that exercise jump-style movements and list mutations, not just single steps.

## Risks / Trade-offs

- [Snapshot churn: the scrollbar changes the right border in overflow scenarios] → overflow-only rendering (D5) keeps short-fixture goldens byte-identical; but the existing goldens must be **audited for overflow at the small matrix sizes** (60×16 renders only ~11 Full-mode rows) — any fixture that overflows there will legitimately change and must be re-pinned deliberately, not rubber-stamped. New goldens are added for overflow, including thumb-at-top and thumb-at-bottom endpoints.
- [Core-side viewport math can drift from TUI layout math] → the per-mode row-count derivation is spec-pinned (Full/Tree `body_h − 1`, Brief `body_h`, tab strip −1) and covered by core tests at fixed sizes; the same derivation replaces the TUI's slightly-wrong `entries_visible` as the paging step so both consumers share one source of truth.
- [Stale viewport for one frame after resize] → reconciliation re-runs on `Resize`, so the cursor is back in view by the next frame; the transient is accepted.
- [Brief-mode index math (linear position ↔ column window) is fiddly] → the column-boundary invariant in D4 is spec-tested with exact scenarios at fixed dimensions.
- [A new theme role touches every built-in theme] → the role is additive with defaults chosen per theme; theme updates are tasks in this change, not an afterthought.

## Open Questions

- None. Scope (all three list modes) and scrollbar visibility (overflow-only) were confirmed by the user; scroll feel is fixed to NC 5.5 minimal-shift by the project's charter.
