## MODIFIED Requirements

### Requirement: Automatic panel re-read on completion

When a job finishes — including a cancellation after partial progress — every tab, on either panel, whose directory matches an affected path SHALL be refreshed so its listing reflects the resulting file-system state without a manual refresh. A tab that is currently active on its panel SHALL re-read immediately, reusing the streaming listing path. A tab that is not currently active (a background tab — see `panel-tabs`) SHALL instead be marked stale and re-read automatically the moment it becomes active, rather than eagerly re-read while off-screen.

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
