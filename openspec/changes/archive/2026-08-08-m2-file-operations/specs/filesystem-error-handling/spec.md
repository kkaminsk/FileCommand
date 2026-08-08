## ADDED Requirements

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
