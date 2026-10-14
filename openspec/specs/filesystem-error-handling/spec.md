# filesystem-error-handling Specification

## Purpose
TBD - created by archiving change m2-file-operations. Update Purpose after archive.
## Requirements
### Requirement: Narrow internal fs trait seam with deterministic fault injection

All file-system access inside `fs_ops` SHALL be routed through a single narrow internal trait (metadata/identity query, read-dir, create-dir, copy-file, rename, remove-file, remove-dir, set-attributes, reparse-point inspection). The project SHALL provide a real Windows-backed implementation and a fake implementation usable by `filecommand-core` unit tests. The fake SHALL be able to deterministically inject at least permission-denied, sharing-violation, and disk-full failures at chosen operations without a terminal and without provoking a real disk fault. No code in `fs_ops` SHALL call `std::fs` (or platform fs APIs) directly, bypassing the trait.

#### Scenario: Injected permission-denied surfaces as a typed error

- **WHEN** the fake fs is configured to return permission-denied for a copy-file call and a copy job reaches that file
- **THEN** `fs_ops` emits an error event classified as permission-denied for that specific path, carrying enough context (path, operation kind) for the recovery flow, without touching any real disk

#### Scenario: Injected sharing-violation and disk-full are distinguishable

- **WHEN** the fake fs is configured to return a sharing-violation on one file and disk-full on another during the same job
- **THEN** `fs_ops` emits two error events whose error classes are distinct (sharing-violation vs disk-full), each attributed to its originating path

#### Scenario: All fs access goes through the seam

- **WHEN** a copy/move/delete/mkdir job runs under the fake fs with no real filesystem side effects allowed
- **THEN** the job completes (or fails) driven entirely by the fake, proving no `fs_ops` code path reaches the real filesystem outside the trait

### Requirement: `\\?\` long-path abstraction centralizes prefixing

Every filesystem call SHALL pass through a path abstraction that applies the `\\?\` (and `\\?\UNC\` for UNC paths) extended-length prefix as needed, chosen over relying on the Windows 10 1607+ `LongPathsEnabled` registry setting plus manifest `longPathAware` declaration. Because `\\?\` paths bypass Windows path normalization, the abstraction SHALL fully canonicalize a path (resolve `.`/`..`, convert forward slashes to backslashes, remove relative components) before prefixing. Callers within `fs_ops` SHALL NOT hand-build `\\?\`-prefixed paths.

#### Scenario: Operation succeeds on a path exceeding MAX_PATH

- **WHEN** a copy or delete targets a path longer than 260 characters
- **THEN** the abstraction supplies the `\\?\`-prefixed form and the operation completes without a path-too-long failure caused by the absence of the prefix

#### Scenario: Canonicalization precedes prefixing

- **WHEN** the abstraction is given a path containing `.`/`..` components or forward slashes
- **THEN** it fully canonicalizes to an absolute backslash-separated path with relative components resolved before applying the `\\?\` prefix, so no malformed extended-length path is ever produced

#### Scenario: UNC paths use the UNC prefix form

- **WHEN** the abstraction is given a UNC path (`\\server\share\...`)
- **THEN** it applies the `\\?\UNC\` prefix form rather than the plain `\\?\` form

### Requirement: Read-only attribute handling before overwrite and delete

`fs_ops` SHALL detect the read-only attribute on a target and clear it as needed before overwriting or deleting that target, so a read-only attribute alone does not cause an operation to fail. Attribute inspection and clearing SHALL go through the fs trait seam so this behavior is testable under the fake fs.

#### Scenario: Read-only target is deleted after clearing the attribute

- **WHEN** a delete job reaches a file whose read-only attribute is set
- **THEN** `fs_ops` clears the read-only attribute via the trait and then removes the file, without raising the attribute as an error

#### Scenario: Read-only target is overwritten after clearing the attribute

- **WHEN** an overwrite of an existing read-only target has been chosen and the copy proceeds
- **THEN** `fs_ops` clears the read-only attribute on the target before writing over it, so the overwrite is not blocked by the attribute

### Requirement: Inline panel read-error state offering re-read or drive change

When reading or re-reading a panel's directory fails (for example drive removed or access denied), the panel SHALL enter an inline error state rendered in the error style rather than crashing or leaving the process. The error state SHALL offer at least re-read and drive-change actions so the user can recover in place. A filesystem error during listing SHALL never crash the application.

#### Scenario: Access-denied listing shows the inline error state

- **WHEN** a panel attempts to list a directory and the read fails with access denied
- **THEN** the panel enters an inline error state (not a panic or exit) that presents re-read and drive-change options

#### Scenario: Re-read from the error state recovers when the fault clears

- **WHEN** a panel is in the read-error state because its drive was removed and the user chooses re-read after the drive returns
- **THEN** the panel retries the listing and, on success, replaces the error state with the normal directory display

#### Scenario: Drive change from the error state leaves the failed location

- **WHEN** a panel is in the read-error state and the user chooses drive change
- **THEN** the panel navigates to the selected drive/path and exits the error state, without the app crashing on the prior failure

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

### Requirement: Real filesystem engine is integration-tested; fault injection is test-only

The real filesystem implementation of the fs trait seam SHALL be covered by an automated integration-test suite. The suite SHALL run file-operation jobs through the real implementation against temporary directories on the real filesystem, and SHALL remove those directories afterwards. It SHALL cover at least:

- copying a file that carries alternate data streams, attributes, and timestamps;
- a junction inside a tree that is moved;
- paths longer than 260 characters;
- overwriting a read-only target;
- a case-only rename;
- cleanup after a cancelled copy.

Tests that depend on Windows- or NTFS-specific behavior SHALL be compiled and run only on Windows. The fake filesystem and its fault-injection API SHALL be compiled only for tests and SHALL NOT be part of the release binary.

#### Scenario: The suite exercises the real implementation on temporary directories
- **WHEN** `cargo test --workspace` runs on Windows
- **THEN** the real-filesystem suite runs copy, move, delete, and rename jobs through the real implementation on temporary directories, asserts the resulting on-disk state, and leaves none of its temporary directories behind

#### Scenario: Windows-specific tests are gated
- **WHEN** the test suite is built for a non-Windows target
- **THEN** the alternate-data-stream, attribute, junction, and long-path tests are not compiled, and the platform-neutral real-filesystem tests still run

#### Scenario: The release binary carries no fault-injection double
- **WHEN** the workspace is built without tests (for example `cargo build --release`)
- **THEN** the fake filesystem and its fault-injection types are not compiled into `filecommand-core`, and no non-test code can refer to them

