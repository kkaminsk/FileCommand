# Tasks: streaming-listing-performance

Prerequisite: `listing-request-guard` is merged (listing request ids and the listing worker's `CancelFlag`).

## 1. Precomputed sort keys (`crates/filecommand-core/src/listing.rs`)

- [x] 1.1 Add private `Entry::sort_key: Box<str>` with `Entry::new(name, kind, size, modified, hidden)`, `sort_key()`, and key computation in `Entry::parent_dir()` and `From<RawDirEntry>` (design D4)
- [x] 1.2 Rework `cmp_by_name`, `cmp_by_extension`, `cmp_by_time`, `cmp_by_size` to compare `sort_key()`; remove `lower_name` (design D4)
- [x] 1.3 Proptest: new comparators agree with the previous allocating implementation (kept in the test module) for every mode and direction (panel-navigation "Orders are unchanged by precomputed keys")
- [x] 1.4 Migrate every `Entry { .. }` struct literal in core, TUI, tests, and `bench_100k_ignored.rs` to `Entry::new`

## 2. Streaming reader and batching (`listing.rs`, `crates/filecommand-tui/src/worker.rs`)

- [x] 2.1 Add `FsReader::for_each_entry` with a default over `read_dir`; implement it in `StdFsReader` as the real per-entry loop and implement `StdFsReader::read_dir` by collecting through it (design D1)
- [x] 2.2 Add `BatchPolicy` (`min 256`, `max 16_384`, growth `sent / 4`, `max_age 50 ms`) with a pure `should_flush(batch_len, sent, since_last_flush)` and unit tests for each bound (design D2)
- [x] 2.3 Replace `list_dir_chunked` with `list_dir_streaming(reader, path, cancel, policy, now, on_chunk)`: break the enumeration via `ControlFlow` when the cancel flag is set; flush per the policy; flush the remainder at the end (design D1, D2)
- [x] 2.4 Tests with a fake reader and an injected clock: batches grow geometrically on fast input; an entry stream advancing the clock 60 ms per entry flushes every entry; a flag set mid-enumeration stops reading and sends nothing further (panel-navigation "First entries appear before enumeration finishes")
- [x] 2.5 `worker.rs` `spawn_listing`: drive `list_dir_streaming` with `Instant::now` (keeping the `..` chunk first and the request id on every reply)

## 3. Merge per batch (`crates/filecommand-core/src/panel.rs`, `update.rs`)

- [x] 3.1 Add `PanelState::merge_streamed(Vec<Entry>)`: stable-sort the batch, back-to-front in-place merge with existing-first ties, `Unsorted` appends; cursor pinned at 0 until the user moves, otherwise advanced by the number of batch entries merged before it (design D3)
- [x] 3.2 `update.rs` `apply_listing_event`: call `merge_streamed` once per `ListingChunk` instead of `insert_streamed` per entry; remove `insert_streamed`
- [x] 3.3 Re-read anchor: build the name→rank map at capture; `merge_streamed` resolves the best-ranked present name from the batch only and keeps its index current across later merges; `apply_reread_anchor` keeps its offset restore/clamp (design D6)
- [x] 3.4 Tests: merge result equals a bulk stable sort of all entries for random batch splits and every sort mode (proptest); cursor stays on its entry across a batch that sorts before it (panel-navigation "Cursor stays on its entry while entries stream in"); cursor stays at 0 when the user has not moved; existing re-read anchor tests pass unchanged plus one where the anchored entry arrives in the last batch of many, one where a better-ranked captured name arrives after a worse-ranked one (cursor moves to it, as today), one where the user moves during a re-read stream (the next batch re-applies the anchor, as today), and one with a Ctrl+F3 re-sort mid-re-read (design D6)

## 4. Cached visible list (`panel.rs`, `update.rs`, `crates/filecommand-tui/src/views/panel.rs`)

- [x] 4.1 Add private `filtered: Option<Vec<usize>>`, rebuilt in `merge_streamed`, `resort`, `set_sort_mode`, `begin_new_listing`, the quick-filter mutators, `adopt_tab_data`, and new `set_entries` (design D5)
- [x] 4.2 Add `visible_len`, `visible_at`, `visible_position`, `visible_window`; quick-filter matching uses `sort_key()` and a once-lowercased pattern (design D4, D5)
- [x] 4.3 Switch `ensure_cursor_visible`, `ensure_cursor_visible_brief`, `move_cursor_filtered`, `snap_cursor_to_visible`, `apply_reread_anchor`, and `update::jump_to_prefix` (no entry clones) to the accessors
- [x] 4.4 Switch `views/panel.rs` `hit_test`, the Full-mode row loop, and the Brief-mode body to `visible_len`/`visible_window`; scrollbar thumb uses `visible_len`
- [x] 4.5 Keep `visible_indices()` as a documented allocating convenience; migrate tests that assign `entries` under an active filter to `set_entries`; add a `debug_assert!` that cached indices are in range
- [x] 4.6 Tests: cache equals a from-scratch filter after every mutator, including across a tab switch and a merge with an active filter; all existing quick-filter, scrolling, and Brief-mode tests pass; insta snapshots unchanged

## 5. Budgeted drain (`crates/filecommand-tui/src/app.rs`)

- [x] 5.1 `drain_events`: stop after 64 commands or 8 ms, hold the next taken message in a one-slot `Runtime` buffer applied first by the next drain, and return a `backlog` flag (design D7)
- [x] 5.2 Main loop: when `backlog` is set, poll input with a zero timeout and draw before draining further; the pre-draw drain uses the same budget (panel-navigation "Input stays responsive while a 100,000-entry directory streams in")
- [x] 5.3 Tests: a channel pre-filled with 1,000 commands is applied in budgeted slices with order preserved and nothing lost; the held message is applied first next time

## 6. Benchmark and spike document

- [x] 6.1 Extend `crates/filecommand-tui/tests/bench_100k_ignored.rs`: streamed merge in policy batch sizes (sorted and shuffled arrival), old per-entry insert with shuffled arrival (reference), 100k entries applied through `update()` chunks, re-sort by each mode, render at 1k and 100k with and without a quick filter; print the ratios the spec uses (design D8)
- [x] 6.2 Run `cargo test --release -p filecommand-tui --test bench_100k_ignored -- --ignored --nocapture`; confirm merge total ≤ 3× bulk sort and 100k render ≤ 2× 1k render (panel-navigation "Streaming 100,000 entries costs about one sort", "Frame cost does not grow with directory size")
- [x] 6.3 Rewrite `docs/spikes/c-100k-entry-render-benchmark.md` with the new method, numbers, and conclusions, replacing the stale "render cost is independent of directory size" claim and noting that the original insert bench used pre-sorted input
- [x] 6.4 Record a one-off 1M-entry measurement in the spike document to settle the design's open question

## 7. Verify

- [x] 7.1 `cargo test --workspace` green
- [x] 7.2 `cargo clippy --workspace --all-targets` with no new warnings
- [x] 7.3 `openspec validate streaming-listing-performance --strict` clean
- [ ] 7.4 Manual (Windows): open a 100k-entry directory and a large directory on a network share → `Reading… N` grows while entries fill in sorted order; holding Down while it streams moves the cursor smoothly
