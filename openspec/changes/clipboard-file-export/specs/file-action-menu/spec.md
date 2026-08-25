# file-action-menu Delta

## MODIFIED Requirements

### Requirement: Menu contents, ordering, and navigation

The file-action menu SHALL list the entries View, Edit, Copy, Rename, Move, Delete, Send to clipboard in that order, and SHALL additionally list Run as the first entry when the target is executable (PATHEXT match or `.lnk`). The menu SHALL render as a primary-style modal dialog (§4.4) with the first entry highlighted on open. Up/Down SHALL move the highlight, Enter SHALL activate the highlighted entry and close the menu, Esc SHALL close the menu with no action taken, and pressing an entry's first letter SHALL activate that entry directly (R resolves to Run when Run is listed, otherwise to Rename). Rendering SHALL use only ANSI-16 named color roles and CP437 glyphs.

#### Scenario: Non-executable menu contents

- **WHEN** the menu opens for `notes.txt`
- **THEN** it lists View, Edit, Copy, Rename, Move, Delete, Send to clipboard in that order with View highlighted

#### Scenario: Executable gets Run first

- **WHEN** the menu opens for `setup.exe`
- **THEN** it lists Run, View, Edit, Copy, Rename, Move, Delete, Send to clipboard with Run highlighted
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

### Requirement: Menu actions route to existing flows

Each file-action menu entry SHALL route into the existing capability flow for that action, applied to the menu's target entry: View SHALL open the F3 viewer; Edit SHALL follow the F4 edit path (external editor when configured, otherwise the built-in editor rules); Copy SHALL open the F5 destination-input dialog pre-filled with the opposite panel's path, scoped to the single target entry; Move SHALL open the F6 destination-input dialog pre-filled with the opposite panel's path, scoped to the single target entry; Delete SHALL open the F8 delete-confirmation flow for the single target entry; Send to clipboard SHALL run the `clipboard-export` Files action scoped to the single target entry; Run (executables only) SHALL use the existing suspended-TUI spawn path. Downstream behavior — overwrite conflicts, progress, error recovery, skipped-files summary, and panel re-read on completion — SHALL follow the existing `file-operations` and `operation-dialogs` requirements unchanged.

#### Scenario: View opens the viewer

- **WHEN** the user activates View for `notes.txt`
- **THEN** the menu closes and the F3 viewer opens on `notes.txt`

#### Scenario: Copy opens the destination dialog for the single entry

- **WHEN** the left panel is active, the right panel shows `D:\backup`, and the user activates Copy for `report.txt`
- **THEN** the menu closes and the destination-input dialog opens pre-filled with `D:\backup`
- **AND** accepting it starts a copy job whose scope is exactly `report.txt`

#### Scenario: Delete requires the existing confirmation

- **WHEN** the user activates Delete for `notes.txt`
- **THEN** the menu closes and the delete-confirmation dialog names `notes.txt` and states that deletion is permanent
- **AND** declining the confirmation deletes nothing

#### Scenario: Send to clipboard copies the single entry

- **WHEN** the user activates Send to clipboard for `notes.txt`
- **THEN** the menu closes and the clipboard holds a file object for exactly `notes.txt`

#### Scenario: Run spawns via the suspended-TUI path

- **WHEN** the user activates Run for `build.bat`
- **THEN** the menu closes and `build.bat` runs via the suspended-TUI spawn path in the panel's current directory, exactly as a command-line invocation would

### Requirement: No mutation without an intervening dialog

No file-action menu entry SHALL mutate the filesystem directly upon activation: Copy, Move, Rename, and Delete SHALL each interpose their dialog (destination input, rename input, or delete confirmation) between activation and any filesystem change, and View, Edit, Run, and Send to clipboard SHALL never mutate the target through the menu itself. Dismissing the menu with Esc, or cancelling any interposed dialog, SHALL leave the filesystem untouched.

#### Scenario: Activating a mutating action changes nothing by itself

- **WHEN** the user activates Copy, Move, Rename, or Delete from the menu
- **THEN** no filesystem change occurs before the corresponding dialog has been accepted

#### Scenario: Cancelling the interposed dialog aborts fully

- **WHEN** the user activates Move and then presses Esc in the destination-input dialog
- **THEN** no job starts and the filesystem is unchanged

#### Scenario: Send to clipboard never mutates

- **WHEN** the user activates Send to clipboard for `notes.txt`
- **THEN** the filesystem is unchanged and only the clipboard contents differ
