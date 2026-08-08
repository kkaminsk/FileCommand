# type-ahead-jump (delta)

## MODIFIED Requirements

### Requirement: Exiting type-ahead and restoring command-line routing

The system SHALL exit type-ahead mode when the user presses any panel movement key, after which plain printable keys typed over the focused panel SHALL again be routed to the command line. Esc SHALL NOT exit type-ahead; over the panels it requests application quit, and cancelling that dialog leaves type-ahead active (application-shell "Quit request keys and confirmation").

#### Scenario: A movement key exits type-ahead and is applied to the panel

- **WHEN** type-ahead is active and the user presses a movement key (e.g. Down)
- **THEN** type-ahead mode ends
- **AND** the movement key is applied to the panel cursor as a normal movement

#### Scenario: Esc leaves type-ahead active

- **WHEN** type-ahead is active and the user presses Esc, then cancels the quit-confirmation dialog
- **THEN** type-ahead mode is still active with its pattern intact

#### Scenario: Printable keys return to the command line after exit

- **WHEN** type-ahead has exited via a movement key and the user then presses a plain printable key while the panel is focused
- **THEN** the key is appended to the command line and does not start or extend a search
