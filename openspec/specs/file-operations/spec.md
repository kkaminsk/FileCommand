# file-operations Specification

## Purpose
TBD - created by archiving change m2-file-operations. Update Purpose after archive.
## Requirements
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

---

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

When a job finishes — including a cancellation after partial progress — every tab, on either panel, whose directory matches an affected path SHALL be refreshed so its listing reflects the resulting file-system state without a manual refresh. A tab that is currently active on its panel SHALL re-read immediately, reusing the streaming listing path. A tab that is not currently active (a background tab — see `panel-tabs`) SHALL instead be marked stale and re-read automatically the moment it becomes active, rather than eagerly re-read while off-screen. Each such re-read of a panel's current directory SHALL preserve that panel's cursor entry (or its nearest survivor) and scroll offset as specified by `panel-navigation` — "Same-directory re-read preserves cursor and scroll position".

#### Scenario: Panels refresh after a completed operation
- **WHEN** a copy/move/delete/mkdir job completes
- **THEN** the source and/or destination panels re-read and display the updated directory contents automatically

#### Scenario: Panels refresh after a cancelled operation with partial changes
- **WHEN** a job is cancelled after it has already changed some files on disk
- **THEN** the affected panels still re-read automatically so the listing reflects the partial result

#### Scenario: The opposite panel sharing the affected directory also refreshes
- **WHEN** both panels are browsing the same directory and a delete job completes in the active panel
- **THEN** the opposite (inactive) panel also re-reads automatically and no longer shows the deleted entry

#### Scenario: A background tab on the affected directory is marked stale, not eagerly re-read
- **WHEN** a panel has a background tab (not the active tab) browsing a directory affected by a completed job
- **THEN** that background tab is marked stale rather than re-read immediately, and its cached listing is left untouched until it becomes active

#### Scenario: Post-delete re-read keeps the user's place
- **WHEN** a delete job completes in a panel scrolled partway down a long listing
- **THEN** the panel re-reads, the deleted entry is gone, the window stays where it was, and the cursor is on the entry that followed the deleted one

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

### Requirement: Progress updates are rate-limited

While a file-operation job runs, the system SHALL deliver progress updates to the UI at most about 20 times per second, however quickly the job processes bytes or files. The system SHALL always deliver the job's latest progress before it asks an overwrite-conflict or error question and before it reports completion or cancellation, so those moments never show stale file names or counts. Rate-limiting SHALL NOT delay cancellation: the job SHALL still observe the cancel signal at every file boundary and between chunk copies.

#### Scenario: A fast copy does not flood the UI

- **WHEN** a single 10 GB file is copied at about 2 GB/s
- **THEN** the UI receives no more than about 20 progress updates per second, rather than one per 256 KiB chunk

#### Scenario: Deleting many small files does not flood the UI

- **WHEN** a delete job removes 100,000 small files in a few seconds
- **THEN** the UI receives no more than about 20 progress updates per second

#### Scenario: The final progress is always delivered

- **WHEN** a copy job of 3 files totalling 6,000 bytes completes in less than 50 ms
- **THEN** the last progress update the UI receives before completion reports `files_done` = 3 and `bytes_done` = 6,000

#### Scenario: Progress is current when a conflict is asked

- **WHEN** a copy job reports progress for having copied `a.txt` and, less than 50 ms later, asks an overwrite-conflict question for `b.txt`
- **THEN** the progress update for `a.txt` reaches the UI before the conflict question does

---

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

