# builtin-editor Specification

## Purpose
TBD - created by archiving change m5-editor-and-modern-extras. Update Purpose after archive.
## Requirements
### Requirement: Editor invocation and size cap

The system SHALL open the built-in editor on F4 for the file under the cursor when no external editor is configured, loading files smaller than 10 MB into an in-memory line buffer, and SHALL redirect files of 10 MB or larger to the F3 viewer with a notice rather than loading them into the editor. The built-in editor SHALL edit only UTF-8 text. A file smaller than 10 MB SHALL NOT be loaded into the editor when it begins with a UTF-16 byte-order mark (`FF FE` or `FE FF`), contains a NUL (`0x00`) byte, or is not valid UTF-8. Such a file SHALL instead be opened in the F3 viewer with a notice explaining that the built-in editor only edits UTF-8 text, using the same redirect and notice mechanism as the size limit, and the file on disk SHALL NOT be modified. A file that begins with a UTF-8 byte-order mark (`EF BB BF`) and is otherwise valid UTF-8 without NUL bytes SHALL open in the editor. The byte-order mark SHALL NOT appear in the editable buffer.

#### Scenario: Small file opens in the editor

- **WHEN** the user presses F4 on a file of size less than 10 MB and no external editor command is configured
- **THEN** the built-in editor opens full-screen, replacing the panels, with the file's contents loaded into the editable buffer and the caret at the start of the first line

#### Scenario: Large file redirects to the viewer

- **WHEN** the user presses F4 on a file whose size is 10 MB or greater
- **THEN** the built-in editor does not load the file and the file is opened in the F3 viewer with a notice explaining it exceeds the editor's size limit

#### Scenario: External editor takes precedence

- **WHEN** a `config.toml` external editor command is set and the user presses F4 on a file
- **THEN** the built-in editor does not open and the external editor is launched instead

#### Scenario: Non-UTF-8 file redirects to the viewer

- **WHEN** no external editor is configured and the user presses F4 on `setup.bat`, a 2 KB file containing the Windows-1252 byte `E9` (`é`), which is not valid UTF-8
- **THEN** the built-in editor does not load the file, the file is opened in the F3 viewer, and a notice explains that the built-in editor only edits UTF-8 text
- **AND** `setup.bat` on disk is byte-for-byte unchanged

#### Scenario: UTF-16 file redirects to the viewer

- **WHEN** no external editor is configured and the user presses F4 on a file whose first two bytes are `FF FE`
- **THEN** the built-in editor does not load the file, the file is opened in the F3 viewer, and a notice explains that the built-in editor only edits UTF-8 text

#### Scenario: File containing a NUL byte redirects to the viewer

- **WHEN** no external editor is configured and the user presses F4 on a file smaller than 10 MB that contains a `00` byte
- **THEN** the built-in editor does not load the file, the file is opened in the F3 viewer, and a notice explains that the built-in editor only edits UTF-8 text

#### Scenario: UTF-8 file with a byte-order mark is editable

- **WHEN** no external editor is configured and the user presses F4 on a file that begins with `EF BB BF` followed by valid UTF-8 text `hello`
- **THEN** the built-in editor opens with `hello` as the first line, and the byte-order mark is not part of the buffer, so the caret at the start of the first line sits before `h`

#### Scenario: External editor receives non-UTF-8 files unchanged

- **WHEN** a `config.toml` external editor command is set and the user presses F4 on a UTF-16 file
- **THEN** the external editor is launched on that file and no encoding check or redirect takes place

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

The system SHALL save the buffer in place on F2, writing UTF-8 text and preserving the file's original dominant line ending (CRLF or LF) detected on load, so a CRLF file stays CRLF and an LF file stays LF. When the file began with a UTF-8 byte-order mark on load, the saved file SHALL begin with the same byte-order mark. The save SHALL be atomic. The encoded content SHALL be written to a temporary file in the same directory as the target and flushed to stable storage, and only then SHALL that temporary file replace the original in a single rename-style operation, so at every moment the file at the target path holds either its complete previous content or its complete new content. If any step fails, the original file SHALL remain byte-for-byte intact, the temporary file SHALL be removed, the buffer SHALL remain modified, and the editor SHALL display the failure message until the next edit or successful save. The save SHALL preserve the original file's attributes (including hidden and system) and its creation time. On Windows it SHALL also preserve the file's alternate data streams and, where the volume supports access-control lists, its discretionary access-control list (DACL). The system SHALL NOT clear a file's read-only attribute in order to save it. Saving a read-only file SHALL fail with a message stating that the file is read-only, and the file SHALL be left untouched.

#### Scenario: CRLF file stays CRLF on save

- **WHEN** a file whose dominant line ending is CRLF is edited and saved with F2
- **THEN** the file is written back using CRLF terminators

#### Scenario: LF file stays LF on save

- **WHEN** a file whose dominant line ending is LF is edited and saved with F2
- **THEN** the file is written back using LF terminators

#### Scenario: Save clears the modified state

- **WHEN** the buffer differs from the last saved state and the user presses F2
- **THEN** the file on disk is updated in place and the buffer is no longer considered modified

#### Scenario: UTF-8 byte-order mark is written back

- **WHEN** a file that began with `EF BB BF` is edited and saved with F2
- **THEN** the saved file begins with exactly one `EF BB BF` sequence, followed by the buffer's UTF-8 text

#### Scenario: A failed save leaves the original intact

- **WHEN** the user presses F2 and writing the new content fails partway, for example because the disk is full
- **THEN** the file at the target path still holds its complete previous content, no temporary file remains in its directory, the buffer is still marked modified, and the editor displays the failure message

#### Scenario: Content is flushed before it replaces the original

- **WHEN** the user presses F2 on a modified buffer
- **THEN** the new content is written to a temporary file in the target's directory and flushed to stable storage before that file replaces the original, and the original is never truncated or rewritten in place

#### Scenario: Attributes and creation time survive a save

- **WHEN** a hidden file with creation time T is edited and saved with F2
- **THEN** after the save the file is still hidden and its creation time is still T

#### Scenario: Alternate data streams survive a save on Windows

- **WHEN** on Windows a file carrying a `Zone.Identifier` alternate data stream is edited and saved with F2
- **THEN** after the save the file still carries that `Zone.Identifier` stream with its previous content

#### Scenario: Read-only file is not saved

- **WHEN** the file being edited has the read-only attribute and the user presses F2
- **THEN** the file is not modified, its read-only attribute remains set, the buffer stays marked modified, and the editor displays a message stating that the file is read-only

#### Scenario: Save failure message clears on the next edit

- **WHEN** the editor is displaying a save failure message and the user types a character
- **THEN** the failure message is no longer displayed

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

