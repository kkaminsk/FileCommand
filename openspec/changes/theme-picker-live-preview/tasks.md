# Tasks: theme-picker-live-preview

## 1. Core derivation helper

- [ ] 1.1 Add `State::render_theme()` to `filecommand-core` — highlighted built-in while `theme_picker` is open (lookup-miss falls back to the active theme), active theme otherwise (theme-selection: "Live theme preview while the picker is open")
- [ ] 1.2 Unit tests: open → equals active theme; after `ThemePickerMove` → equals highlighted theme; after `ThemePickerCancel` → active theme; after `ThemePickerConfirm` → newly applied theme; `state.theme` and persistence untouched by moves (theme-selection: "Live theme preview while the picker is open")

## 2. Render path

- [ ] 2.1 Resolve the effective theme once per frame in `filecommand-tui` (`views/mod.rs` render entry + the `app.rs` draw path) and pass it to every renderer currently taking `&state.theme` (theme-selection: "Live theme preview while the picker is open")
- [ ] 2.2 Keep the picker's active-theme marker bound to `state.theme.name` while the dialog styles itself from the previewed theme (theme-selection: "Live theme preview while the picker is open")

## 3. Tests

- [ ] 3.1 `insta` snapshot: picker open at 80×24 with a non-active theme highlighted — whole frame (panels, key bar, picker) rendered in the highlighted theme, marker still on the active theme's row (theme-selection: "Live theme preview while the picker is open")
- [ ] 3.2 Snapshot/regression: picker open with highlight on the active theme is byte-identical to the pre-change picker-open frame (theme-selection: "Live theme preview while the picker is open")
- [ ] 3.3 Full `cargo build --workspace` and `cargo test --workspace` pass
