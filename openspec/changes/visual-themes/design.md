# Design: visual-themes

## Context

The M1 theme system already does the heavy lifting: every renderer resolves colors exclusively through named roles in the active theme, themes are compiled-in role tables validated for completeness, and the color policy is mandatory ANSI-16 named values with optional truecolor overrides. Swapping the active theme restyles the whole application with no renderer changes — the `nc-classic` → `nc-mono` swap scenario already proves this. `config.toml` carries a `theme =` key read at startup. What's missing is more themes and a runtime switcher: the F9 Options menu lists "Themes" but renders it disabled.

## Goals / Non-Goals

**Goals:**

- Four new compiled-in themes — `terminal-green`, `purple-lights`, `yellow-storm`, `inverted` — each a complete role table passing the existing validation.
- Options → Themes opens a picker; Enter applies the theme immediately (next frame) and persists it; Esc leaves everything untouched.
- Startup honors the persisted choice, falling back to `nc-classic` for unset or unknown names (existing behavior).

**Non-Goals:**

- No user theme files (`themes/*.toml` loading) — deferred; the picker lists compiled-in themes only.
- No per-role customization UI, no live preview while moving the highlight (Enter applies; Esc cancels).
- No changes to the role model, ANSI-16/truecolor policy, CP437 glyph policy, or the existing `nc-classic`/`nc-mono` tables.

## Decisions

### D1: Themes are compiled-in role tables, like the existing two

Each new theme is a complete role table in `filecommand-core::theme`, validated by the existing "every role defined" check. Rationale: zero new infrastructure; the theme swap scenario in `theme-system` already guarantees a full restyle with no renderer edits. Alternative considered: shipping the new themes as TOML files in the config directory — rejected; file loading is a separate feature with parsing/fallback concerns, and the design doc's user-theme story stays open for a later change.

### D2: Theme identities are anchored by normative role samples, not exhaustive tables

The spec pins each theme's identity with representative role anchors (backdrop, frame, directory, cursor, key-bar label) plus a construction rule, mirroring how the `nc-classic` and `nc-mono` requirements are written. `terminal-green` and `yellow-storm` are single-hue themes built like `nc-mono` (base hue on black, bright variant for directories/selected, inversion for cursor/menus/dialogs) with the mono rule that color never carries meaning not also carried by case, position, or inversion. `purple-lights` follows the `nc-classic` structure with magenta/bright-magenta standing in for blue/cyan. `inverted` is a high-contrast accessibility theme built as the light counterpart of `nc-mono`: base black on bright-white, every `nc-mono` inversion becomes bright-white on black, directories/selected entries distinguished by inversion or weight rather than hue, and no hue is used anywhere — maximum contrast for vision-impaired users under the same mono rule (color never carries meaning not also carried by case, position, or inversion). Rationale: keeps the spec testable without freezing all ~30 roles; the construction rules make the remaining roles derivable and reviewable in snapshots.

### D3: Enter applies and persists; Esc cancels; no extra confirmation

Applying a theme is instantly visible and instantly reversible by reopening the picker, so no confirmation step is warranted. Selection writes `theme = "<name>"` to `config.toml` atomically (same atomic-write discipline as `history.json`) and updates the active theme in the same reducer step. Rationale: "switches at runtime" per design doc §6; a Save-setup-gated persistence would let the visible state and config silently diverge. Alternative considered: apply in memory and persist only via Options → Save setup — rejected as surprising (theme reverts on restart unless the user knows to save).

### D4: The picker is a standard primary-style modal dialog

A small modal list dialog (§4.4 primary style, single-line frame like other pickers) titled `Themes`, listing the six built-in themes by name with the active theme marked (`▶` cursor conventions and CP437 glyphs only). Up/Down move, Enter applies and closes, Esc closes without change. State lives in `filecommand-core` with mutations through `core::update`, matching every other dialog. Alternative considered: cycling themes directly from the menu item without a dialog — rejected; six themes deserve a visible list, and the menu already closes on activation.

### D5: `pulldown-menus` scenario example moves off Themes

The "not-yet-available renders disabled" scenario currently uses Themes as its example. The MODIFIED requirement keeps the disabled-item rule but re-anchors the example on a still-unimplemented item (Attributes), and states that Options → Themes dispatches the theme-selection dialog. Rationale: specs must stay truthful; the generic rule is unchanged.

## Risks / Trade-offs

- [ANSI-16 magenta/yellow render very differently across terminal palettes, so `purple-lights`/`yellow-storm` may look harsh in some terminals] → mandatory ANSI-16 values keep every theme legible everywhere; optional truecolor overrides deliver the intended deep-purple/amber look on capable terminals.
- [`inverted` legibility depends on the terminal's bright-white actually rendering bright] → the theme uses only black and bright-white, the two poles of every terminal palette, so contrast is maximal on any conformant terminal; snapshot tests render representative screens under every new theme so any unreadable combination is visible in review.
- [Config write-back on every theme switch could clobber user comments/formatting in `config.toml`] → scope the write to updating the `theme` key via the existing config save path with an atomic write; the config module already owns serialization.

## Open Questions

- None. Theme roster and menu placement were given by the user; persistence and picker behavior follow the design doc and existing dialog conventions.
