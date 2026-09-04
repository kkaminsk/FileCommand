# Design: theme-picker-live-preview

## Context

`handle_theme_picker` in `filecommand-core/src/update.rs` drives the picker: Move mutates `ThemePickerState::highlight`, Confirm swaps `state.theme` in the same reducer step and emits `Effect::PersistTheme`, Cancel drops the dialog. Confirm already resolves the theme via `BUILTIN_THEME_NAMES.get(picker.highlight)` + `Theme::by_name`. Rendering resolves every color from `&state.theme` at ~25 call sites in `filecommand-tui/src/views/mod.rs`; the picker view additionally takes `&state.theme.name` to draw the active-theme marker. The theme-system spec guarantees whole-screen restyling is a pure role-table swap with no renderer branches.

## Goals / Non-Goals

**Goals:**

- Whole-screen preview of the highlighted theme, updating on every highlight move, with zero risk of a preview leaking into applied state or `config.toml`.
- Enter/Esc semantics byte-identical to today from the reducer's perspective.

**Non-Goals:**

- No preview outside the picker; no partial/split previews; no persistence of preview state; no changes to picker navigation or the theme list.

## Decisions

### D1: Preview is a render-time derivation, not a state mutation

Core gains a pure helper — `State::render_theme()` — returning `Theme::by_name(BUILTIN_THEME_NAMES[picker.highlight])` while `state.theme_picker` is open (falling back to the active theme on any lookup miss) and the active `state.theme` otherwise. The TUI resolves it once per frame and hands it to every renderer. Nothing in `State` changes on highlight moves beyond the existing `highlight` index; Esc restores the old look by construction (picker gone → helper returns the active theme), and no crash, resize, or interleaved effect can persist or leak a previewed theme. Alternative rejected: reducer swaps `state.theme` on every `ThemePickerMove` and restores a saved original on cancel — mutation plus restore bookkeeping recreates exactly the leak class this design avoids (e.g. anything that reads `state.theme` mid-preview sees the wrong "active" theme), for no benefit.

### D2: The active-theme marker tracks the applied theme, not the preview

The picker keeps receiving `state.theme.name` for its marker. The highlight bar already communicates "what you are previewing"; the marker keeps answering "what is applied," which is exactly the distinction preview introduces. Rebinding the marker to the highlight would make the two indicators redundant and lose the applied-theme anchor.

### D3: Every surface previews, including overlays above the picker

The helper is resolved once at the frame's root, so anything drawn that frame — including the quit-confirmation overlay if invoked over the picker — renders in the previewed theme. One consistent rule; carving out exceptions would require per-surface theme plumbing, reintroducing precisely what the role system removed.

### D4: Per-frame theme construction is acceptable

`Theme::by_name` builds a ~40-entry role table; at the 33ms poll cadence this is microseconds and allocation-trivial. Caching keyed on highlight index was rejected as premature: it adds invalidation state for no measurable win.

## Risks / Trade-offs

- [Reducer tests that assert on rendered theme via `state.theme` stay green but no longer describe what's on screen during preview] → new unit tests target `render_theme()` directly for the open/move/cancel/confirm matrix.
- [A future non-built-in theme source would bypass `BUILTIN_THEME_NAMES`] → the helper's lookup-miss fallback to the active theme makes that safe-by-default; noted for whenever external themes are specced.

## Open Questions

- None. The interaction model (browse = preview, Enter = commit, Esc = walk away) was requested directly by the user.
