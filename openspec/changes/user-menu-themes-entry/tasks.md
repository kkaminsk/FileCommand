# Tasks: user-menu-themes-entry

## 1. Core reducer

- [ ] 1.1 Extend the user-menu cursor domain to include the built-in Themes slot at index `entries.len()`, with Up/Down clamping across user entries + built-in entry as one list (`dialogs.rs` UserMenuState, `update.rs` UserMenuMove) (user-menu: "Navigate and dismiss the user menu")
- [ ] 1.2 UserMenuConfirm on the built-in slot closes the user menu first, then opens the theme picker via the existing `ThemePickerState::open(&state.theme.name)` path, emitting no shell effect (user-menu: "Built-in Themes entry opens the theme selector")
- [ ] 1.3 UserMenuConfirm on a user entry keeps its exact shell dispatch (`Effect::RunShellCommand`, active-panel cwd) (user-menu: "Run the selected entry's command via the shell in the active panel directory")

## 2. TUI view

- [ ] 2.1 Render the separator row and built-in `Themes` row below the user entries in `views/user_menu.rs`, including above-separator placeholder in the empty case; update height/width math (user-menu: "Open the F2 user menu")
- [ ] 2.2 Highlight rendering covers the built-in row; the separator row is never rendered highlighted (user-menu: "Navigate and dismiss the user menu")

## 3. Tests

- [ ] 3.1 Reducer tests: Down past the last user entry lands on Themes and clamps there; Enter on Themes opens the picker pre-highlighted on the active theme with no shell effect; Enter on a user entry still emits RunShellCommand; picker Esc after F2-origin returns to panels without reopening the menu (user-menu: "Built-in Themes entry opens the theme selector"; "Navigate and dismiss the user menu")
- [ ] 3.2 Snapshot tests: user menu with entries + separator + Themes; empty-file variant with placeholder + separator + Themes (user-menu: "Open the F2 user menu")
- [ ] 3.3 Migrate existing user-menu tests/snapshots affected by the two added rows (user-menu: "Open the F2 user menu")
