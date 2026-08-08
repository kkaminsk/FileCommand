## ADDED Requirements

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

When the user presses Enter with a non-empty command-line buffer, the system SHALL run the typed line through the configured shell in the active panel's current directory with the TUI suspended, then return control to the TUI and re-read the active panel.

Running a command SHALL leave raw mode and the alternate screen, spawn the child inheriting stdio in the panel directory, wait for it to exit, prompt the user to press a key, then re-enter the alternate screen and raw mode and redraw. Terminal restore SHALL be idempotent so a failing or panicking child cannot leave the terminal in raw mode or the alternate screen.

Enter on an executable target (PATHEXT match or `.lnk`) SHALL use this same suspended-spawn path.

#### Scenario: Enter runs the typed command
- **WHEN** the active panel is `C:\NORTON`, the command buffer is `dir`, and the user presses Enter
- **THEN** the TUI suspends, the shell runs `dir` with the working directory `C:\NORTON`, the user is prompted to press a key, and after a keypress the TUI is restored and the active panel is re-read

#### Scenario: Command buffer cleared after run
- **WHEN** a command finishes and the TUI is restored
- **THEN** the command-line buffer is empty and the prompt shows the active panel's path

#### Scenario: Terminal restored after a failing child
- **WHEN** the spawned child exits with an error or the spawn fails
- **THEN** the TUI is restored to the alternate screen and raw mode exactly once, and the app does not crash

### Requirement: Command history navigation

The system SHALL maintain a command history and, while the command-line buffer is non-empty, Up and Down SHALL navigate previous and next history entries into the buffer. While the buffer is empty, Up and Down SHALL instead move the panel cursor. Esc SHALL clear the command-line buffer, which is the explicit mechanism that hands Up/Down back to the panel. Command history SHALL persist to `history.json`, written atomically.

#### Scenario: Up recalls previous command while composing
- **WHEN** the command buffer contains text and the user presses Up
- **THEN** the buffer is replaced with the previous history entry and the panel cursor does not move

#### Scenario: Up moves panel cursor when buffer empty
- **WHEN** the command buffer is empty and the user presses Up
- **THEN** the panel cursor moves up and no history entry is recalled

#### Scenario: Esc clears buffer to release Up/Down to panel
- **WHEN** the command buffer is non-empty and the user presses Esc, then presses Up
- **THEN** the buffer is cleared by Esc and the subsequent Up moves the panel cursor

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
