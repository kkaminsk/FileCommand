# Spike (c): 100k-entry directory benchmark

**Question:** does FileCommand stay responsive against a directory with
100,000 entries — streaming the listing in, re-sorting it, and rendering a
panel over it, with and without a quick filter? This is the scenario that
exposes an accidentally-O(n) per-frame loop or an O(n²) streaming path before
a real user hits it.

**History.** The first version of this spike (M1) measured a pre-sorted
100k insert at ~248 ms against a ~14.5 ms bulk sort and a 0.209 ms frame,
and concluded that rendering was "independent of directory size" and that the
per-entry insert was a forward-looking risk only. The audit of 2026-10-12
re-ran it (257.8 ms vs 13.1 ms, 1.237 ms/frame) and found the drift
(P-01..P-04): the reader collected the whole directory before anything
reached the panel, every entry was inserted one at a time, every
comparison lowercased two names, and the visible-index list was rebuilt
several times per frame. Two things about the original measurement were
also wrong, and this rewrite corrects them:

- The insert benchmark fed `insert_sorted` names that were **already in
  sorted order**, so every insert landed at the end and nothing shifted. Its
  248 ms was almost entirely comparator allocation, not the quadratic shift it
  was meant to measure. With shuffled arrival — the order Unix `readdir`,
  ReFS and network redirectors actually produce — the same code takes
  **over four seconds** (below).
- "Render cost is independent of directory size" was true only while no
  filter was active and `visible_indices()` was cheap; the per-frame
  allocation it did was O(n), which the audit's 1.237 ms/frame exposed.

`streaming-listing-performance` replaced those paths; this document records
what the replacement measures.

## Method

`crates/filecommand-tui/tests/bench_100k_ignored.rs` exercises the production
code paths — not synthetic microbenchmarks — against 100,000 synthetic
`Entry` values (`file-000000.txt` …, distinct sizes), built through
`Entry::new` so each carries its precomputed lowercase key. "Shuffled arrival"
is a fixed xorshift Fisher–Yates permutation, identical on every run and
machine. Run with:

```sh
cargo test --release -p filecommand-tui --test bench_100k_ignored -- --ignored --nocapture
```

`bench_streaming_listing_performance_100k` measures, in one process:

1. **Bulk sort** — `sort_by(cmp_entries)` over the 100k entries, once on
   already-sorted input (the original spike's baseline, kept for comparison)
   and once on the shuffled input. The spec's "one bulk sort of the same
   100,000 entries" is the shuffled one: the same entries in the same order
   the stream delivers them.
2. **Per-batch sort share** — sorting each policy batch on its own.
3. **Old per-entry insert** — `insert_sorted` once per entry, with sorted
   and with shuffled arrival (reference only; production no longer does this).
4. **Streamed merge** — `PanelState::merge_streamed` fed the batches the
   reader's `BatchPolicy` produces on a fast enumeration (`clamp(sent / 4,
   256, 16_384)` entries each: 25 batches for 100k), sorted and shuffled
   arrival, with the worst single merge recorded.
5. **End to end through the reducer** — the same shuffled batches applied as
   `Command::ListingChunk`s plus `ListingComplete` through `update()`, which
   also runs the re-read anchor and viewport reconciliation per batch.
6. **Re-sort** — `set_sort_mode` for every mode at 100k entries.
7. **Render** — 200 frames of `views::render` at 80×24 through a real
   `ratatui::backend::TestBackend`, cursor mid-list, against a 1,000-entry
   and a 100,000-entry panel, each with no filter and with the quick filter
   `9` active.

It prints the ratios the panel-navigation requirement "Large directories
stream progressively and stay responsive" states and asserts them: streamed
merge ≤ 3× the bulk sort, 100k frame ≤ 2× the 1k frame. Because both sides of
each ratio are measured in the same run, the check holds on any machine.

`bench_1m_streamed_merge` is a separate, heavier one-off (a million entries,
several hundred MB) that settles the design's open question about the
million-entry range.

## Results

Release build, Windows 11 Enterprise, AMD Ryzen 7 5800H, 31 GB RAM, three
runs of the 100k benchmark (run-to-run noise is roughly ±15 ms on the
larger figures; ranges below span the runs).

| Benchmark (N = 100,000) | Result |
|---|---|
| Bulk `sort_by`, already-sorted input (original baseline) | 2.3–3.3 ms |
| Bulk `sort_by`, shuffled input (the spec's baseline) | **50–53 ms** |
| Sorting each policy batch on its own, shuffled | 25.6 ms |
| Old per-entry `insert_sorted`, sorted arrival (what the original spike timed) | 16 ms |
| Old per-entry `insert_sorted`, shuffled arrival | **4.1–4.8 s** |
| `merge_streamed`, 25 policy batches, sorted arrival | 32–46 ms (worst single merge 7–11 ms) |
| `merge_streamed`, 25 policy batches, shuffled arrival | **70–76 ms** (worst single merge 11–14 ms) |
| `update()`: 25 `ListingChunk`s + `ListingComplete`, shuffled | 71–92 ms (worst single chunk 11–13 ms) |
| `set_sort_mode(Extension)` | 5.7–6.8 ms |
| `set_sort_mode(Time)` | 3.4–3.9 ms |
| `set_sort_mode(Size)` | 0.8–1.5 ms |
| `set_sort_mode(Unsorted)` | 0.6–1.2 ms |
| `set_sort_mode(Name)` | 2.8–3.1 ms |
| Render one frame, 1k entries, no filter | 0.32–0.42 ms |
| Render one frame, 100k entries, no filter | **0.35–0.44 ms** |
| Render one frame, 1k entries, quick filter `9` | 0.38–0.43 ms |
| Render one frame, 100k entries, quick filter `9` | **0.30–0.46 ms** |

| Ratio | Measured | Required |
|---|---|---|
| Streamed merge (shuffled) / bulk sort (shuffled) | **1.3–2.1×** | ≤ 3× |
| 100k frame / 1k frame, no filter | **1.04–1.09×** | ≤ 2× |
| 100k frame / 1k frame, quick filter active | **0.7–1.2×** | ≤ 2× |
| Old per-entry insert (shuffled) / streamed merge | 41–66× | — |

### One-off 1M-entry measurement

| Benchmark (N = 1,000,000, shuffled arrival) | Result |
|---|---|
| Bulk `sort_by`, shuffled input | 626 ms |
| `merge_streamed`, 80 policy batches | 2,115 ms total; worst single merge **55 ms** |
| Render one frame, 1M entries, no filter | 0.32 ms |
| Streamed merge / bulk sort | 3.4× |

(Before the merge kept its output buffer between batches, the same run took
2,743 ms with an 85 ms worst merge; the retained buffer saves the per-batch
allocation and page-faulting of a fresh multi-megabyte vector.)

## Findings

- **Streaming now costs about one sort.** Merging 25 geometrically growing
  batches into the panel costs 1.3–2.1× a single bulk sort of the same
  shuffled entries, against the requirement's 3×. Roughly a third of that is
  the per-batch sorts themselves (25.6 ms); the rest is the 25 linear merges
  (Σ batch-so-far ≈ 5n entry moves with the policy's growth factor) and the
  reducer's per-batch anchor and viewport work. The old per-entry insert with
  realistic (shuffled) arrival is 41–66× slower — four seconds of reducer
  time during which no key was processed. The worst single merge at 100k is
  11–14 ms, inside one frame, and the event loop's drain budget (~8 ms or 64
  messages) interleaves input between merges in any case.
- **Frame cost no longer depends on directory size — with or without a
  filter.** A 100k-entry frame costs within 10% of a 1k-entry frame, filtered
  or not, because rendering, hit-testing and cursor movement now walk only
  the window of rows shown through the cached visible-index list and the
  comparators and filter read precomputed keys. The original spike's claim
  was accidentally right for the unfiltered case it measured and wrong in
  general; it is now true by construction and checked by the benchmark.
- **Re-sorting is cheap.** Every Ctrl+F3..F7 re-sort of 100k entries takes
  under 7 ms (Extension, which splits each key, is the slowest), down from
  the two allocations per comparison the old comparators made.
- **The million-entry range (design open question).** At 1M entries the
  16,384-entry batch cap holds the batch count to 80 and the worst single
  merge to ~55 ms, with ~2.1 s of total reducer time spread across the
  listing. The panel stays usable — input and redraws interleave between
  merges under the drain budget — but each late merge is a visible hitch of a
  few frames. The listing itself is still delivered progressively and renders
  in 0.32 ms/frame at 1M. **Conclusion:** no structural change is made for
  this range now; it is a known, measured cost. If million-entry directories
  become a target, the follow-up is either a larger late-batch cap (fewer,
  bigger merges — fewer total moves) or an unsorted tail merged on a timer,
  decided against the requirement that entries appear in sorted position as
  they arrive.

## Conclusion

The three audit findings this spike covers are closed by measurement:
streaming a 100k-entry directory costs about one sort and never inserts
entries one at a time (P-02), the first entries reach the panel while the
reader is still enumerating (P-01, see the `list_dir_streaming` tests), the
per-frame visible-list allocation is gone (P-03), and the comparators no
longer allocate (P-04). The `bench_100k_ignored` test remains the manual
acceptance measurement for the two ratio scenarios; a CI performance gate is
left to `ci-and-build-tooling`.
