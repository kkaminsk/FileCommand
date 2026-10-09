# Tasks: config-profile-directory

## 1. Launch arguments (`crates/filecommand-tui/src/main.rs`)

- [ ] 1.1 `parse_launch_args`: accept `--config-dir <path>` and `--config-dir=<path>`; a missing value is a startup warning, not a crash (application-shell "Configuration profile directory": "Explicit --config-dir override")
- [ ] 1.2 `print_usage`: add the flag line and `Default profile directory: <path>` (scenario "Usage names the profile directory"; design D4)

## 2. Profile resolution (`crates/filecommand-tui/src/app.rs`)

- [ ] 2.1 Add `resolve_profile_dir(flag: Option<PathBuf>, exe_dir: Option<&Path>) -> (PathBuf, Option<String>)` implementing D1 (returns an optional warning for the last-resort cwd fallback)
- [ ] 2.2 `create_dir_all`; on failure set `persist_enabled = false` and the D2 warning (scenario "Unwritable profile falls back without losing the session")
- [ ] 2.3 Replace the bare `CONFIG_FILE`/`HISTORY_FILE`/`USERMENU_FILE` paths with `profile.join(..)` for loading and for `Runtime.config_path`/`history_path`/the user-menu path (scenarios "Platform default is used when nothing overrides it", "History and frecency are shared across launch directories")
- [ ] 2.4 Skip `PersistTheme`/`PersistPanelSplit`/`PersistHistory` effects when `persist_enabled` is false

## 3. Legacy notice (`crates/filecommand-core/src/config.rs`)

- [ ] 3.1 Add pure `legacy_files_present(cwd: &Path, profile: &Path) -> Vec<&'static str>` (empty when the profile already has `config.toml`) (design D3)
- [ ] 3.2 `app.rs` turns a non-empty result into the D3 `startup_warning` (scenario "Legacy files in the working directory produce a notice, not a migration")

## 4. Tests

- [ ] 4.1 `main.rs`: `--config-dir` parsing (both forms; missing value)
- [ ] 4.2 `app.rs`: `resolve_profile_dir` — flag wins; portable marker wins over the platform default; platform default from `APPDATA` (cfg(windows)) / XDG (cfg(not(windows)))
- [ ] 4.3 `config.rs`: `legacy_files_present` with temp dirs — each file individually, none, and profile-already-populated
- [ ] 4.4 Existing config/history/user-menu loader tests pass unchanged (they already take explicit paths)

## 5. Docs

- [ ] 5.1 README: rewrite the "Configuration" intro for the profile directory, add `--config-dir` to the flag table, document portable mode and the legacy notice; remove the `themes/*.toml` subsection (no loader exists) and the `themes/` mention in the `theme` key row
- [ ] 5.2 `installer/README.md`: one line stating where per-user settings live

## 6. Verify

- [ ] 6.1 `cargo test --workspace` green; `openspec validate config-profile-directory --strict` clean
- [ ] 6.2 Manual: launch from two different directories, change the theme in one, relaunch from the other → theme persists; launch from a directory holding an old `config.toml` → notice shown; `filecommand --help` prints the default path
