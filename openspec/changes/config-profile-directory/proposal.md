# Change: config-profile-directory

## Why

FileCommand resolves `config.toml`, `history.json`, and `usermenu.toml` as bare relative paths (`app.rs` `CONFIG_FILE`, `config::HISTORY_FILE`, `config::USERMENU_FILE`), so they land in whatever directory the process was launched from. Every `cd somewhere; filecommand` creates three files there, and command history and fuzzy-jump frecency fragment per launch directory, so Ctrl+J never learns the user's habits across launches. The design doc (§3.1) and the `fuzzy-jump` spec ("persist that history in `history.json` in the platform config directory") both already call for `%APPDATA%\FileCommand`; the code never implemented it and the README documents the cwd behavior as if intended.

## What Changes

- A single **profile directory** is resolved once at startup and used for every configuration file: `config.toml`, `history.json`, `usermenu.toml`.
- Resolution order: `--config-dir <path>` launch flag → portable mode (a file named `portable` beside the executable → the executable's directory) → platform default (`%APPDATA%\FileCommand` on Windows; `$XDG_CONFIG_HOME/filecommand` or `~/.config/filecommand` elsewhere).
- The directory is created on first run. If it cannot be created or written, the session runs on in-memory defaults, persistence is skipped, and the existing dismissable startup-warning dialog names the path and error.
- **Legacy notice, no silent migration**: when the profile directory has no `config.toml` and the launch directory contains `config.toml`, `history.json`, or `usermenu.toml`, the startup warning tells the user where FileCommand now reads from and that the old files can be moved. Nothing is copied or deleted.
- `--help` usage gains `--config-dir <path>` and prints the resolved default profile directory.
- README: the Configuration section is rewritten for the profile directory; the documented `themes/*.toml` directory is dropped from the README because no code loads user theme files (theme-selection lists built-ins only).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `application-shell`: adds the requirement "Configuration profile directory" (resolution order, creation, write-failure fallback, legacy notice, usage output).

## Impact

- `crates/filecommand-tui/src/main.rs` — `parse_launch_args` gains `--config-dir`; `print_usage` gains the flag and the resolved default directory line.
- `crates/filecommand-tui/src/app.rs` — a `resolve_profile_dir` resolver (flag → portable marker → platform default) replaces the three bare file names; `Runtime.history_path`/`config_path` and the user-menu path become `profile_dir.join(..)`; `create_dir_all` with warning fallback; legacy-files check feeds `state.startup_warning`.
- `crates/filecommand-core/src/config.rs` — unchanged API (all loaders/savers already take a `&Path`); new pure helper `legacy_files_present(cwd, profile)` for testability.
- README Configuration section; `installer/README.md` note that per-user settings live under `%APPDATA%\FileCommand` (the installer itself ships no config files).
- Non-breaking for code; **behavioral change for users** who relied on per-directory config files (the legacy notice covers it).
