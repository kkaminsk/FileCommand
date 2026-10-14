## MODIFIED Requirements

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
