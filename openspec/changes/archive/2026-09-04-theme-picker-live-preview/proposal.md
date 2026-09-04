# Change: theme-picker-live-preview

## Why

The theme picker (Options → Themes, F2 → Themes) applies a theme only on Enter: the user must commit blind, check the result, and re-open the picker to try the next candidate — six themes means up to six open/apply/reopen cycles. Live preview turns selection into a single browse: as the highlight moves, the entire screen renders in the highlighted theme, and Esc walks away with nothing changed. This is the interaction users expect from every modern theme switcher, and the theme system was explicitly built for whole-screen restyling with no renderer changes ("Runtime swap to a new theme restyles everything"), so the capability is already latent.

## What Changes

- While the theme picker is open, every rendered surface — panels, key bar, command line, clock, the picker dialog itself, and any overlay above it — resolves its roles through the **highlighted** theme instead of the active theme. Moving the highlight repaints the whole frame in the newly highlighted theme on the next frame.
- The preview is render-only: `state.theme`, `config.toml`, and the picker's active-theme marker all continue to reflect the **applied** theme throughout.
- Enter (apply + persist + close) and Esc (close, nothing changed) keep their exact semantics; Esc now also visually restores the applied theme by construction, since the preview ends when the picker closes.
- Opening the picker is visually a no-op: the highlight starts on the active theme, so the first previewed frame is identical to the current screen.

## Capabilities

### New Capabilities

*(none)*

### Modified Capabilities

- `theme-selection`: one ADDED requirement — live whole-screen preview of the highlighted theme while the picker is open, with cancel restoring the applied theme.

## Impact

- **Crates:** `filecommand-core` — a pure derivation helper (e.g. `State::render_theme()`) returning the highlighted built-in theme while `state.theme_picker` is open, else the active theme; no reducer changes. `filecommand-tui` — `views/mod.rs` (and the `app.rs` draw entry) resolve one effective theme per frame via the helper and pass it to every renderer; the picker keeps receiving `state.theme.name` for its marker.
- **Depends on:** `visual-themes` (the picker and its spec), M1 theme system.
- **Out of scope:** previewing user-defined/external themes (none exist), debounce/animation, previewing from the F9 Options row itself without opening the picker.
