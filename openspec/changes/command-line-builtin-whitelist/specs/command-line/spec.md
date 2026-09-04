## MODIFIED Requirements

### Requirement: Run command via shell in suspended-TUI mode

The command line's typed buffer SHALL NOT run arbitrary text through a shell. The suspended-TUI shell-spawn mechanism described here is reachable only from the file-action menu's Run entry and the F2 user menu, each via their own trigger — not from typed command-line text (see "Command-line builtin whitelist" for what the typed buffer itself recognizes).

Running a command through this mechanism SHALL leave raw mode and the alternate screen, spawn the child inheriting stdio in the panel directory, wait for it to exit, prompt the user to press a key, then re-enter the alternate screen and raw mode and redraw. Terminal restore SHALL be idempotent so a failing or panicking child cannot leave the terminal in raw mode or the alternate screen.

Enter on an executable target (PATHEXT match or `.lnk`) SHALL NOT spawn the target directly; it SHALL open the file-action menu for that entry, whose Run entry uses this same suspended-spawn path.

#### Scenario: Terminal restored after a failing child
- **WHEN** a child spawned via the Run entry or F2 user menu exits with an error or the spawn fails
- **THEN** the TUI is restored to the alternate screen and raw mode exactly once, and the app does not crash

#### Scenario: Enter on an executable opens the menu instead of spawning
- **WHEN** the command-line buffer is empty and the user presses Enter with the cursor on `setup.exe`
- **THEN** no child process spawns and the file-action menu opens for `setup.exe`
- **AND** activating the menu's Run entry spawns `setup.exe` via the suspended-TUI path described above

## ADDED Requirements

### Requirement: Command-line builtin whitelist

When the user presses Enter with a non-empty command-line buffer, the system SHALL recognize only the built-in verbs `cd`, `del`, and `rmdir` (case-insensitive, matching classic NC/cmd usage). Any other typed text SHALL be rejected: no process SHALL be spawned, and the active panel's error state SHALL be set to indicate the command was not recognized. A recognized builtin's own requirement governs its behavior once dispatched.

#### Scenario: An unrecognized command is rejected without spawning anything
- **WHEN** the command buffer is `dir` and the user presses Enter
- **THEN** no shell process is spawned and the active panel shows an error indicating the command was not recognized

#### Scenario: Builtin verbs are case-insensitive
- **WHEN** the command buffer is `CD sub` and the user presses Enter
- **THEN** the `cd` builtin is dispatched exactly as if the buffer had been `cd sub`

### Requirement: cd navigates the active panel or rejects a nonexistent target

`cd <path>` SHALL resolve `<path>` against the active panel's current directory (supporting `.`, `..`, bare drive letters, UNC paths, and relative paths, exactly as today). Before navigating, the system SHALL verify the resolved target exists and is a directory. If it does, the active panel SHALL navigate to it exactly as today. If it does not — the target doesn't exist, or exists but is not a directory — the system SHALL reject the command: the active panel's `cwd` SHALL be left completely unchanged, no listing read SHALL be attempted, and the panel's error state SHALL be set to indicate the target could not be found.

#### Scenario: cd navigates to a valid relative subdirectory
- **WHEN** the active panel is at `C:\NORTON` containing subdirectory `sub`, and the command buffer is `cd sub`
- **THEN** the active panel navigates to `C:\NORTON\sub`

#### Scenario: cd to a nonexistent directory is rejected without navigating
- **WHEN** the active panel is at `C:\NORTON` and the command buffer is `cd \NOSUCHDIR`
- **THEN** the active panel's current directory remains `C:\NORTON`, no listing read is started, and the panel shows an error indicating the target was not found

#### Scenario: cd to a file (not a directory) is rejected
- **WHEN** the active panel is at `C:\NORTON`, `readme.txt` is a file in it, and the command buffer is `cd readme.txt`
- **THEN** the active panel's current directory remains `C:\NORTON` and the panel shows an error indicating the target is not a directory

### Requirement: del and rmdir route into the existing delete-confirmation flow

`del <target>` and `rmdir <target>` SHALL resolve `<target>` against the active panel's current directory. `del` SHALL only accept a target that is a file; `rmdir` SHALL only accept a target that is a directory. A target that doesn't exist, or whose type doesn't match the verb, SHALL be rejected: the panel's error state SHALL be set accordingly and no dialog SHALL open. A valid, type-matched target SHALL open the same delete-confirmation dialog used by F8 and the file-action menu's Delete entry, scoped to that single entry — including the existing non-empty-directory second confirmation for `rmdir` — and SHALL NOT delete anything until that dialog is accepted.

#### Scenario: del on a file opens the delete-confirmation dialog
- **WHEN** the active panel is at `C:\NORTON`, `notes.txt` is a file in it, and the command buffer is `del notes.txt`
- **THEN** the delete-confirmation dialog opens naming `notes.txt`, and nothing is deleted until it is accepted

#### Scenario: rmdir on a directory opens the delete-confirmation dialog
- **WHEN** the active panel is at `C:\NORTON`, `docs` is a subdirectory in it, and the command buffer is `rmdir docs`
- **THEN** the delete-confirmation dialog opens naming `docs`, requiring the existing second confirmation if it is non-empty, and nothing is deleted until accepted

#### Scenario: del rejects a directory target
- **WHEN** the active panel is at `C:\NORTON`, `docs` is a subdirectory in it, and the command buffer is `del docs`
- **THEN** the command is rejected, no dialog opens, and the panel shows an error indicating the target is a directory

#### Scenario: rmdir rejects a file target
- **WHEN** the active panel is at `C:\NORTON`, `notes.txt` is a file in it, and the command buffer is `rmdir notes.txt`
- **THEN** the command is rejected, no dialog opens, and the panel shows an error indicating the target is not a directory

#### Scenario: del/rmdir on a nonexistent target is rejected
- **WHEN** the command buffer is `del \NOSUCHFILE.TXT`
- **THEN** the command is rejected, no dialog opens, and the panel shows an error indicating the target was not found
