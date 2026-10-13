## ADDED Requirements

### Requirement: Relative destinations resolve against the source panel directory

A path the user confirms in the destination input dialog SHALL be interpreted relative to the **source panel's** current directory. This applies to the Copy (F5) and Rename/Move (F6) destination, however the dialog was opened (keyboard, file-action menu, or a drop), and to the make-directory (F7) name. The source panel is the panel whose entries the operation acts on; for a drop, it is the panel the drag started from. The path SHALL NOT be interpreted against the application's launch directory or any other process-wide current directory.

The entered text SHALL resolve as follows:

- A relative path is joined to the source panel's directory.
- `.` and `..` components are resolved.
- A bare drive letter (`D:`) means that drive's root.
- A drive-rooted path (`\x`) takes the source panel's drive.
- A drive-relative path (`D:x`) is taken relative to the source panel's directory when that directory is on drive `D:`, and relative to `D:\` otherwise.
- A UNC path or an absolute path is used unchanged.

The resolved destination SHALL be an absolute path with no `.` or `..` components. The automatic panel re-read on completion SHALL match panels against this resolved path. If the entered text resolves to no directory (`..` at a drive root), the dialog SHALL stay open with its text unchanged and no operation SHALL start.

#### Scenario: A relative name lands inside the source panel's directory
- **WHEN** the left panel shows `C:\work` with `report.txt` selected, FileCommand was launched from `C:\Users\me`, and the user presses F6, replaces the field with `backup`, and presses Enter
- **THEN** the move's destination is `C:\work\backup`
- **AND** nothing is moved into `C:\Users\me\backup`

#### Scenario: A parent-relative path is resolved and normalized
- **WHEN** the source panel shows `C:\work\proj` and the user confirms the destination `..\archive`
- **THEN** the operation's destination is `C:\work\archive`
- **AND** if the other panel is showing `C:\work\archive`, it re-reads automatically when the operation completes

#### Scenario: A bare drive letter means that drive's root
- **WHEN** the source panel shows `C:\work` and the user confirms the destination `D:`
- **THEN** the operation's destination is `D:\`

#### Scenario: A drive-rooted path keeps the source panel's drive
- **WHEN** the source panel shows `C:\work` and the user confirms the destination `\archive`
- **THEN** the operation's destination is `C:\archive`

#### Scenario: A drive-relative path resolves against the source panel or the drive root
- **WHEN** the source panel shows `D:\work` and the user confirms the destination `D:old`
- **THEN** the operation's destination is `D:\work\old`
- **AND** when the source panel instead shows `C:\work`, the same entry resolves to `D:\old`

#### Scenario: UNC and absolute destinations are used unchanged
- **WHEN** the user confirms the destination `\\server\share\dest`, or `E:\backup`
- **THEN** the operation's destination is exactly `\\server\share\dest`, or `E:\backup`, regardless of the source panel's directory

#### Scenario: An edited drop destination resolves against the drag's source panel
- **WHEN** entries dragged from the inactive right panel showing `D:\src` are dropped on the left panel, and the user replaces the drop dialog's field with `sub` and activates `[ Copy ]`
- **THEN** the copy's destination is `D:\src\sub`

#### Scenario: A make-directory name with a parent path creates it where it points
- **WHEN** the active panel shows `C:\work\proj` and the user presses F7, types `..\shared`, and presses Enter
- **THEN** the directory `C:\work\shared` is created
- **AND** a panel showing `C:\work` re-reads when the operation completes

#### Scenario: An unresolvable destination keeps the dialog open
- **WHEN** the source panel shows the drive root `C:\` and the user confirms the destination `..`
- **THEN** the dialog stays open with `..` in the field and no operation starts
