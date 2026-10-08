//! Render-only motion model for the decorative UI animations (ui-animations
//! "Render-only motion model driven by the reducer clock").
//!
//! Every animation is a pure function of `elapsed = State::clock_ms - stamp`,
//! where `stamp` is the reducer clock reading recorded when the animated
//! surface opened or changed. This module owns only the arithmetic — per-mille
//! progress, reveal counts, the title-sweep position, and the dialog iris
//! rectangle — plus the named duration constants (design D6), so renderers
//! and tests reference one definition. It reads no clock and draws nothing.

use crate::dialogs::OverlayRect;

/// Full progress, in per-mille. Progress is carried as an integer so the
/// helpers below stay exact and snapshot-stable (no float rounding).
pub const FULL: u16 = 1000;

/// Splash frame trace: perimeter cells revealed clockwise from the top-left
/// corner over `0..SPLASH_TRACE_MS`.
pub const SPLASH_TRACE_MS: u64 = 200;
/// When the first identity line starts revealing.
pub const SPLASH_IDENTITY_START_MS: u64 = SPLASH_TRACE_MS;
/// Delay between one identity line starting and the next.
pub const SPLASH_IDENTITY_STAGGER_MS: u64 = 80;
/// How long one identity line takes to reveal outward from its center.
pub const SPLASH_IDENTITY_LINE_MS: u64 = 120;
/// How long a freshly revealed identity character stays in `splash.frame`
/// before settling to its line's final role.
pub const SPLASH_IDENTITY_FLASH_MS: u64 = 66;
/// The product-name highlight sweep window in time.
pub const SPLASH_SWEEP_START_MS: u64 = 560;
pub const SPLASH_SWEEP_END_MS: u64 = 760;
/// Width of the sweeping `splash.version` window, in cells.
pub const SPLASH_SWEEP_WINDOW: u16 = 3;
/// After this the splash is identical to the non-animated splash — still
/// inside the unchanged minimum hold.
pub const SPLASH_ENTRANCE_MS: u64 = SPLASH_SWEEP_END_MS;
// The entrance must finish inside the existing hold (startup-splash "Splash
// entrance animation"; design D6) — enforced at compile time so a retimed
// constant can't silently outrun the splash.
const _: () = assert!(SPLASH_ENTRANCE_MS < crate::update::SPLASH_MIN_HOLD_MS);
/// Panels wipe in top-to-bottom after the splash.
pub const PANEL_WIPE_MS: u64 = 150;
/// F9 menu bar sweeps in left-to-right.
pub const MENU_BAR_SWEEP_MS: u64 = 100;
/// Pop-up menus (pull-downs, F2 user menu, Enter action menu) unfold.
pub const POPUP_UNFOLD_MS: u64 = 100;
/// A newly highlighted pop-up row renders in `menu.hotkey` this long.
pub const HIGHLIGHT_SETTLE_MS: u64 = 50;
/// Modal dialogs open through a centered iris.
pub const DIALOG_IRIS_MS: u64 = 100;

/// Progress through `duration_ms` after `elapsed_ms`, in per-mille
/// (`0..=FULL`). Clamps at both ends; a zero duration is already complete.
pub fn progress(elapsed_ms: u64, duration_ms: u64) -> u16 {
    if duration_ms == 0 || elapsed_ms >= duration_ms {
        return FULL;
    }
    // elapsed < duration, so the quotient is < FULL and fits `u16`.
    (elapsed_ms.saturating_mul(FULL as u64) / duration_ms) as u16
}

/// How many of `total` items are revealed at `progress` (per-mille): 0 at
/// progress 0, exactly `total` at `FULL`, never more.
pub fn reveal_count(progress: u16, total: usize) -> usize {
    let p = progress.min(FULL) as usize;
    total.saturating_mul(p) / FULL as usize
}

/// Left edge of a `window`-cell highlight crossing a span `width` cells wide
/// at `progress`: fully off the left edge (`-window`) at progress 0, fully
/// off the right edge (`width`) at `FULL`, so the window enters and exits
/// the span rather than popping in or out.
pub fn sweep_pos(progress: u16, width: u16, window: u16) -> i32 {
    let travel = width as i64 + window as i64;
    let p = progress.min(FULL) as i64;
    (-(window as i64) + travel * p / FULL as i64) as i32
}

/// The centered iris window onto a dialog whose finished rectangle is
/// `finish`: 2×1 at progress 0, exactly `finish` at `FULL`, never larger
/// than `finish` and always centered in it. A dialog narrower than two cells
/// or empty of rows keeps its own size rather than inflating.
pub fn iris_rect(finish: OverlayRect, progress: u16) -> OverlayRect {
    let grow = |start: u16, end: u16| -> u16 {
        let start = start.min(end);
        let span = (end - start) as usize;
        start + reveal_count(progress, span) as u16
    };
    let width = grow(2, finish.width);
    let height = grow(1, finish.height);
    OverlayRect {
        x: finish.x + (finish.width - width) / 2,
        y: finish.y + (finish.height - height) / 2,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_clamps_at_both_ends() {
        assert_eq!(progress(0, 100), 0);
        assert_eq!(progress(50, 100), 500);
        assert_eq!(progress(99, 100), 990);
        assert_eq!(progress(100, 100), FULL);
        assert_eq!(progress(10_000, 100), FULL, "past the end stays complete");
        assert_eq!(progress(0, 0), FULL, "a zero duration is already complete");
    }

    #[test]
    fn progress_is_monotonic() {
        let mut last = 0;
        for t in 0..=DIALOG_IRIS_MS + 20 {
            let p = progress(t, DIALOG_IRIS_MS);
            assert!(p >= last, "progress went backwards at {t} ms");
            last = p;
        }
    }

    #[test]
    fn reveal_count_hits_the_boundaries_exactly() {
        assert_eq!(reveal_count(0, 110), 0);
        assert_eq!(reveal_count(FULL, 110), 110);
        assert_eq!(reveal_count(500, 110), 55);
        assert_eq!(reveal_count(500, 80), 40);
        assert_eq!(reveal_count(FULL + 500, 10), 10, "over-range progress never reveals more than total");
        assert_eq!(reveal_count(500, 0), 0);
    }

    #[test]
    fn reveal_count_is_monotonic_and_bounded() {
        for total in [0usize, 1, 2, 7, 19, 110] {
            let mut last = 0;
            for p in 0..=FULL {
                let n = reveal_count(p, total);
                assert!(n >= last && n <= total, "total {total} progress {p} -> {n}");
                last = n;
            }
        }
    }

    #[test]
    fn sweep_enters_and_exits_the_span() {
        assert_eq!(sweep_pos(0, 11, 3), -3, "starts fully off the left edge");
        assert_eq!(sweep_pos(FULL, 11, 3), 11, "ends fully off the right edge");
        let mid = sweep_pos(500, 11, 3);
        assert!(mid > -3 && mid < 11, "mid-sweep is part-way across: {mid}");
    }

    #[test]
    fn iris_starts_two_by_one_and_ends_at_the_finished_rect() {
        let finish = OverlayRect { x: 10, y: 5, width: 40, height: 9 };
        let start = iris_rect(finish, 0);
        assert_eq!((start.width, start.height), (2, 1));
        assert_eq!(iris_rect(finish, FULL), finish);
    }

    #[test]
    fn iris_never_exceeds_the_finished_rect_and_stays_centered() {
        for finish in [
            OverlayRect { x: 10, y: 5, width: 40, height: 9 },
            OverlayRect { x: 0, y: 0, width: 62, height: 19 },
            OverlayRect { x: 3, y: 2, width: 2, height: 1 },
            OverlayRect { x: 7, y: 7, width: 1, height: 1 },
        ] {
            for p in 0..=FULL {
                let r = iris_rect(finish, p);
                assert!(r.x >= finish.x && r.y >= finish.y, "{r:?} escaped {finish:?} at {p}");
                assert!(r.x + r.width <= finish.x + finish.width, "{r:?} overran {finish:?} at {p}");
                assert!(r.y + r.height <= finish.y + finish.height, "{r:?} overran {finish:?} at {p}");
                assert!(r.width <= finish.width && r.height <= finish.height);
            }
        }
    }

    #[test]
    fn iris_midway_is_roughly_half_the_dialog() {
        let finish = OverlayRect { x: 20, y: 6, width: 40, height: 11 };
        let half = iris_rect(finish, 500);
        assert!((half.width as i32 - 21).abs() <= 1, "width {}", half.width);
        assert!((half.height as i32 - 6).abs() <= 1, "height {}", half.height);
    }

    #[test]
    fn splash_timeline_is_contiguous() {
        // The last identity line ends at 200 + 3*80 + 120 = 560, exactly where
        // the title sweep starts (design D6); the sweep ends the entrance at
        // 760, inside the 800 ms hold (checked at compile time above).
        let last_line_end = SPLASH_IDENTITY_START_MS + 3 * SPLASH_IDENTITY_STAGGER_MS + SPLASH_IDENTITY_LINE_MS;
        assert_eq!(last_line_end, SPLASH_SWEEP_START_MS);
    }
}
