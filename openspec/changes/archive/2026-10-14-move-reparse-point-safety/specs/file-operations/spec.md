## MODIFIED Requirements

### Requirement: Reparse-point (symlink/junction) semantics

Reparse points SHALL be handled distinctly from ordinary directories: deleting a reparse point SHALL remove the link itself and never the target's contents; copying a reparse point SHALL copy the link target's content by default. Recursive operations SHALL carry recursion-cycle protection via a visited file-identity set and SHALL NOT traverse into a junction that points inside the source tree being processed.

A move performed as copy-then-delete (a cross-volume move) SHALL treat every reparse point in the source — a selected link or one anywhere inside a selected directory tree — as a leaf for its source-removal half. The copy half copies the link target's content to the destination per the copy rule above. The source-removal half SHALL remove only the link itself, exactly as delete does, and SHALL NOT delete, empty, or change the attributes of any file or directory reached through the link. A link SHALL be removed only after every item copied through it was copied successfully. If any item beneath it was skipped or failed, the link SHALL be left in place. A link that cannot be removed SHALL be reported in the end-of-job skipped-files summary rather than ignored.

#### Scenario: Delete removes the link, not the target
- **WHEN** a job deletes a junction or symlink
- **THEN** the link is removed and the files under the link's target directory remain untouched

#### Scenario: Copy duplicates the link target's content
- **WHEN** a job copies a reparse point
- **THEN** the destination receives a copy of the target's content (not merely a re-created link) by default

#### Scenario: Recursion cycle is not re-entered
- **WHEN** a recursive copy encounters a junction pointing to a directory already inside the source tree (present in the visited-identity set)
- **THEN** the job does not traverse into it, avoiding an infinite recursion cycle

#### Scenario: Cross-volume move of a tree containing a junction to an outside directory leaves that directory's files intact
- **WHEN** `C:\work\proj` contains `file.txt` and a junction `shared` pointing to `C:\shared-libs` (which holds `lib.dll` and a subdirectory `x86`), and the user moves `proj` to `D:\backup`
- **THEN** `D:\backup\proj\file.txt`, `D:\backup\proj\shared\lib.dll`, and `D:\backup\proj\shared\x86` exist at the destination as copies of the content
- **AND** `C:\shared-libs`, `C:\shared-libs\lib.dll`, and `C:\shared-libs\x86` still exist with their original content and attributes
- **AND** the source `C:\work\proj`, including the junction `shared` itself, is removed

#### Scenario: Cross-volume move of a selected junction removes only the link
- **WHEN** the user moves a selected junction `C:\work\link` (pointing to `C:\data`) to `D:\backup`
- **THEN** `D:\backup\link` receives a copy of the content of `C:\data`
- **AND** the junction `C:\work\link` is removed while `C:\data` and every file in it remain untouched

#### Scenario: A link whose content was not fully copied is kept
- **WHEN** a cross-volume move copies through a junction and one file beneath it is skipped (by an overwrite-conflict Skip or an error-recovery Skip)
- **THEN** the junction is left in place in the source, the link target's files are untouched, and the skipped file is listed in the end-of-job skipped-files summary
