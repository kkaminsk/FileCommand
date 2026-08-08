# Tasks: responsive-layout

## 1. Core size model

- [ ] 1.1 Change `MIN_COLS`/`MIN_ROWS` in `filecommand-core::update` to 60/16, keeping the single `too_small()` gate for normal and splash states (application-shell: "Resize handling with 60x16 hard floor and placeholder")
- [ ] 1.2 Update the placeholder message in `filecommand-tui/src/views/placeholder.rs` to name the 60x16 floor (application-shell: "Resize handling with 60x16 hard floor and placeholder")
- [ ] 1.3 Verify splash skip/replace/no-return behavior against the new floor and splash rendering throughout the degraded band (startup-splash: "Resize and below-minimum-size behavior")

## 2. Panel content degradation

- [ ] 2.1 Implement the Full-mode column ladder in `views/panel.rs` — MIN_NAME_W 12, rightmost-first drop (Time → Date → Size), header row matching rendered columns (panel-navigation: "Full display mode layout")
- [ ] 2.2 Replace Brief mode's hardcoded three columns with `max(1, floor(interior_width / 12))`, remainder to the last column (additional-panel-modes: "Brief display mode")
- [ ] 2.3 Key all panel-content breakpoints off the individual panel width and make degradation reversible on growth (responsive-layout: "Degraded band and per-panel breakpoints")
- [ ] 2.4 Implement mini-status field dropping in ladder order, clock hide-not-collide, and command-line caret scrolling with left-truncated prompt (responsive-layout: "Chrome degradation")

## 3. F-key bar

- [ ] 3.1 Implement the three canonical forms (full 67 cols, short 50 cols, numbers-only 20 cols) with widest-form-that-fits selection in `views/keybar.rs`, removing the mid-label truncation (responsive-layout: "F-key bar degradation forms")
- [ ] 3.2 Apply the same form selection to the Ctrl/Alt modifier variants and the viewer/editor key bars with their own short-form labels (responsive-layout: "F-key bar degradation forms")

## 4. Overlay geometry

- [ ] 4.1 Add the core-owned `overlay_rect(preferred, minimum, terminal)` helper to `filecommand-core::dialogs`, generalizing `help_window_height` (responsive-layout: "Unified overlay geometry")
- [ ] 4.2 Declare preferred/minimum sizes (all minimums ≤ 58×14) and migrate every overlay view — splash, About, operation/input/confirmation/error/progress dialogs, drive select, find-file, fuzzy jump, user menu, quit dialog — to the helper, with `…` interior truncation and re-centering on resize (responsive-layout: "Unified overlay geometry")
- [ ] 4.3 Apply size clamping with shift-left-instead-of-center positioning to the F9 pull-down boxes (responsive-layout: "Unified overlay geometry")
- [ ] 4.4 Route the Help window through the helper with preferred 62×19 / minimum 40×10, keeping core scroll math and rendering on the shared geometry (help-and-about: "F1 Help window frame and identity header")
- [ ] 4.5 Implement viewer/editor header indicator dropping right-to-left with left-truncated path (responsive-layout: "Full-screen surface degradation")

## 5. Adjustable split

- [ ] 5.1 Add `split_percent` state (default 50) and adjust/reset commands to `filecommand-core`, bound to Ctrl+Left / Ctrl+Right / Ctrl+= and overridable via the keymap (panel-split: "Adjust and reset the panel split")
- [ ] 5.2 Derive the effective split in `filecommand-tui/src/layout.rs` — round-half-up percentage, 20-column per-panel minimum, non-destructive clamping, limit adjustments as no-ops (panel-split: "Split ratio semantics and panel minimum")
- [ ] 5.3 Persist `panel_split = <percent>` to `config.toml` via the atomic write path, load at startup with fallback to 50 on unset/invalid values (panel-split: "Split persistence to configuration")
- [ ] 5.4 Verify Ctrl+= delivery in Windows Terminal and conhost per the design-doc §5 key-delivery matrix; document the config-override alternate if undeliverable (panel-split: "Adjust and reset the panel split")

## 6. Tests

- [ ] 6.1 Reducer/property tests: column ladder over panel widths 20..80, Brief formula, `overlay_rect` contained in terminal at all sizes ≥ 60×16, split percent round-trip and clamp spring-back, divider monotonicity over widths 60..200 (responsive-layout: "Degraded band and per-panel breakpoints"; panel-split: "Split ratio semantics and panel minimum")
- [ ] 6.2 `insta` snapshot matrix at 60×16, 70×20, 80×24, and 120×30 covering panels (Full and Brief), key bar forms, and a clamped dialog (responsive-layout: "Unified overlay geometry"; responsive-layout: "F-key bar degradation forms")
- [ ] 6.3 Regression assertion that all existing 80×24 snapshots are byte-identical after the change (panel-navigation: "Full display mode layout"; additional-panel-modes: "Brief display mode")
- [ ] 6.4 Reducer tests for split adjust/reset/no-op-at-limit and persistence including invalid-value fallback (panel-split: "Adjust and reset the panel split"; panel-split: "Split persistence to configuration")
- [ ] 6.5 Snapshot tests for placeholder below the floor and splash at 60×16 (application-shell: "Resize handling with 60x16 hard floor and placeholder"; startup-splash: "Resize and below-minimum-size behavior")
