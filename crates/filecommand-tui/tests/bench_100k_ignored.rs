//! Spike (c) benchmark, extended by `streaming-listing-performance` (design
//! D8): exercises the real streaming-merge, listing-apply, re-sort and render
//! code paths at 100k entries so `docs/spikes/c-100k-entry-render-benchmark.md`
//! is backed by actual numbers, not guesses, and prints the ratios the
//! panel-navigation spec compares ("Streaming 100,000 entries costs about one
//! sort", "Frame cost does not grow with directory size"). Marked `#[ignore]`
//! since it is a manual perf measurement, not a correctness test — run with:
//!
//! ```sh
//! cargo test --release -p filecommand-tui --test bench_100k_ignored -- --ignored --nocapture
//! ```
//!
//! `bench_1m_streamed_merge` is the one-off million-entry measurement that
//! settles the design's open question; it needs a few hundred MB and runs
//! for several seconds.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::backend::TestBackend;
use ratatui::Terminal;

use filecommand_core::listing::{BatchPolicy, Entry, EntryKind, SortMode};
use filecommand_core::panel::{cmp_entries, insert_sorted, ListingProgress, PanelState, SortDirection};
use filecommand_core::theme::{ColorDepth, Theme};
use filecommand_core::{update, Command, PanelSide, State};

use filecommand_tui::views;

const N: usize = 100_000;

/// `n` plain files named `file-000000.txt` … in ascending (already sorted)
/// order, with distinct sizes so Size order is a real permutation.
fn synthetic_entries(n: usize) -> Vec<Entry> {
    (0..n).map(|i| Entry::new(format!("file-{i:06}.txt"), EntryKind::File, i as u64, None, false, false)).collect()
}

/// A deterministic Fisher–Yates shuffle (xorshift64), so "shuffled arrival"
/// is the same permutation on every run and every machine.
fn shuffled(mut entries: Vec<Entry>) -> Vec<Entry> {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    for i in (1..entries.len()).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let j = (x % (i as u64 + 1)) as usize;
        entries.swap(i, j);
    }
    entries
}

/// Split `entries` into the batches the reader's policy would produce on a
/// fast local enumeration: `clamp(sent / 4, 256, 16_384)` each.
fn policy_batches(entries: Vec<Entry>) -> Vec<Vec<Entry>> {
    let policy = BatchPolicy::default();
    let mut batches = Vec::new();
    let mut sent = 0usize;
    let mut rest = entries.into_iter().peekable();
    while rest.peek().is_some() {
        let take = policy.target_len(sent);
        let batch: Vec<Entry> = rest.by_ref().take(take).collect();
        sent += batch.len();
        batches.push(batch);
    }
    batches
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn time<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let out = f();
    (out, start.elapsed())
}

/// Bulk `sort_by` of `entries` under Name order — the "one sort" the spec
/// compares the streamed merge against.
fn bulk_sort(mut entries: Vec<Entry>) -> (Vec<Entry>, Duration) {
    let (_, d) = time(|| entries.sort_by(|a, b| cmp_entries(a, b, SortMode::Name, SortDirection::Asc)));
    (entries, d)
}

/// Merge `batches` into a fresh panel through `merge_streamed`, returning the
/// total time and the longest single merge.
fn streamed_merge(batches: Vec<Vec<Entry>>) -> (PanelState, Duration, Duration) {
    let mut panel = PanelState::new(PathBuf::from(r"C:\bench"));
    let mut total = Duration::ZERO;
    let mut worst = Duration::ZERO;
    for batch in batches {
        let (_, d) = time(|| panel.merge_streamed(batch));
        total += d;
        worst = worst.max(d);
    }
    (panel, total, worst)
}

/// The pre-change strategy — `insert_sorted` once per entry — kept for the
/// record; `insert_sorted` itself survives for single-entry callers.
fn per_entry_insert(entries: Vec<Entry>) -> (Vec<Entry>, Duration) {
    let mut sorted: Vec<Entry> = Vec::with_capacity(entries.len());
    let (_, d) = time(|| {
        for e in entries {
            insert_sorted(&mut sorted, e, SortMode::Name, SortDirection::Asc);
        }
    });
    (sorted, d)
}

fn panel_of(n: usize, quick_filter: Option<&str>) -> PanelState {
    let mut panel = PanelState::new(PathBuf::from(r"C:\bench"));
    let (entries, _) = bulk_sort(synthetic_entries(n));
    panel.set_entries(entries);
    panel.progress = ListingProgress::Complete { count: n };
    panel.cursor = n / 2;
    panel.cursor_user_moved = true;
    panel.set_quick_filter(quick_filter.map(str::to_string));
    panel.ensure_cursor_visible(20);
    panel
}

/// Average frame time of `FRAMES` full-screen renders of `state` at 80×24
/// through a real `TestBackend`, after one warm-up frame.
fn render_frame_avg(state: &State) -> Duration {
    const FRAMES: usize = 200;
    let identity_lines = [String::new(), String::new(), String::new(), String::new()];
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let draw = |terminal: &mut Terminal<TestBackend>| {
        terminal
            .draw(|f| {
                let area = f.area();
                views::render(f.buffer_mut(), area, state, ColorDepth::Ansi16, &identity_lines, "3:04 PM", None);
            })
            .unwrap();
    };
    draw(&mut terminal);
    let (_, d) = time(|| {
        for _ in 0..FRAMES {
            draw(&mut terminal);
        }
    });
    d / FRAMES as u32
}

fn state_with(panel: PanelState) -> State {
    let mut state = State { left: panel.clone(), right: panel, ..State::empty(Theme::classic()) };
    state.term_size = (80, 24);
    state
}

#[test]
#[ignore]
fn bench_streaming_listing_performance_100k() {
    eprintln!();
    eprintln!("== streaming-listing-performance benchmark, N = {N} (release build) ==");

    // --- sorting and streaming ------------------------------------------
    let (sorted, bulk_presorted) = bulk_sort(synthetic_entries(N));
    eprintln!("bulk sort_by (Name), already-sorted input        {:>9.3} ms   (the original spike's bench: pre-sorted input)", ms(bulk_presorted));
    let (_, bulk) = bulk_sort(shuffled(synthetic_entries(N)));
    eprintln!("bulk sort_by (Name), shuffled input              {:>9.3} ms   (the spec's \"one bulk sort of the same entries\")", ms(bulk));
    let mut batches_to_sort = policy_batches(shuffled(synthetic_entries(N)));
    let (_, batch_sorts) = time(|| {
        for batch in &mut batches_to_sort {
            batch.sort_by(|a, b| cmp_entries(a, b, SortMode::Name, SortDirection::Asc));
        }
    });
    eprintln!("sorting each policy batch on its own, shuffled   {:>9.3} ms   (the merge's per-batch sort share)", ms(batch_sorts));

    let (_, old_sorted) = per_entry_insert(synthetic_entries(N));
    eprintln!("old per-entry insert_sorted, sorted arrival      {:>9.3} ms   (pre-sorted input: every insert lands at the end)", ms(old_sorted));
    let (_, old_shuffled) = per_entry_insert(shuffled(synthetic_entries(N)));
    eprintln!("old per-entry insert_sorted, shuffled arrival    {:>9.3} ms   (reference only)", ms(old_shuffled));

    let sorted_batches = policy_batches(synthetic_entries(N));
    let n_batches = sorted_batches.len();
    let (panel_sorted, merge_sorted, worst_sorted) = streamed_merge(sorted_batches);
    assert_eq!(panel_sorted.entries, sorted);
    eprintln!(
        "merge_streamed, {n_batches:>2} policy batches, sorted arrival   {:>9.3} ms   (worst single merge {:.3} ms)",
        ms(merge_sorted),
        ms(worst_sorted)
    );

    let shuffled_batches = policy_batches(shuffled(synthetic_entries(N)));
    let (panel_shuffled, merge_shuffled, worst_shuffled) = streamed_merge(shuffled_batches);
    assert_eq!(panel_shuffled.entries, sorted, "the merge equals one bulk sort");
    eprintln!(
        "merge_streamed, {n_batches:>2} policy batches, shuffled arrival {:>9.3} ms   (worst single merge {:.3} ms)",
        ms(merge_shuffled),
        ms(worst_shuffled)
    );

    // --- the whole reducer path: ListingChunk → merge → anchor → viewport ----
    let mut state = State::empty(Theme::classic());
    state.term_size = (80, 24);
    state.left.listing_request = Some(1);
    let mut apply_total = Duration::ZERO;
    let mut apply_worst = Duration::ZERO;
    for batch in policy_batches(shuffled(synthetic_entries(N))) {
        let (s, d) = time(|| update(state, Command::ListingChunk { panel: PanelSide::Left, entries: batch, request: 1 }).0);
        state = s;
        apply_total += d;
        apply_worst = apply_worst.max(d);
    }
    let (s, d) = time(|| update(state, Command::ListingComplete { panel: PanelSide::Left, total: N, unreadable: 0, request: 1 }).0);
    state = s;
    apply_total += d;
    assert_eq!(state.left.entries.len(), N);
    assert_eq!(state.left.entries, sorted);
    eprintln!(
        "update(): {n_batches} ListingChunks + ListingComplete, shuffled {:>9.3} ms   (worst single chunk {:.3} ms)",
        ms(apply_total),
        ms(apply_worst)
    );

    // --- re-sort by each mode (Ctrl+F3..F6) ------------------------------
    let mut panel = panel_of(N, None);
    for mode in [SortMode::Extension, SortMode::Time, SortMode::Size, SortMode::Unsorted, SortMode::Name] {
        let (_, d) = time(|| panel.set_sort_mode(mode));
        eprintln!("set_sort_mode({mode:?}) at {N}                 {:>9.3} ms", ms(d));
    }

    // --- rendering ---------------------------------------------------------
    let render_1k = render_frame_avg(&state_with(panel_of(1_000, None)));
    let render_100k = render_frame_avg(&state_with(panel_of(N, None)));
    let render_1k_filtered = render_frame_avg(&state_with(panel_of(1_000, Some("9"))));
    let render_100k_filtered = render_frame_avg(&state_with(panel_of(N, Some("9"))));
    eprintln!("render frame, 1k entries, no filter               {:>9.3} ms/frame", ms(render_1k));
    eprintln!("render frame, 100k entries, no filter             {:>9.3} ms/frame", ms(render_100k));
    eprintln!("render frame, 1k entries, quick filter \"9\"        {:>9.3} ms/frame", ms(render_1k_filtered));
    eprintln!("render frame, 100k entries, quick filter \"9\"      {:>9.3} ms/frame", ms(render_100k_filtered));

    // --- the ratios the spec states --------------------------------------
    let merge_ratio = merge_shuffled.as_secs_f64() / bulk.as_secs_f64();
    let render_ratio = render_100k.as_secs_f64() / render_1k.as_secs_f64();
    let render_ratio_filtered = render_100k_filtered.as_secs_f64() / render_1k_filtered.as_secs_f64();
    eprintln!();
    eprintln!("ratio: streamed merge (shuffled) / bulk sort       {merge_ratio:.2}x   (spec: <= 3x)");
    eprintln!("ratio: 100k frame / 1k frame, no filter            {render_ratio:.2}x   (spec: <= 2x)");
    eprintln!("ratio: 100k frame / 1k frame, quick filter         {render_ratio_filtered:.2}x   (spec: <= 2x)");
    eprintln!("ratio: old per-entry insert (shuffled) / merge     {:.1}x", old_shuffled.as_secs_f64() / merge_shuffled.as_secs_f64());
    eprintln!();
    assert!(merge_ratio <= 3.0, "panel-navigation \"Streaming 100,000 entries costs about one sort\": {merge_ratio:.2}x");
    assert!(render_ratio <= 2.0, "panel-navigation \"Frame cost does not grow with directory size\": {render_ratio:.2}x");
    assert!(
        render_ratio_filtered <= 2.0,
        "panel-navigation \"Frame cost does not grow with directory size\" (filtered): {render_ratio_filtered:.2}x"
    );
}

/// The design's open question (streaming-listing-performance): does a
/// million-entry directory need a different structure while streaming? One
/// measurement of the merge path at 1M, shuffled arrival, in policy batches.
#[test]
#[ignore]
fn bench_1m_streamed_merge() {
    const M: usize = 1_000_000;
    eprintln!();
    eprintln!("== one-off 1M-entry measurement (release build) ==");
    let (_, bulk) = bulk_sort(shuffled(synthetic_entries(M)));
    eprintln!("bulk sort_by (Name) at 1M, shuffled input        {:>9.3} ms", ms(bulk));
    let batches = policy_batches(shuffled(synthetic_entries(M)));
    let n_batches = batches.len();
    let (panel, total, worst) = streamed_merge(batches);
    assert_eq!(panel.entries.len(), M);
    eprintln!("merge_streamed, {n_batches} policy batches, shuffled   {:>9.3} ms total, worst single merge {:.3} ms", ms(total), ms(worst));
    let render = render_frame_avg(&state_with({
        let mut p = panel;
        p.progress = ListingProgress::Complete { count: M };
        p.cursor = M / 2;
        p.cursor_user_moved = true;
        p.ensure_cursor_visible(20);
        p
    }));
    eprintln!("render frame, 1M entries, no filter               {:>9.3} ms/frame", ms(render));
    eprintln!("ratio: streamed merge / bulk sort at 1M            {:.2}x", total.as_secs_f64() / bulk.as_secs_f64());
}
