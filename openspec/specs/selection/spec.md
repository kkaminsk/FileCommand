# selection Specification

## Purpose
TBD - created by archiving change m2-file-operations. Update Purpose after archive.
## Requirements
### Requirement: Toggle current entry with Ins

The system SHALL toggle the selection state of the entry under the cursor when the user presses `Ins`, then advance the cursor to the next entry. The parent-directory pseudo-entry (`▶UP--DIR◀`, `..`) SHALL never be selectable, and pressing `Ins` on it SHALL advance the cursor without changing the selection.

#### Scenario: Select an unselected entry and advance

- **WHEN** the cursor is on an unselected file and the user presses `Ins`
- **THEN** that file becomes selected
- **AND** the cursor moves to the next entry in the listing

#### Scenario: Deselect a selected entry and advance

- **WHEN** the cursor is on a selected entry and the user presses `Ins`
- **THEN** that entry becomes unselected
- **AND** the cursor moves to the next entry in the listing

#### Scenario: Ins on the last entry does not wrap

- **WHEN** the cursor is on the last entry and the user presses `Ins`
- **THEN** that entry's selection state toggles
- **AND** the cursor remains on the last entry

#### Scenario: Parent directory cannot be selected

- **WHEN** the cursor is on the `▶UP--DIR◀` (`..`) entry and the user presses `Ins`
- **THEN** no entry becomes selected
- **AND** the cursor advances to the next entry

### Requirement: Select a group by wildcard with `+`

The system SHALL open a wildcard input dialog when the user presses `+`, and on confirmation SHALL add to the selection set every entry in the current panel whose original `OsString` name matches the entered wildcard pattern. Matching SHALL run against the original stored name (not the lossy display name), and the parent-directory pseudo-entry SHALL be excluded.

#### Scenario: Wildcard adds matching entries

- **WHEN** the user presses `+`, enters `*.txt`, and confirms
- **THEN** every entry whose name matches `*.txt` is added to the selection set
- **AND** entries not matching the pattern retain their prior selection state

#### Scenario: Group select is additive

- **WHEN** some entries are already selected and the user selects a group with `+`
- **THEN** the previously selected entries remain selected
- **AND** the newly matched entries are added to the selection

#### Scenario: Cancel leaves selection unchanged

- **WHEN** the user presses `+` and dismisses the dialog with `Esc`
- **THEN** the selection set is unchanged

### Requirement: Deselect a group by wildcard with `-`

The system SHALL open a wildcard input dialog when the user presses `-`, and on confirmation SHALL remove from the selection set every entry in the current panel whose original `OsString` name matches the entered wildcard pattern. Matching SHALL run against the original stored name.

#### Scenario: Wildcard removes matching entries

- **WHEN** all entries are selected and the user presses `-`, enters `*.bak`, and confirms
- **THEN** every entry matching `*.bak` is removed from the selection set
- **AND** all non-matching entries remain selected

#### Scenario: Deselecting an unmatched pattern is a no-op

- **WHEN** the user deselects a group whose pattern matches no entries
- **THEN** the selection set is unchanged

### Requirement: Invert selection with `*`

The system SHALL invert the panel's selection set when the user presses `*`: every currently selected selectable entry becomes unselected and every currently unselected selectable entry becomes selected. The parent-directory pseudo-entry SHALL be excluded from inversion.

#### Scenario: Invert a partial selection

- **WHEN** exactly the `*.txt` entries are selected and the user presses `*`
- **THEN** the `*.txt` entries become unselected
- **AND** every other selectable entry becomes selected

#### Scenario: Invert twice restores the original selection

- **WHEN** the user presses `*` twice in succession
- **THEN** the selection set matches its state before the first press

#### Scenario: Parent directory stays unselected after invert

- **WHEN** the user presses `*`
- **THEN** the `▶UP--DIR◀` entry remains unselected

### Requirement: Selection mini-status summary

The system SHALL show, in the panel mini-status line whenever one or more entries are selected, the text `N files selected, X bytes`, where `N` is the count of selected entries and `X` is the sum of the byte sizes of the selected entries. Selected directories SHALL contribute 0 bytes to `X`. When no entries are selected, the mini-status SHALL revert to the current entry's name/size/date/time display.

#### Scenario: Summary counts files and sums bytes

- **WHEN** three files of 100, 200, and 300 bytes are selected
- **THEN** the mini-status reads `3 files selected, 600 bytes`

#### Scenario: Selected directories contribute zero bytes

- **WHEN** two files totaling 500 bytes and one directory are selected
- **THEN** the mini-status reports a total of `500 bytes`
- **AND** the selected-count includes the directory

#### Scenario: Empty selection restores the per-entry status

- **WHEN** the last selected entry is deselected
- **THEN** the mini-status line shows the cursor entry's name, size, date, and time

### Requirement: Selection persists across in-panel navigation and re-sort

The system SHALL key the selection set by entry identity (the original `OsString`/entry identity), not by row index, so that selection is preserved when the cursor moves, when the panel is re-sorted, and when scrolling within the same directory listing. Changing the panel's directory SHALL clear the selection for the newly listed directory.

#### Scenario: Selection survives cursor movement

- **WHEN** entries are selected and the user moves the cursor up and down within the panel
- **THEN** the same entries remain selected

#### Scenario: Selection survives a re-sort

- **WHEN** entries are selected and the panel sort mode changes (e.g. Name to Size)
- **THEN** the same entries remain selected under their new row positions

#### Scenario: Entering a new directory clears the selection

- **WHEN** entries are selected and the user navigates into a different directory in the panel
- **THEN** the new directory's listing shows no selected entries

