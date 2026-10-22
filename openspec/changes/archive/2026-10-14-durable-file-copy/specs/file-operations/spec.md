## MODIFIED Requirements

### Requirement: Same-volume move is a rename; cross-volume move is copy-then-verified-delete

A move (F6 Rename/Move) whose source and destination reside on the same volume SHALL be performed as a single `rename` (instant, no data movement). A move across volumes SHALL be performed as a copy followed by deletion of the source, and the source SHALL be deleted only after the copy has been verified successful. Make-directory (F7) SHALL create the named directory at the destination.

A copied file SHALL count as verified only when all of the following hold:

- its data has been flushed to stable storage;
- the destination's length equals the number of bytes copied;
- no write, flush, or close error was reported.

Any such error, or a length mismatch, SHALL be treated as a failure of that file's copy. The failure SHALL be offered through the error-recovery dialog, and the source SHALL be kept.

Removing a source after a verified copy SHALL also be offered through the error-recovery dialog when it fails. A source that cannot be removed SHALL NOT be silently ignored. When the user skips it, it SHALL be listed in the end-of-job skipped-files summary as copied but not removed.

#### Scenario: Same-volume move renames instead of copying
- **WHEN** an entry is moved to a destination on the same volume
- **THEN** the operation is a single `rename` and no byte-copy of the content occurs

#### Scenario: Cross-volume move copies then deletes the source
- **WHEN** an entry is moved to a destination on a different volume
- **THEN** the content is copied to the destination and, only after the copy is verified, the source is deleted

#### Scenario: Cross-volume source survives a failed copy
- **WHEN** a cross-volume copy fails before completing/verifying
- **THEN** the source is NOT deleted

#### Scenario: A flush failure keeps the source
- **WHEN** a cross-volume move has written all of `a.txt`'s data to the destination but flushing it to stable storage reports an error
- **THEN** the error-recovery dialog is shown for `a.txt` and the source `a.txt` is not deleted
- **AND** if the user chooses Skip, no file named `a.txt` and no partial file is left at the destination

#### Scenario: A source that cannot be removed is reported
- **WHEN** a cross-volume move copies and verifies `a.txt` but removing the source `a.txt` fails because another program has it open
- **THEN** the error-recovery dialog is shown for `a.txt`
- **AND** if the user chooses Skip, the verified copy stays at the destination and the end-of-job summary lists `a.txt` as copied but not removed

### Requirement: Copy preserves alternate data streams, attributes, and timestamps

A copy, including the copy half of a cross-volume move, SHALL preserve on the copied file:

- the source's named alternate NTFS data streams;
- the read-only, hidden, system, and archive attributes;
- the creation and last-write timestamps.

On a platform or filesystem that lacks some of these, the copy SHALL preserve the ones it supports, and at minimum the last-write time and the read-only state.

A copy SHALL NOT be required to preserve:

- the source's security descriptor (ACLs and owner). The copy takes the permissions inherited at its destination.
- the last-access time.
- sparse or compression state.
- EFS encryption on a destination that cannot hold it. The copy is then written unencrypted.

Where a target already exists and carries a read-only attribute that would block an overwrite or delete, the operation SHALL clear that attribute as needed to proceed.

#### Scenario: Alternate data streams and metadata survive a copy
- **WHEN** a file carrying an alternate data stream and specific timestamps/attributes is copied
- **THEN** the copied file has the same alternate data stream content, attributes, and timestamps as the source

#### Scenario: Read-only target is cleared before overwrite
- **WHEN** an overwrite (or delete) targets a file whose read-only attribute would otherwise block the operation
- **THEN** the read-only attribute is cleared so the operation can complete

#### Scenario: Hidden and system attributes survive a copy
- **WHEN** a file with the hidden, system, and archive attributes set is copied
- **THEN** the copy has the hidden, system, and archive attributes set, so it renders as hidden in the destination panel just as the source does

#### Scenario: Creation time survives a copy
- **WHEN** a file created at 2001-02-03 04:05 and last written at 2002-03-04 05:06 is copied
- **THEN** the copy reports a creation time of 2001-02-03 04:05 and a last-write time of 2002-03-04 05:06

#### Scenario: A moved file keeps its metadata
- **WHEN** a read-only file carrying an alternate data stream is moved across volumes
- **THEN** the file at the destination has the same alternate data stream content, the read-only attribute, and the source's creation and last-write timestamps

## ADDED Requirements

### Requirement: Copies land via a temporary partial file

A copy, including the copy half of a cross-volume move, SHALL write each file's data to a temporary file in the target's directory named `<name>.~fc-partial`. If that name is already taken, a numbered variant `<name>.~fc-partial-2`, `-3`, … SHALL be used. If the name would exceed the filename length limit, `<name>` SHALL be shortened. The temporary file SHALL be created exclusively, so no existing file is ever overwritten or removed to make room for it.

Only after the copy of that file has succeeded SHALL the temporary file be renamed to the target name. On Overwrite, that rename replaces the existing target.

On every other outcome for that file, the temporary file SHALL be removed. That covers a copy or verification error, a rename error, a Skip, Skip All, or Abort chosen in the error-recovery dialog, and cancellation.

As a result, an existing target chosen for Overwrite SHALL remain intact until its replacement is complete, and a failed copy SHALL never leave a truncated file under the target's name.

#### Scenario: A successful copy appears under the target name with no partial file left
- **WHEN** `report.txt` is copied to `D:\backup` and the copy completes
- **THEN** `D:\backup\report.txt` holds the complete content and no `D:\backup\report.txt.~fc-partial` exists

#### Scenario: A failed overwrite keeps the existing target
- **WHEN** the user chooses Overwrite for an existing `D:\backup\report.txt` and the copy then fails partway (for example, the disk becomes full)
- **THEN** `D:\backup\report.txt` still holds its previous, complete content
- **AND** no `D:\backup\report.txt.~fc-partial` remains once the user answers the error-recovery dialog

#### Scenario: Skip after a failed copy leaves nothing behind
- **WHEN** copying `big.iso` to `D:\backup` (where no `big.iso` existed) fails partway and the user chooses Skip in the error-recovery dialog
- **THEN** neither `D:\backup\big.iso` nor `D:\backup\big.iso.~fc-partial` exists, and `big.iso` is listed in the end-of-job skipped-files summary

#### Scenario: Cancel removes the partial file
- **WHEN** the user cancels a job while `big.iso` is partway through being copied to `D:\backup`
- **THEN** neither `D:\backup\big.iso` nor any `D:\backup\big.iso.~fc-partial*` file remains from that copy

#### Scenario: An existing file with the temporary name is left alone
- **WHEN** `D:\backup` already contains a file named `report.txt.~fc-partial` and `report.txt` is copied there
- **THEN** that existing file is not modified or removed, the copy uses `report.txt.~fc-partial-2` as its temporary file, and `D:\backup\report.txt` receives the copy
