## MODIFIED Requirements

### Requirement: Key bar, menu bar, pull-down items, and dialog buttons are clickable

A left-click on a function-key-bar slot SHALL dispatch the same command as that function key; a click on a menu-bar title SHALL open that pull-down; a click on an enabled pull-down item SHALL activate it; a click outside an open pull-down SHALL close it; a click on a dialog button SHALL activate that button.

#### Scenario: Key bar Copy

- **WHEN** the user clicks the `5Copy` slot
- **THEN** the destination-input dialog opens exactly as for F5

#### Scenario: Key bar Refresh

- **WHEN** the user clicks the `12Refresh` slot (or `12Rfr` / `12` in a narrower form)
- **THEN** the active panel re-reads exactly as for F12 and Ctrl+R

#### Scenario: Menu item activation

- **WHEN** the user clicks `Files` in the menu bar and then clicks `Delete`
- **THEN** the delete-confirmation flow starts exactly as for F8

#### Scenario: Dialog button

- **WHEN** the overwrite-conflict dialog is open and the user clicks `Skip All`
- **THEN** the choice is applied exactly as if selected by keyboard
