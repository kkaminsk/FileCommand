# operation-dialogs Specification

## Purpose
TBD - created by archiving change m2-file-operations. Update Purpose after archive.
## Requirements
### Requirement: Destination input dialog

The system SHALL present, when the user starts a Copy (F5) or Rename/Move (F6) operation, a primary-style input dialog whose editable destination field is pre-filled with the opposite panel's current path, so the common "copy to the other panel" case requires no typing. The field SHALL be a shared text field per `text-field-editing` (caret keys, word keys, paste, scrolling window), opening with the caret at the end of the pre-filled path.

#### Scenario: Destination pre-filled from the opposite panel

- **WHEN** the user presses F5 with the left panel active and the right panel showing `D:\backup`
- **THEN** the destination input dialog opens with its field pre-filled with `D:\backup`
- **AND** the caret is at the end of the field so the user can edit or accept it

#### Scenario: Editing the pre-filled destination in place

- **WHEN** the field reads `D:\backup` and the user presses Home, Delete, then types `E`
- **THEN** the field reads `E:\backup`

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

The system SHALL present an overwrite-conflict dialog when a copy or move would overwrite an existing target. The dialog SHALL display the source's and the target's size and date and SHALL offer the choices Overwrite, Skip, Rename, Overwrite All, Skip All, and Cancel. Cancel SHALL be chosen with Esc, with the `C` hotkey, or by clicking the dialog's Cancel control.

Cancel SHALL abort the job:

- the conflicting target SHALL be left untouched and SHALL NOT be recorded as a skipped item;
- no further items SHALL be processed;
- the job SHALL end as cancelled through the same path as the progress dialog's Cancel, so the affected panels re-read and any items skipped earlier in the job are listed in the end-of-job skipped-files summary.

While the Rename name field is open, Esc SHALL close the field and return to the dialog's choices without cancelling the job.

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

#### Scenario: Esc cancels the job from the conflict dialog

- **WHEN** a ten-file copy reaches a conflict on its third file, `report.txt`, and the user presses Esc
- **THEN** the existing `report.txt` at the destination is unchanged, files four to ten are not copied, and the job ends as cancelled
- **AND** the panels re-read, and no skipped-files summary lists `report.txt`

#### Scenario: The Cancel control and hotkey match Esc

- **WHEN** the overwrite-conflict dialog is shown and the user presses `C` or clicks the dialog's Cancel control
- **THEN** the job is cancelled exactly as if Esc had been pressed

#### Scenario: Esc in the Rename field returns to the choices

- **WHEN** the user has chosen Rename and is typing a new name, then presses Esc
- **THEN** the name field closes, the dialog shows its choices again, and the job is still waiting for an answer

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

The system SHALL require confirmation before a Delete (triggered by F8 or the Delete key), naming the single item when one item is targeted or showing the count when a multi-selection is targeted, warning that deletion is permanent (no recycle bin), and requiring a second confirmation before removing a non-empty directory. The Delete key SHALL be a fixed (non-rebindable) alias for F8 and SHALL trigger deletion only when pressed without modifier keys.

#### Scenario: Single item named

- **WHEN** the user presses F8 with the cursor on `notes.txt` and nothing selected
- **THEN** the confirmation dialog names `notes.txt` and states that the deletion is permanent

#### Scenario: The Delete key opens the confirmation

- **WHEN** the user presses the Delete key (unmodified) with the cursor on `notes.txt` and nothing selected
- **THEN** the same confirmation dialog opens naming `notes.txt` and stating the deletion is permanent, exactly as if F8 had been pressed
- **AND** nothing is deleted until the confirmation is accepted

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

The system SHALL display, when a job finishes and one or more items were skipped (for example via Skip, Skip All, or a latched skip policy from a conflict or error), a summary dialog listing the skipped items so the user knows what was not processed. The system SHALL also display the summary when a job finishes having recorded one or more warnings (see "Metadata-restore failures are reported in the end-of-job summary"), even if nothing was skipped. A job that finishes with neither skipped items nor warnings SHALL show no summary.

#### Scenario: Summary lists skipped items

- **WHEN** a copy job completes after the user chose Skip on two conflicting files and Skip All latched a third
- **THEN** an end-of-job summary dialog lists those three skipped items

#### Scenario: No summary when nothing was skipped

- **WHEN** a job completes with no items skipped and no warnings recorded
- **THEN** no skipped-files summary dialog is shown

#### Scenario: Summary shown for warnings alone

- **WHEN** a copy job completes with no items skipped but one copied file's modification time could not be restored
- **THEN** the end-of-job summary dialog is shown, titled by its warning count, and lists that warning

### Requirement: Metadata-restore failures are reported in the end-of-job summary

After a file's content has been copied successfully, any metadata the system restores on the copy that cannot be restored SHALL be recorded as a warning for that file, never discarded silently. This covers:

- an alternate data stream that cannot be listed or copied;
- the modification time;
- the read-only attribute.

When the destination volume does not support alternate data streams at all, the job SHALL record one warning instead of one per file. That warning SHALL name the destination and the number of files whose streams were not copied.

A file with only warnings SHALL count as copied: it SHALL NOT be recorded as a skipped item, and it SHALL count toward the progress dialog's completed files.

The end-of-job summary SHALL list warnings separately from skipped items. Each warning SHALL show the file's full path and what could not be restored, with the operating system's message. A summary that holds only warnings SHALL be titled by its warning count; one that holds both SHALL give both counts. When the summary is shown is defined by "End-of-job skipped-files summary".

#### Scenario: Timestamp restore failure becomes a warning

- **WHEN** a copy of `report.docx` succeeds but setting the destination's modification time fails
- **THEN** `report.docx` is counted as copied and is not listed as skipped
- **AND** the end-of-job summary is shown, titled with one warning, and lists `report.docx`'s full destination path with "modification time not restored" and the operating system's message

#### Scenario: Alternate data stream copy failure becomes a warning

- **WHEN** a copy of `photo.jpg` that carries a `Zone.Identifier` stream succeeds for the main content but copying the stream fails
- **THEN** the summary lists a warning naming `photo.jpg` and the `Zone.Identifier` stream with the operating system's message

#### Scenario: A destination without stream support yields one warning

- **WHEN** forty downloaded files, each carrying a `Zone.Identifier` stream, are copied to a FAT32 USB drive
- **THEN** all forty files are copied, and the summary shows a single warning naming the destination and stating that the streams of 40 files were not copied

#### Scenario: Warnings and skipped items appear together

- **WHEN** a job skips two conflicting files and records one read-only-attribute warning on a third
- **THEN** a single end-of-job summary lists the two skipped items and, separately, the one warning, and its title gives both counts

#### Scenario: Clean job shows no summary

- **WHEN** a job completes with no skipped items and every metadata restore succeeds
- **THEN** no end-of-job summary is shown

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

### Requirement: Cancellation is observed while a job waits for an answer

While a job is paused waiting for an answer to an overwrite-conflict or error-recovery dialog, a cancellation request SHALL take effect without the dialog being answered. Quitting the application is one source of such a request. The job SHALL then:

- stop waiting;
- treat the pending prompt as aborted, neither overwriting nor skipping the item in question;
- perform its normal cancellation cleanup;
- end as cancelled.

If the dialog's answer can no longer reach the job, the job SHALL likewise end as cancelled rather than proceed with a default answer. Esc on the error-recovery dialog SHALL choose Abort.

#### Scenario: A cancel request ends a job waiting on an error prompt

- **WHEN** a job is paused on the error-recovery dialog for `locked.db` and a cancellation is requested without the dialog being answered
- **THEN** the job stops waiting promptly, does not retry or skip `locked.db`, processes no further items, and ends as cancelled

#### Scenario: A cancel request ends a job waiting on a conflict prompt

- **WHEN** a job is paused on the overwrite-conflict dialog and a cancellation is requested without the dialog being answered
- **THEN** the conflicting target is left untouched, no further items are processed, and the job ends as cancelled

#### Scenario: A lost answer does not resume the job with a default

- **WHEN** a job is paused on a conflict or error prompt and the channel that would deliver the answer is closed
- **THEN** the job ends as cancelled instead of continuing with a default Skip or Abort answer applied to that item and later items

#### Scenario: Esc on the error-recovery dialog aborts

- **WHEN** the error-recovery dialog is shown and the user presses Esc
- **THEN** the job stops and no further items are processed, exactly as if Abort had been selected

