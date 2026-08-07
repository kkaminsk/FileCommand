## ADDED Requirements

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
- **THEN** the letter is consumed by type-ahead and is not appended to the command line

### Requirement: Extending the pattern with printable keys

While type-ahead mode is active, the system SHALL append plain printable keys to the search pattern and re-move the panel cursor to the first entry matching the extended pattern, rather than routing those keys to the command line.

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
- **AND** none of them are appended to the command line

### Requirement: Mini-status display of the active pattern

While type-ahead mode is active, the system SHALL display the current search pattern in the panel's mini-status line using the `panel.ministatus` role.

#### Scenario: Pattern is shown as it is built

- **WHEN** type-ahead is active with pattern `re`
- **THEN** the panel's mini-status line shows the pattern `re` rendered in the `panel.ministatus` role

#### Scenario: Mini-status reverts on exit

- **WHEN** type-ahead is active and then exits
- **THEN** the panel's mini-status line reverts to its normal display (current entry's name/size/date/time or selection summary)

### Requirement: Shortening the pattern with Backspace

While type-ahead mode is active, the system SHALL remove the last character of the search pattern when the user presses Backspace and re-move the panel cursor to the first entry matching the shortened pattern.

#### Scenario: Backspace removes the last character

- **WHEN** type-ahead is active with pattern `rea` and the user presses Backspace
- **THEN** the pattern becomes `re`
- **AND** the panel cursor moves to the first entry matching `re`

#### Scenario: Backspace on a single-character pattern

- **WHEN** type-ahead is active with a single-character pattern and the user presses Backspace
- **THEN** the pattern becomes empty and type-ahead remains active
- **AND** the panel cursor holds its current position

### Requirement: Exiting type-ahead and restoring command-line routing

The system SHALL exit type-ahead mode when the user presses Esc or any panel movement key, after which plain printable keys typed over the focused panel SHALL again be routed to the command line.

#### Scenario: Esc exits type-ahead

- **WHEN** type-ahead is active and the user presses Esc
- **THEN** type-ahead mode ends and the mini-status reverts to its normal display

#### Scenario: A movement key exits type-ahead and is applied to the panel

- **WHEN** type-ahead is active and the user presses a movement key (e.g. Down)
- **THEN** type-ahead mode ends
- **AND** the movement key is applied to the panel cursor as a normal movement

#### Scenario: Printable keys return to the command line after exit

- **WHEN** type-ahead has exited via Esc or a movement key and the user then presses a plain printable key while the panel is focused
- **THEN** the key is appended to the command line and does not start or extend a search
