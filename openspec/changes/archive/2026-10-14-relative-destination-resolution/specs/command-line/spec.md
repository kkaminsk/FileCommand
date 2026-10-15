## ADDED Requirements

### Requirement: cd, del and rmdir targets resolve to a normalized absolute path

The path a `cd`, `del` or `rmdir` target resolves to SHALL be an absolute path with no `.` or `..` components, whenever the active panel's directory is absolute. The supported forms keep their meaning: `.`, `..`, a bare drive letter, a UNC path, an absolute path, and a path relative to the active panel's directory. Two forms are defined precisely:

- A path containing `..` components, such as `..\x`, SHALL resolve to the directory it names after those components are removed, and SHALL NOT be kept as typed.
- A drive-relative path (`D:x`) SHALL resolve against the active panel's directory when that directory is on drive `D:`, and against `D:\` otherwise. It SHALL NOT depend on any process-wide current directory.

These are the same rules the destination input dialog uses (operation-dialogs "Relative destinations resolve against the source panel directory"). The existence check, the navigation, and the delete-confirmation flow SHALL all use the resolved path.

#### Scenario: cd with a parent-relative path lands on the normalized directory
- **WHEN** the active panel is at `C:\work\proj`, `C:\work\x` exists, and the command buffer is `cd ..\x`
- **THEN** the active panel navigates to `C:\work\x` and lists it successfully
- **AND** the panel's current directory contains no `..` component

#### Scenario: cd with a drive-relative path resolves against the panel or the drive root
- **WHEN** the active panel is at `D:\work`, `D:\work\old` exists, and the command buffer is `cd D:old`
- **THEN** the active panel navigates to `D:\work\old`
- **AND** when the active panel is instead at `C:\work` and `D:\old` exists, the same command navigates to `D:\old`

#### Scenario: del with a parent-relative path names the resolved file
- **WHEN** the active panel is at `C:\work\proj`, `C:\work\old.txt` is a file, and the command buffer is `del ..\old.txt`
- **THEN** the delete-confirmation dialog opens for `C:\work\old.txt`, and nothing is deleted until it is accepted
