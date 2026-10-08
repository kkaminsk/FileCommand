# Design: ui-animations

## Context

The TUI event loop (`filecommand-tui/src/app.rs`) polls crossterm with a 33 ms timeout; every timeout delivers `Command::Tick(now)` from the injected monotonic `Clock`, the reducer stores it in `State::clock_ms`, and the loop redraws. The startup splash already uses this heartbeat for its 800 ms minimum hold (`UiPhase::Splash { started_at_ms }`). All rendering is a pure function of `State` through `views::render`, every overlay is positioned by `dialogs::overlay_rect`, and every color comes from a theme role. User theme files in `themes/*.toml` are validated against the full role list — a missing role is a hard error. Snapshot tests build `State` directly via `State::empty` and pin the clock.

## Goals / Non-Goals

**Goals:**

- Decorative motion on the splash, the panel reveal after it, the F9 bar and all pop-up menus, and every modal dialog — Norton Commander glyphs and colors only.
- Zero impact on responsiveness: state transitions are instant; animations are interpolation of elapsed time at render; nothing waits for an animation to finish.
- Deterministic and snapshot-testable; existing snapshots unchanged.
- One switch to turn it all off (`animations = false`, `--noanimations`), yielding today's exact rendering.
- No new theme roles; no breakage for user theme files.

**Non-Goals:**

- Animating loading feedback (design doc §4.10's static-text rule stands for listings, Info panel, drive select, progress dialog counters/gauge).
- Multiple intensity levels, OS reduced-motion detection, or a runtime Options toggle.
- Changing any overlay's final geometry, content, timing (800 ms hold), or key handling.
- ASCII-art logos or new glyphs outside the CP437 set.

## Decisions

### D1: Animations are render-only interpolation of `clock_ms − stamp`; the reducer only records when things opened

The reducer transitions state exactly as today and additionally stamps the open time of the affected overlay with `state.clock_ms` (the last `Tick` reading — no clock read inside `update`, so it stays pure). Views compute `elapsed = state.clock_ms.saturating_sub(stamp)` and draw the frame for that instant via helpers in a new `filecommand-core::anim` module (`progress(elapsed, duration) -> 0..=1000` per-mille, `reveal_count(progress, total)`, `sweep_pos(progress, width)`). Because the next `Tick` lands ≤ 33 ms later and always marks the frame dirty, a 100 ms animation gets ~4 frames with no event-loop changes. Alternative considered: an `Effect::Animate` that schedules extra redraws — rejected; the heartbeat already exists. Alternative: passing a real `now` into `render` — rejected; using `clock_ms` for both stamp and now keeps `render(State)` a pure function of state and makes snapshots trivially reproducible. Trade-off: sustained key-repeat faster than 33 ms suppresses ticks and holds an in-flight animation on its current frame until input pauses; accepted (the state underneath is already final).

### D2: No new theme roles — animation colors reuse existing roles

Adding roles would make every existing user `themes/*.toml` fail validation. Instead: splash reveal intermediate = `splash.frame`; splash title sweep window = `splash.version`; pop-up highlight settle = `menu.hotkey`; unrevealed cells = `screen.backdrop` (or the surface's own body style). This keeps the splash inside its spec'd `splash.*` roles and holds for hue-free themes (`nc-mono`, `inverted`, `terminal-green`, `yellow-storm`): the structural motion (trace, unfold, iris, wipe) is glyph-based and theme-independent, and the color steps degrade to subtle or invisible differences without ever carrying meaning. Alternative considered: optional `anim.*` roles with built-in defaults when absent — rejected as a second role-resolution path for a purely decorative feature.

### D3: One shared "reveal" helper renders the final surface to a scratch buffer and blits a sub-rectangle

For the dialog iris and the panel wipe, the view renders the finished surface into a scratch `ratatui::Buffer` sized to its final rectangle, then copies onto the screen only the cells inside the reveal window — a centered rectangle growing from 2×1 to full size (iris) or the top `n` rows (wipe). This needs no per-dialog changes: `render_phase` wraps whichever modal dialog view is active. Content under the window is the *finished* dialog, so the user sees the real dialog growing, never placeholder art. Hit-maps are built from final geometry, so a click during the 100 ms iris lands where the button will be. Alternative: per-dialog frame-drawing animations — rejected; ~12 dialog views would each need edits.

### D4: Stamps live on the state that opened

- `MenuState { opened_at_ms, pulldown_opened_at_ms, selected_at_ms }` — `pulldown_opened_at_ms` is re-stamped on Left/Right traversal and hotkey jumps (the new pull-down unfolds while open and interactive); `selected_at_ms` on every selection change.
- `UserMenuState { opened_at_ms, selected_at_ms }`, `FileActionMenuState { opened_at_ms, selected_at_ms }` — same unfold/settle treatment via one shared pop-up helper.
- `State::dialog_opened_at_ms` — set whenever a modal dialog becomes visible (phase enters `FileOpSetup`/`FileOpRunning`/`FileOpSummary`, or `help`/`theme_picker`/`drive_select`/`fuzzy_jump`/`find_file` become `Some`). Only one of these is visible at a time, so one stamp suffices; a replacement dialog (e.g. progress → conflict) re-stamps and irises again.
- `State::quit_opened_at_ms` — the quit dialog layers over others, so it has its own stamp and the underlying dialog does not re-animate.
- `State::panels_revealed_at_ms: Option<u64>` — set on the `Splash → Panels` transition only (hold elapsed or key). `None` when the splash was skipped, so frame 1 with `--nosplash` is still the complete panels.
- `UiPhase::Splash { started_at_ms }` already exists and drives the splash entrance.

### D5: `State::animations` defaults off in `State::empty`, on in `Config::default`

`Config::default().animations = true` so real launches animate; `apply_config` copies it into `State::animations`; `State::empty` sets `false`. Every view checks `state.animations` first and short-circuits to the final frame when off. Result: all existing snapshot and reducer tests (which construct `State::empty`) are unchanged; animation snapshots opt in by setting the flag and pinning `clock_ms`.

### D6: Fixed timing budget, all ≤ the existing splash hold

| Animation | Duration | Notes |
|---|---|---|
| Splash frame trace | 0–200 ms | perimeter cells revealed clockwise from top-left |
| Splash identity reveal | 200–560 ms | line *i* starts at 200 + 80·i, lasts 120 ms, reveals outward from center; revealed chars in `splash.frame` for ≤ 66 ms, then final role |
| Splash title sweep | 560–760 ms | 3-cell `splash.version` window crosses the product name once |
| Panel wipe after splash | 150 ms | top-to-bottom rows |
| Menu bar sweep | 100 ms | left-to-right columns |
| Pop-up menu unfold | 100 ms | frame top row first, rows added downward, bottom edge tracks |
| Highlight settle | 50 ms | new highlight row in `menu.hotkey` style, then `menu.highlight` |
| Dialog iris | 100 ms | centered window from 2×1 to full rectangle |

Durations are constants in `core::anim` so tests reference them by name. Everything on the splash completes by 760 ms, inside the unchanged 800 ms minimum hold.

### D7: Disable precedence mirrors `splash`/`mouse`

`animations = false` in the general options of `config.toml`, or `--noanimations` at launch; the flag wins when they disagree. Parsed in `config::parse` next to `splash` and in `parse_launch_args` next to `--nosplash`; listed in `print_usage` and the README. The design doc §4.10 sentence is superseded for decorative chrome only; this change is the normative record of that.

## Risks / Trade-offs

- [Mid-animation frame appears in a snapshot that did not intend it] → `State::empty` has animations off; only tests that set `animations = true` see motion, and they pin `clock_ms` at named offsets.
- [Dialog iris briefly hides the dialog title so a user cannot read what opened for ~100 ms] → 100 ms is ≈ 4 frames; the window reveals the center (title/prompt region) first; disable flag for users who dislike it.
- [Scratch-buffer blit every frame while a dialog irises] → bounded to ≤ 4 frames per open; a dialog never exceeds the terminal minus the 2-cell overlay margin (the largest, the Help window, is 62×19 at 80×24), so each blit is at most a few thousand cells.
- [Hue-free themes show little color change] → structural motion carries the effect; color steps are decorative, consistent with the mono-theme rule that color never carries meaning.
- [Key-repeat starvation of `Tick` freezes an animation mid-way] → state is already final; animation resumes/completes on the next idle poll. Accepted (D1).
- [Progress/conflict/error dialogs iris during a file job] → dialog state is final and Cancel/Retry buttons are live from frame 1; the iris is cosmetic.

## Open Questions

- None. Surfaces, intensity, disable mechanism, and design-doc scoping were settled with the user.
