# quick-filter Specification

## Purpose
TBD - created by archiving change m5-editor-and-modern-extras. Update Purpose after archive.
## Requirements
### Requirement: Activating the quick filter

The system SHALL bind Ctrl+P (by default) to enter quick-filter mode on the active panel, in which an inline filter input is edited within that panel's mini-status line. The system SHALL render the quick-filter input in the mini-status area using the `panel.ministatus` role, replacing the normal mini-status content while the filter is active.

#### Scenario: Ctrl+P enters quick-filter mode

- **WHEN** a panel is focused with no quick filter active and the user presses Ctrl+P
- **THEN** the active panel enters quick-filter mode
- **AND** the mini-status line shows the quick-filter input (initially empty) rendered with the `panel.ministatus` role

#### Scenario: Quick filter only affects the active panel

- **WHEN** the user activates the quick filter on the active panel
- **THEN** only the active panel enters quick-filter mode and is narrowed
- **AND** the opposite panel's listing and mini-status are unaffected

### Requirement: Substring narrowing as the pattern is typed

While quick-filter mode is active, the system SHALL narrow the panel to only those entries whose displayed name contains the typed pattern as a substring, updating the visible list as each character is added or removed. The `..` parent entry SHALL remain visible regardless of the pattern so the user can always navigate upward.

#### Scenario: Typing narrows the panel to substring matches

- **WHEN** the quick filter is active and the user types the pattern `rep`
- **THEN** the panel body shows only entries whose displayed name contains `rep` as a substring (plus the `..` parent entry)
- **AND** entries not containing `rep` are hidden from the panel body

#### Scenario: Editing the pattern re-narrows live

- **WHEN** a filter pattern is active and the user presses Backspace to shorten it
- **THEN** the panel re-evaluates matches against the shortened pattern and reveals entries that now match
- **AND** the updated pattern is reflected in the mini-status input

#### Scenario: No matches yields an empty body

- **WHEN** the typed pattern matches no entries
- **THEN** the panel body shows no entries other than the `..` parent
- **AND** the panel remains in quick-filter mode so the user can edit or clear the pattern

### Requirement: Cursor and mini-status behavior under an active filter

The system SHALL keep the panel cursor positioned on a currently visible (matching) entry while a quick filter is active. When the entry under the cursor is filtered out by a pattern change, the system SHALL move the cursor to the nearest remaining visible entry.

#### Scenario: Cursor moves to a visible entry when its target is filtered out

- **WHEN** the cursor is on an entry and the user extends the pattern so that entry no longer matches
- **THEN** the cursor moves to a still-visible matching entry rather than a hidden one

#### Scenario: Navigation is restricted to matching entries

- **WHEN** a quick filter is active and the user moves the cursor with the arrow keys
- **THEN** the cursor traverses only the visible matching entries (and `..`), skipping filtered-out entries

### Requirement: Clearing the quick filter

The system SHALL exit quick-filter mode and clear the filter when the user presses Esc, restoring the full (unfiltered) entry list and the normal mini-status display. The system SHALL preserve the current sort mode and selection set across activation and clearing of the filter; the quick filter narrows what is shown but does not alter the underlying listing.

#### Scenario: Esc clears the filter and restores the panel

- **WHEN** a quick filter is active and the user presses Esc
- **THEN** quick-filter mode exits and the pattern is discarded
- **AND** the panel shows the full unfiltered entry list
- **AND** the mini-status line reverts to its normal current-entry / selection display

#### Scenario: Selection survives filtering

- **WHEN** entries are selected, the user applies then clears a quick filter
- **THEN** the previously selected entries remain selected after the filter is cleared

### Requirement: Overridable binding

Because Ctrl+P deviates from classic Norton Commander behavior, the system SHALL allow the quick-filter activation key to be remapped through the keybinding-override mechanism in `config.toml`, using the same override path as every other default binding rather than a special case.

#### Scenario: Default binding is Ctrl+P

- **WHEN** no keybinding override for the quick filter is present in `config.toml`
- **THEN** Ctrl+P activates the quick filter

#### Scenario: Config override changes the activation key

- **WHEN** `config.toml` remaps the quick-filter action to a different key
- **THEN** that configured key activates the quick filter
- **AND** Ctrl+P no longer activates it unless it is also mapped to the action

