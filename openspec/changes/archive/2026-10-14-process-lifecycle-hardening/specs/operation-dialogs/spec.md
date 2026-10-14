## MODIFIED Requirements

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

## ADDED Requirements

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
