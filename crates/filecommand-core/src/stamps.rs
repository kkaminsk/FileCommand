//! The reducer's animation-stamp post-condition (ui-animations "Render-only
//! motion model driven by the reducer clock"; design D1/D4).
//!
//! Animations interpolate `State::clock_ms - stamp` at render time, so the
//! reducer's only animation duty is to record *when* an overlay opened or
//! changed. Doing that at each of the many places an overlay opens would
//! scatter the rule (and miss ones added later), so — like the drag-clearing
//! rule in `update` — it is enforced once, as a post-condition on every
//! public `update` call: [`Snapshot::capture`] a few cheap marks before the
//! transition, then [`Snapshot::apply`] stamps whatever opened or changed
//! after it, using `state.clock_ms` (the last `Tick` reading). Nothing here
//! reads a clock, delays a transition, or changes anything but the stamps.

use std::mem::{discriminant, Discriminant};

use crate::fs_ops::dialog::{FileOpSetup, RunningDialog};
use crate::menu::{MenuId, MenuState};
use crate::update::{Overlay, State, UiPhase};

/// What the F9 menu looked like before the transition.
struct MenuMark {
    active: MenuId,
    pulldown_open: bool,
    selected: usize,
    opened_at_ms: u64,
    pulldown_opened_at_ms: u64,
    selected_at_ms: Option<u64>,
}

/// What a cursor-over-a-list pop-up (F2 user menu, Enter action menu) looked
/// like before the transition.
struct PopupMark {
    cursor: usize,
    opened_at_ms: u64,
    selected_at_ms: Option<u64>,
}

/// Which modal dialog is visible, at the granularity "a different dialog
/// replaced the previous one". Typing into an input, a progress counter
/// ticking, or a Help page turning leaves the mark unchanged; a progress
/// dialog giving way to a conflict dialog, or any dialog opening, changes it.
#[derive(Clone, Copy, PartialEq, Eq)]
struct DialogMark {
    setup: Option<Discriminant<FileOpSetup>>,
    running: Option<Discriminant<RunningDialog>>,
    summary: bool,
    /// Which of the overlays that iris in (drive select, fuzzy jump, find
    /// file, theme picker, help, the startup warning) is in the slot. The
    /// pop-up menus animate on their own stamps and are not counted here.
    iris_overlay: Option<Discriminant<Overlay>>,
}

impl DialogMark {
    fn of(state: &State) -> DialogMark {
        DialogMark {
            setup: match &state.phase {
                UiPhase::FileOpSetup(setup) => Some(discriminant(setup)),
                _ => None,
            },
            running: match &state.phase {
                UiPhase::FileOpRunning { dialog, .. } => Some(discriminant(dialog)),
                _ => None,
            },
            summary: matches!(state.phase, UiPhase::FileOpSummary(_)),
            iris_overlay: match state.overlay() {
                Some(
                    overlay @ (Overlay::DriveSelect(_)
                    | Overlay::FuzzyJump(_)
                    | Overlay::FindFile(_)
                    | Overlay::ThemePicker(_)
                    | Overlay::Help(_)
                    | Overlay::StartupWarning(_)),
                ) => Some(discriminant(overlay)),
                _ => None,
            },
        }
    }

    fn any(&self) -> bool {
        self.setup.is_some() || self.running.is_some() || self.summary || self.iris_overlay.is_some()
    }
}

/// The pre-transition marks [`Snapshot::apply`] diffs against.
pub(crate) struct Snapshot {
    menu: Option<MenuMark>,
    user_menu: Option<PopupMark>,
    file_action_menu: Option<PopupMark>,
    history_dialog: Option<PopupMark>,
    dialog: DialogMark,
    quit_confirm: bool,
    in_splash: bool,
}

impl Snapshot {
    pub(crate) fn capture(state: &State) -> Snapshot {
        Snapshot {
            menu: state.menu().map(|m| MenuMark {
                active: m.active,
                pulldown_open: m.pulldown_open,
                selected: m.selected,
                opened_at_ms: m.opened_at_ms,
                pulldown_opened_at_ms: m.pulldown_opened_at_ms,
                selected_at_ms: m.selected_at_ms,
            }),
            user_menu: state.user_menu().map(|m| PopupMark { cursor: m.cursor, opened_at_ms: m.opened_at_ms, selected_at_ms: m.selected_at_ms }),
            file_action_menu: state.file_action_menu().map(|m| PopupMark {
                cursor: m.cursor,
                opened_at_ms: m.opened_at_ms,
                selected_at_ms: m.selected_at_ms,
            }),
            history_dialog: state.history_dialog().map(|d| PopupMark {
                cursor: d.cursor,
                opened_at_ms: d.opened_at_ms,
                selected_at_ms: d.selected_at_ms,
            }),
            dialog: DialogMark::of(state),
            quit_confirm: state.quit_confirm,
            in_splash: matches!(state.phase, UiPhase::Splash { .. }),
        }
    }

    /// Stamp, from `state.clock_ms`, everything that opened or changed since
    /// [`Snapshot::capture`].
    pub(crate) fn apply(self, state: &mut State) {
        let now = state.clock_ms;

        if let Some(menu) = state.menu_mut() {
            stamp_menu(self.menu.as_ref(), menu, now);
        }
        if let Some(menu) = state.user_menu_mut() {
            let (opened, selected) = stamp_popup(self.user_menu.as_ref(), menu.cursor, now);
            menu.opened_at_ms = opened;
            menu.selected_at_ms = selected;
        }
        if let Some(menu) = state.file_action_menu_mut() {
            let (opened, selected) = stamp_popup(self.file_action_menu.as_ref(), menu.cursor, now);
            menu.opened_at_ms = opened;
            menu.selected_at_ms = selected;
        }
        if let Some(dialog) = state.history_dialog_mut() {
            let (opened, selected) = stamp_popup(self.history_dialog.as_ref(), dialog.cursor, now);
            dialog.opened_at_ms = opened;
            dialog.selected_at_ms = selected;
        }

        // A dialog opening, or a different one replacing it, irises; closing
        // one stamps nothing (there is nothing left to animate).
        let dialog = DialogMark::of(state);
        if dialog.any() && dialog != self.dialog {
            state.dialog_opened_at_ms = now;
        }
        if state.quit_confirm && !self.quit_confirm {
            state.quit_opened_at_ms = now;
        }
        // Splash → Panels only: leaving the splash for the placeholder (a
        // resize below the floor, which now lands on `Panels` with
        // `undersized` set) or never showing it records nothing.
        if self.in_splash && matches!(state.phase, UiPhase::Panels) && !state.undersized {
            state.panels_revealed_at_ms = Some(now);
        }
    }
}

/// Stamp the F9 menu. A freshly opened bar stamps all three clocks; an
/// already-open one keeps `opened_at_ms` (handlers that rebuild the whole
/// `MenuState`, like a title click, would otherwise reset it to `0`), re-
/// stamps the pull-down when a different one opens or a closed one reopens,
/// and stamps the highlight only when it moves within a pull-down that was
/// already unfolding or settled — a new pull-down's first highlight belongs
/// to its unfold, not to a settle flash.
fn stamp_menu(before: Option<&MenuMark>, menu: &mut MenuState, now: u64) {
    match before {
        None => {
            menu.opened_at_ms = now;
            menu.pulldown_opened_at_ms = now;
            menu.selected_at_ms = None;
        }
        Some(b) => {
            menu.opened_at_ms = b.opened_at_ms;
            let new_pulldown = menu.pulldown_open && (!b.pulldown_open || menu.active != b.active);
            if new_pulldown {
                menu.pulldown_opened_at_ms = now;
                menu.selected_at_ms = None;
            } else {
                menu.pulldown_opened_at_ms = b.pulldown_opened_at_ms;
                menu.selected_at_ms = if menu.selected != b.selected { Some(now) } else { b.selected_at_ms };
            }
        }
    }
}

/// `(opened_at_ms, selected_at_ms)` for a cursor-over-a-list pop-up.
fn stamp_popup(before: Option<&PopupMark>, cursor: usize, now: u64) -> (u64, Option<u64>) {
    match before {
        None => (now, None),
        Some(b) => (b.opened_at_ms, if cursor != b.cursor { Some(now) } else { b.selected_at_ms }),
    }
}
