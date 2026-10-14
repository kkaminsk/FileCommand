# Design: streaming-listing-performance

## Context

Verified on `build/open-proposals-2026-10` (commit `1bada6f`):

- **Reader.** `FsReader` has one method, `read_dir(&self, &Path) -> io::Result<Vec<RawDirEntry>>` (`listing.rs:439-441`). `StdFsReader::read_dir` (`listing.rs:465-481`) loops over `std::fs::read_dir` and pushes every entry into a `Vec` before returning. `list_dir_chunked` (`listing.rs:499-513`) calls it, then slices the finished vector with `raw.chunks(chunk_size)` and clones each raw entry into an `Entry`. `worker::spawn_listing` (`worker.rs:178-203`) passes `CHUNK_SIZE = 256`. Nothing reaches the UI until enumeration has finished (P-01 confirmed). Other `FsReader` users: `worker::gather_info`, `listing::list_child_dirs`, `listing::find_in_subtree`, and test fakes in `listing.rs` and `worker.rs`.
- **Apply.** `apply_listing_event` (`update.rs:3848-3852`) calls `PanelState::insert_streamed` once per entry; `insert_streamed` (`panel.rs:467-475`) calls `insert_sorted` (`panel.rs:1001-1004`: `partition_point` + `Vec::insert`). While the user has not moved, the cursor is pinned to 0; once moved it is only clamped, so its **index** stays fixed and the entry under it changes whenever an entry is inserted above it. Per chunk the handler then runs the in-flight re-read anchor (`apply_reread_anchor`, `panel.rs:589-611`) and `reconcile_panel_viewport`.
- **Drain.** `drain_events` (`app.rs:828-839`) is `while let Ok(cmd) = rx.try_recv() { apply(..) }` with no budget, called at the top of every loop iteration and again just before drawing (`app.rs:350`, `app.rs:443-447`).
- **Visible list.** `visible_indices()` (`panel.rs:696-701`) returns `(0..len).collect()` without a filter and, with one, lowercases every name and the pattern per entry. Callers: `ensure_cursor_visible` (`panel.rs:406`), `ensure_cursor_visible_brief` (`panel.rs:432`), `move_cursor_filtered` (`panel.rs:452`), `apply_reread_anchor` (`panel.rs:606`), `snap_cursor_to_visible` (`panel.rs:740`), `update::jump_to_prefix` (`update.rs:2020`, which also clones every visible `Entry`), and per frame `views/panel.rs` `hit_test` (`:385`), Full-mode rows (`:611`), Brief-mode rows (`:739`).
- **Comparators.** `lower_name` (`listing.rs:147-149`) allocates; `cmp_by_name`, `cmp_by_extension`, `cmp_by_time`, `cmp_by_size` (`listing.rs:162-183`) call it for both operands (the latter two on ties).
- **Benchmark.** `crates/filecommand-tui/tests/bench_100k_ignored.rs` feeds `insert_sorted` names that are *already in sorted order* (`file-000000.txt`, …), so every insert lands at the end and nothing shifts: its 248–258 ms is almost entirely comparator allocation (P-04), not the quadratic shift P-02 describes — the shift cost only shows for unsorted arrival order, which the benchmark does not exercise. The render bench assigns `panel.entries` directly.
- **Unnoticed by the audit:** the in-flight re-read anchor resolves the cursor with `names.iter().find_map(|n| entries.iter().position(..))` on every chunk. Until the anchored entry has arrived, each chunk costs O(captured names × entries so far) — for a re-read of a 100k-entry directory with the cursor mid-list that is billions of comparisons per chunk, a far larger stall than P-02.

## Goals / Non-Goals

**Goals:**

- First entries and a live `Reading… N` count while enumeration is still running, on local disks and slow shares alike.
- No listing work blocks input or drawing for more than a frame-sized slice; a worker flood can never starve input.
- Total streaming cost close to one bulk sort; per-frame cost proportional to visible rows, with or without a quick filter.
- Entries still appear in sorted position as they arrive (existing "Streaming listing mini-status" requirement), and the cursor stays on its entry once the user has moved it.
- Measured, documented numbers that replace the stale spike conclusion.

**Non-Goals:**

- Request ids and cancellation of superseded listings (`listing-request-guard`, which lands first).
- How links and unreadable entries are classified (`link-aware-listing`, R-05/R-21).
- Quick-filter cursor edge cases at roots without `..`, hidden selections, and `wildcard_match` (`filter-selection-safety`, R-17/R-19).
- Idle redraw, key coalescing, progress throttling (`idle-aware-event-loop`, P-05/P-06/P-11).
- `hit_test` cloning each visible row's name and duplicated dialog geometry (B-08, `reducer-overlay-structure`).
- A CI perf gate (B-03/B-09 CI part, `ci-and-build-tooling`); this change keeps `bench_100k_ignored` as a manual measurement.
- The selection mini-status total. While any entry is selected, `selection_status` → `selected_bytes` (`panel.rs:669-671`) walks every entry with a set lookup on each frame. That is O(n) per frame only while a selection exists and is not cited by the audit; the requirement's frame-cost clause is scoped to "no entries selected" and a cached selection total is left to a follow-up.

## Decisions

### D1: Streaming reader via a callback with a `Vec` default

Add to `FsReader`:

```rust
fn for_each_entry(&self, path: &Path, f: &mut dyn FnMut(RawDirEntry) -> ControlFlow<()>) -> io::Result<()> {
    for e in self.read_dir(path)? { if f(e).is_break() { break; } }
    Ok(())
}
```

Fakes keep implementing only `read_dir`. `StdFsReader` overrides `for_each_entry` with the real per-entry loop over `std::fs::read_dir` and implements `read_dir` by collecting through it, so `gather_info`, `list_child_dirs`, and `find_in_subtree` are untouched. `ControlFlow::Break` lets the listing worker stop mid-enumeration when `listing-request-guard`'s `CancelFlag` is set — the check moves from "between chunks" to "per entry".

*Alternatives:* return an `Iterator` — rejected: a boxed iterator over `ReadDir` with per-item `io::Result` and `to_long_path` lifetimes is clumsier for fakes and gains nothing over a callback on a dedicated worker thread. A channel per entry — rejected: per-entry sends are exactly the flood the batching avoids.

### D2: Batches bounded by a growing count and ~50 ms, decided by a pure policy

`list_dir_streaming(reader, path, cancel, policy, now: &dyn Fn() -> Instant, on_chunk)` replaces `list_dir_chunked`. It flushes the batch when either

- the batch reaches `clamp(sent_so_far / 4, 256, 16_384)` entries — the size grows geometrically with what has already been sent, so the number of batches for *n* entries is O(log n) on fast media, which keeps D3's total merge cost near-linear; or
- at least 50 ms have passed since the last flush when the next entry arrives — so slow media still show progress at ~20 Hz.

The `..` chunk is still sent first. `BatchPolicy::should_flush(batch_len, sent, since_last_flush)` is a pure function, and the clock is injected, so the policy is unit-tested without sleeping. The cap bounds the worst single merge (D3) at large *n*.

*Alternatives:* fixed 256-entry chunks — ~390 merges for 100k entries, each O(n), ≈20M element moves; acceptable but wasteful, and pathological at 1M. Time-only batching — a fast local enumeration would produce one giant batch, i.e. P-01 again.

### D3: Sort each batch, merge once — not per-entry insert, not sort-on-complete

`PanelState::merge_streamed(batch: Vec<Entry>)` stable-sorts the batch with `cmp_entries` (cheap with D4 keys), then merges it into `entries` in one O(n + k) pass: extend `entries` by `k`, then merge from the back so no second buffer is allocated. Ties keep existing entries first, matching today's `insert_sorted` (`partition_point(.. != Greater)`). `Unsorted` mode appends, which is enumeration order. During the same pass the merge counts how many batch entries land before the cursor's old index: if the user has moved, `cursor += that count` (the cursor stays on its entry); otherwise it stays pinned at 0. Selection is keyed by name and needs nothing. On a same-directory re-read, the in-flight re-read anchor (D6) is applied after the merge, exactly as today, and still decides the cursor while the re-read streams; the requirement's "stays on its entry" clause defers to it explicitly.

*Alternatives:*
- *Append and sort once on `ListingComplete`* — the cheapest total (one sort), but entries would show in enumeration order while streaming and then reshuffle at completion, with the cursor's entry jumping; it breaks the existing "Entries SHALL be inserted in sorted position as they arrive" and "Cursor holds on first row during streaming" contract. Rejected.
- *Append and fully re-sort per batch* — O(n log n) per batch, strictly worse than a merge.
- *Keep per-entry insert with cheaper keys* — fixes P-04's constant but not P-02's O(n²) shift for unsorted arrival (Unix `readdir`, ReFS, network redirectors). Rejected.

### D4: A lowercase key computed once per entry

`Entry` gains a private `sort_key: Box<str>` (`name.to_string_lossy().to_lowercase()`) set by `Entry::new(name, kind, size, modified, hidden)`, `Entry::parent_dir()`, and `From<RawDirEntry>` — the only constructors — with a `sort_key()` accessor. `cmp_by_name` compares keys; `cmp_by_extension` takes `extension_of` on the keys; `cmp_by_time`/`cmp_by_size` break ties on keys. Quick-filter matching becomes `entry.sort_key().contains(pattern_lower)` with the pattern lowercased once per edit. Orderings are byte-for-byte what they are today (same lowercase function, same tie-breaks), which a proptest pins against the old allocating comparators. The key is private so no struct literal can construct an `Entry` with an inconsistent key; the ~60 literal sites (mostly tests and the benchmark) move to `Entry::new`. `name` stays `pub` for reading; nothing in production mutates it after construction, and the field doc says so.

Memory: one extra boxed string per entry (≈ name length + 16 bytes; ≈4 MB at 100k entries), accepted.

*Alternative:* allocation-free per-comparison case folding (`chars().flat_map(char::to_lowercase)`) — no struct change, but still O(name length) per comparison, slightly different from `str::to_lowercase` for word-final `Σ`, and does nothing for filter matching. Rejected.

### D5: Cached visible list; identity without a filter

`PanelState` gains a private `filtered: Option<Vec<usize>>`: `None` while no quick filter is active (the visible list is the identity, never materialized), `Some(ascending entry indices)` while one is. It is rebuilt — one O(n) pass over precomputed keys — by every method that changes the entries or the pattern: `merge_streamed`, `resort`/`set_sort_mode`, `begin_new_listing`, `quick_filter_push`/`_backspace`/`activate_quick_filter`/`clear_quick_filter`, `adopt_tab_data` (the cache is not stashed in `TabData`; it is rebuilt from the restored pattern), and a new `set_entries(Vec<Entry>)` for wholesale replacement (tests, benchmark). Accessors:

- `visible_len()`;
- `visible_at(pos) -> Option<usize>` (entry index at a visible position);
- `visible_position(entry_index) -> Option<usize>` — identity, or `binary_search` in the ascending list (O(log n));
- `visible_window(start, len) -> impl Iterator<Item = usize>` for rendering.

All production callers listed in Context switch to these; `ensure_cursor_visible*` become O(log n); `jump_to_prefix` matches over the visible window without cloning entries. `visible_indices()` stays as an allocating convenience documented as not for per-frame use (tests and the TUI integration test call it). A `debug_assert!` in the accessors checks cached indices are in range, catching a direct `entries` mutation that bypassed `set_entries`.

*Alternatives:* a lazily recomputed cache behind `RefCell` with a generation stamp — interior mutability in `State` and a stamp that direct `entries` writes would not bump; rejected. Recomputing only per frame in the TUI — leaves the reducer's per-keystroke O(n) work. Rejected.

### D6: Re-read anchor resolution proportional to the batch

When the in-flight re-read anchor is captured, also build `rank: HashMap<OsString, usize>` giving each captured name its priority in the existing resolution order (`names[cursor]` = 0, then `names[cursor+1..]`, then `names[..cursor]` reversed). `merge_streamed` looks up only the **batch's** entries in the map and keeps the best rank seen so far plus that entry's index (updated by the merge's position bookkeeping). The cursor resolves to the best-ranked present name — the same answer as today's scan, at O(k) per batch instead of O(names × entries). The anchor's scroll-offset restore and clamp are unchanged.

This is a pure re-implementation: the resolved cursor and offset are re-applied after every batch exactly when today's `apply_reread_anchor` would apply them (whenever some captured name is present, whether or not the best rank changed with this batch), the anchor is still consumed on `ListingComplete` after find-file's deferred settle, and `cursor_user_moved` is set on a hit as today. So every scenario of the in-flight "Same-directory re-read preserves cursor and scroll position" holds unchanged, including the case where a better-ranked name arrives in a later batch. A re-sort while the re-read streams (Ctrl+F3..F6) invalidates the tracked index; it is re-found with one linear scan for the best-ranked name, still O(n) rather than O(names × entries).

### D7: Budgeted drain with a backlog signal

`drain_events` stops after 64 commands or 8 ms, whichever comes first, and reports `backlog: bool`. Because `mpsc::Receiver` cannot peek, a message taken by the budget check is held in a one-slot buffer on `Runtime` and applied first by the next drain. When `backlog` is set, the loop polls input with a zero timeout, then draws, then continues draining — so input and redraw interleave with a flood instead of waiting for it. The pre-draw drain in the `dirty` branch uses the same budget.

### D8: Benchmarks are the acceptance measurement

`bench_100k_ignored.rs` stays the measurement (`cargo test --release -p filecommand-tui --test bench_100k_ignored -- --ignored --nocapture`) and gains: streamed merge of 100k entries in D2's batch sizes, sorted and shuffled arrival; the old per-entry insert with shuffled arrival (for the record); 100k entries applied through `update()` `ListingChunk`s end to end; re-sort by each mode; render at 1k and 100k entries with and without an active quick filter. The spec scenarios compare numbers measured in the same run (merge total ≤ 3× bulk sort; 100k render ≤ 2× 1k render), so they hold on any machine. The spike document is re-run and rewritten with the new numbers, and its "render cost is independent of directory size" claim is replaced by what the new measurement shows.

## Risks / Trade-offs

- [A single merge at ~1M entries still moves the whole vector (tens of MB) in one reducer call] → the 16,384-entry batch cap keeps batches few (~60 at 1M) and each merge a few milliseconds on current hardware; D7 keeps input interleaved between merges. If measurement at 1M shows frame-sized stalls, see the Open Question.
- [The 50 ms bound is checked when an entry arrives, so entries read just before an enumeration stall (for example the end of one SMB directory page) wait in the batch until the next entry or the end of enumeration] → the wait is at most one server round trip; the requirement states the bound as "while entries keep arriving". A flush timer on a second thread would remove it and is left until a measurement on a slow share shows it matters.
- [A fed-back command (drive enumeration) queued behind a worker flood can miss the frame it used to land on] → only under a flood, and only by one frame; the budget is what keeps the flood from starving input.
- [The cursor following its entry is a visible change for users who move during streaming] → it replaces a drift where the highlighted entry silently changed underneath a stationary cursor; covered by a new scenario.
- [Tests that assign `panel.entries` directly with an active filter would read a stale cache] → `set_entries` plus the debug assertion; the migration task covers the quick-filter test module.
- [Extra memory for keys] → ≈4 MB per 100k entries.

## Migration Plan

None — no persisted data or configuration changes.

## Open Questions

- Whether directories in the million-entry range need a different structure during streaming (for example an unsorted tail merged on a timer) — decide from the 1M measurement taken while implementing D8; the requirement is written against the 100k benchmark.
