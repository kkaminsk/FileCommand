## RENAMED Requirements

- FROM: `### Requirement: Exiting type-ahead and restoring command-line routing`
- TO: `### Requirement: Exiting type-ahead`

## MODIFIED Requirements

### Requirement: Entering type-ahead mode with Alt+letter

The system SHALL enter panel type-ahead quick-search mode when the user presses Alt+letter while a panel is focused, seeding the search pattern with that letter and moving the panel cursor to the first entry that matches the pattern.

#### Scenario: Alt+letter starts a search and jumps to the first match

- **WHEN** a panel is focused, no type-ahead is active, and the user presses `Alt+r`
- **THEN** type-ahead mode becomes active with pattern `r`
- **AND** the panel cursor moves to the first entry whose name matches the pattern `r`

#### Scenario: Alt+letter with no matching entry

- **WHEN** a panel is focused with no entry matching the pressed letter and the user presses `Alt+z`
- **THEN** type-ahead mode becomes active with pattern `z`
- **AND** the panel cursor does not move from its current position

#### Scenario: Alt+letter is not routed to the command line

- **WHEN** a panel is focused and the user presses `Alt+letter` to start type-ahead
- **THEN** the letter is consumed by type-ahead and is not inserted anywhere else

### Requirement: Extending the pattern with printable keys

While type-ahead mode is active, the system SHALL append plain printable keys to the search pattern and re-move the panel cursor to the first entry matching the extended pattern; plain printable keys have no other meaning over the panels.

#### Scenario: A plain printable key extends the pattern

- **WHEN** type-ahead is active with pattern `r` and the user presses `e`
- **THEN** the pattern becomes `re`
- **AND** the panel cursor moves to the first entry matching `re`

#### Scenario: Extended pattern no longer matches

- **WHEN** type-ahead is active with pattern `re` matching an entry and the user presses a printable key that makes the pattern match no entry
- **THEN** the pattern includes the new key
- **AND** the panel cursor holds its current position

#### Scenario: Printable keys do not reach the command line while active

- **WHEN** type-ahead is active and the user types plain printable keys
- **THEN** those keys extend the search pattern only
- **AND** none of them are inserted anywhere else

### Requirement: Exiting type-ahead

The system SHALL exit type-ahead mode when the user presses any panel movement key, after which plain printable keys typed over the focused panel SHALL be ignored. Esc SHALL NOT exit type-ahead; over the panels it requests application quit, and cancelling that dialog leaves type-ahead active (application-shell "Quit request keys and confirmation").

#### Scenario: A movement key exits type-ahead and is applied to the panel

- **WHEN** type-ahead is active and the user presses a movement key (e.g. Down)
- **THEN** type-ahead mode ends
- **AND** the movement key is applied to the panel cursor as a normal movement

#### Scenario: Esc leaves type-ahead active

- **WHEN** type-ahead is active and the user presses Esc, then cancels the quit-confirmation dialog
- **THEN** type-ahead mode is still active with its pattern intact

#### Scenario: Printable keys return to the command line after exit

- **WHEN** type-ahead has exited via a movement key and the user then presses a plain printable key while the panel is focused
- **THEN** the key is ignored and does not start or extend a search
