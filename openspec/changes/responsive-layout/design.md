# Design: responsive-layout

## Context

The size model today is binary: a single `too_small()` gate in `filecommand-core` (MIN_COLS 80 / MIN_ROWS 24) decides between the full layout and the `screen.placeholder` message, uniformly across normal and splash states — the M1 design explicitly chose "one code path for 'am I big enough'". Above the gate, geometry is fixed: `layout.rs` computes a hardcoded 50/50 split, `views/panel.rs` uses fixed Size/Date/Time column widths (9/8/5) with the Name column absorbing the remainder, Brief mode hardcodes `interior / 3` columns, and `views/keybar.rs` draws its ten fixed labels left-to-right and simply stops at the right edge. Overlay sizing is per-surface: the splash box is 48×10, About roughly 52×10, and only the Help window has size-derived geometry — `help_window_height` in `filecommand-core::dialogs`, shared between core scroll math and the tui renderer, which is the precedent this change generalizes. Two in-repo precedents matter: `panel-tabs` already specifies deterministic stepwise degradation (label shrinking then scrolling), and the pending `visual-themes` change established immediate atomic persistence of a `config.toml` key.

## Goals / Non-Goals

**Goals:**

- Full NC fidelity at and above 80×24, unchanged to the pixel — existing 80×24 snapshots stay byte-identical.
- A degraded-but-fully-functional band from the 60×16 hard floor up to 80×24; the placeholder appears only below the floor.
- Deterministic, reversible degradation everywhere: panel columns, Brief columns, F-key bar, overlays, chrome — no surface ever silently truncates or paints outside the terminal.
- A user-adjustable, persisted vertical panel split with sane limits.

**Non-Goals:**

- No mouse-drag split adjustment, per-tab split ratios, or horizontal panel stacking.
- No user-configurable breakpoints or degradation order.
- No rendering below the floor beyond the existing placeholder; no scrolling/paging of the placeholder itself.
- No changes to display-mode semantics (what Brief/Full/Info/Tree/Quick View show) beyond geometry.

## Decisions

### D1: The hard floor is 60×16

At 60 columns the default split yields two 30-column panels (28 interior) — a comfortable name-plus-size listing; the compressed F-key bar (50 columns) and the 48-column splash box fit with margin. At 16 rows the panels keep 11 entry rows after panel chrome, command line, and key bar, and the Help window's existing 10-row height floor still fits. 60×16 is also exactly a quarter-screen snap of the 120×30 Windows Terminal default — a size users actually hit. Alternative considered: a lower floor such as 40×12 — rejected because the key bar's numbers-only form barely fits, most dialogs become unrenderable without redesigning their content, and two panels stop being meaningfully usable. Alternative considered: keeping the 80×24 gate — rejected; it fails the goal outright.

### D2: Breakpoints are functions of the individual panel's width

All panel-content degradation (Full-mode column ladder, Brief column count, mini-status fields) keys off the panel's own width, not the terminal's. With an adjustable split a 100-column terminal can hold a 22-column panel, so terminal-width tiers would degrade the wrong panel or fail to degrade at all. Alternative considered: global terminal-size tiers (e.g. "small/medium/large" modes) — rejected as broken by construction once the split is adjustable, and coarser than needed.

### D3: Full mode drops columns rightmost-first to protect a 12-cell Name column

Define MIN_NAME_W = 12 display cells. Full mode renders the widest set from the ladder `Name+Size+Date+Time → Name+Size+Date → Name+Size → Name` that keeps Name ≥ 12. The drop order is rightmost-first (Time, then Date, then Size) — the fields NC users scan least-first — and growth reverses it deterministically. Anchor: at 80×24 with the default split (panel 40, interior 38) the Name column gets 13 cells, so all four columns render — zero change at the nominal size. At the floor (panel 30) Name+Size renders; at the 20-column panel minimum, name-only. Alternative considered: proportionally shrinking all columns — rejected because dates/sizes have fixed natural widths; squeezing them misaligns the columns and breaks the NC look.

### D4: Brief columns come from `max(1, floor(interior / 12))`

The divisor 12 matches MIN_NAME_W. Anchor: at 80×24 the interior is 38, giving exactly 3 columns of widths 12/12/14 (remainder to the last column) — byte-identical to today's hardcoded `interior / 3` output, so existing snapshots survive. Wider panels earn more columns (interior 60 → 5); narrow panels drop to 2, then 1. Alternative considered: keeping 3 columns and widening them — rejected; it wastes exactly the wide terminals this change targets.

### D5: The F-key bar has three canonical forms, widest-that-fits

Full labels (`1Help 2Menu 3View 4Edit 5Copy 6RenMov 7Mkdir 8Delete 9PullDn 10Quit`, 67 columns), short three-letter labels (`1Hlp 2Mnu 3Vew 4Edt 5Cpy 6Ren 7Mkd 8Del 9Pdn 10Qit`, 50 columns), and numbers-only (`1 2 3 4 5 6 7 8 9 10`, 20 columns). The bar renders the widest form that fits the terminal width and never truncates mid-label; the same rule covers the Ctrl/Alt modifier variants and the viewer/editor bars with their own short forms. At the 60-column floor the short form fits; numbers-only is defensive headroom. Alternatives considered: horizontally scrolling the bar — rejected, no NC precedent and it hides commands; dropping trailing slots — rejected, F10 Quit must always stay visible.

### D6: One core-owned `overlay_rect` helper replaces per-view geometry

`overlay_rect(preferred, minimum, terminal)` computes `w = clamp(min(pref_w, term_w − 2), min_w, term_w)` (same for height) and centers the result — a direct generalization of the existing `help_window_height`, kept in `filecommand-core` so reducer scroll math and tui rendering share one truth. Every overlay declares a preferred and minimum size, with all minimums ≤ 58×14 so everything is renderable at the floor; interiors truncate with `…` rather than paint outside the rect; all overlays re-center on every resize. F9 pull-down boxes shift left instead of centering, so each box stays hung from its menu title while remaining fully on-screen. Alternative considered: per-view clamping — rejected; scattered ad-hoc clamps are exactly the current bug class (views that draw nothing, or draw outside, when small).

### D7: The split is a persisted percentage with non-destructive clamping

Stored as integer `split_percent` (default 50); effective `left = round(width × percent / 100)` with round-half-up, then clamped so each panel keeps at least 20 columns. Clamping never rewrites the stored percentage — a saved 75% split on a shrunken terminal renders clamped and springs back when the terminal grows. Adjust steps are 2 columns per keypress (converted to the nearest percentage); adjustments that would violate a panel minimum are no-ops. Alternative considered: storing absolute columns — rejected; a column count is meaningless across terminal resizes, while a percentage preserves intent.

### D8: Split keys are Ctrl+Left / Ctrl+Right / Ctrl+= (reset)

Verified conflict-free: no existing binding in the design doc §5 table, any spec, or the source uses Ctrl+Arrow or Ctrl+=. All three are overridable in `config.toml` per the existing keymap convention. Ctrl+Arrow keys are reliably delivered as native console records on Windows; Ctrl+= is the risky one per the §5 key-delivery matrix, so the tasks include a deliverability check with a documented config-override alternate if a host can't deliver it. Alternative considered: Alt+Arrow — rejected; commonly stolen by terminal hosts and window managers.

### D9: The application-shell requirement is RENAMED, not just modified

Its title embeds "80x24 minimum"; leaving the old title over a 60×16 body would be internally inconsistent. The delta uses a RENAMED entry (supported by openspec CLI 1.4.x) pairing the old and new titles, with the MODIFIED body under the new title. Alternative considered: keeping the stale title — rejected as misleading in the one place people look first.

## Risks / Trade-offs

- [Large insta-snapshot churn from size-dependent rendering] → new snapshots at a pinned matrix (60×16, 70×20, 80×24, 120×30); the 80×24 output is unchanged by construction (D3/D4 anchors), asserted by a regression test.
- [Ctrl+= undeliverable in some terminal hosts] → config-overridable binding plus a documented alternate, following the §5 key-delivery-matrix convention.
- [Percentage rounding causes 1-column divider jitter during resize] → round-half-up specified normatively; a property test walks widths 60..200 asserting monotone, jitter-free divider positions.
- [Three-letter key-bar abbreviations may be unreadable to newcomers] → the forms are enumerated in the spec and reviewed via snapshots; F1 Help and the full-label form at ≥ 67 columns remain the canonical reference.
- [Both `visual-themes` and this change write `config.toml`] → no key conflict (`theme` vs `panel_split`); whichever change builds second reuses the same atomic write-back path rather than adding a parallel one.

## Open Questions

- None. Scope (floor with degradation, adaptive Brief, unified overlay rule, adjustable split) was confirmed by the user; constants and keybindings follow from the analysis above.
