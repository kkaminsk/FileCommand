## MODIFIED Requirements

### Requirement: Destination input dialog

The system SHALL present, when the user starts a Copy (F5) or Rename/Move (F6) operation, a primary-style input dialog whose editable destination field is pre-filled with the opposite panel's current path, so the common "copy to the other panel" case requires no typing. The field SHALL be a shared text field per `text-field-editing` (caret keys, word keys, paste, scrolling window), opening with the caret at the end of the pre-filled path.

#### Scenario: Destination pre-filled from the opposite panel

- **WHEN** the user presses F5 with the left panel active and the right panel showing `D:\backup`
- **THEN** the destination input dialog opens with its field pre-filled with `D:\backup`
- **AND** the caret is at the end of the field so the user can edit or accept it

#### Scenario: Editing the pre-filled destination in place

- **WHEN** the field reads `D:\backup` and the user presses Home, Delete, then types `E`
- **THEN** the field reads `E:\backup`

#### Scenario: Accepting the pre-filled destination starts the job

- **WHEN** the destination input dialog is open with a valid destination and the user activates the default button with Enter
- **THEN** the dialog closes and the operation begins against the entered destination

#### Scenario: Cancelling the destination dialog aborts the operation

- **WHEN** the destination input dialog is open and the user presses Esc
- **THEN** the dialog closes and no file operation is started

#### Scenario: Manual UNC destination is accepted

- **WHEN** the user clears the field and types a UNC destination such as `\\server\share\dest`
- **THEN** the entered UNC path is used as the operation destination
