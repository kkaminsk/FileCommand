## MODIFIED Requirements

### Requirement: Find-file invocation

The system SHALL open a find-file dialog when the user presses Alt+F7 or selects Commands → Find file, presenting an input field for the name pattern to search for. The field SHALL be a shared text field per `text-field-editing`. The dialog SHALL render in the primary dialog style (black text on cyan, black double-line frame) with a bracket-and-dots input field per §4.4, using only §4.11 ANSI-16 roles and CP437-heritage single-cell glyphs.

#### Scenario: Alt+F7 opens the dialog

- **WHEN** a panel is focused and the user presses Alt+F7
- **THEN** the find-file dialog opens centered over the panels with an empty, focused name-pattern input field

#### Scenario: Menu entry opens the dialog

- **WHEN** the user selects Commands → Find file from the F9 pull-down menu
- **THEN** the same find-file dialog opens, identically to the Alt+F7 path

#### Scenario: Pattern is editable mid-string

- **WHEN** the pattern field reads `reprt` and the user presses Left, Left, then types `o`
- **THEN** the field reads `report`

#### Scenario: Dialog uses the primary style and permitted glyphs

- **WHEN** the find-file dialog is rendered
- **THEN** it uses the `dialog.primary` and `dialog.input` roles with no icons, emoji, or non-CP437 glyphs
