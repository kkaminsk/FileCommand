# file-action-menu Delta

## ADDED Requirements

### Requirement: Directory targets and selection-scoped invocation

When the file-action menu is opened by a mouse right-click, the system SHALL allow a directory as the target, omitting View, Edit, and Run from the menu; and when the target entry is a member of the panel's selection set, Copy, Move, Delete, and Send to clipboard SHALL act on the whole selection set, with the resulting dialog naming the count. Enter-key invocation SHALL remain single-target and file-only as previously specified.

#### Scenario: Directory menu contents

- **WHEN** the menu opens by right-click on `src`
- **THEN** it lists Copy, Rename, Move, Delete, Send to clipboard

#### Scenario: Selection-scoped delete

- **WHEN** three entries are selected and the user right-clicks one of them and activates Delete
- **THEN** the delete-confirmation dialog names three entries

#### Scenario: Enter stays single-target

- **WHEN** three entries are selected and the user presses Enter on one of them and activates Copy
- **THEN** the destination-input dialog is scoped to that single entry
