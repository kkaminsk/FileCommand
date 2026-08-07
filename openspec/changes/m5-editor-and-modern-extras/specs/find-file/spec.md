## ADDED Requirements

### Requirement: Find-file invocation

The system SHALL open a find-file dialog when the user presses Alt+F7 or selects Commands → Find file, presenting an input field for the name pattern to search for. The dialog SHALL render in the primary dialog style (black text on cyan, black double-line frame) with a bracket-and-dots input field per §4.4, using only §4.11 ANSI-16 roles and CP437-heritage single-cell glyphs.

#### Scenario: Alt+F7 opens the dialog

- **WHEN** a panel is focused and the user presses Alt+F7
- **THEN** the find-file dialog opens centered over the panels with an empty, focused name-pattern input field

#### Scenario: Menu entry opens the dialog

- **WHEN** the user selects Commands → Find file from the F9 pull-down menu
- **THEN** the same find-file dialog opens, identically to the Alt+F7 path

#### Scenario: Dialog uses the primary style and permitted glyphs

- **WHEN** the find-file dialog is rendered
- **THEN** it uses the `dialog.primary` and `dialog.input` roles with no icons, emoji, or non-CP437 glyphs

### Requirement: Recursive subtree name search

The system SHALL, on submitting a pattern, walk the active panel's directory subtree via the `listing` module and collect entries whose names match the pattern. Matching SHALL operate on the original `OsString` names, and results SHALL be shown to the user via lossy display conversion with the visual marker for non-Unicode names, never using the display form for filesystem access. The walk SHALL go through the same narrow fs trait and `\\?\` long-path abstraction used elsewhere, so long paths and non-Unicode names are handled correctly.

#### Scenario: Matching entries in nested directories are found

- **WHEN** the user submits a pattern and matching files exist in subdirectories below the panel directory
- **THEN** those matches are collected and presented in the result list with their subtree-relative locations

#### Scenario: Non-Unicode names are matched and displayed safely

- **WHEN** a matching entry has a name containing unpaired surrogates or control characters
- **THEN** matching is performed against the original `OsString` and the entry is rendered with lossy conversion and the visual marker, while any later navigation uses the original `OsString`

#### Scenario: No matches

- **WHEN** the walk completes and no entry matches the pattern
- **THEN** the dialog reports an empty result set and offers no navigable results

### Requirement: Non-blocking search with static progress

The system SHALL perform the subtree walk without blocking the UI event loop, and SHALL convey progress using period-authentic static text updated in place rather than spinners or animation glyphs, consistent with §4.10. Results SHALL become selectable as the walk progresses or on completion, and the UI SHALL remain responsive throughout.

#### Scenario: UI stays responsive during a large walk

- **WHEN** a find-file search runs over a deep or large subtree
- **THEN** the UI event loop continues to process input and the search does not freeze the interface

#### Scenario: Progress is shown as static text

- **WHEN** the search is in progress
- **THEN** progress feedback is rendered as static text updated in place, with no spinner or animation glyph

### Requirement: Navigate to a chosen result

The system SHALL, when the user selects a result and confirms, navigate the active panel in place to the directory containing the matched entry and place the panel cursor on that entry. The dialog SHALL then close.

#### Scenario: Selecting a result jumps the cursor

- **WHEN** the user highlights a result and presses Enter
- **THEN** the active panel switches to the result's containing directory with the cursor positioned on the matched entry, and the dialog closes

#### Scenario: Navigation is in place, not a new tab

- **WHEN** the user confirms a result
- **THEN** the active panel's current tab navigates to the target directory rather than opening a new tab

### Requirement: Dismiss the find-file dialog

The system SHALL close the find-file dialog without changing the active panel's location when the user cancels, and SHALL discard any in-progress search on dismissal.

#### Scenario: Esc cancels

- **WHEN** the find-file dialog is open and the user presses Esc
- **THEN** the dialog closes, the active panel remains at its prior directory and cursor position, and any running search is abandoned
