# Design: theme-launch-switch

## Context

`main.rs` hand-parses the argument list with a single `any(|a| a == "--nosplash")`; `app::run(no_splash_flag: bool)` combines it with `config.splash`. The startup theme comes from `resolve_startup_theme(&config)`, which resolves `config.theme` through `Theme::by_name` with a silent `nc-classic` fallback — silence is correct there because the config value may simply be stale, but a CLI value is explicit user intent typed at this launch, which warrants feedback when rejected. The codebase already has the right feedback surface: the dismissable startup-warning modal (`state.startup_warning`) used for a malformed `usermenu.toml`, which renders above both splash and panels.

## Goals / Non-Goals

**Goals:**

- Launch directly into any built-in theme without touching `config.toml`.
- Explicit, non-fatal feedback when the requested theme doesn't exist.
- Zero change to persistence semantics and to launches that don't pass the switch.

**Non-Goals:**

- No general CLI framework or `--help` output; no other new switches.
- No persistence of the override; no partial/prefix theme-name matching; no external theme loading.

## Decisions

### D1: `--theme <name>` with `--theme=<name>` accepted, exact names only

Both spellings are standard for value-taking switches and cost one branch in the parser. Names match `BUILTIN_THEME_NAMES` exactly (the same strings `config.toml` uses); prefix or fuzzy matching was rejected — six fixed names don't need it, and exactness keeps the warning message unambiguous.

### D2: Precedence CLI > config > default, resolved in `resolve_startup_theme`

The existing resolution function gains the override: a valid `--theme` value wins over `config.theme`; an invalid or missing value falls through to today's config-then-`nc-classic` chain unchanged. Keeping this in the one existing resolution point (rather than a second resolution site in `main.rs`) preserves the property that startup theming has a single testable authority. `main.rs` stays a thin parse-and-forward layer; parsing moves to a small pure helper (e.g. `parse_launch_args`) returning a launch-options struct, replacing the `bool` threading before it grows a third parameter.

### D3: Invalid value warns via the existing startup-warning modal and never blocks launch

Explicit intent deserves feedback, so silent fallback (the config behavior) was rejected for the CLI path; refusing to launch was rejected as hostile to a typo when the application has an in-app recovery path (the pickers) one keystroke away. The warning names the rejected value and lists the valid names, reusing `state.startup_warning` — the same dismiss flow, rendering, and over-splash behavior as the `usermenu.toml` warning, no new UI. A bare `--theme` with no following value takes the same path with a "missing value" message.

### D4: Session-only by construction

The override changes only which `Theme` the initial `State` is built with; nothing writes `config.toml` at launch (only `Effect::PersistTheme` from an in-session apply does, unchanged). A subsequent picker apply persists normally — the override does not shadow or suppress persistence, so "launch in X, apply Y, relaunch plain → Y" holds with no additional code.

## Risks / Trade-offs

- [A second value-taking switch someday outgrows hand parsing] → the extracted `parse_launch_args` helper centralizes it; adopting a parsing crate remains a one-file change if ever justified.
- [Warning text drifts from the actual theme list] → the message is built from `BUILTIN_THEME_NAMES` at runtime, never hardcoded; a unit test pins the composition.

## Open Questions

- None. Switch shape, precedence, and failure behavior follow directly from the request plus existing precedents (`--nosplash`, config fallback, startup warning).
