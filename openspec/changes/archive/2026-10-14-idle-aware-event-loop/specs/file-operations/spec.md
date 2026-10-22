## ADDED Requirements

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
