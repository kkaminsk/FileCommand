## MODIFIED Requirements

### Requirement: Enter on a file opens the action menu

When the user presses Enter with the active panel's cursor on a file entry, the system SHALL open a modal file-action menu targeting that entry. The menu SHALL target the cursor entry only — it SHALL NOT consume or alter the multi-entry selection. Enter on a directory or the `..` entry SHALL continue to navigate and is not affected by this menu. Menu state SHALL live in `filecommand-core` with all mutations flowing through `core::update`.

#### Scenario: Enter on a non-executable file opens the menu

- **WHEN** the cursor is on `notes.txt` and the user presses Enter
- **THEN** the file-action menu opens as a modal dialog targeting `notes.txt`

#### Scenario: Selection is not consumed

- **WHEN** 5 entries are selected, the cursor is on `report.txt`, and the user presses Enter
- **THEN** the menu opens targeting only `report.txt`
- **AND** the 5-entry selection remains intact while the menu is open and after it closes

#### Scenario: Non-empty command buffer takes precedence

- **WHEN** a path box has focus, reads `C:\NORTON\dir`, the panel cursor is on a file and the user presses Enter
- **THEN** Enter is handled by the path box (path-box "Enter selects the named entry") and no file-action menu opens

#### Scenario: Enter on a directory still navigates

- **WHEN** the cursor is on a subdirectory and the user presses Enter
- **THEN** the panel navigates into that directory and no file-action menu opens

### Requirement: Menu actions route to existing flows

Each file-action menu entry SHALL route into the existing capability flow for that action, applied to the menu's target entry: Open SHALL launch the target through the platform file association per "Open with the associated application"; View SHALL open the F3 viewer; Edit SHALL follow the F4 edit path (external editor when configured, otherwise the built-in editor rules); Copy SHALL open the F5 destination-input dialog pre-filled with the opposite panel's path, scoped to the single target entry; Move SHALL open the F6 destination-input dialog pre-filled with the opposite panel's path, scoped to the single target entry; Delete SHALL open the F8 delete-confirmation flow for the single target entry; Send to clipboard SHALL run the `clipboard-export` Files action scoped to the single target entry; Run (executables only) SHALL use the existing suspended-TUI spawn path. Downstream behavior — overwrite conflicts, progress, error recovery, skipped-files summary, and panel re-read on completion — SHALL follow the existing `file-operations` and `operation-dialogs` requirements unchanged.

#### Scenario: Open launches the associated application

- **WHEN** the user activates Open for `report.pdf`
- **THEN** the menu closes, `report.pdf` is handed to the platform file association, and the panels remain interactive without any TUI suspension

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
- **THEN** the menu closes and `build.bat` runs via the suspended-TUI spawn path in the panel's current directory
