# Tasks: command-line-clear-key

## 1. Config (`crates/filecommand-core/src/config.rs`)

- [ ] 1.1 `Keys.clear_line: KeyBinding`, default `ctrl+y`; parse `key.clear_line` (command-line "Clear the command line with a dedicated key": "Rebinding the clear key"; design D1)
- [ ] 1.2 Parser tests for the override and for the default

## 2. Input (`crates/filecommand-tui/src/input/mod.rs`)

- [ ] 2.1 Panel arm: when `typing` and the key matches `keys.clear_line`, emit `Command::CommandLineClear`; empty line → no command (scenarios "Ctrl+Y clears a composed line", "Ctrl+Y on an empty line does nothing"; design D2, D3)
- [ ] 2.2 `app.rs` `apply_config`: wire `keys.clear_line` like the other overridable chords
- [ ] 2.3 `input/tests.rs`: both scenarios; `CommandLineClear` is never emitted while a dialog, menu, viewer, or editor owns input (their existing handling is unchanged)

## 3. Reducer test (`crates/filecommand-core/src/update/tests.rs`)

- [ ] 3.1 Recall a history entry with Up, send `CommandLineClear` → buffer empty, `history_cursor` is `None`, next Up moves the panel cursor (scenario "Clearing ends history recall")

## 4. Docs

- [ ] 4.1 README: "Typing" table row for Ctrl+Y, `key.clear_line` in the config key list and the overridable-bindings sentence; F1 Help "Command line" page

## 5. Verify

- [ ] 5.1 `cargo test --workspace` green; `openspec validate command-line-clear-key --strict` clean
- [ ] 5.2 Manual: type text, Ctrl+Y → empty; Up → panel cursor moves; rebind to `ctrl+k` in `config.toml` → Ctrl+K clears
