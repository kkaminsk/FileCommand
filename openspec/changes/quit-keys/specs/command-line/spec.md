# command-line (delta)

## MODIFIED Requirements

### Requirement: Command history navigation

The system SHALL maintain a command history and, while the command-line buffer is non-empty, Up and Down SHALL navigate previous and next history entries into the buffer. While the buffer is empty, Up and Down SHALL instead move the panel cursor. Backspacing the buffer to empty SHALL be the mechanism that hands Up/Down back to the panel; Esc SHALL NOT clear the buffer — over the panels it requests application quit (application-shell "Quit request keys and confirmation"). Command history SHALL persist to `history.json`, written atomically.

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
