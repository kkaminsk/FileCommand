# Spike (c): 100k-entry directory render benchmark

**Question:** does FileCommand stay responsive against a directory with
100,000 entries — both rendering a panel and streaming the listing in? This
is the scenario that would expose an accidentally-O(n) render loop or an
accidentally-O(n²) streaming-insert loop before a real user hits it.

## Method

`crates/filecommand-tui/tests/bench_100k_ignored.rs` exercises the actual
production code paths (not a synthetic microbenchmark) against 100,000
synthetic `Entry` values:

1. `bench_bulk_sort_100k` — sort 100k entries once with
   `filecommand_core::panel::cmp_entries` (what a "sort on completion"
   strategy would cost).
2. `bench_streamed_sorted_insert_100k` — insert 100k entries one at a time via
   `filecommand_core::panel::insert_sorted`, exactly as
   `update::apply_listing_event` does today for every entry in every worker
   chunk (what the *current* streaming strategy costs).
3. `bench_render_frame_with_100k_entries` — draw 200 frames of
   `views::render` against a fully-populated 100k-entry panel through a real
   `ratatui::backend::TestBackend`, at the cursor parked mid-list.

Run via `cargo test --release -p filecommand-tui --test bench_100k_ignored --
--ignored --nocapture` (release mode — debug-build timings are not
representative and were not used for the numbers below).

## Results (this machine, release build)

| Benchmark | Result |
|---|---|
| Bulk `sort_by` over 100k entries | **14.5 ms** |
| Streamed `insert_sorted` × 100k (current production path) | **248 ms** total (~17× slower than one bulk sort) |
| Render one frame against a 100k-entry panel (80×24, cursor mid-list) | **0.209 ms/frame** average over 200 frames |

## Findings

- **Rendering is not the risk.** `views::panel::render_panel` only iterates
  the visible viewport rows (`area.height`-bounded, ~20 rows at 80×24), never
  the full entry list, so render cost is independent of directory size. A
  100k-entry panel renders in the same sub-millisecond ballpark as an
  empty one. No change needed here for M1 or later.
- **Streaming insert is the risk.** `PanelState::insert_streamed` →
  `insert_sorted` does a binary-search + `Vec::insert`, which shifts every
  element after the insertion point. Called once per entry (not once per
  worker chunk) for every entry in every `ListingChunk`, this is O(n²) in the
  worst case (reverse-sorted or shuffled arrival order). At 100k entries it's
  already the dominant cost in the streaming pipeline — 17× the cost of just
  sorting the same data once — even though it's still comfortably
  sub-second and doesn't threaten M1's target directory sizes. It will not
  scale gracefully to directories an order of magnitude larger (e.g. a
  node_modules-style tree with 500k–1M entries), where the same approach
  extrapolates toward multi-second insert stalls.

## Conclusion for M1

No action required — M1's realistic directories (thousands, not hundreds of
thousands, of entries) are well within budget, and the render path has no
scaling risk at all. This is filed as a known, *measured* forward-looking
risk for whichever milestone first targets very large directories: prefer
batch-appending each worker chunk and re-sorting (or merging two sorted runs)
over calling `insert_sorted` per entry, since the benchmark shows a single
bulk sort of the same 100k entries is ~17× cheaper than the current
per-entry insertion strategy.
