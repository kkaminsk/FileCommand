# Change: theme-launch-switch

## Why

The only ways to choose a theme are the in-app pickers and hand-editing `config.toml` — there is no way to *launch* FileCommand in a chosen theme. A `--theme <name>` switch lets shortcuts, scripts, and demos start straight into a specific look (a green-phosphor console here, an amber one there) without editing configuration or re-picking on every start, and it mirrors the existing `--nosplash` launch-switch precedent.

## What Changes

- New command-line switch `--theme <name>` (also accepted as `--theme=<name>`): the session starts with the named built-in theme active, taking precedence over the `theme` key in `config.toml`.
- The override is **session-only**: launching with `--theme` never writes `config.toml`. Applying a theme from a picker during such a session persists exactly as today, so the next unswitched launch uses whatever was last applied.
- An unknown theme name — or `--theme` with no value — starts the application normally with the configured theme and raises the existing dismissable startup-warning dialog, naming the rejected value and listing the valid theme names. It does not prevent launch.
- The switch is documented in the F1 Help Configuration topic alongside the existing launch behavior notes.

## Capabilities

### New Capabilities

*(none)*

### Modified Capabilities

- `theme-selection`: one ADDED requirement — launch-time theme override via `--theme`, with config precedence, session-only semantics, and warn-and-fall-back handling of invalid values.

## Impact

- **Crates:** `filecommand-tui` — `main.rs` (parse the value-taking switch; extract parsing into a testable helper since `any()` no longer suffices), `app.rs` (`run` takes the parsed launch options; `resolve_startup_theme` gains the override and produces the warning). `filecommand-core` — Help Configuration topic text in `dialogs.rs`; no reducer or state changes (`startup_warning` already exists).
- **Depends on:** `visual-themes` (the six built-in themes), user-menu spec's startup-warning dialog.
- **Out of scope:** other launch switches, a `--help`/usage printer, environment-variable overrides, persisting the override, and abbreviated/fuzzy theme-name matching.
