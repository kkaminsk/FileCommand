## ADDED Requirements

### Requirement: Error messages name the affected path

A user-facing filesystem error SHALL identify the file or directory it concerns together with the reason for the failure. A bare operating-system message, such as "Access is denied. (os error 5)", SHALL NOT be shown on its own.

The status-line messages that are built from an operating-system error SHALL name the item by its file or directory name; the panel's title already shows the directory it lives in. They SHALL take these forms:

- a panel whose directory cannot be read SHALL show `Cannot read <directory>: <OS message>` in its read-error state;
- a failure to open a file in the F3 viewer or the built-in editor SHALL read `Cannot open <file>: <OS message>`;
- a failed built-in editor save SHALL read `Cannot save <file>: <reason>`, where the reason is the operating system's message or the editor's own explanation of why it refused to save.

Status-line messages that already name their target, such as the command line's `cd`, `del`, and `rmdir` rejections and the Open-with-associated-application failure, keep their existing text.

The multi-line job dialogs SHALL show the full path:

- the error-recovery dialog SHALL show the full path of the failing item above the operating-system message;
- each row of the end-of-job summary SHALL show the item's full path followed by its reason.

#### Scenario: Access-denied listing names the directory

- **WHEN** a panel navigates to `C:\Windows\CSC` and the read fails with access denied
- **THEN** the panel's read-error message reads `Cannot read CSC: Access is denied. (os error 5)`, and the panel title shows `C:\Windows\CSC`

#### Scenario: Viewer open failure names the file

- **WHEN** F3 is pressed on `locked.db` and the file cannot be opened because another process holds it exclusively
- **THEN** the panel's status line reads `Cannot open locked.db: ` followed by the operating system's sharing-violation message

#### Scenario: Editor save failure names the file

- **WHEN** F2 in the built-in editor fails to write `notes.txt` because the disk is full
- **THEN** the editor reports `Cannot save notes.txt: ` followed by the operating system's message, and the buffer stays modified

#### Scenario: The error-recovery dialog shows the full path

- **WHEN** a copy job fails to read `D:\data\reports\q3.xlsx` with a permission error
- **THEN** the error-recovery dialog shows `D:\data\reports\q3.xlsx` on one line and the operating system's message on the next

#### Scenario: Summary rows show the full path and reason

- **WHEN** a job ends having skipped `D:\data\old.log` because the target already exists
- **THEN** the end-of-job summary row reads `D:\data\old.log: target already exists`
