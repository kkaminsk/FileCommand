# operation-dialogs Specification

## Purpose
TBD - created by archiving change m2-file-operations. Update Purpose after archive.
## Requirements
### Requirement: Destination input dialog

The system SHALL present, when the user starts a Copy (F5) or Rename/Move (F6) operation, a primary-style input dialog whose editable destination field is pre-filled with the opposite panel's current path, so the common "copy to the other panel" case requires no typing.

#### Scenario: Destination pre-filled from the opposite panel

- **WHEN** the user presses F5 with the left panel active and the right panel showing `D:\backup`
- **THEN** the destination input dialog opens with its field pre-filled with `D:\backup`
- **AND** the text cursor is positioned in the field so the user can edit or accept it

#### Scenario: Accepting the pre-filled destination starts the job

- **WHEN** the destination input dialog is open with a valid destination and the user activates the default button with Enter
- **THEN** the dialog closes and the operation begins against the entered destination

#### Scenario: Cancelling the destination dialog aborts the operation

- **WHEN** the destination input dialog is open and the user presses Esc
- **THEN** the dialog closes and no file operation is started

#### Scenario: Manual UNC destination is accepted

- **WHEN** the user clears the field and types a UNC destination such as `\\server\share\dest`
- **THEN** the entered UNC path is used as the operation destination

### Requirement: Drop-initiated destination dialog

When a drag-and-drop ends over a valid target, the system SHALL open the destination input dialog with its field pre-filled with the exact drop path and with a button row — `Copy`, `Move`, `Cancel` — which the keyboard-initiated dialog does not have, the focused button being the verb proposed by the drag; the title SHALL name the focused verb and the item count. Activating `Copy` or `Move` SHALL start the corresponding job against the entered destination through the existing overwrite-conflict, progress, error-recovery, and summary flows; `Cancel` or Esc SHALL close the dialog with no effect. Keyboard-initiated F5/F6 dialogs SHALL be unchanged.

#### Scenario: Drop dialog contents

- **WHEN** a plain drag of three entries ends over `D:\BACKUP\OLD`
- **THEN** the dialog is titled `Copy 3 files`, the field reads `D:\BACKUP\OLD`, and `[ Copy ]` is focused

#### Scenario: Switching the verb in the dialog

- **WHEN** the drop dialog is open with `[ Copy ]` focused and the user activates `[ Move ]`
- **THEN** a move job starts against the entered destination

#### Scenario: F5 dialog unchanged

- **WHEN** the user presses F5
- **THEN** the destination dialog renders as today — title, prompt, and field with the opposite panel's path, no button row, Enter confirms and Esc cancels

### Requirement: Overwrite-conflict dialog

The system SHALL present, when a copy or move would overwrite an existing target, an overwrite-conflict dialog that displays the source's and the target's size and date and offers the choices Overwrite, Skip, Rename, Overwrite All, and Skip All.

#### Scenario: Source vs target details shown

- **WHEN** copying `report.txt` (2,048 bytes, 2026-08-01) over an existing `report.txt` (900 bytes, 2026-07-15) at the destination
- **THEN** the overwrite-conflict dialog shows both the source size/date and the target size/date so the user can compare them

#### Scenario: Overwrite All latches for later conflicts

- **WHEN** the user selects Overwrite All on the first conflict of a multi-file job
- **THEN** that first target is overwritten
- **AND** every subsequent overwrite conflict in the same job is auto-resolved as Overwrite without re-prompting

#### Scenario: Skip All latches and records skips

- **WHEN** the user selects Skip All on a conflict
- **THEN** that conflicting item and all subsequent conflicting items in the job are skipped without re-prompting
- **AND** each skipped item is recorded for the end-of-job skipped-files summary

#### Scenario: Rendered timestamps are deterministic

- **WHEN** the overwrite-conflict dialog renders source and target dates in a snapshot test with time pinned through the injected `Clock`/formatting path
- **THEN** the displayed dates are stable and reproducible across runs

### Requirement: Progress dialog with byte gauge and Cancel

The system SHALL display, while a copy/move/delete job runs, a progress dialog showing file counts, the current file path, and a byte-progress bar drawn with `█` filled and `░` empty block glyphs, with a Cancel control that requests cancellation of the running job.

#### Scenario: Gauge reflects byte progress

- **WHEN** a job has transferred 5,000,000 of 10,000,000 total bytes
- **THEN** the progress dialog's byte bar renders roughly half filled with `█` glyphs and the remainder as `░` glyphs
- **AND** the file counts and current file path reflect the job's latest progress event

#### Scenario: Cancel requests job cancellation

- **WHEN** the user activates Cancel while the progress dialog is shown
- **THEN** the running job is signalled to cancel and stops at the next file boundary rather than continuing to completion

#### Scenario: Progress dialog stays interactive during a long job

- **WHEN** a long-running job is emitting progress events
- **THEN** the progress dialog remains live and its Cancel control responsive, without blocking the UI thread

### Requirement: Error-recovery dialog

The system SHALL present, when a per-file operation error occurs (such as permission denied, path too long, sharing violation, or disk full), an error-style dialog offering Retry, Skip, Skip All, and Abort, pausing the job until the user chooses.

#### Scenario: Retry re-attempts the failed file

- **WHEN** the error-recovery dialog is shown for a file and the user selects Retry
- **THEN** the operation re-attempts that same file and, on success, the job continues with the next file

#### Scenario: Skip All latches for later errors of the same class

- **WHEN** the user selects Skip All on an error
- **THEN** the current file is skipped and subsequent errors of the same class are auto-skipped without re-prompting
- **AND** each skipped file is recorded for the end-of-job skipped-files summary

#### Scenario: Abort ends the job

- **WHEN** the user selects Abort on the error-recovery dialog
- **THEN** the job stops and no further items are processed

### Requirement: Delete confirmation dialog

The system SHALL require confirmation before a Delete (F8), naming the single item when one item is targeted or showing the count when a multi-selection is targeted, warning that deletion is permanent (no recycle bin), and requiring a second confirmation before removing a non-empty directory.

#### Scenario: Single item named

- **WHEN** the user presses F8 with the cursor on `notes.txt` and nothing selected
- **THEN** the confirmation dialog names `notes.txt` and states that the deletion is permanent

#### Scenario: Multi-selection shown as a count

- **WHEN** the user presses F8 with 12 entries selected
- **THEN** the confirmation dialog shows the count of 12 items rather than naming each one, and states that the deletion is permanent

#### Scenario: Non-empty directory requires a second confirmation

- **WHEN** the user confirms deletion of a directory that contains files
- **THEN** a second confirmation dialog is shown before the directory is removed
- **AND** declining the second confirmation leaves the directory intact

#### Scenario: Declining the first confirmation deletes nothing

- **WHEN** the delete confirmation dialog is shown and the user selects No / presses Esc
- **THEN** no item is deleted

### Requirement: End-of-job skipped-files summary

The system SHALL display, when a job finishes and one or more items were skipped (via Skip, Skip All, or a latched skip policy from a conflict or error), a summary dialog listing the skipped items so the user knows what was not processed.

#### Scenario: Summary lists skipped items

- **WHEN** a copy job completes after the user chose Skip on two conflicting files and Skip All latched a third
- **THEN** an end-of-job summary dialog lists those three skipped items

#### Scenario: No summary when nothing was skipped

- **WHEN** a job completes with no items skipped
- **THEN** no skipped-files summary dialog is shown

