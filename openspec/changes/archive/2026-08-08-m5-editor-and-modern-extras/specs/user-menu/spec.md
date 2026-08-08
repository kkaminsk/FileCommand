## ADDED Requirements

### Requirement: Open the F2 user menu

The system SHALL open a modal user menu when F2 is pressed, listing the entries loaded from `usermenu.toml` in file order, each shown by its `label`. The menu SHALL render in the primary dialog style (black on cyan, black double-line frame, §4.4) centered over the panels. When `usermenu.toml` contains no entries, F2 SHALL open the menu showing an empty/placeholder list rather than doing nothing.

#### Scenario: F2 opens the menu with configured entries

- **WHEN** the user presses F2 and `usermenu.toml` defines three entries with labels `Compress`, `Backup`, `Checksum`
- **THEN** a modal user menu opens listing exactly those three labels in file order, in the primary dialog style

#### Scenario: F2 with no entries opens an empty menu

- **WHEN** the user presses F2 and `usermenu.toml` defines zero entries
- **THEN** the user menu opens showing an empty list (or an empty-state placeholder) and remains dismissable with Esc

#### Scenario: Only labels are shown in the list

- **WHEN** the user menu is open for an entry whose `label` is `Backup` and whose `command` is `robocopy . D:\backup /E`
- **THEN** the list row displays the label `Backup` and does not display the underlying command string

### Requirement: Parse label and command entries from usermenu.toml

The system SHALL load the F2 user menu from `usermenu.toml` in the platform config directory (§6), where each entry provides a `label` (display text) and a `command` (shell command string). Parsing SHALL be performed by the `config` module in `filecommand-core` and MUST be unit-testable without a terminal.

#### Scenario: Well-formed entries are loaded in order

- **WHEN** `usermenu.toml` defines entries `[{label="A", command="cmd-a"}, {label="B", command="cmd-b"}]`
- **THEN** the parsed user menu model contains entry A followed by entry B, each pairing its label with its command string

#### Scenario: Unknown keys warn rather than fail

- **WHEN** `usermenu.toml` contains a recognized entry plus an unrecognized key on that entry
- **THEN** the entry is still loaded and the unknown key produces a warning, not a hard parse failure

### Requirement: Run the selected entry's command via the shell in the active panel directory

The system SHALL, when the user activates a user-menu entry (Enter on the highlighted row), run that entry's `command` through the shell passthrough with the working directory set to the active panel's current directory, and close the menu. The menu itself SHALL NOT reinterpret or transform the command beyond handing it to the shell passthrough.

#### Scenario: Enter runs the highlighted command in the panel directory

- **WHEN** the active panel is at `C:\Projects\app` and the user highlights the `Build` entry (command `cargo build`) and presses Enter
- **THEN** the menu closes and `cargo build` is dispatched to the shell passthrough with `C:\Projects\app` as its working directory

#### Scenario: Command is passed through unmodified

- **WHEN** the user activates an entry whose `command` is `echo %CD% && dir`
- **THEN** the exact command string `echo %CD% && dir` is handed to the shell passthrough without alteration

### Requirement: Navigate and dismiss the user menu

The system SHALL let the user move the highlight between entries with the Up/Down arrow keys and SHALL close the menu without running any command when Esc is pressed. While the menu is open it is modal: panel and command-line input SHALL NOT be processed until it closes.

#### Scenario: Esc closes the menu without running a command

- **WHEN** the user menu is open and the user presses Esc
- **THEN** the menu closes, no command is dispatched to the shell, and focus returns to the active panel

#### Scenario: Arrow keys move the highlight

- **WHEN** the user menu lists three entries with the first highlighted and the user presses Down twice
- **THEN** the third entry is highlighted, and Enter would activate the third entry

### Requirement: Create and recover the usermenu.toml file

The system SHALL create `usermenu.toml` with default entries on first run when the file is absent, and when the file is malformed SHALL raise a startup warning dialog, fall back to default entries, and MUST NOT silently overwrite the existing file (§6).

#### Scenario: Missing file is created with defaults

- **WHEN** FileCommand starts and no `usermenu.toml` exists in the config directory
- **THEN** a `usermenu.toml` containing default entries is written, and F2 opens a menu populated from those defaults

#### Scenario: Malformed file warns and falls back without overwriting

- **WHEN** FileCommand starts and `usermenu.toml` contains invalid TOML
- **THEN** a startup warning dialog is shown, the user menu falls back to default entries, and the malformed `usermenu.toml` file is left unchanged on disk
