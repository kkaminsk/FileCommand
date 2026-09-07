## MODIFIED Requirements

### Requirement: Menu contents, ordering, and navigation

The file-action menu SHALL list the entries View, Edit, Send to clipboard, Copy, Rename, Move, Delete in that order, and SHALL additionally list Run as the first entry when the target is executable (PATHEXT match or `.lnk`). The menu SHALL render as a primary-style modal dialog (§4.4) with the first entry highlighted on open. Up/Down SHALL move the highlight, Enter SHALL activate the highlighted entry and close the menu, Esc SHALL close the menu with no action taken, and pressing an entry's first letter SHALL activate that entry directly (R resolves to Run when Run is listed, otherwise to Rename).  Rendering SHALL use only ANSI-16 named color roles and CP437 glyphs.

#### Scenario: Non-executable menu contents

- **WHEN** the menu opens for `notes.txt`
- **THEN** it lists View, Edit, Send to clipboard, Copy, Rename, Move, Delete in that order with View highlighted

#### Scenario: Executable gets Run first

- **WHEN** the menu opens for `setup.exe`
- **THEN** it lists Run, View, Edit, Send to clipboard, Copy, Rename, Move, Delete with Run highlighted
- **AND** pressing Enter immediately activates Run

#### Scenario: Esc closes with no action

- **WHEN** the menu is open and the user presses Esc
- **THEN** the menu closes, no action runs, and the panel cursor and selection are unchanged

#### Scenario: First-letter hotkey activates directly

- **WHEN** the menu is open for `notes.txt` and the user presses `D`
- **THEN** the Delete action is activated exactly as if it had been highlighted and Enter pressed

#### Scenario: S activates Send to clipboard

- **WHEN** the menu is open for `notes.txt` and the user presses `S`
- **THEN** the Send to clipboard action is activated exactly as if it had been highlighted and Enter pressed

### Requirement: Directory targets and selection-scoped invocation

When the file-action menu is opened by a mouse right-click, the system SHALL allow a directory as the target, omitting View, Edit, and Run from the menu; and when the target entry is a member of the panel's selection set, Copy, Move, Delete, and Send to clipboard SHALL act on the whole selection set, with the resulting dialog naming the count. Enter-key invocation SHALL remain single-target and file-only as previously specified. The directory menu SHALL list its entries as Send to clipboard, Copy, Rename, Move, Delete in that order, matching the file-target menu's placement of Send to clipboard immediately after the (here-omitted) View/Edit entries.

#### Scenario: Directory menu contents

- **WHEN** the menu opens by right-click on `src`
- **THEN** it lists Send to clipboard, Copy, Rename, Move, Delete in that order

#### Scenario: Selection-scoped delete

- **WHEN** three entries are selected and the user right-clicks one of them and activates Delete
- **THEN** the delete-confirmation dialog names three entries

#### Scenario: Enter stays single-target

- **WHEN** three entries are selected and the user presses Enter on one of them and activates Copy
- **THEN** the destination-input dialog is scoped to that single entry
