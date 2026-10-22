# quick-filter Specification

## Purpose
TBD - created by archiving change m5-editor-and-modern-extras. Update Purpose after archive.
## Requirements
### Requirement: Activating the quick filter

The system SHALL bind Ctrl+P (by default) to enter quick-filter mode on the active panel, in which an inline filter input is edited within that panel's mini-status line. The system SHALL render the quick-filter input in the mini-status area using the `panel.ministatus` role, replacing the normal mini-status content while the filter is active.

#### Scenario: Ctrl+P enters quick-filter mode

- **WHEN** a panel is focused with no quick filter active and the user presses Ctrl+P
- **THEN** the active panel enters quick-filter mode
- **AND** the mini-status line shows the quick-filter input (initially empty) rendered with the `panel.ministatus` role

#### Scenario: Quick filter only affects the active panel

- **WHEN** the user activates the quick filter on the active panel
- **THEN** only the active panel enters quick-filter mode and is narrowed
- **AND** the opposite panel's listing and mini-status are unaffected

### Requirement: Substring narrowing as the pattern is typed

While quick-filter mode is active, the system SHALL narrow the panel to only those entries whose displayed name contains the typed pattern as a substring, updating the visible list as each character is added or removed. The `..` parent entry SHALL remain visible regardless of the pattern so the user can always navigate upward.

#### Scenario: Typing narrows the panel to substring matches

- **WHEN** the quick filter is active and the user types the pattern `rep`
- **THEN** the panel body shows only entries whose displayed name contains `rep` as a substring (plus the `..` parent entry)
- **AND** entries not containing `rep` are hidden from the panel body

#### Scenario: Editing the pattern re-narrows live

- **WHEN** a filter pattern is active and the user presses Backspace to shorten it
- **THEN** the panel re-evaluates matches against the shortened pattern and reveals entries that now match
- **AND** the updated pattern is reflected in the mini-status input

#### Scenario: No matches yields an empty body

- **WHEN** the typed pattern matches no entries
- **THEN** the panel body shows no entries other than the `..` parent
- **AND** the panel remains in quick-filter mode so the user can edit or clear the pattern

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

### Requirement: Clearing the quick filter

The system SHALL exit quick-filter mode and clear the filter when the user presses the quick-filter activation key (Ctrl+P by default, honoring any remap) while a filter is active — the activation key toggles the filter — restoring the full (unfiltered) entry list and the normal mini-status display. Esc SHALL NOT exit the filter; over the panels it requests application quit, and cancelling that dialog leaves the filter active (application-shell "Quit request keys and confirmation"). The system SHALL preserve the current sort mode and selection set across activation and clearing of the filter; the quick filter narrows what is shown but does not alter the underlying listing.

#### Scenario: Activation key toggles the filter off

- **WHEN** a quick filter is active and the user presses the quick-filter activation key
- **THEN** quick-filter mode exits and the pattern is discarded
- **AND** the panel shows the full unfiltered entry list
- **AND** the mini-status line reverts to its normal current-entry / selection display

#### Scenario: Esc leaves the filter in place

- **WHEN** a quick filter is active and the user presses Esc, then cancels the quit-confirmation dialog
- **THEN** the filter pattern and the narrowed listing are unchanged

#### Scenario: Selection survives filtering

- **WHEN** entries are selected, the user applies then clears a quick filter
- **THEN** the previously selected entries remain selected after the filter is cleared

### Requirement: Overridable binding

Because Ctrl+P deviates from classic Norton Commander behavior, the system SHALL allow the quick-filter activation key to be remapped through the keybinding-override mechanism in `config.toml`, using the same override path as every other default binding rather than a special case.

#### Scenario: Default binding is Ctrl+P

- **WHEN** no keybinding override for the quick filter is present in `config.toml`
- **THEN** Ctrl+P activates the quick filter

#### Scenario: Config override changes the activation key

- **WHEN** `config.toml` remaps the quick-filter action to a different key
- **THEN** that configured key activates the quick filter
- **AND** Ctrl+P no longer activates it unless it is also mapped to the action

