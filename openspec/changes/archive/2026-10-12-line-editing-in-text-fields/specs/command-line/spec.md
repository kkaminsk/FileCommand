## MODIFIED Requirements

### Requirement: Command-line prompt and printable-key routing

The system SHALL render a command line below the panels showing a shell prompt with the active panel's current path (e.g. `C:\NORTON>`), and while a panel is focused and no quick-search or dialog is active, printable keys SHALL be inserted into the command-line buffer at its caret rather than sent to the panel, matching classic NC behavior. The buffer SHALL be backed by the shared line-edit model (`text-field-editing`). While the buffer is non-empty, Left and Right SHALL move the caret and Ctrl+Backspace SHALL delete the word before it; while the buffer is empty, Left and Right SHALL have no command-line meaning. Home, End, PgUp, PgDn, Delete, and Ctrl+Left/Ctrl+Right SHALL keep their panel meanings regardless of buffer content.

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

#### Scenario: Caret editing mid-line
- **WHEN** the buffer holds `cd srv` and the user presses Left, Left, then types `c`
- **THEN** the buffer reads `cd scrv` with the caret after the `c` and the panel cursor does not move

#### Scenario: Left on an empty line does nothing to the panel
- **WHEN** the buffer is empty and the user presses Left
- **THEN** neither the buffer nor the panel cursor changes

#### Scenario: Home still moves the panel cursor while composing
- **WHEN** the buffer holds `cd docs` and the user presses Home
- **THEN** the panel cursor jumps to the first entry and the buffer and its caret are unchanged

#### Scenario: Quick-search mode captures printables instead
- **WHEN** type-ahead quick-search mode is active and the user types a printable key
- **THEN** the key extends the quick-search pattern and the command-line buffer is left unchanged

### Requirement: Paste filename and path to command line

The system SHALL paste the cursor entry's filename onto the command line when Ctrl+Enter is pressed, and the cursor entry's full path when Ctrl+] is pressed, appending to the end of the command-line buffer (after a separating space when the buffer is non-empty and does not already end in one) and leaving the caret at the end. Ctrl+] (ASCII 0x1D) SHALL be available on all platforms; Ctrl+Enter SHALL be available on Windows and best-effort elsewhere (available only where the kitty keyboard protocol delivers it). Both bindings SHALL be overridable in `config.toml`.

#### Scenario: Ctrl+Enter pastes filename
- **WHEN** the cursor is on entry `README.md` in `C:\NORTON` and the user presses Ctrl+Enter
- **THEN** `README.md` is inserted at the end of the command-line buffer and the caret is at the end

#### Scenario: Ctrl+] pastes full path
- **WHEN** the cursor is on entry `README.md` in `C:\NORTON` and the user presses Ctrl+]
- **THEN** `C:\NORTON\README.md` is inserted at the end of the command-line buffer and the caret is at the end

#### Scenario: Paste after caret editing lands at the end
- **WHEN** the buffer holds `del ` with the caret after `d`, and the user presses Ctrl+Enter on `old.log`
- **THEN** the buffer reads `del old.log` and the caret is at the end
