# Change: responsive-layout

## Why

FileCommand today treats 80×24 as a cliff: one column or row below it and the entire application is replaced by the "terminal too small" placeholder (design doc §4; `application-shell` "Resize handling with 80x24 minimum and placeholder"). Above the cliff the layout is rigid — a fixed 50/50 panel split (§4.1), Full-mode columns at hardcoded widths, Brief mode locked to exactly three columns (§4.2), an F-key bar that silently truncates mid-label when it runs out of width, and per-surface ad-hoc dialog sizes (§4.4, the 48-column splash in §4.8, the 62×19 Help and 52×10 About in §4.9). Modern terminal users work in split panes, quarter-screen snaps, and ultrawide windows; the NC 5.5 look should survive all of them. This change makes the UI genuinely responsive: full NC fidelity at the nominal 80×24 and above, graceful tiered degradation down to a hard floor of 60×16, one consistent geometry rule for every overlay, and a user-adjustable panel split. It supersedes the design doc's §4 "minimum supported size is 80×24" statement — 80×24 becomes the *nominal full-fidelity* size and 60×16 the new *supported floor*; the design doc is cited, not edited, and the OpenSpec specs are the source of truth going forward.

## What Changes

- **BREAKING** (requirement-level): the size gate moves from 80×24 to a 60×16 hard floor. The "terminal too small" placeholder no longer appears between 60×16 and 80×24; in that band the UI renders in degraded-but-fully-functional form. The single size check governing normal and splash states is retained, re-pointed at the floor, and the placeholder message text changes accordingly.
- Panel-content degradation is keyed to the **individual panel's width**, not the terminal width (mandatory once the split is adjustable): Full mode drops columns rightmost-first (Time → Date → Size) to keep the Name column at least 12 display cells; the column-header row and mini-status drop the same fields in the same order.
- Brief mode derives its column count from panel width — `max(1, floor(interior_width / 12))` — instead of hardcoding three columns; at 80×24 with the default split this yields exactly today's three-column output.
- The F-key bar renders the widest of three canonical forms that fits (full labels, three-letter labels, numbers-only) and never truncates mid-label; the rule extends to the Ctrl/Alt modifier variants and the viewer/editor bars.
- One unified overlay-geometry rule — a core-owned clamp-and-center helper — governs every overlay: splash, Help, About, all operation/error/progress dialogs, drive select, find-file, fuzzy-jump, user menu, quit dialog, and the F9 pull-down boxes (which shift left to stay on-screen instead of centering). All overlays re-center on every resize and never paint outside the terminal.
- Remaining chrome degrades deterministically: the clock hides entirely when it would collide with the panel title, the command line scrolls to keep the caret visible, and the viewer/editor header indicators drop right-to-left.
- New user-facing feature: the vertical panel split is adjustable with Ctrl+Left / Ctrl+Right (2-column steps), reset with Ctrl+= , stored as a percentage with a 20-column per-panel minimum, and persisted to `config.toml`.

## Capabilities

### New Capabilities

- `responsive-layout`: The cross-cutting responsive geometry system — the degraded band between the 60×16 floor and the 80×24 nominal size, per-panel-width breakpoints, the three F-key-bar forms, the unified overlay clamp-and-center rule, and chrome/full-screen-surface degradation.
- `panel-split`: The user-adjustable vertical panel split — keybindings, percentage semantics with per-panel minimum width and non-destructive clamping, and persistence to `config.toml`.

### Modified Capabilities

- `application-shell`: The resize requirement is renamed and re-anchored — reflow at any size at or above the 60×16 floor; placeholder only below the floor.
- `startup-splash`: Below-minimum behavior re-anchored to the floor; the splash box re-centers via the unified overlay rule and renders throughout the degraded band.
- `help-and-about`: The Help window's geometry flows through the unified overlay rule, clamping below 80×24 instead of assuming it.
- `additional-panel-modes`: Brief mode's "three columns" becomes the width-derived column formula.
- `panel-navigation`: Full mode's fixed `Name | Size | Date | Time` header becomes the column ladder keyed to panel width.

## Impact

- **Crates:** `filecommand-core` — floor constants and size gate in `update.rs`, split state and adjust/reset commands, generalized overlay-geometry helper in `dialogs.rs` (extending the existing `help_window_height` precedent), `panel_split` key in `config.rs`. `filecommand-tui` — `layout.rs` (percentage split, clamping), `views/panel.rs` (column ladder, Brief formula), `views/keybar.rs` (canonical forms), `views/placeholder.rs` (message text), and every dialog/overlay view migrated to the shared geometry helper.
- **Depends on:** M1 (application shell, resize gate, theme roles), M2–M4 (panels, dialogs, menus), M5 (viewer/editor chrome, Help window).
- **Out of scope:** mouse-drag split adjustment, per-tab split ratios, horizontal (top/bottom) panel stacking, user-configurable breakpoints, and any sub-floor "tiny" rendering mode.
