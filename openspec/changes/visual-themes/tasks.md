# Tasks: visual-themes

## 1. New built-in themes

- [ ] 1.1 Add the `terminal-green` role table to `filecommand-core::theme` (green/bright-green on black, black-on-green inversions, optional truecolor phosphor overrides) (theme-system: "Built-in terminal-green theme")
- [ ] 1.2 Add the `purple-lights` role table (nc-classic structure with magenta/bright-magenta, optional violet truecolor overrides) (theme-system: "Built-in purple-lights theme")
- [ ] 1.3 Add the `yellow-storm` role table (yellow/bright-yellow on black, bright-white selected, black-on-yellow inversions, optional amber truecolor overrides) (theme-system: "Built-in yellow-storm theme")
- [ ] 1.4 Add the `inverted` high-contrast role table (black on bright-white base, bright-white-on-black inversions, no hue anywhere) (theme-system: "Built-in inverted high-contrast theme")
- [ ] 1.5 Verify all four tables pass the every-role-defined validation and ANSI-16 policy checks (theme-system: "New themes satisfy validation and swap semantics")

## 2. Theme picker

- [ ] 2.1 Add theme-picker dialog state to `filecommand-core` (theme list, active marker, highlight) with open/navigate/apply/cancel handled in `core::update` (theme-selection: "Options menu opens the theme picker")
- [ ] 2.2 Wire Options → Themes in the pull-down menu dispatch to open the picker, replacing the disabled placeholder (pulldown-menus: "Menu contents")
- [ ] 2.3 Implement apply-on-Enter: switch the active theme for the next frame and close the dialog; Esc closes with no change (theme-selection: "Picker navigation, apply, and cancel")
- [ ] 2.4 Add the picker dialog view in `filecommand-tui/src/views/` (primary style, active-theme marker, CP437 glyphs only) and modal input routing (theme-selection: "Options menu opens the theme picker")

## 3. Persistence

- [ ] 3.1 Write `theme = "<name>"` to `config.toml` atomically on apply via the config module's save path (theme-selection: "Applied theme persists to configuration")
- [ ] 3.2 Confirm startup loads the configured theme and falls back to `nc-classic` on unset/unknown values (theme-selection: "Applied theme persists to configuration")

## 4. Tests

- [ ] 4.1 Reducer tests: picker open from Options menu, navigation, Enter applies + persists, Esc no-op (theme-selection: "Picker navigation, apply, and cancel")
- [ ] 4.2 Theme validation tests for the four new tables, including the no-hue property of `inverted` and selected-entry distinction of `yellow-storm` (theme-system: "New themes satisfy validation and swap semantics")
- [ ] 4.3 `insta` snapshot tests: representative screens (panels, dialog, key bar) rendered under each new theme (theme-system: "New themes satisfy validation and swap semantics")
- [ ] 4.4 Snapshot test for the theme-picker dialog with the active theme marked (theme-selection: "Options menu opens the theme picker")
