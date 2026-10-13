## ADDED Requirements

### Requirement: Operations disclose selected entries hidden by the quick filter

When Copy (F5), Move (F6), or Delete (F8 or the Delete key) acts on the panel's selection, it SHALL act on every selected entry, including entries the active quick filter currently hides. This applies equally when the operation is started from a selection-scoped file-action menu or by dragging selected entries; the Enter-opened file-action menu stays single-target as file-action-menu specifies. When one or more of the selected entries the operation acts on are hidden by the quick filter, two dialogs SHALL state how many of them are hidden by the filter: the delete confirmation dialog, and the copy/move destination dialog whether keyboard- or drop-initiated. The statement SHALL appear in addition to the dialog's existing content. When no selected entry the operation acts on is hidden, both dialogs SHALL be unchanged.

#### Scenario: Delete confirmation states the hidden count

- **WHEN** five entries are selected, a quick filter hides two of them, and the user presses F8
- **THEN** the confirmation dialog shows the count of 5 items, states that the deletion is permanent, and states that 2 of the 5 selected entries are hidden by the filter
- **AND** confirming deletes all five entries

#### Scenario: Destination dialog states the hidden count

- **WHEN** five entries are selected, a quick filter hides two of them, and the user presses F5
- **THEN** the destination input dialog opens pre-filled with the opposite panel's path and states that 2 of the 5 selected entries are hidden by the filter
- **AND** accepting the destination copies all five entries

#### Scenario: A single hidden selected entry is disclosed

- **WHEN** exactly one entry, `old.log`, is selected, the quick filter hides it, and the user presses F8
- **THEN** the confirmation dialog names `old.log` and states that the selected item is hidden by the filter

#### Scenario: No hidden selection leaves the dialogs unchanged

- **WHEN** entries are selected and none of them is hidden by a quick filter, and the user presses F5 or F8
- **THEN** the destination or confirmation dialog appears exactly as it does without a quick filter

### Requirement: Wildcard pattern matching for group select and deselect

The wildcard patterns entered for `+` (select a group) and `-` (deselect a group) SHALL be matched against entry names by one shared matcher. In that matcher `*` matches any run of characters (including none), `?` matches exactly one character, and every other pattern character matches itself case-insensitively. A character SHALL be one Unicode scalar value of the name, regardless of how many bytes it occupies in any encoding. Matching SHALL take time at most proportional to the product of the pattern length and the name length, so no pattern can make the panel unresponsive.

#### Scenario: Question mark matches one multi-byte character

- **WHEN** the panel lists `é.txt` and `ab.txt`, and the user presses `+`, enters `?.txt`, and confirms
- **THEN** `é.txt` is selected and `ab.txt` is not

#### Scenario: Deselect uses the same matching

- **WHEN** `é.txt` is selected and the user presses `-`, enters `?.txt`, and confirms
- **THEN** `é.txt` is no longer selected

#### Scenario: Matching ignores case

- **WHEN** the panel lists `README.TXT` and the user selects the group `*.txt`
- **THEN** `README.TXT` is selected

#### Scenario: Pathological pattern completes promptly

- **WHEN** the panel lists 1,000 entries, each named 200 `a` characters followed by a distinct four-digit number, and the user selects the group `*a*a*a*a*a*a*a*a*a*a*b`
- **THEN** the dialog closes and the panel responds to the next key without a perceptible delay, and no entry is selected
