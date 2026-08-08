# builtin-editor Specification

## Purpose
TBD - created by archiving change m5-editor-and-modern-extras. Update Purpose after archive.
## Requirements
### Requirement: Editor invocation and size cap

The system SHALL open the built-in editor on F4 for the file under the cursor when no external editor is configured, loading files smaller than 10 MB into an in-memory line buffer, and SHALL redirect files of 10 MB or larger to the F3 viewer with a notice rather than loading them into the editor.

#### Scenario: Small file opens in the editor

- **WHEN** the user presses F4 on a file of size less than 10 MB and no external editor command is configured
- **THEN** the built-in editor opens full-screen, replacing the panels, with the file's contents loaded into the editable buffer and the caret at the start of the first line

#### Scenario: Large file redirects to the viewer

- **WHEN** the user presses F4 on a file whose size is 10 MB or greater
- **THEN** the built-in editor does not load the file and the file is opened in the F3 viewer with a notice explaining it exceeds the editor's size limit

#### Scenario: External editor takes precedence

- **WHEN** a `config.toml` external editor command is set and the user presses F4 on a file
- **THEN** the built-in editor does not open and the external editor is launched instead

### Requirement: Insert and overwrite text entry

The system SHALL support both insert and overwrite text-entry modes, toggled while editing, and SHALL indicate that overwrite mode is active in the header row.

#### Scenario: Insert mode shifts existing text

- **WHEN** the editor is in insert mode and the user types a printable character mid-line
- **THEN** the character is inserted at the caret and the remainder of the line shifts right by one cell

#### Scenario: Overwrite mode replaces the character

- **WHEN** the editor is in overwrite mode and the user types a printable character over an existing character
- **THEN** the existing character is replaced in place, the caret advances one cell, and `Ovr` is shown at the right of the header row

#### Scenario: Toggling the mode updates the header

- **WHEN** the user toggles from insert to overwrite mode
- **THEN** subsequent typing overwrites and the `Ovr` indicator appears; toggling back removes the indicator and restores inserting behavior

### Requirement: Line-based selection with cut, copy, and paste

The system SHALL provide whole-line selection anchored with F3 (Mark) as an `[anchor_line, cursor_line]` range, render the selected lines as inverse rows, and support cut, copy, and paste operating on whole lines only.

#### Scenario: Marking selects whole lines

- **WHEN** the user presses F3 (Mark) to set an anchor and then moves the cursor down two lines
- **THEN** the three lines from the anchor to the cursor render as inverse rows and form the current line selection

#### Scenario: Cut removes and captures selected lines

- **WHEN** a line selection is active and the user cuts it
- **THEN** the selected lines are removed from the buffer, captured to the clipboard as whole lines, and the caret settles on the line that followed the removed range

#### Scenario: Paste inserts captured lines

- **WHEN** the clipboard holds previously copied or cut lines and the user pastes at the caret
- **THEN** those whole lines are inserted at the caret position without splitting the surrounding lines

### Requirement: Search and search-and-replace without regex

The system SHALL provide plain-text (non-regex) search on F7 and search-and-replace on F4, matching literal substrings within the buffer.

#### Scenario: Search moves to the next match

- **WHEN** the user invokes F7 search and enters a literal string that occurs later in the buffer
- **THEN** the caret moves to the next occurrence of that string and the matching text is brought into view

#### Scenario: Replace substitutes a match

- **WHEN** the user invokes F4 replace, supplies a literal search string and a replacement string, and confirms a replacement
- **THEN** the matched literal text is replaced with the replacement string and the buffer is marked modified

#### Scenario: Search treats input literally

- **WHEN** the user enters a search string containing regex metacharacters such as `.` or `*`
- **THEN** the editor matches those characters literally and does not interpret them as a pattern

### Requirement: Single-level undo

The system SHALL keep exactly one prior-state snapshot and provide a single-level undo that swaps the current buffer to that snapshot.

#### Scenario: Undo restores the prior state

- **WHEN** the user makes an edit and then invokes undo
- **THEN** the buffer is restored to the snapshot captured before that edit

#### Scenario: Undo does not go back more than one level

- **WHEN** the user makes two successive edits and invokes undo twice
- **THEN** only a single level of history is available and the buffer is not restored to a state older than the most recent snapshot

### Requirement: Save in place with line-ending and encoding preservation

The system SHALL save the buffer in place on F2, writing UTF-8 text and preserving the file's original dominant line ending (CRLF or LF) detected on load, so a CRLF file stays CRLF and an LF file stays LF.

#### Scenario: CRLF file stays CRLF on save

- **WHEN** a file whose dominant line ending is CRLF is edited and saved with F2
- **THEN** the file is written back using CRLF terminators

#### Scenario: LF file stays LF on save

- **WHEN** a file whose dominant line ending is LF is edited and saved with F2
- **THEN** the file is written back using LF terminators

#### Scenario: Save clears the modified state

- **WHEN** the buffer differs from the last saved state and the user presses F2
- **THEN** the file on disk is updated in place and the buffer is no longer considered modified

### Requirement: Modified indicator and save-on-exit prompt

The system SHALL derive a modified indicator from whether the buffer differs from the last saved state, display ` *` after the file path in the header row while modified, and raise a save-on-exit confirmation when the user quits (F10) with unsaved changes.

#### Scenario: Modified indicator appears on edit

- **WHEN** the user makes an edit that causes the buffer to differ from the saved state
- **THEN** ` *` is appended after the path in the header row

#### Scenario: Quitting with unsaved changes prompts

- **WHEN** the user presses F10 while the buffer is modified
- **THEN** a save-on-exit confirmation dialog is raised rather than exiting immediately

#### Scenario: Quitting an unmodified buffer exits directly

- **WHEN** the user presses F10 while the buffer matches the saved state
- **THEN** the editor closes and returns to the panels without a prompt

### Requirement: Full-screen editor chrome

The system SHALL render the editor full-screen with a header row, a text body, and a bottom F-key bar. The header row (black on cyan, full width) SHALL show `Edit: <path>` at the left with ` *` when modified, the `Line L/N   Col C` position at the center, and the file size in bytes plus `Ovr` when overwrite is active at the right. The body SHALL render the file text in the editor text style with the caret as the terminal cursor. The F-key bar SHALL read `1Help 2Save 3Mark 4Replac 5 6 7Search 8 9 10Quit`, rendering unused slots as the number with an empty label block.

#### Scenario: Header reflects position and modified state

- **WHEN** the caret is on line 12 of a 440-line unsaved buffer at column 8
- **THEN** the header shows `Edit: <path> *` at the left and `Line 12/440   Col 8` at the center

#### Scenario: Overwrite indicator in the header

- **WHEN** overwrite mode is active
- **THEN** the right of the header shows the file size in bytes followed by `Ovr`

#### Scenario: F-key bar labels

- **WHEN** the editor is displayed
- **THEN** the bottom bar shows `1Help 2Save 3Mark 4Replac 7Search 10Quit` with slots 5, 6, 8, and 9 rendered as bare numbers with empty label blocks

