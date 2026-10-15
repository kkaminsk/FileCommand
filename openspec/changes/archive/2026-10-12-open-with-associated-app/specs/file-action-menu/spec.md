## MODIFIED Requirements

### Requirement: Menu contents, ordering, and navigation

The file-action menu SHALL list the entries Open, View, Edit, Send to clipboard, Copy, Rename, Move, Delete in that order for a non-executable file, and SHALL list Run first (before Open) when the target is executable (PATHEXT match or `.lnk`). The menu SHALL render as a primary-style modal dialog (§4.4) with the first entry highlighted on open. Up/Down SHALL move the highlight, Enter SHALL activate the highlighted entry and close the menu, Esc SHALL close the menu with no action taken, and pressing an entry's first letter SHALL activate that entry directly (`O` resolves to Open; `R` resolves to Run when Run is listed, otherwise to Rename). Rendering SHALL use only ANSI-16 named color roles and CP437 glyphs.

#### Scenario: Non-executable menu contents

- **WHEN** the menu opens for `notes.txt`
- **THEN** it lists Open, View, Edit, Send to clipboard, Copy, Rename, Move, Delete in that order with Open highlighted

#### Scenario: Executable gets Run first

- **WHEN** the menu opens for `setup.exe`
- **THEN** it lists Run, Open, View, Edit, Send to clipboard, Copy, Rename, Move, Delete with Run highlighted
- **AND** pressing Enter immediately activates Run

#### Scenario: Esc closes with no action

- **WHEN** the menu is open and the user presses Esc
- **THEN** the menu closes, no action runs, and the panel cursor and selection are unchanged

#### Scenario: First-letter hotkey activates directly

- **WHEN** the menu is open for `notes.txt` and the user presses `D`
- **THEN** the Delete action is activated exactly as if it had been highlighted and Enter pressed

#### Scenario: O activates Open

- **WHEN** the menu is open for `report.pdf` and the user presses `O`
- **THEN** the Open action is activated exactly as if it had been highlighted and Enter pressed

#### Scenario: S activates Send to clipboard

- **WHEN** the menu is open for `notes.txt` and the user presses `S`
- **THEN** the Send to clipboard action is activated exactly as if it had been highlighted and Enter pressed

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
- **THEN** the menu closes and `build.bat` runs via the suspended-TUI spawn path in the panel's current directory, exactly as a command-line invocation would

### Requirement: Directory targets and selection-scoped invocation

When the file-action menu is opened by a mouse right-click, the system SHALL allow a directory as the target, omitting View, Edit, and Run from the menu; and when the target entry is a member of the panel's selection set, Copy, Move, Delete, and Send to clipboard SHALL act on the whole selection set, with the resulting dialog naming the count. Open SHALL always act on the single target, never the selection. Enter-key invocation SHALL remain single-target and file-only as previously specified. The directory menu SHALL list its entries as Open, Send to clipboard, Copy, Rename, Move, Delete in that order, with Open highlighted; Open on a directory SHALL show that directory in the platform file browser.

#### Scenario: Directory menu contents

- **WHEN** the menu opens by right-click on `src`
- **THEN** it lists Open, Send to clipboard, Copy, Rename, Move, Delete in that order with Open highlighted

#### Scenario: Open on a directory shows it in the file browser

- **WHEN** the user activates Open for the directory `src`
- **THEN** `src` is handed to the platform file association (Explorer on Windows) and the panel does not navigate

#### Scenario: Selection-scoped delete

- **WHEN** three entries are selected and the user right-clicks one of them and activates Delete
- **THEN** the delete-confirmation dialog names three entries

#### Scenario: Open ignores the selection

- **WHEN** three entries are selected and the user right-clicks one of them and activates Open
- **THEN** only that entry is handed to the platform file association

#### Scenario: Enter stays single-target

- **WHEN** three entries are selected and the user presses Enter on one of them and activates Copy
- **THEN** the destination-input dialog is scoped to that single entry

## ADDED Requirements

### Requirement: Open with the associated application

The system SHALL launch the Open target through the platform's file association — `ShellExecute` with the `open` verb on Windows, `xdg-open`/`open` elsewhere — with the active panel's directory as the working directory, without suspending the TUI, without waiting for the launched program, and without re-reading the panel. Paths SHALL be passed without the `\\?\` verbatim prefix. When the launch fails, the system SHALL report the reason on the active panel's status line: `No application is associated with <name>` when no association exists, `<name> not found` when the target is missing, otherwise `Could not open <name> (error <code>)`.

#### Scenario: Document opens while the panels stay live

- **WHEN** the user activates Open for `budget.xlsx`
- **THEN** the associated application starts, the terminal is not suspended, and the next key press is handled by the panels

#### Scenario: No association is reported inline

- **WHEN** the user activates Open for `data.zzz` and no application is associated with `.zzz`
- **THEN** the active panel's status line reads `No application is associated with data.zzz` and no dialog opens

#### Scenario: Long path is passed without the verbatim prefix

- **WHEN** the target's absolute path exceeds 260 characters
- **THEN** the path handed to the platform API carries no `\\?\` prefix
