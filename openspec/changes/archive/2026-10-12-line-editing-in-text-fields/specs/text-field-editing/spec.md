## ADDED Requirements

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
