# Change: visual-themes

## Why

FileCommand ships only two looks — the default NC blue (`nc-classic`) and the accessibility-oriented `nc-mono` — and the Options → Themes menu item is a disabled placeholder, so there is no way to change the appearance at runtime. The design doc already promises configurable themes switched from Options → Themes (§6); this change delivers that promise with a set of distinct built-in themes on top of the default blue.

## What Changes

- Add four compiled-in themes alongside `nc-classic` and `nc-mono`:
  - **`terminal-green`** — green-phosphor monochrome: green on black base, bright-green directories/highlights, inverse black-on-green cursor/menus/dialogs.
  - **`purple-lights`** — magenta/violet take on the classic layout: magenta backdrop, bright-magenta frames and inverse accents, optional truecolor deep-purple/violet overrides.
  - **`yellow-storm`** — amber-terminal look: yellow on black base, bright-yellow directories/highlights, inverse black-on-yellow accents, optional truecolor amber overrides.
  - **`inverted`** — high-contrast black-on-white accessibility theme for vision-impaired users: black text on a bright-white background with bright-white-on-black inversion for cursor/menus/dialogs — the light counterpart of `nc-mono`, using no hues at all.
- Activate the **Options → Themes** pull-down item: it opens a modal theme-picker dialog listing every built-in theme with the active one marked; Enter applies the chosen theme to the whole screen immediately at runtime, Esc closes without changing anything.
- Persist the chosen theme: applying a theme writes `theme = "<name>"` to `config.toml` (atomic write), so the selection survives restart; on startup the configured theme loads as before, falling back to `nc-classic` when unset or unknown.
- All new themes obey the existing theme-system rules: every renderer role defined, mandatory ANSI-16 named colors with optional `#RRGGBB` truecolor overrides, no 256-color output, CP437-only glyphs.

## Capabilities

### New Capabilities

- `theme-selection`: The Options → Themes picker dialog — list of available themes with the active one marked, navigation, immediate full-screen apply on Enter, Esc cancel — and persistence of the selection to `config.toml`.

### Modified Capabilities

- `theme-system`: Adds requirements for the four new built-in themes (`terminal-green`, `purple-lights`, `yellow-storm`, `inverted`); existing `nc-classic` / `nc-mono` requirements are unchanged.
- `pulldown-menus`: The Menu contents requirement changes — Options → Themes is now an enabled item that opens the theme-selection dialog (it was the canonical "renders disabled" example).

## Impact

- **Crates:** `filecommand-core` — four new role tables in `theme.rs`, theme-picker dialog state and reducer routing in `update.rs`/`dialogs.rs`, config write-back of the `theme` key in `config.rs`. `filecommand-tui` — theme-picker dialog view, Options-menu wiring; no renderer changes (all rendering already resolves through theme roles).
- **Depends on:** M1 (`theme-system` role model and ANSI-16 policy), M3 (`pulldown-menus` Options menu).
- **Out of scope:** loading user theme TOML files from the config directory (`themes/*.toml`) — the picker lists compiled-in themes only for now.
