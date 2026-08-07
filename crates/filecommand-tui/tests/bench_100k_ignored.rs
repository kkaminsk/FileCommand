//! Spike (c) benchmark: exercises the real streaming-insert and render code
//! paths at 100k entries so `docs/spikes/c-100k-entry-render-benchmark.md`
//! is backed by actual numbers, not guesses. Marked `#[ignore]` since it's a
//! manual perf spike, not a correctness test — run with:
//!
//! ```sh
//! cargo test --release -p filecommand-tui --test bench_100k_ignored -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::time::Instant;

use ratatui::backend::TestBackend;
use ratatui::Terminal;

use filecommand_core::listing::{Entry, EntryKind};
use filecommand_core::panel::{insert_sorted, ListingProgress, PanelState, SortDirection};
use filecommand_core::theme::{ColorDepth, Theme};
use filecommand_core::{PanelSide, State, UiPhase};

use filecommand_tui::views;

const N: usize = 100_000;

fn synthetic_entries(n: usize) -> Vec<Entry> {
    (0..n)
        .map(|i| Entry { name: format!("file-{i:06}.txt").into(), kind: EntryKind::File, size: i as u64, modified: None })
        .collect()
}

#[test]
#[ignore]
fn bench_streamed_sorted_insert_100k() {
    let entries = synthetic_entries(N);
    let start = Instant::now();
    let mut sorted: Vec<Entry> = Vec::new();
    for e in entries {
        insert_sorted(&mut sorted, e, SortDirection::Asc);
    }
    let elapsed = start.elapsed();
    eprintln!("streamed insert_sorted x {N}: {elapsed:?} ({:.2} ms/entry avg)", elapsed.as_secs_f64() * 1000.0 / N as f64);
    assert_eq!(sorted.len(), N);
}

#[test]
#[ignore]
fn bench_bulk_sort_100k() {
    let mut entries = synthetic_entries(N);
    let start = Instant::now();
    entries.sort_by(|a, b| filecommand_core::panel::cmp_entries(a, b, SortDirection::Asc));
    let elapsed = start.elapsed();
    eprintln!("bulk sort_by x {N}: {elapsed:?}");
    assert_eq!(entries.len(), N);
}

#[test]
#[ignore]
fn bench_render_frame_with_100k_entries() {
    let mut panel = PanelState::new(PathBuf::from(r"C:\bench"));
    let mut entries = synthetic_entries(N);
    entries.sort_by(|a, b| filecommand_core::panel::cmp_entries(a, b, SortDirection::Asc));
    panel.entries = entries;
    panel.progress = ListingProgress::Complete { count: N };
    panel.cursor = N / 2;

    let state = State {
        left: panel.clone(),
        right: panel,
        active: PanelSide::Left,
        command_line: String::new(),
        phase: UiPhase::Panels,
        theme: Theme::classic(),
        term_size: (80, 24),
    };
    let identity_lines = [String::new(), String::new(), String::new(), String::new()];

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    // Warm up allocations once before timing the steady-state frame cost.
    terminal
        .draw(|f| {
            let area = f.area();
            views::render(f.buffer_mut(), area, &state, ColorDepth::Ansi16, &identity_lines);
        })
        .unwrap();

    const FRAMES: usize = 200;
    let start = Instant::now();
    for _ in 0..FRAMES {
        terminal
        .draw(|f| {
            let area = f.area();
            views::render(f.buffer_mut(), area, &state, ColorDepth::Ansi16, &identity_lines);
        })
        .unwrap();
    }
    let elapsed = start.elapsed();
    eprintln!(
        "render {FRAMES} frames against a {N}-entry panel: {elapsed:?} ({:.3} ms/frame avg)",
        elapsed.as_secs_f64() * 1000.0 / FRAMES as f64
    );
}
