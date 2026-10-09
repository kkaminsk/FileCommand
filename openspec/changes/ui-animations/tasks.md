# Tasks: ui-animations

## 1. Core motion model

- [ ] 1.1 Add `filecommand-core::anim` with per-mille `progress`, `reveal_count`, `sweep_pos`, iris-rect helper, and the named duration constants from design D6 (ui-animations: "Render-only motion model driven by the reducer clock")
- [ ] 1.2 Add `State::animations`, `State::dialog_opened_at_ms`, `State::quit_opened_at_ms`, `State::panels_revealed_at_ms`; `State::empty` sets `animations = false` (ui-animations: "Animations are disabled by config or launch flag")
- [ ] 1.3 Stamp `MenuState` (`opened_at_ms`, `pulldown_opened_at_ms`, `selected_at_ms`), `UserMenuState` and `FileActionMenuState` (`opened_at_ms`, `selected_at_ms`) from `state.clock_ms` in the reducer on open, traversal, and selection change (ui-animations: "Pop-up menus unfold and highlights settle")
- [ ] 1.4 Stamp `dialog_opened_at_ms` where each modal dialog becomes visible (including the About dialog and the startup warning, stamped on the first `Tick` when set before it) and `quit_opened_at_ms` where `quit_confirm` is set; stamp `panels_revealed_at_ms` on the Splash → Panels transition only (ui-animations: "Modal dialogs open with an iris"; "Panels wipe in after the splash")

## 2. Configuration and launch flag

- [ ] 2.1 Parse `animations = <bool>` in `config::parse` (general options; default `true`) (ui-animations: "Animations are disabled by config or launch flag")
- [ ] 2.2 Parse `--noanimations` in `parse_launch_args`; flag overrides config in `run` (alongside the existing `config.splash && !launch.no_splash` computation, since `apply_config` takes no launch options) (ui-animations: "Animations are disabled by config or launch flag")
- [ ] 2.3 Add `--noanimations` to `main.rs` `print_usage` and to the README flag table; add `animations` to the README config key table (ui-animations: "Animations are disabled by config or launch flag")

## 3. Splash and panel reveal

- [ ] 3.1 Animate `views/splash.rs`: clockwise frame trace, center-out identity-line reveal with `splash.frame` intermediate, `splash.version` title sweep; final frame identical to today's at ≥ 760 ms or when disabled (startup-splash: "Splash entrance animation"; "Frame-1 splash rendering")
- [ ] 3.2 Add the shared scratch-buffer reveal helper in `views/mod.rs` (iris window and top-rows wipe) (ui-animations: "Modal dialogs open with an iris")
- [ ] 3.3 Wipe the panels top-to-bottom over 150 ms when `panels_revealed_at_ms` is set (ui-animations: "Panels wipe in after the splash")

## 4. Menus

- [ ] 4.1 Menu bar left-to-right sweep in `views/menubar.rs` (pulldown-menus: "Menu bar entrance sweep")
- [ ] 4.2 Shared pop-up unfold + highlight-settle helper applied to F9 pull-downs, `views/user_menu.rs`, and `views/file_action_menu.rs` (ui-animations: "Pop-up menus unfold and highlights settle")

## 5. Dialogs

- [ ] 5.1 Wrap every modal dialog view in `render_phase`/`render` with the iris helper keyed on `dialog_opened_at_ms` (quit dialog on `quit_opened_at_ms`) (ui-animations: "Modal dialogs open with an iris")
- [ ] 5.2 Confirm hit-maps still use final geometry during the iris (ui-animations: "Modal dialogs open with an iris")

## 6. Tests

- [ ] 6.1 Unit tests for `anim` helpers (progress clamping, reveal counts at boundaries, iris rect never exceeds final rect) (ui-animations: "Render-only motion model driven by the reducer clock")
- [ ] 6.2 Reducer tests: stamps set from `clock_ms` on open/traversal/selection; key during splash still dismisses and is consumed; `panels_revealed_at_ms` is `None` when splash skipped (ui-animations; startup-splash: "Minimum hold and key dismissal")
- [ ] 6.3 Config/flag tests: default on, `animations = false`, `--noanimations` wins over `animations = true` (ui-animations: "Animations are disabled by config or launch flag")
- [ ] 6.4 `insta` snapshots with `animations = true` and pinned `clock_ms`: splash at 0 / 100 / 400 / 650 / 800 ms; pull-down at 0 / 50 / 100 ms; dialog iris at 0 / 50 / 100 ms; panel wipe at 75 ms; menu bar sweep at 50 ms (startup-splash; pulldown-menus; ui-animations)
- [ ] 6.5 Verify the full existing snapshot suite is byte-identical (animations off in `State::empty`) (ui-animations: "Animations are disabled by config or launch flag")
