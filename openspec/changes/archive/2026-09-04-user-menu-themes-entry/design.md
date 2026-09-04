# Design: user-menu-themes-entry

## Context

The F2 user menu and the theme picker are currently disjoint systems. `UserMenuEntry` carries only `label` and `command` (`crates/filecommand-core/src/config.rs:668`); the hand-rolled parser warns on unknown keys and flags non-`key = "value"` lines as malformed (`config.rs:703`). `UserMenuState { cursor }` is a flat cursor with no menu stack (`crates/filecommand-core/src/dialogs.rs:19`), and `UserMenuConfirm`'s only outcome is `Effect::RunShellCommand` in the active panel's directory (`crates/filecommand-core/src/update.rs`, `handle_user_menu`). The theme picker (shipped by `visual-themes`, merged to `main`) opens via `Command::ThemePickerOpen` → `ThemePickerState::open(&state.theme.name)`, pre-highlighting the active theme; Enter applies immediately and emits `Effect::PersistTheme`, Esc cancels — today reachable only from `MenuAction::OpenThemes` in the Options pull-down (`crates/filecommand-core/src/menu.rs:193`). Modal key routing in `crates/filecommand-tui/src/input/mod.rs` checks `user_menu` **before** `theme_picker`. The user-menu view has an empty-state placeholder `(no entries — see usermenu.toml)` and no scrolling (`crates/filecommand-tui/src/views/user_menu.rs`); the picker view is modeled closely on it (`views/theme_picker.rs`).

## Goals / Non-Goals

**Goals:**

- Themes reachable from F2 in two keystrokes (F2, Enter — when it's the only/highlighted entry path).
- Picker behavior identical from both entry points (Options → Themes and F2 → Themes).
- User-defined entries completely unaffected: same parsing, same shell dispatch, same file semantics.

**Non-Goals:**

- No `usermenu.toml` schema change; the file remains a pure list of `{label, command}` shell entries.
- No general submenu/nesting support in the user menu.
- No new picker features and no change to the Options → Themes route.
- No hotkey/first-letter activation (the user menu has none today).

## Decisions

### D1: Compiled-in entry, not a config-schema extension

The built-in entry is application code — like `FileActionMenuEntry` and `MenuAction` are — appended by the app, not read from `usermenu.toml` (user decision). This keeps the existing spec clause "the menu SHALL NOT reinterpret or transform the command" intact and reaches every user, including those with existing customized files. Alternatives rejected: an `action = "themes"` entry key (existing `usermenu.toml` files would never gain it, since the file is never overwritten; and the parser would need conflict rules for `command` + `action`); real nested submenus (a menu-stack state model for a single entry — overkill).

### D2: Placement below a separator; label `Themes`

The built-in entry renders below all user entries, after a non-selectable separator row. The label is `Themes`, matching the Options pull-down item (`menu.rs:193`). In the empty-file case the placeholder row stays above the separator — it still teaches the user about `usermenu.toml`.

### D3: Cursor-domain extension, not entry injection

The built-in entry is NOT appended to `state.user_menu_entries` — the config model and its round-trip stay pure. Instead the reducer treats cursor index `entries.len()` as the built-in slot; Up/Down clamp over `0..=entries.len()`; the view renders the extra row after the separator. Confirm on that index opens the picker.

### D4: Confirm closes the user menu before opening the picker

`UserMenuConfirm` on the built-in slot does `state.user_menu.take()` first, then opens the picker (same pattern as `handle_file_action_menu`). Required because modal routing checks `user_menu` before `theme_picker` — leaving the F2 menu open behind the picker would steal its keys.

### D5: Esc in the picker returns to the panels, not back to the F2 menu

One picker behavior for both entry points. Forking cancel semantics by origin ("go back to whoever opened me") would add origin-tracking state for no real benefit; the F2 menu is one keystroke away.

## Risks / Trade-offs

- [Existing user-menu snapshots and row-count assertions churn] → migrate mechanically; the menu gains exactly a separator plus one row, and the existing `area.height < box_h` guard still applies.
- [`visual-themes` is implemented and merged but not yet archived/synced — `openspec/specs/theme-selection/` doesn't exist] → this change deltas **only** `user-menu` and references the picker behaviorally, so it neither depends on nor conflicts with the future sync. (`openspec/specs/pulldown-menus/spec.md` also still carries a stale pre-visual-themes "Themes disabled" scenario; out of scope here, noted for that sync.)

## Open Questions

- None. Mechanism (compiled-in entry), placement/label, and cancel semantics were settled with the user before authoring.
