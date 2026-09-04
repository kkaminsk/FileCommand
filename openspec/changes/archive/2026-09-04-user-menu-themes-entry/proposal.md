# Proposal: user-menu-themes-entry

## Why

Switching themes today requires F9 → Options → Themes — three keystrokes of pull-down navigation for a setting users reach for right after install. The F2 user menu is the application's quick-access menu, but it can only run shell commands from `usermenu.toml`; it has no way to surface built-in functionality. Appending a compiled-in Themes entry makes theme switching a two-keystroke flow (F2, Enter) and establishes the pattern of the user menu carrying built-in sub-options below the user's own entries.

## What Changes

- The F2 user menu gains a **built-in `Themes` entry**, rendered below the user's `usermenu.toml` entries and set off by a separator row. It is always present regardless of `usermenu.toml` content and is not configurable through the file.
- Activating it closes the user menu and opens the **existing theme-selection dialog** — same picker, navigation, immediate-apply, and `config.toml` persistence as Options → Themes (`theme-selection` capability, unchanged).
- Cursor navigation spans the user entries plus the built-in entry; the separator row is never highlighted. Clamping (no wrap) is unchanged.
- Empty user section: the `(no entries — see usermenu.toml)` placeholder remains, with separator + Themes below it — the menu is never functionally empty anymore.
- `usermenu.toml` schema, parsing, defaults, recovery, and the shell dispatch of user entries are untouched.

## Capabilities

### Modified

- `user-menu` — "Open the F2 user menu" and "Navigate and dismiss the user menu" gain the built-in section; new requirement "Built-in Themes entry opens the theme selector". (`theme-selection` is reused as-is — no delta.)

## Impact

- `crates/filecommand-core/src/dialogs.rs` — `UserMenuState` cursor domain includes the built-in slot
- `crates/filecommand-core/src/update.rs` — `handle_user_menu`: move/clamp over extended domain; Confirm on built-in slot opens the theme picker instead of a shell effect
- `crates/filecommand-tui/src/views/user_menu.rs` — separator + built-in row rendering, height/width math
- Tests: `crates/filecommand-core/src/update/tests.rs`, `crates/filecommand-tui/tests/snapshot_views.rs` (existing user-menu snapshots churn)
