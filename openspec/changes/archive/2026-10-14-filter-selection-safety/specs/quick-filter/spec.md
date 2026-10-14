## MODIFIED Requirements

### Requirement: Cursor and mini-status behavior under an active filter

The system SHALL keep the panel cursor positioned on a currently visible (matching) entry while a quick filter is active. When the entry under the cursor is filtered out by a pattern change, the system SHALL move the cursor to the nearest remaining visible entry.

When the pattern leaves no entry visible, the panel SHALL have no current entry. This can happen only where the listing has no `..` parent entry, such as a drive root. While there is no current entry, every command that acts on the entry under the cursor SHALL do nothing. That covers:

- Enter;
- F3 and F4;
- F5, F6, F8, and the Delete key when no entries are selected;
- Ins;
- opening the file-action menu;
- pasting the cursor entry's name or path to the command line.

No command that targets the entry under the cursor SHALL act on an entry the filter hides. Copy, Move, and Delete of selected entries are governed by selection "Operations disclose selected entries hidden by the quick filter" instead. When the pattern changes so that at least one entry is visible again, the cursor SHALL move to the nearest visible entry.

#### Scenario: Cursor moves to a visible entry when its target is filtered out

- **WHEN** the cursor is on an entry and the user extends the pattern so that entry no longer matches
- **THEN** the cursor moves to a still-visible matching entry rather than a hidden one

#### Scenario: Navigation is restricted to matching entries

- **WHEN** a quick filter is active and the user moves the cursor with the arrow keys
- **THEN** the cursor traverses only the visible matching entries (and `..`), skipping filtered-out entries

#### Scenario: No visible entry at a drive root means no current entry

- **WHEN** the active panel shows the root of drive `C:` (no `..` entry), the cursor is on `pagefile.sys`, and the user types a quick-filter pattern that matches no entry
- **THEN** the panel body is empty and the panel has no current entry

#### Scenario: Cursor-targeted operations do nothing while no entry is visible

- **WHEN** a quick filter at a drive root matches no entry, nothing is selected, and the user presses F8, F5, F6, F3, F4, or Enter
- **THEN** no dialog, viewer, editor, or menu opens, and no file is affected

#### Scenario: Ins does nothing while no entry is visible

- **WHEN** a quick filter at a drive root matches no entry and the user presses Ins
- **THEN** the selection set is unchanged

#### Scenario: Widening the pattern restores a current entry

- **WHEN** a quick filter matches no entry and the user presses Backspace until the pattern matches `notes.txt`
- **THEN** the cursor is on a visible entry and cursor-targeted commands act on that entry again
