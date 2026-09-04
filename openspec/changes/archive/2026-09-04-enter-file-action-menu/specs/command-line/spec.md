# command-line Specification (delta)

## MODIFIED Requirements

### Requirement: Run command via shell in suspended-TUI mode

When the user presses Enter with a non-empty command-line buffer, the system SHALL run the typed line through the configured shell in the active panel's current directory with the TUI suspended, then return control to the TUI and re-read the active panel.

Running a command SHALL leave raw mode and the alternate screen, spawn the child inheriting stdio in the panel directory, wait for it to exit, prompt the user to press a key, then re-enter the alternate screen and raw mode and redraw. Terminal restore SHALL be idempotent so a failing or panicking child cannot leave the terminal in raw mode or the alternate screen.

Enter on an executable target (PATHEXT match or `.lnk`) SHALL NOT spawn the target directly; it SHALL open the file-action menu for that entry, whose Run entry uses this same suspended-spawn path.

#### Scenario: Enter runs the typed command
- **WHEN** the active panel is `C:\NORTON`, the command buffer is `dir`, and the user presses Enter
- **THEN** the TUI suspends, the shell runs `dir` with the working directory `C:\NORTON`, the user is prompted to press a key, and after a keypress the TUI is restored and the active panel is re-read

#### Scenario: Command buffer cleared after run
- **WHEN** a command finishes and the TUI is restored
- **THEN** the command-line buffer is empty and the prompt shows the active panel's path

#### Scenario: Terminal restored after a failing child
- **WHEN** the spawned child exits with an error or the spawn fails
- **THEN** the TUI is restored to the alternate screen and raw mode exactly once, and the app does not crash

#### Scenario: Enter on an executable opens the menu instead of spawning
- **WHEN** the command-line buffer is empty and the user presses Enter with the cursor on `setup.exe`
- **THEN** no child process spawns and the file-action menu opens for `setup.exe`
- **AND** activating the menu's Run entry spawns `setup.exe` via the suspended-TUI path described above
