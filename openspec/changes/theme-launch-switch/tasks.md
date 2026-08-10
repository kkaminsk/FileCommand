# Tasks: theme-launch-switch

## 1. Argument parsing

- [x] 1.1 Extract a pure `parse_launch_args` helper in `filecommand-tui` returning launch options (`no_splash`, `theme: Option<String>`), accepting `--theme <name>` and `--theme=<name>`, and thread it through `main.rs` → `app::run` (theme-selection: "Launch-time theme override via --theme")
- [x] 1.2 Unit tests for the parser: both spellings, missing value, absent switch, `--nosplash` unaffected, combined switches in either order (theme-selection: "Launch-time theme override via --theme")

## 2. Startup resolution

- [x] 2.1 Extend `resolve_startup_theme` with the override: valid name wins over `config.theme`; invalid or missing value falls through to the existing config-then-default chain and yields a warning message naming the rejected value and listing `BUILTIN_THEME_NAMES` (theme-selection: "Launch-time theme override via --theme")
- [x] 2.2 Raise the returned warning through `state.startup_warning` at startup, reusing the existing dismiss flow; confirm no `config.toml` write occurs at launch with or without the switch (theme-selection: "Launch-time theme override via --theme")

## 3. Documentation & tests

- [x] 3.1 Document `--theme` (and its session-only semantics) in the F1 Help Configuration topic text in `filecommand-core/src/dialogs.rs` (theme-selection: "Launch-time theme override via --theme")
- [x] 3.2 Unit tests: override wins over config; invalid override falls back with warning; valid override produces no warning; picker apply during an overridden session persists normally (theme-selection: "Launch-time theme override via --theme")
- [x] 3.3 Full `cargo build --workspace` and `cargo test --workspace` pass
