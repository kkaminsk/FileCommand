## ADDED Requirements

### Requirement: A copy never overwrites its own source

When a planned copy's target resolves to the same file as its source, the copy SHALL NOT write to the target. This applies to a Copy and to the copy half of a move performed as copy-then-delete. The target SHALL count as the same file when either of these holds:

- after following links, it has the same file identity as the source;
- the identity of either one cannot be determined, and the two normalized absolute paths are equal, compared case-insensitively on Windows.

Such an item SHALL NOT raise the overwrite-conflict dialog. It SHALL be skipped and listed in the end-of-job skipped-files summary with a reason stating that the source and destination are the same file. A move in that situation SHALL NOT delete the source.

#### Scenario: F5 into the directory the file is already in leaves it unchanged
- **WHEN** both panels show `C:\work`, the cursor is on `report.txt` (2,048 bytes), and the user presses F5 and accepts the pre-filled destination `C:\work`
- **THEN** `report.txt` still holds its original 2,048 bytes
- **AND** the end-of-job summary lists `report.txt` as skipped because the source and destination are the same file

#### Scenario: The same file reached through a link is not overwritten
- **WHEN** `C:\alias` is a junction to `C:\work` and the user copies `C:\work\report.txt` to `C:\alias`
- **THEN** `report.txt` is not modified and is listed as skipped because the source and destination are the same file

#### Scenario: A copy-then-delete move onto the same file keeps the source
- **WHEN** a move that runs as copy-then-delete reaches `report.txt` and its target is the same file as the source
- **THEN** `report.txt` is neither modified nor deleted
- **AND** the end-of-job summary lists `report.txt` as skipped because the source and destination are the same file
