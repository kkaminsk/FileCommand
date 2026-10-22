## Why

The panel spec promises that a large directory streams in — a running `Reading… 12,345` count and a partial listing that stays interactive — but the code does not deliver it, and the way it applies listing data can freeze the UI (audit 2026-10-12, §6 "Streaming listing mini-status": **DRIFT**):

- **P-01 (High):** `StdFsReader::read_dir` collects the entire directory into a `Vec` before `list_dir_chunked` slices it into chunks, so a 100k-entry directory or a slow network share shows `Reading… 0` for the whole enumeration and then receives every chunk in one burst.
- **P-02 (High):** every entry of every chunk is inserted one at a time with `partition_point` + `Vec::insert` (O(n²) for unsorted arrival), inside a `drain_events` loop with no budget. Because P-01 delivers all chunks at once, the whole insert runs before the loop returns to input or drawing — no key, resize, or redraw is processed meanwhile, contradicting "a partially filled panel is fully interactive". The audit's release run measured 257.8 ms for 100k streamed inserts vs 13.1 ms for one bulk sort; 1M entries (a large `node_modules` or log directory) extrapolates to multi-second freezes.
- **P-03 (Medium):** `visible_indices()` allocates an O(n) `Vec` on every call — four times per frame for rendering and hit-testing, again for each cursor move and viewport re-clamp — and with a quick filter it lowercases every name and the pattern per entry per call. Render cost at 100k entries is now 1.237 ms/frame (the spike recorded 0.209 ms and concluded "independent of directory size"; that conclusion is stale).
- **P-04 (Medium):** the name comparators call `lower_name` — `to_string_lossy().to_lowercase()` — for both operands on every comparison, so every sort, re-sort (Ctrl+F3..F6), and insert allocates two `String`s per comparison; this dominates the measured insert cost.

## What Changes

- The directory reader streams: a new callback-style `FsReader` enumeration hands entries to the listing worker as the OS returns them (tests keep the `Vec` API through a default method), and the worker sends batches bounded by a growing entry count and by ~50 ms of enumeration time, so the first entries and a live count appear while enumeration continues. The cancel check introduced by `listing-request-guard` moves inside the enumeration.
- Each batch is sorted once and merged into the panel's sorted list in a single linear pass (no per-entry mid-list insertion). Entries still appear in sorted position as they arrive; once the user has moved the cursor during streaming, the cursor stays on its entry while batches land around it.
- `Entry` carries a lowercase name key computed once when the entry is read; all name-based comparators and quick-filter matching use it instead of allocating per comparison.
- `PanelState` caches the quick-filter-narrowed index list, rebuilt only when the entries or the pattern change; with no filter the visible list is the identity and is never materialized. Rendering, hit-testing, cursor movement, and viewport re-clamping use non-allocating accessors.
- The event loop drains worker messages under a budget (~8 ms or 64 messages) before it reads input and draws; a backlog makes the next input poll non-blocking so the rest is applied on the following iterations.
- The in-flight `reread-preserves-cursor` anchor lookup, which scans the whole captured name list against the whole listing on every batch, becomes proportional to the batch size.
- The 100k-entry benchmark is extended (streamed merge, shuffled arrival, quick-filter render, re-sort) and the spike document is re-run and corrected.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `panel-navigation`: ADDED "Large directories stream progressively and stay responsive".

## Impact

- **Core (`crates/filecommand-core`):**
  - `listing.rs` — `FsReader::for_each_entry` (default implemented over `read_dir`), streaming `StdFsReader`, `list_dir_chunked` replaced by a batching `list_dir_streaming` with an injectable clock and batch policy, `Entry::new` + private `sort_key`, comparators (`cmp_by_name`, `cmp_by_extension`, `cmp_by_time`, `cmp_by_size`) and `lower_name` reworked.
  - `panel.rs` — `merge_streamed` replaces `insert_streamed`; `insert_sorted` kept only for callers that insert single entries; cached visible-index list with `visible_len`/`visible_at`/`visible_position` accessors; `matches_quick_filter`, `ensure_cursor_visible`, `ensure_cursor_visible_brief`, `move_cursor_filtered`, `snap_cursor_to_visible`, `apply_reread_anchor`, `adopt_tab_data`, `resort`, `set_sort_mode`, `begin_new_listing` maintain or use the cache; `set_entries` for wholesale replacement.
  - `update.rs` — `apply_listing_event` calls `merge_streamed` once per chunk; `jump_to_prefix` uses the accessors.
  - All `Entry { .. }` struct literals (core, TUI, tests, benchmarks) move to `Entry::new`.
- **TUI (`crates/filecommand-tui`):** `worker.rs` `spawn_listing` drives `list_dir_streaming`; `app.rs` `drain_events` gains a budget and a backlog signal used by the loop's poll; `views/panel.rs` `hit_test`, `render_panel` (Full) and the Brief body iterate only the window via the accessors.
- **Benchmarks/docs:** `crates/filecommand-tui/tests/bench_100k_ignored.rs` (kept as the measurement, extended), `docs/spikes/c-100k-entry-render-benchmark.md` (re-run, numbers and conclusion updated — the spike half of B-09).
- **No persisted-format, config, or dependency changes.**

**Audit findings:** P-01, P-02, P-03, P-04 (plus the spike-document refresh part of B-09)

**Coordination:** `listing-request-guard` **lands first** — this change builds on its listing request ids and the `CancelFlag` parameter of the listing worker. `link-aware-listing` changes how `StdFsReader` classifies each entry (and R-21's skip-unreadable rule); that per-entry body moves into `for_each_entry` here, so whichever lands second rebases its classification onto the other. `filter-selection-safety` (R-17) changes quick-filter cursor logic that now reads the cached visible list. `idle-aware-event-loop` restructures the same event loop; its poll-timeout logic must honor the drain backlog introduced here. The in-flight `reread-preserves-cursor` anchor is re-implemented for per-batch cost without changing its behavior.
