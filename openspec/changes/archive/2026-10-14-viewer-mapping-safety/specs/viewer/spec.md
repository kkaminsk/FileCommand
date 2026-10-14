## MODIFIED Requirements

### Requirement: Instant open of arbitrarily large files

The viewer SHALL open any file, including multi-GB files, with first paint independent of file size. It MUST read the file through bounded positioned reads on an open handle, on every platform, and MUST NOT memory-map the file. It MUST NOT build a full line index or perform any whole-file scan on open. Only the bytes required for the visible screen SHALL be read to produce the first frame. The file length SHALL be treated as a snapshot taken at open, and all navigation offsets MUST be clamped to that length.

The viewer MUST NOT prevent other programs from changing the file while it is shown. On Windows it SHALL open the file with read, write, and delete sharing (`FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`), so other programs can keep appending to, truncating, deleting, or renaming the file. A file that is truncated or otherwise shrinks while it is shown MUST NOT crash or hang the application: a read beyond the file's new end SHALL return only the bytes that still exist, and the viewer SHALL keep rendering and navigating within the snapshot length. When the viewer closes, it and any F7 search still running for it SHALL release the file.

#### Scenario: First frame reads only the visible window

- **WHEN** F3 opens a multi-GB file
- **THEN** the viewer paints the first screen without reading the whole file and without constructing a line index
- **AND** only the byte range needed for the visible rows is read from the file

#### Scenario: Network and special files are read the same way

- **WHEN** F3 opens a file on a network path, or a zero-length file
- **THEN** the viewer serves the visible window through the same positioned reads it uses for every other file, without panicking
- **AND** the rendered content is identical to what any other read of that byte range returns

#### Scenario: Offsets clamped to the snapshot length

- **WHEN** navigation would move the top anchor beyond the length captured at open
- **THEN** the top offset is clamped to the file length and the view does not read past the snapshot length

#### Scenario: Another program keeps writing to the viewed file

- **WHEN** a log file is open in the viewer and the program writing it appends more lines
- **THEN** the writes succeed
- **AND** the viewer keeps showing the file up to the length captured at open

#### Scenario: Viewed file can be truncated, rotated, or deleted

- **WHEN** a file is open in the viewer and another program truncates it, renames it away, or deletes it
- **THEN** that operation succeeds on every platform, including Windows

#### Scenario: Truncation while viewing never crashes

- **WHEN** a 10 MB file is open in the viewer and another program truncates it to 1 KB
- **AND** the user scrolls or jumps to an offset past the new end
- **THEN** the application keeps running, rows past the new end render empty, and navigation stays within the snapshot length

#### Scenario: Closing the viewer releases the file

- **WHEN** the user closes the viewer, including while an F7 search is still scanning the file
- **THEN** the viewer's handle is closed and the search stops scanning and closes its handle
- **AND** no FileCommand thread holds the file open afterwards
