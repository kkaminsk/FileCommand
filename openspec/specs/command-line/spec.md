# command-line Specification

## Purpose
TBD - created by archiving change m3-command-line-and-menus. Update Purpose after archive.
## Requirements
### Requirement: Command-line prompt and printable-key routing

The system SHALL render a command line below the panels showing a shell prompt with the active panel's current path (e.g. `C:\NORTON>`), and while a panel is focused and no quick-search or dialog is active, printable keys SHALL be appended to the command-line buffer rather than the panel, matching classic NC behavior.

The prompt SHALL follow the active panel: switching panels or navigating the active panel to a new directory SHALL update the displayed prompt path to that panel's current path.

Because printable keys route to the command line while a panel is focused, the command line is a distinct typing sink from the §4.7 type-ahead quick-search: when quick-search mode is active it consumes plain printable keys, and only one sink SHALL consume any given key.

#### Scenario: Prompt shows active panel path
- **WHEN** the left panel is active and its current directory is `C:\NORTON`
- **THEN** the command line renders the prompt `C:\NORTON>` followed by the (empty) command buffer

#### Scenario: Prompt updates on panel switch
- **WHEN** the right panel's directory is `D:\WORK` and the user presses Tab to make the right panel active
- **THEN** the command-line prompt updates to `D:\WORK>`

#### Scenario: Printable key routes to command line
- **WHEN** a panel is focused, no quick-search or dialog is active, and the user types `d`, `i`, `r`
- **THEN** the command-line buffer becomes `dir` and the panel cursor does not move

#### Scenario: Quick-search mode captures printables instead
- **WHEN** type-ahead quick-search mode is active and the user types a printable key
- **THEN** the key extends the quick-search pattern and the command-line buffer is left unchanged

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

### Requirement: Command history navigation

The system SHALL maintain a command history and, while the command-line buffer is non-empty, Up and Down SHALL navigate previous and next history entries into the buffer. While the buffer is empty, Up and Down SHALL instead move the panel cursor. Backspacing the buffer to empty SHALL be the mechanism that hands Up/Down back to the panel; Esc SHALL NOT clear the buffer — over the panels it requests application quit (application-shell "Quit request keys and confirmation"). Command history SHALL persist to `history.json`, written atomically.

#### Scenario: Up recalls previous command while composing

- **WHEN** the command buffer contains text and the user presses Up
- **THEN** the buffer is replaced with the previous history entry and the panel cursor does not move

#### Scenario: Backspacing to empty releases Up/Down to panel

- **WHEN** the command buffer is non-empty and the user backspaces until it is empty, then presses Up
- **THEN** the subsequent Up moves the panel cursor and recalls no history entry

#### Scenario: Esc does not clear the buffer

- **WHEN** the command buffer is non-empty and the user presses Esc
- **THEN** the buffer is not cleared and the quit-confirmation dialog opens instead

#### Scenario: Executed command persisted to history

- **WHEN** the user runs a command with Enter
- **THEN** the command is appended to the history and `history.json` is written atomically

### Requirement: Paste filename and path to command line

The system SHALL paste the cursor entry's filename onto the command line when Ctrl+Enter is pressed, and the cursor entry's full path when Ctrl+] is pressed, inserting at the command-line buffer. Ctrl+] (ASCII 0x1D) SHALL be available on all platforms; Ctrl+Enter SHALL be available on Windows and best-effort elsewhere (available only where the kitty keyboard protocol delivers it). Both bindings SHALL be overridable in `config.toml`.

#### Scenario: Ctrl+Enter pastes filename
- **WHEN** the cursor is on entry `README.md` in `C:\NORTON` and the user presses Ctrl+Enter
- **THEN** `README.md` is inserted into the command-line buffer

#### Scenario: Ctrl+] pastes full path
- **WHEN** the cursor is on entry `README.md` in `C:\NORTON` and the user presses Ctrl+]
- **THEN** `C:\NORTON\README.md` is inserted into the command-line buffer

### Requirement: Panels on/off reveals terminal scrollback

When the user presses Ctrl+O, the system SHALL leave the alternate screen to expose the host terminal's scrollback containing prior command output, and any subsequent key press SHALL return to the alternate screen and redraw. The system SHALL NOT maintain its own command-output buffer; the visible output history is whatever the host terminal retains.

#### Scenario: Ctrl+O leaves the alternate screen
- **WHEN** the user presses Ctrl+O
- **THEN** the app leaves the alternate screen so the terminal's scrollback (including prior command output) is visible

#### Scenario: Any key returns to panels
- **WHEN** the terminal scrollback is showing after Ctrl+O and the user presses any key
- **THEN** the app re-enters the alternate screen and redraws the panels

### Requirement: Configurable shell with documented latency tradeoff

The system SHALL construct the shell invocation from a configurable shell setting, defaulting on Windows to `cmd.exe /C` for minimal spawn latency. The `config.toml` `shell =` key SHALL select an alternate shell such as PowerShell or `pwsh`, and the configuration SHALL document that PowerShell adds roughly 200 ms or more of spawn latency per command. The shell invocation SHALL be constructed as `shell + args + user text` in `filecommand-core` so it is unit-testable without a terminal.

#### Scenario: Default shell is cmd.exe on Windows
- **WHEN** no `shell =` value is configured on Windows and the user runs the command `dir`
- **THEN** the invocation is built from `cmd.exe /C` with the user text appended

#### Scenario: Configured PowerShell shell is used
- **WHEN** `config.toml` sets `shell = "powershell"` and the user runs a command
- **THEN** the invocation is built from the configured PowerShell executable rather than `cmd.exe`

#### Scenario: Command construction is terminal-independent
- **WHEN** a unit test constructs a command line for user text `dir` with the default shell
- **THEN** the constructed invocation and working directory are produced by `filecommand-core` without requiring a terminal or an actual spawn

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

