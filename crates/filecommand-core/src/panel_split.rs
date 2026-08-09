//! The user-adjustable vertical panel split: percentage semantics,
//! round-half-up rounding, and per-panel minimum-width clamping — shared
//! by `crate::update`'s reducer (`Command::SplitGrow`/`SplitShrink`) and
//! `filecommand-tui::layout::compute`'s renderer, so the two never
//! disagree about where the divider actually sits (panel-split "Split
//! ratio semantics and panel minimum"; design D7).

/// Neither panel ever shrinks below this many columns.
pub const MIN_PANEL_W: u16 = 20;
/// Columns the divider moves per Ctrl+Left/Ctrl+Right keypress.
pub const SPLIT_STEP: u16 = 2;
/// The default (and Ctrl+= reset) split: an even 50/50.
pub const DEFAULT_SPLIT_PERCENT: u16 = 50;

/// `terminal_width * percent / 100`, rounded half up (not floored) — the
/// shared rounding rule behind [`effective_left_width`] and
/// [`adjust_percent`]'s inverse column-to-percent conversion.
pub fn round_half_up(terminal_width: u16, percent: u16) -> u16 {
    ((terminal_width as u32 * percent as u32 + 50) / 100) as u16
}

/// The left panel's effective width for a stored `percent` at the current
/// `terminal_width`: `round_half_up(terminal_width, percent)`, then
/// clamped so each panel keeps at least [`MIN_PANEL_W`] columns. Clamping
/// never touches the stored `percent` itself — see [`adjust_percent`]'s
/// doc comment and panel-split "Clamping preserves the stored intent".
pub fn effective_left_width(percent: u16, terminal_width: u16) -> u16 {
    let raw = round_half_up(terminal_width, percent);
    let max_left = terminal_width.saturating_sub(MIN_PANEL_W);
    if max_left < MIN_PANEL_W {
        // The terminal is narrower than two panel minimums put together —
        // unreachable at the application's real 60-column floor, but kept
        // total rather than panicking: split as evenly as the terminal
        // allows.
        return terminal_width / 2;
    }
    raw.clamp(MIN_PANEL_W, max_left)
}

/// Move the divider `delta_cols` columns (negative = left, positive =
/// right) from wherever `percent` currently places it at `terminal_width`,
/// returning the new stored percentage — or `None` if the move would push
/// either panel below [`MIN_PANEL_W`], per panel-split "Adjustment at the
/// limit is a no-op". The returned percentage is the nearest round-half-up
/// percentage of the target column position — exact for the panel-split
/// spec's worked examples (e.g. 50% + 2 columns at 100 columns yields
/// exactly 52%, i.e. 52 columns), and within a column of the target in
/// general (rounding is not always perfectly invertible).
pub fn adjust_percent(percent: u16, delta_cols: i32, terminal_width: u16) -> Option<u16> {
    let max_left = terminal_width.saturating_sub(MIN_PANEL_W);
    if max_left < MIN_PANEL_W {
        return None; // No room to adjust at all.
    }
    let current_left = effective_left_width(percent, terminal_width) as i32;
    let target_left = current_left + delta_cols;
    if target_left < MIN_PANEL_W as i32 || target_left > max_left as i32 {
        return None;
    }
    let terminal_width = terminal_width.max(1) as i64;
    let target_left = target_left as i64;
    let percent_new = ((target_left * 100 + terminal_width / 2) / terminal_width) as u16;
    Some(percent_new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_half_up_rounds_point_five_up() {
        // 3 * 50 / 100 = 1.5 -> 2.
        assert_eq!(round_half_up(3, 50), 2);
    }

    #[test]
    fn effective_left_width_at_default_split() {
        assert_eq!(effective_left_width(DEFAULT_SPLIT_PERCENT, 80), 40);
    }

    #[test]
    fn effective_left_width_scales_across_resizes() {
        // panel-split "Percentage scales across resizes": 60% at 100 -> at
        // 160 the left panel is 96 columns, the same 60% of the new width.
        assert_eq!(effective_left_width(60, 100), 60);
        assert_eq!(effective_left_width(60, 160), 96);
    }

    #[test]
    fn effective_left_width_clamps_to_the_panel_minimum() {
        // panel-split "Clamping preserves the stored intent": 75% at 60
        // columns holds the right panel at its 20-column minimum.
        let left = effective_left_width(75, 60);
        assert_eq!(left, 40);
        assert_eq!(60 - left, MIN_PANEL_W);
    }

    #[test]
    fn effective_left_width_clamp_springs_back_on_regrow() {
        // The same 75% split, enlarged back to 120 columns, restores a
        // 90-column left panel — the stored 75% unchanged.
        assert_eq!(effective_left_width(75, 120), 90);
    }

    #[test]
    fn adjust_percent_moves_the_divider_two_columns_right() {
        // panel-split "Divider moves in 2-column steps".
        let new_percent = adjust_percent(50, 2, 100).unwrap();
        assert_eq!(effective_left_width(new_percent, 100), 52);
    }

    #[test]
    fn adjust_percent_is_a_no_op_at_the_limit() {
        // Right panel already at its 20-column minimum (left = 80 of 100).
        assert_eq!(adjust_percent(80, 2, 100), None);
    }

    #[test]
    fn adjust_percent_is_a_no_op_at_the_left_limit_too() {
        assert_eq!(adjust_percent(20, -2, 100), None);
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn effective_left_width_never_violates_either_panel_minimum(
                percent in 0u16..=100,
                terminal_width in 60u16..300,
            ) {
                let left = effective_left_width(percent, terminal_width);
                prop_assert!(left >= MIN_PANEL_W);
                prop_assert!(terminal_width - left >= MIN_PANEL_W);
            }

            /// A property test walking widths 60..200 at a fixed percent,
            /// asserting `effective_left_width` is monotone non-decreasing
            /// in width with no jitter (design D7's "Percentage rounding
            /// causes 1-column divider jitter during resize" risk).
            #[test]
            fn effective_left_width_is_monotonic_in_width(percent in 1u16..100, width in 60u16..199) {
                let a = effective_left_width(percent, width);
                let b = effective_left_width(percent, width + 1);
                prop_assert!(b >= a, "percent={percent} width={width}: {a} -> {b}");
            }

            /// `adjust_percent`'s column-to-percent conversion is a
            /// round-half-up rounding, not always an exact inverse — the
            /// spec's own worked examples (50%+2 at 100 columns, etc.) do
            /// round-trip exactly (covered by the unit tests above), but
            /// this property only asserts the general contract: the
            /// result lands within a column of the requested target and
            /// never violates either panel's minimum.
            #[test]
            fn adjust_percent_lands_within_a_column_of_the_target(
                percent in 20u16..80,
                terminal_width in 60u16..300,
            ) {
                let current_left = effective_left_width(percent, terminal_width) as i32;
                let max_left = terminal_width.saturating_sub(MIN_PANEL_W) as i32;
                if max_left < MIN_PANEL_W as i32 {
                    return Ok(());
                }
                if current_left + 2 <= max_left {
                    let new_percent = adjust_percent(percent, 2, terminal_width).unwrap();
                    let new_left = effective_left_width(new_percent, terminal_width) as i32;
                    prop_assert!((new_left - (current_left + 2)).abs() <= 1, "current={current_left} new={new_left}");
                    prop_assert!(new_left >= MIN_PANEL_W as i32);
                    prop_assert!(terminal_width as i32 - new_left >= MIN_PANEL_W as i32);
                }
            }
        }
    }
}
