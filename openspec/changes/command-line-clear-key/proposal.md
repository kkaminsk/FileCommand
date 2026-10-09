# Change: command-line-clear-key

## Why

Since `quit-keys`, Esc over the panels requests quit instead of clearing the command line, and the spec deliberately says so ("Esc SHALL NOT clear the buffer"). The only way to discard a typed line is to Backspace through it. `Command::CommandLineClear` already exists in `core::update` (it clears the text and resets the history cursor) but no key emits it. A single chord fixes this without reopening the Esc decision.

## What Changes

- **Ctrl+Y** clears the command line while the panels own input; it also ends any history recall in progress. On an empty line it does nothing. Rebindable via `key.clear_line` in `config.toml`, following the existing `key.*` override mechanism.
- Esc behavior is unchanged (still requests quit). The chord is bound only over the panels; the keymaps of the built-in editor, viewer, menus, and dialogs are not touched (today their unguarded `y` arms treat Ctrl+Y as a plain `y`, which stays as is).
- README and F1 Help list the key.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `command-line`: "Command history navigation" names the clear key as the second way (besides Backspace) to hand Up/Down back to the panel; a new requirement "Clear the command line with a dedicated key" defines the binding, its no-op on an empty line, and its rebinding.

## Impact

- `crates/filecommand-core/src/config.rs` — `Keys.clear_line` (default Ctrl+Y), parsed from `key.clear_line`.
- `crates/filecommand-tui/src/input/mod.rs` — panel arm emitting `Command::CommandLineClear` when the chord matches and the buffer is non-empty.
- `crates/filecommand-tui/src/app.rs` `apply_config` wiring; README key tables; F1 Help "Command line" page.
- Tests: `input/tests.rs`, `config.rs` parsing, one `update/tests.rs` assertion that `CommandLineClear` also drops `history_cursor` (already true).
- Non-breaking; additive.
