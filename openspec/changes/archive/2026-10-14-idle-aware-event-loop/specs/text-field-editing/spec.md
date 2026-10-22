## ADDED Requirements

### Requirement: Terminal paste arrives as one paste

On platforms where FileCommand's terminal input layer reports a bracketed paste as a single event — every supported platform except Windows — the system SHALL enable bracketed-paste mode while it owns the terminal, SHALL disable it before suspending for a shell command, the external editor, or the scrollback view and on every exit path including a panic, and SHALL re-enable it on resume. A reported paste received while a single-line text field owns the keyboard — the same field Ctrl+V would paste into, including the command line over the panels — SHALL be inserted at that field's caret as one paste operation, with line breaks replaced by a single space, exactly as "Paste into a text field" inserts clipboard text. A paste into the built-in editor's text SHALL be inserted at the caret as if typed, line breaks included; a paste into the editor's or the viewer's search or replace prompt SHALL be entered as if typed, with line breaks replaced by a single space. A paste received while no text input has focus SHALL be ignored. On Windows the system SHALL NOT enable bracketed-paste mode; a paste arrives as individual key events, which SHALL be handled as typed input, rendered as described in application-shell "Bursts of key input render once". The single-paste guarantees of this requirement, including flattening line breaks and ignoring pastes outside text input, therefore apply only off Windows.

#### Scenario: Paste a path into the command line

- **WHEN** the panels are focused and the terminal reports a paste of `C:\Program Files\App`
- **THEN** the command line contains `C:\Program Files\App` inserted at its caret in one operation, and the panel cursor, selection, and quick search are unaffected by the pasted characters

#### Scenario: Multi-line paste is flattened and not executed

- **WHEN** the command line has focus and the terminal reports a paste of two lines
- **THEN** the two lines are inserted joined by one space and no command is executed

#### Scenario: Paste into a dialog field

- **WHEN** the Copy destination field is focused with the caret at the end of `D:\` and the terminal reports a paste of `backup`
- **THEN** the field reads `D:\backup` with the caret at the end

#### Scenario: Paste with no focused text input is ignored

- **WHEN** the delete confirmation dialog is open and the terminal reports a paste of `y`
- **THEN** nothing is deleted and the dialog stays open

#### Scenario: Child programs get a terminal without bracketed paste

- **WHEN** off Windows, the user runs a command from the F2 user menu, which suspends FileCommand while it runs
- **THEN** bracketed-paste mode is disabled while the command runs and re-enabled when FileCommand resumes

#### Scenario: On Windows a paste is typed input

- **WHEN** FileCommand runs on Windows and the user pastes `C:\Program Files\App` while the command line has focus
- **THEN** bracketed-paste mode was never enabled, the characters are inserted as typed keys in order, and the screen is redrawn once per burst of keys rather than once per character
