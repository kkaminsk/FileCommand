## ADDED Requirements

### Requirement: Cancellable file-operation jobs with progress events

Copy, move, delete, and make-directory operations SHALL each execute as a `Job` on a worker thread so the UI thread never performs blocking I/O. While a job runs it SHALL emit progress events carrying the current file being processed, bytes done and bytes total, and files done and files total, folded into core state through `core::update`. A job SHALL observe a cancel signal at every file boundary and between chunk copies of a large file, stopping promptly when set. Selected directories SHALL contribute 0 bytes to `bytes_total` (no directory sizing in v1); their file contents contribute normally when a copy/move/delete recurses into them.

#### Scenario: Progress totals accumulate across a multi-file copy
- **WHEN** a copy job of 3 files totalling 6,000 bytes runs to completion
- **THEN** the worker emits progress events whose `files_total` is 3 and `bytes_total` is 6,000
- **AND** the final progress event reports `files_done` = 3 and `bytes_done` = 6,000

#### Scenario: Cancel is honored mid-job
- **WHEN** the cancel signal is set while a copy job is between files or mid-chunk of a large file
- **THEN** the worker stops before starting the next file (or next chunk) and emits a terminal `Done`/cancelled event rather than completing the remaining work

#### Scenario: Selected directory adds no bytes to the total
- **WHEN** a job's source list includes a directory entry alongside a 1,000-byte file
- **THEN** `bytes_total` counts only the bytes of files (the directory itself adds 0), while the directory's own contained files are still counted when the job recurses into it

#### Scenario: UI stays interactive during a long job
- **WHEN** a copy job is in progress on the worker thread
- **THEN** the UI thread continues to process input and repaint without waiting on the job's I/O

---

### Requirement: Same-volume move is a rename; cross-volume move is copy-then-verified-delete

A move (F6 Rename/Move) whose source and destination reside on the same volume SHALL be performed as a single `rename` (instant, no data movement). A move across volumes SHALL be performed as a copy followed by deletion of the source, and the source SHALL be deleted only after the copy has been verified successful. Make-directory (F7) SHALL create the named directory at the destination.

#### Scenario: Same-volume move renames instead of copying
- **WHEN** an entry is moved to a destination on the same volume
- **THEN** the operation is a single `rename` and no byte-copy of the content occurs

#### Scenario: Cross-volume move copies then deletes the source
- **WHEN** an entry is moved to a destination on a different volume
- **THEN** the content is copied to the destination and, only after the copy is verified, the source is deleted

#### Scenario: Cross-volume source survives a failed copy
- **WHEN** a cross-volume copy fails before completing/verifying
- **THEN** the source is NOT deleted

---

### Requirement: Identity-aware case-only rename

The target-exists check used by rename/move SHALL compare file identity (volume plus file index), not the name string. A rename that changes only the case of a name (for example `foo` → `Foo`) SHALL succeed and SHALL NOT be treated as an overwrite conflict or a self-overwrite.

#### Scenario: Case-only rename succeeds
- **WHEN** an entry named `foo` is renamed to `Foo` on a case-insensitive volume where the source and target resolve to the same file identity
- **THEN** the rename completes successfully and does not raise an overwrite conflict

#### Scenario: Distinct existing target still conflicts
- **WHEN** a rename target resolves to a different file identity than the source and that target already exists
- **THEN** the operation raises an overwrite conflict rather than silently overwriting

---

### Requirement: Copy preserves alternate data streams, attributes, and timestamps

A copy SHALL preserve alternate NTFS data streams, file attributes, and timestamps of the source on the copied file. Where a target already exists and carries a read-only attribute that would block an overwrite or delete, the operation SHALL clear that attribute as needed to proceed.

#### Scenario: Alternate data streams and metadata survive a copy
- **WHEN** a file carrying an alternate data stream and specific timestamps/attributes is copied
- **THEN** the copied file has the same alternate data stream content, attributes, and timestamps as the source

#### Scenario: Read-only target is cleared before overwrite
- **WHEN** an overwrite (or delete) targets a file whose read-only attribute would otherwise block the operation
- **THEN** the read-only attribute is cleared so the operation can complete

---

### Requirement: Reparse-point (symlink/junction) semantics

Reparse points SHALL be handled distinctly from ordinary directories: deleting a reparse point SHALL remove the link itself and never the target's contents; copying a reparse point SHALL copy the link target's content by default. Recursive operations SHALL carry recursion-cycle protection via a visited file-identity set and SHALL NOT traverse into a junction that points inside the source tree being processed.

#### Scenario: Delete removes the link, not the target
- **WHEN** a job deletes a junction or symlink
- **THEN** the link is removed and the files under the link's target directory remain untouched

#### Scenario: Copy duplicates the link target's content
- **WHEN** a job copies a reparse point
- **THEN** the destination receives a copy of the target's content (not merely a re-created link) by default

#### Scenario: Recursion cycle is not re-entered
- **WHEN** a recursive copy encounters a junction pointing to a directory already inside the source tree (present in the visited-identity set)
- **THEN** the job does not traverse into it, avoiding an infinite recursion cycle

---

### Requirement: Long-path correctness

All file-system access performed by a job SHALL route through the path abstraction that applies the `\\?\` (and `\\?\UNC\`) prefix as needed so operations on paths exceeding the legacy limit succeed without depending on a machine-wide `LongPathsEnabled` setting. Callers SHALL NOT hand-build prefixed paths; the abstraction SHALL fully canonicalize a path (no relative `.`/`..` components, no forward slashes) before applying the prefix.

#### Scenario: Operation on a path longer than the legacy limit succeeds
- **WHEN** a copy/move/delete targets a path longer than 260 characters
- **THEN** the operation succeeds via the `\\?\`-prefixed path without requiring the registry long-path opt-in

#### Scenario: Prefixing canonicalizes first
- **WHEN** a path containing relative components or forward slashes is prefixed for a job
- **THEN** it is canonicalized to a fully-qualified backslash path before the `\\?\` prefix is applied

---

### Requirement: Automatic panel re-read on completion

When a job finishes — including a cancellation after partial progress — the affected panel or panels SHALL re-read automatically, reusing the streaming listing path, so the on-screen listing reflects the resulting file-system state without a manual refresh.

#### Scenario: Panels refresh after a completed operation
- **WHEN** a copy/move/delete/mkdir job completes
- **THEN** the source and/or destination panels re-read and display the updated directory contents automatically

#### Scenario: Panels refresh after a cancelled operation with partial changes
- **WHEN** a job is cancelled after it has already changed some files on disk
- **THEN** the affected panels still re-read automatically so the listing reflects the partial result
