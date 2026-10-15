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
use crate::update::{Overlay, State, UiPhase};

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

/// The frame cadence [`next_wake_ms`] requests while an animation is in
/// flight — the ~33 ms cadence the event loop used to tick at even when
/// idle (idle-aware-event-loop design D2).
pub const FRAME_MS: u64 = 33;

/// The earliest reducer-clock time at which `update(Tick)` or a render could
/// produce something different from the current frame (idle-aware-event-loop
/// design D2) — the event loop's "when must I wake even without input"
/// answer, so an idle screen is never redrawn and nothing timed is ever
/// late. Three deadline kinds, earliest first:
///
/// - `clock_ms + FRAME_MS` while any stamped surface is still inside its
///   animation duration (only when `State::animations` is on — with them
///   off, every surface renders its final frame immediately);
/// - the splash hold's end while in `UiPhase::Splash` (regardless of
///   `animations` — the hold is not decorative);
/// - the earliest clipboard-feedback `expires_at_ms` still showing.
///
/// `None` means nothing timed is pending: the loop may sleep until input or
/// the (TUI-side) header clock's next minute flip.
pub fn next_wake_ms(state: &State) -> Option<u64> {
    let mut wake: Option<u64> = None;
    if state.animations && animation_running(state) {
        wake = Some(state.clock_ms + FRAME_MS);
    }
    if let UiPhase::Splash { started_at_ms } = &state.phase {
        let hold_end = started_at_ms.saturating_add(crate::update::SPLASH_MIN_HOLD_MS);
        wake = Some(wake.map_or(hold_end, |w: u64| w.min(hold_end)));
    }
    for panel in [&state.left, &state.right] {
        if let Some(feedback) = &panel.clipboard_feedback {
            wake = Some(wake.map_or(feedback.expires_at_ms, |w: u64| w.min(feedback.expires_at_ms)));
        }
    }
    wake
}

/// Whether any stamped surface is still inside its animation duration at
/// `state.clock_ms`, so the frame one `FRAME_MS` from now would differ from
/// the current one. The stamp inventory mirrors the renderers in
/// `filecommand-tui::views` one-to-one — a surface added there must be added
/// here, or it would freeze mid-animation while idle (idle-aware-event-loop
/// design Risks).
fn animation_running(state: &State) -> bool {
    let now = state.clock_ms;
    let within = |stamp: u64, duration: u64| now.saturating_sub(stamp) < duration;
    // Pop-up menus (F9 pull-downs, F2 user menu, Enter action menu, Alt+F8
    // history) unfold top-down, and a just-moved highlight flashes for
    // `HIGHLIGHT_SETTLE_MS`.
    let popup = |opened_at_ms: u64, selected_at_ms: Option<u64>| {
        within(opened_at_ms, POPUP_UNFOLD_MS) || selected_at_ms.is_some_and(|at| within(at, HIGHLIGHT_SETTLE_MS))
    };

    if let UiPhase::Splash { started_at_ms } = &state.phase {
        if within(*started_at_ms, SPLASH_ENTRANCE_MS) {
            return true;
        }
    }
    if let Some(revealed) = state.panels_revealed_at_ms {
        if within(revealed, PANEL_WIPE_MS) {
            return true;
        }
    }
    if let Some(menu) = state.menu() {
        if within(menu.opened_at_ms, MENU_BAR_SWEEP_MS) {
            return true;
        }
        // The pull-down renders only while open, so only then do its unfold
        // and highlight-settle stamps animate.
        if menu.pulldown_open && popup(menu.pulldown_opened_at_ms, menu.selected_at_ms) {
            return true;
        }
    }
    if let Some(menu) = state.user_menu() {
        if popup(menu.opened_at_ms, menu.selected_at_ms) {
            return true;
        }
    }
    if let Some(menu) = state.file_action_menu() {
        if popup(menu.opened_at_ms, menu.selected_at_ms) {
            return true;
        }
    }
    if let Some(dialog) = state.history_dialog() {
        if popup(dialog.opened_at_ms, dialog.selected_at_ms) {
            return true;
        }
    }
    // The dialog iris keys on `dialog_opened_at_ms`, which outlives the
    // dialog itself — count it only while one of the dialogs that render
    // through the iris is actually up (the same inventory
    // `stamps::DialogMark` tracks).
    if dialog_visible(state) && within(state.dialog_opened_at_ms, DIALOG_IRIS_MS) {
        return true;
    }
    // The quit confirmation layers over any dialog with its own stamp.
    state.quit_confirm && within(state.quit_opened_at_ms, DIALOG_IRIS_MS)
}

/// Whether a modal dialog that irises in on `State::dialog_opened_at_ms` is
/// currently visible: the file-op phase dialogs (setup, running, summary)
/// plus the overlays rendered through `views`'s iris — drive select, fuzzy
/// jump, find file, theme picker, help, and the startup warning. The pop-up
/// menus animate on their own stamps and are not listed here.
fn dialog_visible(state: &State) -> bool {
    matches!(state.phase, UiPhase::FileOpSetup(_) | UiPhase::FileOpRunning { .. } | UiPhase::FileOpSummary(_))
        || matches!(
            state.overlay(),
            Some(
                Overlay::DriveSelect(_)
                    | Overlay::FuzzyJump(_)
                    | Overlay::FindFile(_)
                    | Overlay::ThemePicker(_)
                    | Overlay::Help(_)
                    | Overlay::StartupWarning(_)
            )
        )
}

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
    OverlayRect { x: finish.x + (finish.width - width) / 2, y: finish.y + (finish.height - height) / 2, width, height }
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

    // -----------------------------------------------------------------
    // idle-aware-event-loop: `next_wake_ms` wake deadlines (design D2;
    // task 1.2)
    // -----------------------------------------------------------------

    use crate::menu::MenuState;
    use crate::panel::ClipboardFeedback;
    use crate::theme::Theme;

    /// An animations-on panels state parked at clock `now`, with nothing
    /// open and nothing stamped.
    fn animated_state_at(now: u64) -> State {
        let mut state = State::empty(Theme::classic());
        state.animations = true;
        state.clock_ms = now;
        state
    }

    /// Assert the wake contract for one surface stamped at `t`: a frame wake
    /// right after opening, still a frame wake just before the duration
    /// ends, and nothing due once it has.
    fn assert_surface(mut state: State, t: u64, duration: u64) {
        state.clock_ms = t;
        assert_eq!(next_wake_ms(&state), Some(t + FRAME_MS), "just opened: one frame out");
        state.clock_ms = t + duration - 1;
        assert_eq!(next_wake_ms(&state), Some(t + duration - 1 + FRAME_MS), "still animating one ms before the end");
        state.clock_ms = t + duration;
        assert_eq!(next_wake_ms(&state), None, "finished: no wake");
    }

    #[test]
    fn splash_entrance_wakes_until_its_duration_then_only_the_hold_end() {
        let mut state = animated_state_at(1_000);
        state.phase = UiPhase::Splash { started_at_ms: 1_000 };
        let hold_end = 1_000 + crate::update::SPLASH_MIN_HOLD_MS;
        assert_eq!(next_wake_ms(&state), Some(1_000 + FRAME_MS), "the entrance animates a frame at a time");
        state.clock_ms = 1_000 + SPLASH_ENTRANCE_MS - 1;
        assert_eq!(next_wake_ms(&state), Some(1_000 + SPLASH_ENTRANCE_MS - 1 + FRAME_MS));
        state.clock_ms = 1_000 + SPLASH_ENTRANCE_MS;
        assert_eq!(next_wake_ms(&state), Some(hold_end), "the entrance is done; only the hold remains");
    }

    #[test]
    fn splash_hold_end_is_a_deadline_even_with_animations_off() {
        let mut state = State::empty(Theme::classic());
        state.clock_ms = 42_000;
        state.phase = UiPhase::Splash { started_at_ms: 42_000 };
        assert_eq!(next_wake_ms(&state), Some(42_000 + crate::update::SPLASH_MIN_HOLD_MS));
    }

    #[test]
    fn panel_wipe_wakes_until_its_duration() {
        let mut state = animated_state_at(0);
        state.panels_revealed_at_ms = Some(5_000);
        assert_surface(state, 5_000, PANEL_WIPE_MS);
    }

    #[test]
    fn menu_bar_sweep_wakes_until_its_duration() {
        let mut state = animated_state_at(0);
        let mut menu = MenuState::opened();
        menu.pulldown_open = false;
        menu.opened_at_ms = 2_000;
        state.open_overlay(Overlay::Menu(menu));
        assert_surface(state, 2_000, MENU_BAR_SWEEP_MS);
    }

    #[test]
    fn pulldown_unfold_and_highlight_settle_wake_until_their_durations() {
        let mut state = animated_state_at(0);
        let mut menu = MenuState::opened();
        menu.opened_at_ms = 1_000; // the bar's own sweep long finished
        menu.pulldown_opened_at_ms = 2_500;
        state.open_overlay(Overlay::Menu(menu));
        assert_surface(state.clone(), 2_500, POPUP_UNFOLD_MS);

        state.menu_mut().unwrap().pulldown_opened_at_ms = 2_000; // unfold done
        state.menu_mut().unwrap().selected_at_ms = Some(3_000);
        assert_surface(state, 3_000, HIGHLIGHT_SETTLE_MS);
    }

    #[test]
    fn a_closed_pulldown_neither_unfolds_nor_settles() {
        let mut state = animated_state_at(0);
        let mut menu = MenuState::opened();
        menu.pulldown_open = false;
        menu.opened_at_ms = 1_000;
        menu.pulldown_opened_at_ms = 2_500;
        menu.selected_at_ms = Some(2_500);
        state.open_overlay(Overlay::Menu(menu));
        state.clock_ms = 2_500;
        assert_eq!(next_wake_ms(&state), None, "a closed pull-down renders nothing animated");
    }

    #[test]
    fn popup_menus_wake_for_unfold_and_highlight_settle() {
        for slot in ["user_menu", "file_action_menu", "history_dialog"] {
            let mut state = animated_state_at(0);
            match slot {
                "user_menu" => {
                    state.open_overlay(Overlay::UserMenu(crate::dialogs::UserMenuState { cursor: 0, opened_at_ms: 4_000, selected_at_ms: None }))
                }
                "file_action_menu" => {
                    state.open_overlay(Overlay::FileActionMenu(crate::dialogs::FileActionMenuState::new("f.txt".into(), false)));
                    state.file_action_menu_mut().unwrap().opened_at_ms = 4_000;
                }
                _ => {
                    state.open_overlay(Overlay::HistoryDialog(crate::dialogs::HistoryDialogState::default()));
                    state.history_dialog_mut().unwrap().opened_at_ms = 4_000;
                }
            }
            assert_surface(state.clone(), 4_000, POPUP_UNFOLD_MS);

            // A highlight that moved after the unfold finished settles.
            let mut state = state;
            let settle_at = 4_000 + POPUP_UNFOLD_MS;
            match slot {
                "user_menu" => state.user_menu_mut().unwrap().selected_at_ms = Some(settle_at),
                "file_action_menu" => state.file_action_menu_mut().unwrap().selected_at_ms = Some(settle_at),
                _ => state.history_dialog_mut().unwrap().selected_at_ms = Some(settle_at),
            }
            assert_surface(state, settle_at, HIGHLIGHT_SETTLE_MS);
        }
    }

    #[test]
    fn a_visible_dialog_irises_but_a_stale_stamp_does_not_wake() {
        let mut state = animated_state_at(0);
        state.dialog_opened_at_ms = 5_000;
        state.clock_ms = 5_000;
        assert_eq!(next_wake_ms(&state), None, "no dialog visible: the stamp alone animates nothing");
        state.set_startup_warning("warn".to_string());
        assert_surface(state, 5_000, DIALOG_IRIS_MS);
    }

    #[test]
    fn the_quit_dialog_irises_on_its_own_stamp() {
        let mut state = animated_state_at(0);
        state.quit_confirm = true;
        state.quit_opened_at_ms = 6_000;
        assert_surface(state, 6_000, DIALOG_IRIS_MS);
    }

    #[test]
    fn animations_off_yields_no_animation_wake() {
        // Every surface stamped at once: with `animations` off, all render
        // their final frame and nothing wakes (application-shell "Idle
        // screen is not redrawn").
        let mut state = State::empty(Theme::classic());
        state.clock_ms = 7_000;
        state.panels_revealed_at_ms = Some(7_000);
        state.open_overlay(Overlay::Menu(MenuState::opened()));
        state.quit_confirm = true;
        state.quit_opened_at_ms = 7_000;
        assert_eq!(next_wake_ms(&state), None);
    }

    #[test]
    fn clipboard_feedback_expiry_is_a_deadline() {
        // application-shell "Clipboard feedback expires without input": the
        // wake lands exactly on `expires_at_ms`, the earlier of the two
        // panels winning.
        let mut state = State::empty(Theme::classic());
        state.clock_ms = 1_000;
        let feedback = |expires_at_ms| ClipboardFeedback { message: "copied".to_string(), is_error: false, expires_at_ms };
        state.left.clipboard_feedback = Some(feedback(9_999));
        assert_eq!(next_wake_ms(&state), Some(9_999));
        state.right.clipboard_feedback = Some(feedback(4_000));
        assert_eq!(next_wake_ms(&state), Some(4_000), "the earliest expiry wins");
        state.right.clipboard_feedback = None;
        state.animations = true;
        assert_eq!(next_wake_ms(&state), Some(9_999), "not mistaken for an animation frame wake");
    }

    #[test]
    fn an_idle_panels_screen_has_no_wake() {
        assert_eq!(next_wake_ms(&animated_state_at(12_345)), None);
    }
}
