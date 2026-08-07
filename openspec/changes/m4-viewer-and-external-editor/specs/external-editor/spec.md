# external-editor

## ADDED Requirements

### Requirement: Config-driven external editor command

The system SHALL read the external editor command from the `editor =` key in `config.toml` and use it as the program launched by the F4 external-editor hook. When `editor =` is unset, the system SHALL NOT attempt to launch any external editor.

#### Scenario: Editor command configured

- **WHEN** `config.toml` contains `editor = "notepad"` and F4 is pressed on a file
- **THEN** the system launches `notepad` as the external editor for that file

#### Scenario: Editor command unset

- **WHEN** `editor =` is absent from `config.toml` and F4 is pressed on a file
- **THEN** the system does not spawn any process and instead shows a message dialog stating that no editor is configured

#### Scenario: Editor selected from config schema key already reserved

- **WHEN** the loaded config carries an `editor =` value that is an empty string
- **THEN** the system treats it as unset and shows the "no editor configured" message rather than spawning an empty command

### Requirement: F4 launches the editor on the current file

The system SHALL, on F4 from a panel with a valid `editor =` configured, launch the external editor targeting the file under the active panel's cursor, spawning the process with the active panel's current directory as its working directory.

#### Scenario: Launch on file under cursor

- **WHEN** the active panel's cursor is on `report.txt` in directory `C:\work` and F4 is pressed
- **THEN** the external editor is spawned with `report.txt` as its argument and its working directory set to `C:\work`

#### Scenario: Cursor on a directory entry

- **WHEN** the active panel's cursor is on a directory (including `..`) and F4 is pressed
- **THEN** the system does not launch the external editor on that directory

#### Scenario: Original OS filename passed through

- **WHEN** the file under the cursor has a name that is not valid Unicode (an `OsString` with unpaired surrogates)
- **THEN** the system passes the original `OsString` path to the editor rather than a lossy-converted name

### Requirement: TUI suspend and restore around the editor

The system SHALL suspend the terminal UI before launching the external editor and restore it after the editor exits, reusing the same suspend/restore mechanism used for shell command passthrough. Suspension SHALL leave raw mode and the alternate screen so the editor owns the terminal; restoration SHALL re-enter raw mode and the alternate screen and repaint.

#### Scenario: Terminal handed to the editor

- **WHEN** the external editor is launched via F4
- **THEN** the system leaves raw mode and the alternate screen before the editor process starts, so the editor renders on the normal terminal

#### Scenario: Terminal restored on editor exit

- **WHEN** the external editor process exits and control returns to FileCommand
- **THEN** the system re-enters raw mode and the alternate screen and repaints the panels

#### Scenario: Terminal restored when the editor crashes

- **WHEN** the external editor process terminates abnormally (crash or non-zero exit)
- **THEN** the terminal is still restored to raw mode and the alternate screen and FileCommand's UI is not left corrupted

### Requirement: Synchronous wait and panel re-read on return

The system SHALL block FileCommand input while the external editor runs and SHALL wait for the editor process to exit before restoring the UI. On return, the system SHALL re-read the active panel so any on-disk changes made by the editor are reflected.

#### Scenario: Input blocked during edit

- **WHEN** the external editor is running
- **THEN** FileCommand does not process its own key events until the editor process exits

#### Scenario: Panel refreshed after edit

- **WHEN** the external editor exits after the user saved changes to the file
- **THEN** the active panel is re-read so the entry's updated size and modification time are shown

### Requirement: Editor spawn errors do not crash the app

The system SHALL surface a failure to launch the configured external editor (for example, the command is not found or cannot be spawned) as an inline error and SHALL restore the UI, without crashing.

#### Scenario: Configured editor not found

- **WHEN** `editor =` names a program that cannot be located or spawned and F4 is pressed
- **THEN** the system restores the terminal and shows an error message, and FileCommand continues running
