# text-field-editing Specification

## Purpose
TBD - created by archiving change line-editing-in-text-fields. Update Purpose after archive.
## Requirements
### Requirement: Caret model and editing keys

Every single-line text field — the Copy/Move destination and Make-directory name field, the file-action Rename field, the overwrite-conflict Rename field, the find-file pattern, the fuzzy-jump pattern, and the command line — SHALL be backed by one shared line-edit model holding the text and a caret position. Printable characters SHALL be inserted at the caret; Backspace SHALL delete the character before the caret and Delete the character after it; Left/Right SHALL move the caret by one character; Home/End SHALL move it to the start/end; Ctrl+Left/Ctrl+Right SHALL move it by one word; Ctrl+Backspace/Ctrl+Delete SHALL delete the word before/after the caret. A field that opens pre-filled SHALL open with the caret at the end of the text. The command line SHALL apply these keys subject to the exceptions in `command-line` "Command-line prompt and printable-key routing" (keys that keep their panel meaning). The model SHALL live in `filecommand-core` with all mutations flowing through `core::update`.

#### Scenario: Insert in the middle of a pre-filled name
- **WHEN** the Rename field opens pre-filled with `draft.txt`, and the user presses Left five times, Backspace, then types `i`
- **THEN** the field reads `drift.txt` with the caret after the `i`

#### Scenario: Home then edit a destination prefix
- **WHEN** the destination field holds `D:\backup\2026`, and the user presses Home, Delete twice, then types `\\server\share`
- **THEN** the field reads `\\server\share\backup\2026`

#### Scenario: Word delete
- **WHEN** the field holds `C:\Users\kevin\docs` with the caret at the end and the user presses Ctrl+Backspace
- **THEN** the field reads `C:\Users\kevin\`

#### Scenario: Pre-filled field accepts unchanged with Enter
- **WHEN** the destination field opens pre-filled with the opposite panel's path and the user presses Enter without any editing key
- **THEN** the operation starts against that path exactly as before this capability existed

### Requirement: Paste into a text field

The system SHALL paste the system clipboard's text at the caret of the focused dialog text field on Shift+Ins or Ctrl+V, with any line breaks replaced by a single space. When the clipboard holds no text, the key SHALL have no effect. Over the panels, Ctrl+V SHALL paste into the command line, while Shift+Ins SHALL keep its selection-toggle meaning and Ctrl+Shift+Ins SHALL remain the clipboard-paths action.

#### Scenario: Paste a path into the destination field
- **WHEN** the clipboard holds `E:\archive` and the destination field is focused with the caret at the end of `D:\`
- **THEN** after Ctrl+V the field reads `D:\E:\archive` with the caret at the end

#### Scenario: Multi-line clipboard text is flattened
- **WHEN** the clipboard holds two lines and the user presses Ctrl+V over the panels
- **THEN** the two lines are inserted into the command line joined by one space, and the panel cursor does not move

#### Scenario: Shift+Ins over the panels still toggles selection
- **WHEN** the clipboard holds text and the user presses Shift+Ins with the panels focused
- **THEN** the cursor entry's selection is toggled and nothing is pasted

#### Scenario: Empty clipboard is a no-op
- **WHEN** the clipboard holds no text and the user presses Ctrl+V in a focused field
- **THEN** the field and caret are unchanged

### Requirement: Field rendering keeps the caret visible

Each text field SHALL render a window of its text sized to the field width, scrolled so the caret is always within the window, and SHALL show the terminal cursor at the caret's screen position while the field is focused. The bracket-and-dots field style, dialog geometry, and roles SHALL be unchanged.

#### Scenario: Long destination scrolls to the caret
- **WHEN** the destination field (40 cells) holds a 60-character path and the caret is at the end
- **THEN** the field shows the last 40 characters and the terminal cursor sits at the field's last cell

#### Scenario: Caret moved left scrolls the window back
- **WHEN** the field above is focused and the user presses Home
- **THEN** the field shows the first 40 characters and the terminal cursor sits at the field's first cell

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

