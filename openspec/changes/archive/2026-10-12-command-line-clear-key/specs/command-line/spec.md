## ADDED Requirements

### Requirement: Clear the command line with a dedicated key

The system SHALL clear the entire command-line buffer when the user presses the clear-line chord (default Ctrl+Y) while the panels own input and the buffer is non-empty; clearing SHALL also end any history recall in progress so that Up/Down return to moving the panel cursor. On an empty buffer the chord SHALL have no effect. The chord SHALL be overridable via `key.clear_line` in `config.toml` using the existing keybinding-override mechanism. Esc SHALL continue to request quit (application-shell "Quit request keys and confirmation"); the chord SHALL be bound only while the panels own input, and the existing key handling of menus, dialogs, the viewer, and the editor SHALL be unchanged.

#### Scenario: Ctrl+Y clears a composed line
- **WHEN** the buffer holds `cd Program Files` and the user presses Ctrl+Y
- **THEN** the buffer is empty, the panel cursor has not moved, and no dialog opens

#### Scenario: Ctrl+Y on an empty line does nothing
- **WHEN** the buffer is empty and the user presses Ctrl+Y
- **THEN** nothing changes: no quit dialog, no cursor movement

#### Scenario: Clearing ends history recall
- **WHEN** the user has recalled a history entry with Up and then presses Ctrl+Y, then Up again
- **THEN** the buffer is empty after Ctrl+Y and the second Up moves the panel cursor instead of recalling history

#### Scenario: Rebinding the clear key
- **WHEN** `config.toml` sets `key.clear_line = "ctrl+k"` and the buffer holds text
- **THEN** Ctrl+K clears the buffer and Ctrl+Y no longer does

## MODIFIED Requirements

### Requirement: Command history navigation

The system SHALL maintain a command history and, while the command-line buffer is non-empty, Up and Down SHALL navigate previous and next history entries into the buffer. While the buffer is empty, Up and Down SHALL instead move the panel cursor. Backspacing the buffer to empty, or pressing the clear-line chord (default Ctrl+Y — see "Clear the command line with a dedicated key"), SHALL be the mechanisms that hand Up/Down back to the panel; Esc SHALL NOT clear the buffer — over the panels it requests application quit (application-shell "Quit request keys and confirmation"). Command history SHALL persist to `history.json`, written atomically.

#### Scenario: Up recalls previous command while composing

- **WHEN** the command buffer contains text and the user presses Up
- **THEN** the buffer is replaced with the previous history entry and the panel cursor does not move

#### Scenario: Backspacing to empty releases Up/Down to panel

- **WHEN** the command buffer is non-empty and the user backspaces until it is empty, then presses Up
- **THEN** the subsequent Up moves the panel cursor and recalls no history entry

#### Scenario: Esc does not clear the buffer

- **WHEN** the command buffer is non-empty and the user presses Esc
- **THEN** the buffer is not cleared and the quit-confirmation dialog opens instead

#### Scenario: Executed command persisted to history

- **WHEN** the user runs a command with Enter
- **THEN** the command is appended to the history and `history.json` is written atomically
