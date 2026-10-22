## MODIFIED Requirements

### Requirement: Delete confirmation dialog

The system SHALL require confirmation before a Delete (triggered by F8 or the Delete key), naming the single item when one item is targeted or showing the count when a multi-selection is targeted, warning that deletion is permanent (no recycle bin), and requiring a second confirmation before removing a non-empty directory. The Delete key SHALL be a fixed (non-rebindable) alias for F8 and SHALL trigger deletion only when pressed without modifier keys.

#### Scenario: Single item named

- **WHEN** the user presses F8 with the cursor on `notes.txt` and nothing selected
- **THEN** the confirmation dialog names `notes.txt` and states that the deletion is permanent

#### Scenario: The Delete key opens the confirmation

- **WHEN** the user presses the Delete key (unmodified) with the cursor on `notes.txt` and nothing selected
- **THEN** the same confirmation dialog opens naming `notes.txt` and stating the deletion is permanent, exactly as if F8 had been pressed
- **AND** nothing is deleted until the confirmation is accepted

#### Scenario: Multi-selection shown as a count

- **WHEN** the user presses F8 with 12 entries selected
- **THEN** the confirmation dialog shows the count of 12 items rather than naming each one, and states that the deletion is permanent

#### Scenario: Non-empty directory requires a second confirmation

- **WHEN** the user confirms deletion of a directory that contains files
- **THEN** a second confirmation dialog is shown before the directory is removed
- **AND** declining the second confirmation leaves the directory intact

#### Scenario: Declining the first confirmation deletes nothing

- **WHEN** the delete confirmation dialog is shown and the user selects No / presses Esc
- **THEN** no item is deleted
