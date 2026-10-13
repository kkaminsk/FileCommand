## MODIFIED Requirements

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

## ADDED Requirements

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
