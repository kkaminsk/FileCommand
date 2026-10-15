# Change: ui-animations

## Why

FileCommand's chrome appears and disappears instantaneously: the splash box pops in fully formed, F9 drops a complete pull-down in one frame, dialogs snap open. That is faithful to a 1990s text-mode redraw, but it leaves no room for the small flourishes a modern terminal can afford — a frame tracing itself in, a menu unfolding, a highlight settling — that make the application feel alive and give visual continuity between states. The user asked for ASCII animations on application load and in the menus, as color changes and more elaborate UI motion, in keeping with the Norton Commander 5.5 aesthetic.

The design doc (§4.10) says FileCommand "never uses spinners or animation glyphs" for *loading feedback*. This change deliberately supersedes that rule for **decorative chrome** — splash, menus, dialogs — while keeping every loading affordance (streaming listings, `Reading… N`, Info-panel `…`, drive-label fill-in) exactly as static as today.

## What Changes

- **Motion model** (new `ui-animations` capability): every animation is a pure function of time elapsed since a state transition, computed from the reducer's existing `Tick` clock reading. The reducer transitions state instantly and only records *when* an overlay opened; the renderer interpolates. Input is never blocked, no hold is lengthened, and any key applies to the final state immediately.
- **Startup splash entrance**: the double-line frame traces itself clockwise from the top-left corner, the four identity lines reveal outward from their centers (appearing first in the frame color, then settling to their final role), and a single highlight sweep crosses the product name — all inside the existing 800 ms minimum hold. After the splash, the panels wipe in top-to-bottom over 150 ms.
- **Menus**: on F9 the menu bar sweeps in left-to-right; every pop-up menu (F9 pull-downs, F2 user menu, Enter file-action menu) unfolds top-down from its frame; when the highlighted item changes, the new highlight row renders one frame in the hotkey accent before settling to the normal highlight style.
- **Modal dialogs**: every modal dialog opens with a 100 ms "iris" — a growing centered window onto the finished dialog — instead of appearing fully formed.
- **Configuration**: `animations = true` (default) as a top-level key in `config.toml`; `--noanimations` launch flag; flag wins. When disabled, every surface renders its final frame immediately — pixel-identical to today.
- **No new theme roles**: all animation colors come from roles that already exist (`splash.frame`, `splash.version`, `menu.hotkey`, `screen.backdrop`), so user `themes/*.toml` files keep validating.
- **Determinism**: animation progress is derived from `State::clock_ms`, so `insta` snapshot tests pin a clock and capture mid-animation frames; states built without config keep animations off, so existing snapshots are unchanged.

## Capabilities

### New Capabilities

- `ui-animations`: the render-only motion model and its clock source; the `animations` config key and `--noanimations` flag; the modal-dialog iris; the pop-up menu unfold and highlight settle (shared by F9 pull-downs, F2 user menu, Enter action menu); the post-splash panel reveal; and the scoping that loading feedback remains static.

### Modified Capabilities

- `startup-splash`: "Frame-1 splash rendering" is reworded so frame 1 is the splash *backdrop plus the first frame of the entrance* when animations are enabled (the complete box when disabled); a new "Splash entrance animation" requirement defines the frame trace, identity-line reveal, and title sweep within the 800 ms hold. Minimum-hold, key-dismissal, disable, and resize requirements are unchanged.
- `pulldown-menus`: adds "Menu bar entrance sweep" for the F9 bar; existing bar, pull-down, navigation, and content requirements are unchanged (horizontal traversal still opens the adjacent pull-down with no intermediate closed state — it unfolds while already open and interactive).

## Impact

- **Crates:** `filecommand-core` — new `anim` module (progress/easing helpers, duration constants); `opened_at_ms`/`selected_at_ms` stamps on `MenuState`, `UserMenuState`, `FileActionMenuState`; `State::animations` flag, `State::dialog_opened_at_ms`, `State::quit_opened_at_ms`, `State::panels_revealed_at_ms`; reducer sets stamps from `state.clock_ms` where overlays open; `config.rs` `animations` key. `filecommand-tui` — `--noanimations` in `parse_launch_args`, `apply_config` wiring, `print_usage` line; `splash.rs`, `menubar.rs`, `user_menu.rs`, `file_action_menu.rs` interpolated rendering; a shared scratch-buffer "reveal" helper in `views/mod.rs` used by the dialog iris and the panel wipe; new snapshot tests.
- **Docs:** README flag table and `config.toml` key table (the `--help` usage block lists it in its OPTIONS list) gain `--noanimations` / `animations`.
- **Depends on:** `startup-splash` (clock + hold), `pulldown-menus`, `user-menu`, `file-action-menu`, `responsive-layout` (overlay geometry), `theme-system` (role-only colors). The `help-flags` change's usage block gains the new flag.
- **Out of scope:** animating loading feedback (listings, Info panel, drive select, progress counters); a reduced-motion/"subtle" intermediate level; a runtime Options toggle; sound; ASCII-art logo; per-theme animation overrides.
