# Design: config-profile-directory

## Context

`app.rs:121` calls `config::load(Path::new("config.toml"))`; `app.rs:150-158` loads `history.json` and `usermenu.toml` the same way and stores the bare names in `Runtime` for atomic saves (`save_theme_atomic`, `save_panel_split_atomic`, `save_history_file_atomic`). `config::load_user_menu` creates `usermenu.toml` with defaults when absent (user-menu "Create and recover the usermenu.toml file"). Nothing reads a `themes/` directory. The design doc §3.1 names `%APPDATA%\FileCommand`.

## Goals / Non-Goals

**Goals:**

- One stable per-user location for all three files.
- A portable escape hatch and an explicit override for testing/scripting.
- No silent data movement.
- Discoverable: usage text and the warning dialog name the location.

**Non-Goals:**

- Automatic migration of old files.
- A config-dir entry in the Info panel or About dialog.
- Loading user theme files (separate proposal, if ever).
- Changing any file format.

## Decisions

### D1: Resolution order is flag → portable marker → platform default

`--config-dir <path>` (also `--config-dir=<path>`) wins outright and may be relative (resolved against the cwd at startup). Otherwise, if `<exe_dir>/portable` exists (any file, contents ignored), the profile is `<exe_dir>`. Otherwise `%APPDATA%\FileCommand` via `std::env::var_os("APPDATA")`; on non-Windows `$XDG_CONFIG_HOME/filecommand` else `$HOME/.config/filecommand`. No new crate: `std::env` suffices. If none of the environment variables resolve, fall back to the cwd with a startup warning (last resort, never silent).

Alternative rejected: an environment-variable override — the flag covers scripting and avoids a second mechanism.

### D2: Create on demand; fail soft

`create_dir_all(profile)` before the first load. On failure the loaders still run (returning defaults for missing files), `Runtime` records `persist_enabled = false` so the three save functions are skipped, and `startup_warning` says `Could not create <path>: <err> — settings will not be saved this session`.

### D3: Legacy notice is a warning, not a migration

Only when the profile has no `config.toml` **and** the cwd has at least one of the three files: `startup_warning = "Found <names> in <cwd>; FileCommand now reads <profile>. Move them there to keep your settings."`. The check is a pure function in `config.rs` over two `&Path`s so it is unit-tested with temp dirs. Precedence with other startup warnings follows the existing single-field behavior (last writer wins), unchanged.

### D4: Usage text prints the resolved default

`print_usage` adds `--config-dir <path>  Use <path> for config.toml, history.json, usermenu.toml` and a trailing line `Default profile directory: <resolved platform default>` so a user can find their files without reading docs.

## Risks / Trade-offs

- [Users with tuned `config.toml` files in project directories see the notice on every launch from such a directory] → The notice is explicit and names both paths; the alternative (silent cwd behavior) is the bug being fixed.
- [A roaming `%APPDATA%` profile means `history.json` churn roams] → It is a few KB; matches the design doc.

## Open Questions

- None.
